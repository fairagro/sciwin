//! Adjustments applied to a packed CWL graph before submitting it to REANA.
//!
//! REANA's job executor doesn't support everything CWL allows: it can't build a container
//! from a `DockerRequirement`'s `dockerFile` itself, and running a bare script interpreter
//! (`python`, `Rscript`, ...) with no container at all isn't reproducible on a shared
//! cluster. [`compatibility_adjustments`] patches a graph to work around both, plus rewrites
//! staged-file paths in the base command -- see
//! <https://github.com/fairagro/sciwin/issues/114>.

use crate::authoring::tool::parser::command::SCRIPT_EXECUTORS;
use crate::execution::{RunnerError, RunnerResult};
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, CommandLineTool},
    requirements::{
        DockerRequirement, InitialWorkDirRequirement, ListingItems, StringOrInclude,
        ToolRequirements, WorkDirItems,
    },
};
use std::{collections::HashMap, io::Write, path::Path};
use tempfile::NamedTempFile;
use tokio::process::Command;
use tracing::{info, warn};

/// Patches every `CommandLineTool` in `graph` to run on REANA: rewrites staged-file paths
/// out of the base command (see [`adjust_basecommand`]), then makes sure each tool has a
/// pullable container -- building and publishing one ephemerally for tools that only carry a
/// `dockerFile`, or falling back to a default image for bare script interpreters.
///
/// # Errors
/// Returns [`RunnerError::DockerUnavailable`] if Docker isn't running, or
/// [`RunnerError::DockerCommandFailed`] if building or publishing an ephemeral image fails.
pub async fn compatibility_adjustments(graph: &mut [CWLDocument]) -> RunnerResult<()> {
    if !docker_running() {
        return Err(RunnerError::DockerUnavailable);
    }
    info!("starting REANA compatibility adjustments");

    let mut docker_jobs: Vec<CommandLineTool> = vec![];
    for item in graph.iter_mut() {
        if let CWLDocument::CommandLineTool(tool) = item {
            adjust_basecommand(tool);
            if !has_docker_pull(tool) {
                docker_jobs.push(tool.clone());
            }
        }
    }

    for mut tool in docker_jobs {
        if !has_docker_pull(&tool) {
            publish_docker_ephemeral(&mut tool).await?;
            if !has_docker_pull(&tool) {
                inject_docker_pull(&mut tool);
            }
        }
        for item in graph.iter_mut() {
            if let CWLDocument::CommandLineTool(existing) = item
                && existing.id == tool.id
            {
                *existing = tool.clone();
            }
        }
    }

    info!("REANA compatibility adjustments completed");
    Ok(())
}

/// Builds and publishes an ephemeral image (to `ttl.sh`, 1 hour TTL) for a tool whose
/// `DockerRequirement` only carries a `dockerFile`. A no-op for tools with no
/// `DockerRequirement`, or one that already has a `dockerFile`.
async fn publish_docker_ephemeral(tool: &mut CommandLineTool) -> RunnerResult<()> {
    let id = tool.id.clone().unwrap_or_default();
    let Some(dr) = tool.get_requirement_mut::<DockerRequirement>() else {
        return Ok(());
    };
    let Some(dockerfile) = &mut dr.docker_file else {
        return Ok(());
    };

    info!("tool {id} depends on a Dockerfile, which REANA can't build -- publishing an ephemeral image");
    if !docker_installed() {
        warn!("Docker not installed, skipping ephemeral image build for {id}");
        return Ok(());
    }

    let tag = format!("ttl.sh/{}:1h", uuid::Uuid::new_v4());
    let docker_content = match dockerfile {
        StringOrInclude::String(src) => src.clone(),
        StringOrInclude::Include(include) => tokio::fs::read_to_string(&include.include).await?,
    };

    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(docker_content.as_bytes())?;
    let file_path = temp_file.into_temp_path();

    run_docker(&[
        "build",
        "-t",
        &tag,
        "-f",
        &file_path.to_string_lossy(),
        ".",
    ])
    .await?;
    info!("built ephemeral Docker image for tool {id}");

    run_docker(&["push", &tag]).await?;
    info!("published {tag} (available for 1 hour) for tool {id}");

    dr.docker_pull = Some(tag);
    dr.docker_file = None;
    dr.docker_image_id = None;
    Ok(())
}

/// Attaches a default image for a bare script interpreter (`python`, `Rscript`, `node`)
/// that still has no pullable container after [`publish_docker_ephemeral`] -- e.g. it had no
/// `DockerRequirement` at all. A no-op for anything else.
fn inject_docker_pull(tool: &mut CommandLineTool) {
    let id = tool.id.clone().unwrap_or_default();
    let Some(OneOrMany::Many(command_vec)) = &tool.base_command else {
        return;
    };
    let Some(executor) = command_vec.first() else {
        return;
    };

    let default_images = HashMap::from([("python", "python"), ("Rscript", "r-base"), ("node", "node")]);

    if SCRIPT_EXECUTORS.contains(&executor.as_str()) {
        warn!("tool {id} uses {executor} and does not use a proper container");
        if let Some(container) = default_images.get(executor.as_str()) {
            tool.append_requirement_mut(ToolRequirements::DockerRequirement(
                DockerRequirement::builder().docker_pull(*container).build(),
            ));
            info!("added container {container} to tool {id}");
        }
    }
}

fn has_docker_pull(tool: &CommandLineTool) -> bool {
    tool.requirements.as_ref().is_some_and(|reqs| {
        reqs.iter().any(|req| {
            matches!(req, ToolRequirements::DockerRequirement(d) if d.docker_pull.is_some())
        })
    })
}

/// Strips directory prefixes off staged files (declared via `InitialWorkDirRequirement`)
/// when they're referenced by the base command -- REANA runs the tool with its working
/// directory already containing the staged file at its bare name, so a base command built
/// against a local relative path (e.g. `scripts/run.py`) needs rewriting to just the
/// filename. See <https://github.com/fairagro/sciwin/issues/114>.
fn adjust_basecommand(tool: &mut CommandLineTool) {
    let mut changed = false;
    let mut command_vec = match &tool.base_command {
        Some(OneOrMany::Many(vec)) => vec.clone(),
        _ => return,
    };

    if let Some(iwdr) = tool.get_requirement_mut::<InitialWorkDirRequirement>() {
        match &mut iwdr.listing {
            WorkDirItems::Expression(_) => {}
            WorkDirItems::ListingItems(items) => {
                for item in items {
                    adjust_iwdr_item(item, &mut command_vec, &mut changed);
                }
            }
        }
    }

    if changed {
        info!(
            "basecommand of {} rewritten to `{}` for REANA",
            tool.id.clone().unwrap_or_default(),
            command_vec.join(" ")
        );
        tool.base_command = Some(OneOrMany::Many(command_vec));
    }
}

fn adjust_iwdr_item(item: &mut ListingItems, command_vec: &mut [String], changed: &mut bool) {
    let ListingItems::Dirent(dirent) = item else {
        return;
    };
    let Some(entryname) = &mut dirent.entryname else {
        return;
    };
    if !command_vec.contains(entryname) {
        return;
    }

    let path = Path::new(entryname);
    if path.parent().is_none() {
        return;
    }

    let pos = command_vec
        .iter()
        .position(|c| c == entryname)
        .expect("entryname was just confirmed present in command_vec");
    *entryname = path
        .file_name()
        .expect("a path with a parent component has a file name")
        .to_string_lossy()
        .into_owned();
    command_vec[pos] = entryname.clone();
    *changed = true;
}

async fn run_docker(args: &[&str]) -> RunnerResult<()> {
    let output = Command::new("docker")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?
        .wait_with_output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(RunnerError::DockerCommandFailed {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn docker_running() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn docker_installed() -> bool {
    std::process::Command::new("docker")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::files::Dirent;

    fn tool_with_command(command: &[&str]) -> CommandLineTool {
        CommandLineTool::builder()
            .base_command(OneOrMany::Many(
                command.iter().map(|s| s.to_string()).collect(),
            ))
            .build()
    }

    #[test]
    fn has_docker_pull_detects_existing_requirement() {
        let mut tool = tool_with_command(&["python3", "script.py"]);
        assert!(!has_docker_pull(&tool));

        tool.append_requirement_mut(ToolRequirements::DockerRequirement(
            DockerRequirement::builder().docker_pull("python").build(),
        ));
        assert!(has_docker_pull(&tool));
    }

    #[test]
    fn inject_docker_pull_attaches_default_image_for_bare_interpreter() {
        let mut tool = tool_with_command(&["python3", "script.py"]);
        inject_docker_pull(&mut tool);
        // python3 is a recognized script executor, but only "python" (not "python3") has a
        // default image -- matches the original packages/remote_execution behavior exactly.
        assert!(!has_docker_pull(&tool));

        let mut tool = tool_with_command(&["python", "script.py"]);
        inject_docker_pull(&mut tool);
        assert!(has_docker_pull(&tool));
    }

    #[test]
    fn inject_docker_pull_ignores_non_script_command() {
        let mut tool = tool_with_command(&["echo", "hello"]);
        inject_docker_pull(&mut tool);
        assert!(!has_docker_pull(&tool));
    }

    #[test]
    fn adjust_basecommand_strips_staged_directory_prefix() {
        let mut tool = tool_with_command(&["python3", "scripts/run.py"]);
        tool.append_requirement_mut(ToolRequirements::InitialWorkDirRequirement(
            InitialWorkDirRequirement {
                listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(
                    Dirent::builder()
                        .entry(StringOrInclude::String("print('hi')".to_string()))
                        .entryname("scripts/run.py")
                        .build(),
                )]),
            },
        ));

        adjust_basecommand(&mut tool);

        assert_eq!(
            tool.base_command,
            Some(OneOrMany::Many(vec![
                "python3".to_string(),
                "run.py".to_string()
            ]))
        );
    }

    #[test]
    fn adjust_basecommand_leaves_bare_filename_untouched() {
        let mut tool = tool_with_command(&["python3", "run.py"]);
        tool.append_requirement_mut(ToolRequirements::InitialWorkDirRequirement(
            InitialWorkDirRequirement {
                listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(
                    Dirent::builder()
                        .entry(StringOrInclude::String("print('hi')".to_string()))
                        .entryname("run.py")
                        .build(),
                )]),
            },
        ));

        let before = tool.base_command.clone();
        adjust_basecommand(&mut tool);
        assert_eq!(tool.base_command, before);
    }
}
