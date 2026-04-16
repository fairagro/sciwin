use clap::Args;
use colored::Colorize;
use commonwl::{DocumentBase, StringOrDocument, load_doc, load_tool, prelude::*};
use ignore::WalkBuilder;
use log::info;
use prettytable::{Cell, Row, Table, row};
use std::{
    env,
    fs::FileType,
    path::{Path, PathBuf},
};

#[derive(Args, Debug, Default)]
pub struct ListCWLArgs {
    pub file: Option<PathBuf>,
    #[arg(short = 'a', long = "all", help = "Outputs the tools with inputs and outputs")]
    pub list_all: bool,
}

pub fn handle_list_command(args: &ListCWLArgs) -> anyhow::Result<()> {
    if let Some(file) = &args.file {
        if file.exists() && file.is_file() {
            list_single_cwl(file)?;
        } else if file.is_dir() {
            list_multiple(file, args.list_all)?;
        }
    } else {
        list_multiple(env::current_dir()?, args.list_all)?;
    }
    Ok(())
}

fn list_single_cwl(filename: impl AsRef<Path>) -> anyhow::Result<()> {
    let filename = filename.as_ref();
    if !filename.exists() {
        info!("Tool does not exist: {}", filename.display());
        return Ok(()); //we are okay with the non existance here!
    }

    let tool = load_doc(filename).map_err(|e| anyhow::anyhow!("Could not load CWL File: {e}"))?;
    match tool {
        CWLDocument::CommandLineTool(clt) => list_clt(&clt, filename),
        CWLDocument::ExpressionTool(et) => list_et(&et, filename),
        CWLDocument::Workflow(wf) => list_wf(&wf, filename),
    }?;

    Ok(())
}

pub(crate) fn list_multiple(cwd: impl AsRef<Path>, detailed: bool) -> anyhow::Result<()> {
    info!("📂 Available CWL Files in: {}", cwd.as_ref().to_string_lossy().blue().bold());

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
        if entry.file_type().is_some_and(|ft: FileType| FileType::is_file(&ft)) {
            let file_name = entry.file_name().to_string_lossy();

            // Only process .cwl files
            if let Some(tool_name) = file_name.strip_suffix(".cwl") {
                let mut inputs_list = Vec::new();
                let mut outputs_list = Vec::new();

                // Read the contents of the file
                let file_path = entry.path();
                let folder = entry.path().parent().unwrap_or_else(|| Path::new("."));
                // Parse content
                if let Ok(doc) = load_doc(file_path) {
                    if detailed {
                        // Extract inputs
                        for input in &doc.inputs {
                            inputs_list.push(format!("{tool_name}/{}", input.id));
                        }
                        // Extract outputs
                        for id in doc.get_output_ids() {
                            outputs_list.push(format!("{tool_name}/{id}"));
                        }

                        // add row to the table
                        table.add_row(Row::new(vec![
                            Cell::new(&format!("{} ({})", tool_name, doc.class)).style_spec("bFg"),
                            Cell::new(&inputs_list.join(", ")),
                            Cell::new(&outputs_list.join(", ")),
                        ]));
                    } else {
                        // Print only the tool name if not all details
                        info!("📄 {} ({}) in {}", tool_name.green().bold(), doc.class, folder.to_string_lossy());
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

fn list_clt(clt: &CommandLineTool, filename: &Path) -> anyhow::Result<()> {
    info!("🔎 CommandLineTool: `{}`", filename.to_string_lossy().blue().bold());
    print_line();

    info!("Basecommand: \t{}", clt.base_command.to_string().bold().green());
    list_base(&clt.base, false);

    info!("{}", "Outputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("ID").style_spec("bFg"),
        Cell::new("Type").style_spec("bFg"),
        Cell::new("glob").style_spec("bFg"),
    ]));

    for output in &clt.outputs {
        let binding = if let Some(b) = &output.output_binding {
            b.glob.as_ref().map_or("not set".to_string(), |g| g.to_string())
        } else {
            "No glob".into()
        };
        table.add_row(Row::new(vec![
            Cell::new(&output.id),
            Cell::new(&output.type_.to_string()),
            Cell::new(&binding),
        ]));
    }
    table.printstd();
    print_line();

    Ok(())
}

fn list_et(et: &ExpressionTool, filename: &Path) -> anyhow::Result<()> {
    info!("🔎 ExpressionTool: `{}`", filename.to_string_lossy().blue().bold());
    print_line();

    info!("Expression: \t{}", et.expression.bold().green());
    list_base(&et.base, false);

    info!("{}", "Outputs:".bold());

    let mut table = Table::new();
    table.add_row(Row::new(vec![Cell::new("ID").style_spec("bFg"), Cell::new("Type").style_spec("bFg")]));

    for output in &et.outputs {
        table.add_row(Row::new(vec![Cell::new(&output.id), Cell::new(&output.type_.to_string())]));
    }
    table.printstd();
    print_line();

    Ok(())
}

fn list_wf(wf: &Workflow, filename: &Path) -> anyhow::Result<()> {
    info!("🔎 Workflow: `{}`", filename.to_string_lossy().blue().bold());
    print_line();
    list_base(&wf.base, true);

    workflow_status(wf, filename)?;
    Ok(())
}

fn list_base(base: &DocumentBase, is_workflow: bool) {
    if let Some(id) = &base.id {
        info!("ID:\t\t{}", id);
    }

    if let Some(label) = &base.label {
        info!("Label:\t\t{}", label);
    }

    if let Some(cwl_version) = &base.cwl_version {
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
    if let Some(doc) = &base.doc {
        info!("{}", "Summary:".bold());
        info!("{}", doc);
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

    for input in &base.inputs {
        let binding = if let Some(b) = &input.input_binding {
            let prefix = b.prefix.as_ref().map_or("not set".to_string(), |p| p.to_string());
            let position = b.position.map_or("not set".to_string(), |p| p.to_string());
            format!("Prefix: {}; Position: {}", prefix, position)
        } else {
            "No Binding".into()
        };

        table.add_row(Row::new(vec![
            Cell::new(&input.id),
            Cell::new(&input.type_.to_string()),
            Cell::new(&binding),
            Cell::new(&input.default.as_ref().map_or("None".to_string(), |d| d.as_value_string())),
        ]));
    }
    table.printstd();
    print_line();
}

fn workflow_status(wf: &Workflow, filename: &Path) -> anyhow::Result<()> {
    info!("Connection status:");
    let path = Path::new(&filename).parent().unwrap_or(Path::new("."));
    let mut table = Table::new();
    table.set_titles(row![bFg => "Tool", "Inputs", "Outputs"]);

    //check if workflow inputs are all connected
    let input_status = wf
        .inputs
        .iter()
        .map(|input| {
            if wf.has_step_input(&input.id) {
                format!("✅    {}", input.id)
            } else if input.default.is_some() {
                format!("🔘    {}", input.id)
            } else {
                format!("❌    {}", input.id)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    //check if workflow outputs are all connected
    let output_status = wf
        .outputs
        .iter()
        .map(|output| {
            if wf.has_step_output(&output.output_source.clone().unwrap_or_default()) {
                format!("✅    {}", output.id)
            } else {
                format!("❌    {}", output.id)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    table.add_row(row![b -> "<Workflow>", input_status, output_status]);
    table.add_row(row![b -> "Steps:"]);

    for step in &wf.steps {
        let tool = match &step.run {
            StringOrDocument::String(run) => {
                load_tool(path.join(run)).map_err(|e| anyhow::anyhow!("Could not load tool {:?}: {e}", path.join(run)))?
            }
            StringOrDocument::Document(boxed_doc) => match &**boxed_doc {
                CWLDocument::CommandLineTool(doc) => doc.clone(),
                _ => unreachable!(), //see #95
            },
        };
        let input_status = tool
            .inputs
            .iter()
            .map(|input| {
                if step.in_.iter().any(|i| i.id == input.id) {
                    format!("✅    {}", input.id)
                } else if input.default.is_some() {
                    format!("🔘    {}", input.id)
                } else {
                    format!("❌    {}", input.id)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let output_status = tool
            .outputs
            .iter()
            .map(|output| {
                if wf
                    .steps
                    .iter()
                    .any(|s| s.in_.clone().iter().any(|v| v.source == Some(format!("{}/{}", step.id, output.id))))
                    || wf.outputs.iter().any(|o| o.output_source == Some(format!("{}/{}", step.id, output.id)))
                {
                    format!("✅    {}", output.id)
                } else {
                    format!("❌    {}", output.id)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let run = if let StringOrDocument::String(run) = &step.run {
            run
        } else {
            &String::from("Inline Document")
        };
        table.add_row(row![b -> run, &input_status, &output_status]);
    }

    table.printstd();

    info!("✅ : connected - 🔘 : tool default - ❌ : no connection");
    Ok(())
}

fn print_line() {
    info!("{}", "_________________________________________________________________".bold());
}
