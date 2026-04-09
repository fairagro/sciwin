use crate::cwl::resolve_filename;
use clap::Args;
use commonwl::documents::CWLDocument;
use commonwl::load_cwl_file;
use dialoguer::Confirm;
use ignore::WalkBuilder;
use log::{info, warn};
use repository::Repository;
use repository::commit;
use std::{env, fs, path::Path};

#[derive(Args, Debug, Default)]
pub struct RemoveCWLArgs {
    pub file: String,
}

pub fn handle_remove_command(args: &RemoveCWLArgs) -> anyhow::Result<()> {
    let filename = if Path::new(&args.file).exists() {
        args.file.to_string()
    } else {
        resolve_filename(&args.file)
            .map_err(|e| anyhow::anyhow!("Could not resolve CWL File: {}", e))?
    };
    remove_cwl_file(&filename)
}

fn remove_cwl_file(filename: impl AsRef<Path>) -> anyhow::Result<()> {
    let filename = filename.as_ref();
    let cwd = env::current_dir()?;
    let repo = Repository::open(&cwd)?;

    if filename.exists() && filename.is_file() && filename.extension().is_some_and(|e| e == "cwl") {
        let folder = filename.parent().expect("Can not get parent dir");
        let tool_name = filename
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let workflows = check_tool_usage_in_workflows(&cwd, tool_name)?;
        if !workflows.is_empty() {
            warn!("Tool '{tool_name}' is used in the following workflows:");
            for wf in &workflows {
                warn!("{wf}");
            }
            let remove_anyway = Confirm::new()
                .with_prompt(format!("Do you still want to remove '{tool_name}'?"))
                .default(false)
                .interact()?;
            if !remove_anyway {
                info!("Aborting removal of '{}'", filename.display());
                return Ok(());
            }
        }
        fs::remove_file(filename)?;

        if folder.read_dir()?.next().is_none() {
            fs::remove_dir_all(folder)?;
        }

        let message = format!("✔️  Removed CWL file: {}", filename.display());
        info!("{}", message);
        commit(&repo, &message)?;
        Ok(())
    } else {
        warn!(
            "File {} does not exist or is not a CWL file.",
            filename.display()
        );
        anyhow::bail!(
            "File does not exist or is not a CWL file: {}",
            filename.display()
        );
    }
}

pub fn check_tool_usage_in_workflows(
    cwd: impl AsRef<Path>,
    tool: &str,
) -> anyhow::Result<Vec<String>> {
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
