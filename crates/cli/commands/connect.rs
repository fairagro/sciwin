use crate::{cwl::resolve_filename, print_diff, print_json};
use clap::Args;
use miette::{IntoDiagnostic, miette};
use sciwin::authoring::workflow::{self, WorkflowSlot, find_step_by_run, step_id_for_tool};
use sciwin::authoring::{
    paths::{WORKFLOWS_FOLDER, get_qualified_filename_by_name},
    workflow::save_workflow,
};
use sciwin::cwl::{
    documents::{CWLDocument, Workflow},
    load_cwl_file,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::{debug, info};

#[derive(Args, Debug)]
pub struct ConnectWorkflowArgs {
    #[arg(help = "Name of the workflow to be altered, or a path to its .cwl file")]
    pub name: String,
    #[arg(
        short = 'f',
        long = "from",
        help = "Starting Node: [tool]/[output], [tool]#[output] or @inputs/[input]. [tool] is a name or a path to a .cwl file"
    )]
    pub from: String,
    #[arg(
        short = 't',
        long = "to",
        help = "Ending Node: [tool]/[input], [tool]#[input] or @outputs/[output]. [tool] is a name or a path to a .cwl file"
    )]
    pub to: String,
    #[arg(
        long = "json",
        help = "Print the result as JSON to stdout instead of a diff"
    )]
    pub json: bool,
}

/// One side of `--from`/`--to`. The slot is separated by `#` if present, else by the last `/`,
/// so the tool may be a path.
#[derive(Debug, PartialEq)]
enum Endpoint<'a> {
    /// `@inputs/id`, `@outputs/id` or a bare `id`
    Workflow(&'a str),
    /// `tool/slot` or `tool#slot`
    Step { tool: &'a str, slot: &'a str },
}

impl<'a> Endpoint<'a> {
    /// `marker` is `@inputs` for `--from` and `@outputs` for `--to`.
    fn parse(spec: &'a str, marker: &str) -> Self {
        match spec.rsplit_once('#').or_else(|| spec.rsplit_once('/')) {
            None => Self::Workflow(spec),
            Some((prefix, id)) if prefix == marker => Self::Workflow(id),
            Some((tool, slot)) => Self::Step { tool, slot },
        }
    }
}

fn has_cwl_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cwl"))
}

/// True when `tool` is a file path rather than a name to look up in the workflows folder.
fn is_path(tool: &str) -> bool {
    tool.contains(['/', '\\']) || (has_cwl_extension(tool) && Path::new(tool).is_file())
}

/// The workflow file `name` refers to: itself if it ends in `.cwl`, else
/// `workflows/<name>/<name>.cwl`.
fn workflow_filename(name: &str) -> String {
    if has_cwl_extension(name) {
        return name.to_string();
    }
    get_qualified_filename_by_name(name, Path::new(WORKFLOWS_FOLDER).join(name))
        .to_string_lossy()
        .into_owned()
}

fn create_workflow_file(name: &str, filename: &str) -> miette::Result<(PathBuf, String)> {
    if !has_cwl_extension(name) {
        return Ok(workflow::create_workflow(name, None, false)?);
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    Ok(workflow::create_workflow(&stem, Some(dir), false)?)
}

fn load_workflow(filename: &str) -> miette::Result<Workflow> {
    let CWLDocument::Workflow(workflow) = load_cwl_file(filename, true)
        .map_err(|e| miette!("Could not load workflow {filename}: {e}"))?
    else {
        miette::bail!("The specified file is not a workflow");
    };
    Ok(workflow)
}

/// A tool to wire up as step: its file and the step id to use.
struct Tool {
    path: PathBuf,
    step: String,
}

/// Resolves a path to its step (see [`step_id_for_tool`]) and a name to the file it
/// stands for, with the name as step id.
fn resolve_tool(workflow: &Workflow, workflow_path: &Path, tool: &str) -> miette::Result<Tool> {
    if is_path(tool) {
        let path = dunce::canonicalize(tool)
            .map_err(|e| miette!("Could not open CWL file {tool}: {e}"))?;
        let step = step_id_for_tool(workflow, workflow_path, &path)?;
        debug!("'{tool}' is a path, using step '{step}'");
        return Ok(Tool { path, step });
    }
    debug!("resolving '{tool}' to a CWL file path");
    let path =
        resolve_filename(tool).map_err(|e| miette!("Could not resolve CWL file {tool}: {e}"))?;
    Ok(Tool {
        path: dunce::canonicalize(path).into_diagnostic()?,
        step: tool.to_string(),
    })
}

/// Step id `tool` refers to in an existing workflow: the step running that file for a path,
/// else `tool` itself.
fn existing_step(workflow: &Workflow, workflow_path: &Path, tool: &str) -> miette::Result<String> {
    if !is_path(tool) {
        return Ok(tool.to_string());
    }
    find_step_by_run(workflow, workflow_path, tool).ok_or_else(|| {
        miette!(
            help = "pass the step name instead if the file no longer exists",
            "no step in the workflow runs {tool}"
        )
    })
}

/// What `connect --json` and `disconnect --json` print.
#[derive(Serialize)]
struct ConnectionChange<'a> {
    workflow: &'a str,
    from: &'a str,
    to: &'a str,
    /// connect only: whether the workflow file was created by this call
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<bool>,
}

/// Saves `workflow` to `filename`, then prints the JSON result or the diff.
fn save_and_report(
    args: &ConnectWorkflowArgs,
    filename: &str,
    workflow: Workflow,
    created: Option<bool>,
) -> miette::Result<()> {
    debug!("serializing and formatting updated workflow document");
    let old = fs::read_to_string(filename).into_diagnostic()?;
    let yaml = save_workflow(&CWLDocument::Workflow(workflow), Path::new(filename))?;
    info!("✔️  Updated Workflow {filename}!");
    if args.json {
        print_json(&ConnectionChange {
            workflow: filename,
            from: &args.from,
            to: &args.to,
            created,
        })
    } else {
        print_diff(&old, &yaml);
        Ok(())
    }
}

pub fn connect_workflow_nodes(args: &ConnectWorkflowArgs) -> miette::Result<()> {
    let filename = workflow_filename(&args.name);
    debug!("target workflow file: {filename}");
    let created = !Path::new(&filename).exists();
    if created {
        debug!(
            "workflow '{}' does not exist yet, creating it first",
            args.name
        );
        let (path, yaml) = create_workflow_file(&args.name, &filename)?;
        info!("📄 Created new Workflow file: {}", path.display());
        if !args.json {
            print_diff("", &yaml);
        }
    }
    // tool and workflow paths both canonical, so the step's `run` comes out relative
    let workflow_path = dunce::canonicalize(&filename).into_diagnostic()?;
    let mut workflow = load_workflow(&filename)?;

    let from = Endpoint::parse(&args.from, "@inputs");
    let to = Endpoint::parse(&args.to, "@outputs");
    debug!("parsed endpoints: from={from:?} to={to:?}");
    match (from, to) {
        (Endpoint::Workflow(input), Endpoint::Step { tool, slot }) => {
            let to_tool = resolve_tool(&workflow, &workflow_path, tool)?;
            workflow::add_workflow_input_connection(
                &mut workflow,
                &workflow_path,
                input,
                WorkflowSlot::new(&to_tool.path, &to_tool.step, slot),
            )
            .map_err(|e| {
                miette!(
                    "Could not add input connection from {input} to {}: {e}",
                    args.to
                )
            })?;
            info!(
                "➕ Added or updated connection from inputs.{input} to {} in workflow",
                args.to
            );
        }
        (Endpoint::Step { tool, slot }, Endpoint::Workflow(output)) => {
            let from_tool = resolve_tool(&workflow, &workflow_path, tool)?;
            workflow::add_workflow_output_connection(
                &mut workflow,
                &workflow_path,
                WorkflowSlot::new(&from_tool.path, &from_tool.step, slot),
                output,
            )
            .map_err(|e| {
                miette!(
                    "Could not add output connection from {} to {output}: {e}",
                    args.from
                )
            })?;
            info!(
                "➕ Added or updated connection from {} to outputs.{output} in workflow!",
                args.from
            );
        }
        (
            Endpoint::Step {
                tool: from_name,
                slot: from_slot,
            },
            Endpoint::Step {
                tool: to_name,
                slot: to_slot,
            },
        ) => {
            debug!("connecting two steps directly (step-to-step)");
            let from_tool = resolve_tool(&workflow, &workflow_path, from_name)?;
            let to_tool = resolve_tool(&workflow, &workflow_path, to_name)?;
            if from_tool.step == to_tool.step && from_tool.path != to_tool.path {
                miette::bail!(
                    help = "rename one of the two files",
                    "{from_name} and {to_name} would both become step {}",
                    from_tool.step
                );
            }
            workflow::add_workflow_step_connection(
                &mut workflow,
                &workflow_path,
                WorkflowSlot::new(&from_tool.path, &from_tool.step, from_slot),
                WorkflowSlot::new(&to_tool.path, &to_tool.step, to_slot),
            )
            .map_err(|e| {
                miette!(
                    "Could not add connection from {} to {}: {e}",
                    args.from,
                    args.to
                )
            })?;
            info!(
                "🔗 Added connection from {} to {} in workflow!",
                args.from, args.to
            );
        }
        (Endpoint::Workflow(_), Endpoint::Workflow(_)) => {
            miette::bail!(
                "Cannot connect workflow input {} directly to workflow output {}",
                args.from,
                args.to
            );
        }
    }

    save_and_report(args, &filename, workflow, Some(created))
}

pub fn disconnect_workflow_nodes(args: &ConnectWorkflowArgs) -> miette::Result<()> {
    let filename = workflow_filename(&args.name);
    debug!("target workflow file: {filename}");
    let mut workflow = load_workflow(&filename)?;
    let workflow_path = Path::new(&filename);

    let from = Endpoint::parse(&args.from, "@inputs");
    let to = Endpoint::parse(&args.to, "@outputs");
    debug!("parsed endpoints: from={from:?} to={to:?}");
    match (from, to) {
        (Endpoint::Workflow(input), Endpoint::Step { tool, slot }) => {
            let step = existing_step(&workflow, workflow_path, tool)?;
            workflow::remove_workflow_input_connection(&mut workflow, input, &step, slot, true)
                .map_err(|e| {
                    miette!(
                        "Could not remove input connection from {input} to {}: {e}",
                        args.to
                    )
                })?;
            info!(
                "➖ Removed connection from inputs.{input} to {} in workflow",
                args.to
            );
        }
        (Endpoint::Step { tool, slot }, Endpoint::Workflow(output)) => {
            let step = existing_step(&workflow, workflow_path, tool)?;
            workflow::remove_workflow_output_connection(&mut workflow, &step, slot, output, true)
                .map_err(|e| {
                miette!(
                    "Could not remove output connection from {} to {output}: {e}",
                    args.from
                )
            })?;
            info!("➖ Removed connection to {output} from workflow!");
        }
        (
            Endpoint::Step {
                tool: from_name,
                slot: from_slot,
            },
            Endpoint::Step {
                tool: to_name,
                slot: to_slot,
            },
        ) => {
            let from_step = existing_step(&workflow, workflow_path, from_name)?;
            let to_step = existing_step(&workflow, workflow_path, to_name)?;
            if !workflow.has_step(&to_step) {
                miette::bail!("Step {to_step} not found!");
            }
            workflow::remove_workflow_step_connection(
                &mut workflow,
                &from_step,
                from_slot,
                &to_step,
                to_slot,
            )
            .map_err(|e| {
                miette!(
                    "Could not remove connection from {} to {}: {e}",
                    args.from,
                    args.to
                )
            })?;
            info!(
                "➖ Removed connection from {} to {} in workflow!",
                args.from, args.to
            );
        }
        (Endpoint::Workflow(_), Endpoint::Workflow(_)) => {
            miette::bail!(
                "Use [tool]/[slot] on at least one side, got {} and {}",
                args.from,
                args.to
            );
        }
    }

    save_and_report(args, &filename, workflow, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_the_last_slash_only() {
        assert_eq!(
            Endpoint::parse("calc/results", "@inputs"),
            Endpoint::Step {
                tool: "calc",
                slot: "results"
            }
        );
        assert_eq!(
            Endpoint::parse("workflows/calc/calc.cwl/results", "@inputs"),
            Endpoint::Step {
                tool: "workflows/calc/calc.cwl",
                slot: "results"
            }
        );
        assert_eq!(
            Endpoint::parse("workflows/calc/calc.cwl#results", "@inputs"),
            Endpoint::Step {
                tool: "workflows/calc/calc.cwl",
                slot: "results"
            }
        );
        assert_eq!(
            Endpoint::parse("calc#results", "@inputs"),
            Endpoint::Step {
                tool: "calc",
                slot: "results"
            }
        );
        assert_eq!(
            Endpoint::parse("@inputs/data", "@inputs"),
            Endpoint::Workflow("data")
        );
        assert_eq!(
            Endpoint::parse("data", "@outputs"),
            Endpoint::Workflow("data")
        );
    }

    #[test]
    fn names_are_not_paths() {
        assert!(!is_path("calc"));
        assert!(is_path("workflows/calc/calc.cwl"));
        assert!(is_path(r"workflows\calc\calc.cwl"));
    }
}
