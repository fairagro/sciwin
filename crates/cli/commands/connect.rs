use crate::{
    commands::{CreateArgs, create_workflow},
    cwl::resolve_filename,
    print_diff,
};
use clap::Args;
use miette::{IntoDiagnostic, miette};
use sciwin::authoring::paths::{WORKFLOWS_FOLDER, get_qualified_filename_by_name};
use sciwin::authoring::workflow::{self, WorkflowSlot};
use sciwin::cwl::{documents::CWLDocument, format::format_cwl, load_cwl_file};
use std::{fs, io::Write, path::Path};
use tracing::{debug, info};

fn workflow_filename(name: &str) -> String {
    get_qualified_filename_by_name(name, Path::new(WORKFLOWS_FOLDER).join(name))
        .to_string_lossy()
        .into_owned()
}

fn resolve(cwl_filename: &str) -> miette::Result<String> {
    debug!("resolving '{cwl_filename}' to a CWL file path");
    resolve_filename(cwl_filename).map_err(|e| miette!("Could not resolve CWL file {cwl_filename}: {e}"))
}

#[derive(Args, Debug)]
pub struct ConnectWorkflowArgs {
    #[arg(help = "Name of the workflow name to be altered")]
    pub name: String,
    #[arg(short = 'f', long = "from", help = "Starting Node: [tool]/[output]")]
    pub from: String,
    #[arg(short = 't', long = "to", help = "Ending Node: [tool]/[input]")]
    pub to: String,
}

pub fn connect_workflow_nodes(args: &ConnectWorkflowArgs) -> miette::Result<()> {
    //get workflow
    let filename = workflow_filename(&args.name);
    debug!("target workflow file: {filename}");
    if !Path::new(&filename).exists() {
        debug!("workflow '{}' does not exist yet, creating it first", args.name);
        let args = CreateArgs {
            name: Some(args.name.clone()),
            ..Default::default()
        };
        create_workflow(&args)?;
    }
    let workflow_path = Path::new(&filename);
    let CWLDocument::Workflow(mut workflow) = load_cwl_file(&filename, true)
        .map_err(|e| miette!("Could not load workflow {filename}: {e}"))?
    else {
        miette::bail!("The specified file is not a workflow");
    };

    let from_parts = args.from.split('/').collect::<Vec<_>>();
    let to_parts = args.to.split('/').collect::<Vec<_>>();
    debug!("parsed endpoints: from={from_parts:?} to={to_parts:?}");
    if from_parts[0] == "@inputs" || from_parts.len() == 1 {
        debug!("'from' side is a workflow input, not a step output");
        let input = match from_parts.as_slice() {
            ["@inputs", input] => input,
            [input] => input,
            _ => miette::bail!("Invalid input path"),
        };

        let to_filename = resolve(to_parts[0])?;
        workflow::add_workflow_input_connection(
            &mut workflow,
            workflow_path,
            input,
            WorkflowSlot::new(Path::new(&to_filename), to_parts[0], to_parts[1]),
        )
        .map_err(|e| {
            miette!(
                "Could not add input connection from {} to {}: {e}",
                input,
                args.to
            )
        })?;
        info!(
            "➕ Added or updated connection from inputs.{input} to {} in workflow",
            args.to
        );
    } else if to_parts[0] == "@outputs" || to_parts.len() == 1 {
        debug!("'to' side is a workflow output, not a step input");
        let output = match to_parts.as_slice() {
            ["@outputs", output] => output,
            [output] => output,
            _ => miette::bail!("Invalid output path"),
        };

        let from_filename = resolve(from_parts[0])?;
        workflow::add_workflow_output_connection(
            &mut workflow,
            workflow_path,
            WorkflowSlot::new(Path::new(&from_filename), from_parts[0], from_parts[1]),
            output,
        )
        .map_err(|e| {
            miette!(
                "Could not add output connection from {} to {}: {e}",
                args.from,
                output
            )
        })?;
        info!(
            "➕ Added or updated connection from {} to outputs.{output} in workflow!",
            args.from
        );
    } else {
        debug!("connecting two steps directly (step-to-step)");
        let from_filename = resolve(from_parts[0])?;
        let to_filename = resolve(to_parts[0])?;
        workflow::add_workflow_step_connection(
            &mut workflow,
            workflow_path,
            WorkflowSlot::new(Path::new(&from_filename), from_parts[0], from_parts[1]),
            WorkflowSlot::new(Path::new(&to_filename), to_parts[0], to_parts[1]),
        )
        .map_err(|e| {
            miette!(
                "Could not add connection from {} to {}:: {e}",
                args.from,
                args.to
            )
        })?;
        info!("🔗 Added connection from {} to {} in workflow!", args.from, args.to);
    }

    //save workflow
    debug!("serializing and formatting updated workflow document");
    let mut yaml = serde_saphyr::to_string(&CWLDocument::Workflow(workflow)).into_diagnostic()?;
    yaml = format_cwl(&yaml).map_err(|e| miette!("Could not format yaml: {e}"))?;
    let old = fs::read_to_string(&filename).into_diagnostic()?;
    let mut file = fs::File::create(&filename).into_diagnostic()?;
    file.write_all(yaml.as_bytes()).into_diagnostic()?;
    info!("✔️  Updated Workflow {filename}!");
    print_diff(&old, &yaml);

    Ok(())
}

pub fn disconnect_workflow_nodes(args: &ConnectWorkflowArgs) -> miette::Result<()> {
    let filename = workflow_filename(&args.name);
    debug!("target workflow file: {filename}");
    let CWLDocument::Workflow(mut workflow) = load_cwl_file(&filename, true)
        .map_err(|e| miette!("Could not load workflow {filename}: {e}"))?
    else {
        miette::bail!("The specified file is not a workflow");
    };

    let from_parts = args.from.split('/').collect::<Vec<_>>();
    let to_parts = args.to.split('/').collect::<Vec<_>>();
    debug!("parsed endpoints: from={from_parts:?} to={to_parts:?}");

    if from_parts[0] == "@inputs" || from_parts.len() == 1 {
        let input = match from_parts.as_slice() {
            ["@inputs", input] => input,
            [input] => input,
            _ => miette::bail!("Invalid input path"),
        };
        if to_parts.len() != 2 {
            miette::bail!("Invalid 'to' format for input connection: {input} to:{}", args.to);
        }

        workflow::remove_workflow_input_connection(
            &mut workflow,
            input,
            to_parts[0],
            to_parts[1],
            true,
        )
        .map_err(|e| {
            miette!(
                "Could not remove input connection from {} to {}: {e}",
                input,
                args.to
            )
        })?;
        info!("➖ Removed connection from inputs.{input} to {} in workflow", args.to);
    } else if to_parts[0] == "@outputs" || to_parts.len() == 1 {
        let output = match to_parts.as_slice() {
            ["@outputs", output] => output,
            [output] => output,
            _ => miette::bail!("Invalid output path"),
        };

        workflow::remove_workflow_output_connection(&mut workflow, output, true).map_err(|e| {
            miette!(
                "Could not remove output connection from {} to {}: {e}",
                args.from,
                output
            )
        })?;
        info!("➖ Removed connection to {output} from workflow!");
    } else {
        if from_parts.len() != 2 {
            miette::bail!(
                "Invalid '--from' format: {}. Please use tool/parameter or @inputs/parameter.",
                args.from
            );
        }
        if to_parts.len() != 2 {
            miette::bail!(
                "Invalid '--to' format: {}. Please use tool/parameter or @outputs/parameter.",
                args.to
            );
        }
        if !workflow.has_step(to_parts[0]) {
            miette::bail!("Step {} not found!", to_parts[0]);
        }

        workflow::remove_workflow_step_connection(&mut workflow, to_parts[0], to_parts[1])
            .map_err(|e| {
                miette!(
                    "Could not remove connection from {} to {}:: {e}",
                    args.from,
                    args.to
                )
            })?;
        info!("➖ Removed connection from {} to {} in workflow!", args.from, args.to);
    }

    debug!("serializing and formatting updated workflow document");
    let mut yaml = serde_saphyr::to_string(&CWLDocument::Workflow(workflow)).into_diagnostic()?;
    yaml = format_cwl(&yaml).map_err(|e| miette!("Could not format yaml: {e}"))?;
    let old = fs::read_to_string(&filename).into_diagnostic()?;
    let mut file = fs::File::create(&filename).into_diagnostic()?;
    file.write_all(yaml.as_bytes()).into_diagnostic()?;
    info!("✔️  Updated Workflow {filename}!");
    print_diff(&old, &yaml);

    Ok(())
}
