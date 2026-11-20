use crate::cwl::resolve_filename;
use crate::interactive::{Prompt, prompt_mode};
use clap::Args;
use dialoguer::Confirm;
use ignore::WalkBuilder;
use miette::IntoDiagnostic;
use sciwin::cwl::documents::CWLDocument;
use sciwin::cwl::documents::Workflow;
use sciwin::cwl::load_cwl_file;
use sciwin::repository::Repository;
use sciwin::repository::commit;
use std::{env, fs, path::Path};
use tracing::{debug, info, warn};

#[derive(Args, Debug, Default)]
pub struct RemoveCWLArgs {
    pub file: String,
}

pub fn handle_remove_command(args: &RemoveCWLArgs) -> miette::Result<()> {
    let filename = if Path::new(&args.file).exists() {
        debug!("'{}' is a direct path, skipping name resolution", args.file);
        args.file.to_string()
    } else {
        debug!("'{}' is not a direct path, resolving it by name", args.file);
        resolve_filename(&args.file)
            .map_err(|e| miette::miette!("Could not resolve CWL File: {}", e))?
    };
    remove_cwl_file(&filename)
}

fn remove_cwl_file(filename: impl AsRef<Path>) -> miette::Result<()> {
    let filename = filename.as_ref();
    let cwd = env::current_dir().into_diagnostic()?;
    let repo = Repository::open(&cwd).into_diagnostic()?;

    if filename.exists() && filename.is_file() && filename.extension().is_some_and(|e| e == "cwl") {
        let folder = filename.parent().expect("Can not get parent dir");
        let tool_name = filename
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        debug!("scanning workflows under {} for uses of step '{tool_name}'", cwd.display());
        let workflows = check_tool_usage_in_workflows(&cwd, tool_name)?;
        if !workflows.is_empty() {
            warn!("Tool '{tool_name}' is used in the following workflows:");
            for wf in &workflows {
                warn!("{wf}");
            }
            let remove_anyway = match prompt_mode() {
                Prompt::AssumeYes => true,
                Prompt::Ask => Confirm::new()
                    .with_prompt(format!("Do you still want to remove '{tool_name}'?"))
                    .default(false)
                    .interact()
                    .into_diagnostic()?,
                Prompt::Unavailable => {
                    miette::bail!(
                        help = "pass --yes to remove it anyway",
                        "'{tool_name}' is still used in: {}",
                        workflows.join(", ")
                    );
                }
            };
            if !remove_anyway {
                info!("Aborting removal of '{}'", filename.display());
                return Ok(());
            }
        }
        fs::remove_file(filename).into_diagnostic()?;

        if folder.read_dir().into_diagnostic()?.next().is_none() {
            debug!("{} is now empty, removing it too", folder.display());
            fs::remove_dir_all(folder).into_diagnostic()?;
        }

        let message = format!("✔️  Removed CWL file: {}", filename.display());
        info!("{}", message);
        debug!("committing removal to git");
        commit(&repo, &message)?;
        Ok(())
    } else {
        warn!(
            "File {} does not exist or is not a CWL file.",
            filename.display()
        );
        miette::bail!(
            "File does not exist or is not a CWL file: {}",
            filename.display()
        );
    }
}

pub fn check_tool_usage_in_workflows(
    cwd: impl AsRef<Path>,
    tool: &str,
) -> miette::Result<Vec<String>> {
    let mut workflows = Vec::new();
    let tool_name = tool.strip_suffix(".cwl").unwrap_or(tool);
    for entry in WalkBuilder::new(cwd)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "cwl") {
            let workflow: Workflow = match load_cwl_file(path, true) {
                Ok(CWLDocument::Workflow(wf)) => wf,
                _ => continue,
            };
            if workflow
                .steps
                .iter()
                .any(|step| step.id == Some(tool_name.to_string()))
            {
                workflows.push(format!(
                    "{} ({})",
                    entry.file_name().to_string_lossy(),
                    path.display(),
                ));
            }
        }
    }
    Ok(workflows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactive::set_assume_yes;
    use fstest::fstest;

    #[fstest(repo = true)]
    fn yes_removes_a_tool_still_used_in_a_workflow() {
        let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
        fs::create_dir_all("workflows/echo").unwrap();
        fs::create_dir_all("workflows/wf").unwrap();
        fs::copy(testdata.join("echo.cwl"), "workflows/echo/echo.cwl").unwrap();
        fs::write(
            "workflows/wf/wf.cwl",
            "cwlVersion: v1.2\nclass: Workflow\ninputs: []\noutputs: []\nsteps:\n- id: echo\n  in: []\n  run: ../echo/echo.cwl\n  out: []\n",
        )
        .unwrap();

        set_assume_yes(true);
        let result = remove_cwl_file("workflows/echo/echo.cwl");
        set_assume_yes(false);

        result.unwrap();
        assert!(!Path::new("workflows/echo/echo.cwl").exists());
    }
}
