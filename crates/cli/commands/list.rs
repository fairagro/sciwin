use crate::print_json;
use clap::Args;
use colored::Colorize;
use ignore::WalkBuilder;
use miette::IntoDiagnostic;
use prettytable::{Cell, Row, Table, row};
use sciwin::authoring::{
    summary::{DocumentSummary, summarize_dir, summarize_file},
    workflow::{SlotState, SlotStatus, connection_status},
};
use sciwin::cwl::{
    Identifiable, docstring,
    documents::{CWLDocument, CommandLineTool, ExpressionTool, Workflow},
    load_cwl_file,
    requirements::DockerRequirement,
};
use std::{
    env,
    fs::FileType,
    path::{Path, PathBuf},
};
use tracing::{debug, info};

#[derive(Args, Debug, Default)]
pub struct ListCWLArgs {
    pub file: Option<PathBuf>,
    #[arg(
        short = 'a',
        long = "all",
        help = "Outputs the tools with inputs and outputs"
    )]
    pub list_all: bool,
    #[arg(
        long = "json",
        help = "Print JSON to stdout, always including inputs and outputs (and connection status for a single workflow)"
    )]
    pub json: bool,
}

pub fn handle_list_command(args: &ListCWLArgs) -> miette::Result<()> {
    let target = match &args.file {
        Some(file) => {
            debug!(
                "explicit target given: {file:?} (is_file={}, is_dir={})",
                file.is_file(),
                file.is_dir()
            );
            file.clone()
        }
        None => {
            debug!("no target given, walking current directory instead");
            env::current_dir().into_diagnostic()?
        }
    };
    match (target.is_file(), target.is_dir(), args.json) {
        (true, _, true) => print_json(&relative_to_cwd(vec![summarize_file(&target)?])?[0]),
        (true, _, false) => list_single_cwl(&target),
        (_, true, true) => print_json(&relative_to_cwd(summarize_dir(&target)?)?),
        (_, true, false) => list_multiple(&target, args.list_all),
        _ => {
            miette::bail!("No such file or directory: {}", target.display());
        }
    }
}

/// Shortens each summary's path to be relative to the working directory where it is below it.
fn relative_to_cwd(mut summaries: Vec<DocumentSummary>) -> miette::Result<Vec<DocumentSummary>> {
    let cwd = env::current_dir().into_diagnostic()?;
    for summary in &mut summaries {
        if let Ok(relative) = summary.path.strip_prefix(&cwd) {
            summary.path = relative.to_path_buf();
        }
    }
    Ok(summaries)
}

fn list_single_cwl(filename: impl AsRef<Path>) -> miette::Result<()> {
    let filename = filename.as_ref();
    if !filename.exists() {
        info!("Tool does not exist: {}", filename.display());
        return Ok(()); //we are okay with the non existance here!
    }
    debug!("parsing CWL document at {}", filename.display());

    let tool = load_cwl_file(filename, true)
        .map_err(|e| miette::miette!("Could not load CWL File: {e}"))?;
    match tool {
        CWLDocument::CommandLineTool(clt) => list_clt(&clt, filename),
        CWLDocument::ExpressionTool(et) => list_et(&et, filename),
        CWLDocument::Workflow(wf) => list_wf(&wf, filename),
        CWLDocument::Operation(_) => {
            info!(
                "Operation found: `{}`",
                filename.to_string_lossy().blue().bold()
            );
            Ok(())
        }
    }?;

    Ok(())
}

pub(crate) fn list_multiple(cwd: impl AsRef<Path>, detailed: bool) -> miette::Result<()> {
    info!(
        "📂 Available CWL Files in: {}",
        cwd.as_ref().to_string_lossy().blue().bold()
    );
    debug!("walking directory tree, respecting .gitignore (detailed={detailed})");

    // Create a table
    let mut table = Table::new();

    // Add table headers
    table.add_row(Row::new(vec![
        Cell::new("Tool").style_spec("bFg"),
        Cell::new("Inputs").style_spec("bFg"),
        Cell::new("Outputs").style_spec("bFg"),
    ]));

    // Walk recursively through all directories and subdirectories
    for entry in WalkBuilder::new(cwd)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .build()
        .filter_map(Result::ok)
    {
        if entry
            .file_type()
            .is_some_and(|ft: FileType| FileType::is_file(&ft))
        {
            let file_name = entry.file_name().to_string_lossy();

            // Only process .cwl files
            if let Some(tool_name) = file_name.strip_suffix(".cwl") {
                let mut inputs_list = Vec::new();
                let mut outputs_list = Vec::new();

                // Read the contents of the file
                let file_path = entry.path();
                debug!("found CWL file: {}", file_path.display());
                let folder = entry.path().parent().unwrap_or_else(|| Path::new("."));
                // Parse content
                if let Ok(doc) = load_cwl_file(file_path, true) {
                    if detailed {
                        // Extract inputs
                        for input in &doc.get_inputs() {
                            inputs_list.push(format!("{tool_name}/{}", input.id.as_ref().unwrap()));
                        }
                        // Extract outputs
                        for id in doc.get_output_ids() {
                            outputs_list.push(format!("{tool_name}/{id}"));
                        }

                        // add row to the table
                        table.add_row(Row::new(vec![
                            Cell::new(&format!("{} ({})", tool_name, doc.get_class()))
                                .style_spec("bFg"),
                            Cell::new(&inputs_list.join(", ")),
                            Cell::new(&outputs_list.join(", ")),
                        ]));
                    } else {
                        // Print only the tool name if not all details
                        info!(
                            "📄 {} ({}) in {}",
                            tool_name.green().bold(),
                            doc.get_class(),
                            folder.to_string_lossy()
                        );
                    }
                }
            }
        }
    }
    // Print the table
    if detailed {
        table.printstd();
    }
    Ok(())
}

fn list_clt(clt: &CommandLineTool, filename: &Path) -> miette::Result<()> {
    info!(
        "🔎 CommandLineTool: `{}`",
        filename.to_string_lossy().blue().bold()
    );
    print_line();

    if let Some(cmd) = &clt.base_command {
        info!("Basecommand: \t{}", cmd.to_string().bold().green());
    }
    list_base(&CWLDocument::CommandLineTool(clt.clone()), false);

    info!("{}", "Inputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("ID").style_spec("bFg"),
        Cell::new("Type").style_spec("bFg"),
        Cell::new("glob").style_spec("bFg"),
    ]));

    for input in &clt.inputs {
        let binding = if let Some(b) = &input.input_binding {
            let prefix = b
                .prefix
                .as_ref()
                .map_or("not set".to_string(), |p| p.to_string());
            let position = b
                .position
                .as_ref()
                .map_or("not set".to_string(), |p| p.to_string());
            format!("Prefix: {}; Position: {}", prefix, position)
        } else {
            "No Binding".into()
        };

        table.add_row(Row::new(vec![
            Cell::new(input.id.as_ref().unwrap()),
            Cell::new(&format!("{:?}", input.r#type)),
            Cell::new(&binding),
            Cell::new(
                &input
                    .default
                    .as_ref()
                    .map_or("None".to_string(), |d| d.to_string()),
            ),
        ]));
    }

    info!("{}", "Outputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("ID").style_spec("bFg"),
        Cell::new("Type").style_spec("bFg"),
        Cell::new("glob").style_spec("bFg"),
    ]));

    for output in &clt.outputs {
        let binding = if let Some(b) = &output.output_binding {
            b.glob
                .as_ref()
                .map_or("not set".to_string(), |g| g.to_string())
        } else {
            "No glob".into()
        };
        table.add_row(Row::new(vec![
            Cell::new(output.id.as_ref().unwrap()),
            Cell::new(&format!("{:?}", output.r#type)),
            Cell::new(&binding),
        ]));
    }
    table.printstd();
    print_line();

    Ok(())
}

fn list_et(et: &ExpressionTool, filename: &Path) -> miette::Result<()> {
    info!(
        "🔎 ExpressionTool: `{}`",
        filename.to_string_lossy().blue().bold()
    );
    print_line();

    info!("Expression: \t{}", et.expression.bold().green());
    list_base(&CWLDocument::ExpressionTool(et.clone()), false);

    info!("{}", "Inputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("ID").style_spec("bFg"),
        Cell::new("Type").style_spec("bFg"),
        Cell::new("glob").style_spec("bFg"),
    ]));

    for input in &et.inputs {
        table.add_row(Row::new(vec![
            Cell::new(input.id.as_ref().unwrap()),
            Cell::new(&format!("{:?}", input.r#type)),
            Cell::new(
                &input
                    .default
                    .as_ref()
                    .map_or("None".to_string(), |d| d.to_string()),
            ),
        ]));
    }

    info!("{}", "Outputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("ID").style_spec("bFg"),
        Cell::new("Type").style_spec("bFg"),
    ]));

    for output in &et.outputs {
        table.add_row(Row::new(vec![
            Cell::new(output.id.as_ref().unwrap()),
            Cell::new(&format!("{:?}", output.r#type)),
        ]));
    }
    table.printstd();
    print_line();

    Ok(())
}

fn list_wf(wf: &Workflow, filename: &Path) -> miette::Result<()> {
    info!(
        "🔎 Workflow: `{}`",
        filename.to_string_lossy().blue().bold()
    );
    print_line();
    list_base(&CWLDocument::Workflow(wf.clone()), true);

    workflow_status(wf, filename)?;
    Ok(())
}

fn list_base(base: &CWLDocument, is_workflow: bool) {
    if let Some(id) = &base.get_id() {
        info!("ID:\t\t{}", id);
    }

    if let Some(label) = &base.get_label() {
        info!("Label:\t\t{}", label);
    }

    if let Some(cwl_version) = &base.cwl_version() {
        info!("CWL Version:\t{}", cwl_version);
    }

    if let Some(dr) = base.get_requirement::<DockerRequirement>() {
        if let Some(image) = &dr.docker_pull {
            info!("Docker Image:\t{}", image);
        }
        if dr.docker_file.is_some() {
            info!("Docker Image:\tLocal Dockerfile");
        }
    }

    print_line();
    if let Some(doc) = base.get_doc() {
        info!("{}", "Summary:".bold());
        info!("{}", docstring(doc.clone()));
        print_line();
    }

    if is_workflow {
        return;
    }

    info!("{}", "Inputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("ID").style_spec("bFg"),
        Cell::new("Type").style_spec("bFg"),
        Cell::new("Binding").style_spec("bFg"),
        Cell::new("Default").style_spec("bFg"),
    ]));

    table.printstd();
    print_line();
}

fn workflow_status(wf: &Workflow, filename: &Path) -> miette::Result<()> {
    info!("Connection status:");
    let status = connection_status(wf, filename)?;
    let mut table = Table::new();
    table.set_titles(row![bFg => "Tool", "Inputs", "Outputs"]);

    let (inputs, outputs) = (slot_lines(&status.inputs), slot_lines(&status.outputs));
    table.add_row(row![b -> "<Workflow>", inputs, outputs]);
    table.add_row(row![b -> "Steps:"]);
    for step in &status.steps {
        let run = step.run.as_deref().unwrap_or("Inline Document");
        let (inputs, outputs) = (slot_lines(&step.inputs), slot_lines(&step.outputs));
        table.add_row(row![b -> run, inputs, outputs]);
    }

    table.printstd();

    info!("✅ : connected - 🔘 : tool default - ❌ : no connection");
    Ok(())
}

fn slot_lines(slots: &[SlotState]) -> String {
    slots
        .iter()
        .map(|slot| {
            let icon = match slot.status {
                SlotStatus::Connected => "✅",
                SlotStatus::Default => "🔘",
                SlotStatus::Unconnected => "❌",
            };
            format!("{icon}    {}", slot.id)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn print_line() {
    info!(
        "{}",
        "_________________________________________________________________".bold()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_target_is_an_error() {
        let args = ListCWLArgs {
            file: Some(PathBuf::from("does/not/exist.cwl")),
            ..Default::default()
        };
        assert!(handle_list_command(&args).is_err());
    }
}
