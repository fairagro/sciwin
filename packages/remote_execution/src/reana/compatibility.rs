use colored::Colorize;
use commonwl::{prelude::*, requirements::WorkDirItem};
use log::{info, warn};
use reana_ext::parser::WorkflowJson;
use std::collections::HashMap;
use std::path::Path;
use util::is_docker_installed;
use anyhow::{Result, anyhow};
use tokio::sync::mpsc::Sender;
use tokio::process::Command as AsyncCommand;
use s4n_core::parser::SCRIPT_EXECUTORS;
use tempfile::NamedTempFile;
use std::io::Write;

pub async fn log_msg(log_sender: &Option<Sender<String>>, message: &str) {
    if let Some(tx) = log_sender {
        let _ = tx.send(format!("{message}\n")).await;
    } else {
        eprintln!("{message}");
    }
}

pub async fn compatibility_adjustments(
    workflow_json: &mut WorkflowJson,
    log_sender: Option<Sender<String>>,
) -> Result<()> {
    if !is_docker_available() {
        return Err(anyhow!("❌ Docker is not running or accessible"));
    }
    log_msg(&log_sender, "🔧 Starting compatibility adjustments...").await;
    let mut docker_jobs: Vec<CommandLineTool> = vec![];
    for item in &mut workflow_json.workflow.specification.graph {
        if let CWLDocument::CommandLineTool(tool) = item {
            adjust_basecommand(tool)?;
            if !has_docker_pull(tool) {
                docker_jobs.push(tool.clone());
            }
        }
    }
    for mut tool in docker_jobs {
        if !has_docker_pull(&tool) {
            publish_docker_ephemeral(&mut tool, &log_sender).await?;
            if !has_docker_pull(&tool) {
                inject_docker_pull(&mut tool, &log_sender).await?;
            }
        }
        for item in &mut workflow_json.workflow.specification.graph {
            if let CWLDocument::CommandLineTool(existing) = item && existing.id == tool.id {
                *existing = tool.clone();
            }
        }
    }
    log_msg(&log_sender, "✅ Compatibility adjustments completed.").await;
    Ok(())
}

pub async fn publish_docker_ephemeral(
    tool: &mut CommandLineTool,
    log_sender: &Option<Sender<String>>,
) -> Result<()> {
    let id = tool.id.clone().unwrap();
    if let Some(dr) = tool.get_requirement_mut::<DockerRequirement>()
        && let Some(dockerfile) = &mut dr.docker_file
    {
        log_msg(log_sender, &format!("⚠️ Tool {} depends on Dockerfile", id)).await;
        if !is_docker_installed() {
            log_msg(log_sender, "⚠️ Docker not installed, skipping image build.").await;
            return Ok(());
        }
        let image_name = uuid::Uuid::new_v4().to_string();
        let tag = format!("ttl.sh/{image_name}:1h");
        let docker_content = match dockerfile {
            commonwl::Entry::Source(src) => src.clone(),
            commonwl::Entry::Include(include) => tokio::fs::read_to_string(&include.include).await?,
        };
        let mut temp_file = NamedTempFile::new()
            .map_err(|e| anyhow!("Failed to create temporary file: {e}"))?;

        temp_file
            .write_all(docker_content.as_bytes())
            .map_err(|e| anyhow!("Failed to write temporary Dockerfile: {e}"))?;

        let file_path = temp_file.into_temp_path();
        let build = AsyncCommand::new("docker")
            .arg("build")
            .arg("-t").arg(&tag)
            .arg("-f").arg(&file_path)
            .arg(".")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let output = build.wait_with_output().await?;
        if !output.status.success() {
            return Err(anyhow!("Docker build failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
        log_msg(log_sender, &format!("✔️ Successfully built Docker image for tool {}", id)).await;
        let push = AsyncCommand::new("docker")
            .arg("push").arg(&tag)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let output = push.wait_with_output().await?;
        if !output.status.success() {
            return Err(anyhow!("Docker push failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
        log_msg(log_sender, &format!("✔️ Docker image was published at {tag} and is available for 1 hour in Tool {}", id)).await;
        dr.docker_pull = Some(tag);
        dr.docker_file = None;
        dr.docker_image_id = None;
    }
    Ok(())
}
pub async fn inject_docker_pull(
    tool: &mut CommandLineTool,
    log_sender: &Option<Sender<String>>,
) -> Result<()> {
    let id = tool.id.clone().unwrap();
    let command_vec = match &tool.base_command {
        Command::Multiple(vec) => vec.clone(),
        _ => return Ok(()),
    };

    let default_images = HashMap::from([
        ("python", "python"),
        ("Rscript", "r-base"),
        ("node", "node"),
    ]);

    if SCRIPT_EXECUTORS.contains(&&*command_vec[0]) {
        warn!("Tool {} is using {} and does not use a proper container", id, command_vec[0]);
        if let Some(container) = default_images.get(&&*command_vec[0]) {
            tool.requirements.push(Requirement::DockerRequirement(DockerRequirement::from_pull(container)));
            log_msg(log_sender, &format!("✔️ Added container {} to tool {}", container, id)).await;
        }
    }

    Ok(())
}

fn is_docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn has_docker_pull(tool: &CommandLineTool) -> bool {
    tool.requirements.iter().any(|req| {
        if let Requirement::DockerRequirement(docker_req) = req {
            docker_req.docker_pull.is_some()
        } else {
            false
        }
    })
}

fn adjust_basecommand(tool: &mut CommandLineTool) -> Result<()> {
    let mut changed = false;
    let mut command_vec = match &tool.base_command {
        Command::Multiple(vec) => vec.clone(),
        _ => return Ok(()),
    };

    if let Some(iwdr) = tool.get_requirement_mut::<InitialWorkDirRequirement>() {
        for item in &mut iwdr.listing {
            if let WorkDirItem::Dirent(dirent) = item
                && let Some(entryname) = &mut dirent.entryname
                && command_vec.contains(entryname)
            {
                let path = Path::new(entryname);
                if path.parent().is_some() {
                    let pos = command_vec.iter().position(|c| c == entryname)
                        .ok_or(anyhow!("Failed to find command item {entryname}"))?;
                    *entryname = path.file_name()
                        .ok_or(anyhow!("Failed to get filename from {path:?}"))?
                        .to_string_lossy()
                        .into_owned();
                    command_vec[pos] = entryname.clone();
                    changed = true;
                }
            }
        }
    }
    if changed {
        info!(
            "Basecommand of {} was modified to `{}` (see https://github.com/fairagro/sciwin/issues/114).",
            tool.id.clone().unwrap().green().bold(),
            command_vec.join(" ")
        );
        tool.base_command = Command::Multiple(command_vec);
    }
    Ok(())
}