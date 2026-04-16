use crate::{
    commands::{CreateArgs, create_workflow},
    cwl::Connectable,
    print_diff,
};
use anyhow::anyhow;
use clap::Args;
use commonwl::{format::format_cwl, load_workflow};
use log::info;
use s4n_core::io::get_workflows_folder;
use std::{fs, io::Write, path::Path};

#[derive(Args, Debug)]
pub struct ConnectWorkflowArgs {
    #[arg(help = "Name of the workflow name to be altered")]
    pub name: String,
    #[arg(short = 'f', long = "from", help = "Starting Node: [tool]/[output]")]
    pub from: String,
    #[arg(short = 't', long = "to", help = "Ending Node: [tool]/[input]")]
    pub to: String,
}

pub fn connect_workflow_nodes(args: &ConnectWorkflowArgs) -> anyhow::Result<()> {
    //get workflow
    let filename = format!("{}{}/{}.cwl", get_workflows_folder(), args.name, args.name);
    if !Path::new(&filename).exists() {
        let args = CreateArgs {
            name: Some(args.name.clone()),
            ..Default::default()
        };
        create_workflow(&args)?;
    }
    let mut workflow = load_workflow(&filename).map_err(|e| anyhow!("Could not load workflow {filename}: {e}"))?;

    let from_parts = args.from.split('/').collect::<Vec<_>>();
    let to_parts = args.to.split('/').collect::<Vec<_>>();
    if from_parts[0] == "@inputs" || from_parts.len() == 1 {
        let input = match from_parts.as_slice() {
            ["@inputs", input] => input,
            [input] => input,
            _ => anyhow::bail!("Invalid input path"),
        };

        workflow
            .add_input_connection(input, &args.to)
            .map_err(|e| anyhow!("Could not add input connection from {} to {}: {e}", input, args.to))?;
    } else if to_parts[0] == "@outputs" || to_parts.len() == 1 {
        let output = match to_parts.as_slice() {
            ["@outputs", output] => output,
            [output] => output,
            _ => anyhow::bail!("Invalid output path"),
        };

        workflow
            .add_output_connection(&args.from, output)
            .map_err(|e| anyhow!("Could not add output connection from {} to {}: {e}", args.from, output))?;
    } else {
        workflow
            .add_step_connection(&args.from, &args.to)
            .map_err(|e| anyhow!("Could not add connection from {} to {}:: {e}", args.from, args.to))?;
    }

    //save workflow
    let mut yaml = serde_yaml::to_string(&workflow)?;
    yaml = format_cwl(&yaml).map_err(|e| anyhow!("Could not format yaml: {e}"))?;
    let old = fs::read_to_string(&filename)?;
    let mut file = fs::File::create(&filename)?;
    file.write_all(yaml.as_bytes())?;
    info!("✔️  Updated Workflow {filename}!");
    print_diff(&old, &yaml);

    Ok(())
}

pub fn disconnect_workflow_nodes(args: &ConnectWorkflowArgs) -> anyhow::Result<()> {
    let filename = format!("{}{}/{}.cwl", get_workflows_folder(), args.name, args.name);
    let mut workflow = load_workflow(&filename).map_err(|e| anyhow!("Could not load workflow {filename}: {e}"))?;

    let from_parts = args.from.split('/').collect::<Vec<_>>();
    let to_parts = args.to.split('/').collect::<Vec<_>>();

    if from_parts[0] == "@inputs" || from_parts.len() == 1 {
        let input = match from_parts.as_slice() {
            ["@inputs", input] => input,
            [input] => input,
            _ => anyhow::bail!("Invalid input path"),
        };

        workflow
            .remove_input_connection(input, &args.to)
            .map_err(|e| anyhow!("Could not remove input connection from {} to {}: {e}", input, args.to))?;
    } else if to_parts[0] == "@outputs" || to_parts.len() == 1 {
        let output = match to_parts.as_slice() {
            ["@outputs", output] => output,
            [output] => output,
            _ => anyhow::bail!("Invalid output path"),
        };

        workflow
            .remove_output_connection(&args.from, output)
            .map_err(|e| anyhow!("Could not remove output connection from {} to {}: {e}", args.from, output))?;
    } else {
        workflow
            .remove_step_connection(&args.from, &args.to)
            .map_err(|e| anyhow!("Could not remove connection from {} to {}:: {e}", args.from, args.to))?;
    }

    let mut yaml = serde_yaml::to_string(&workflow)?;
    yaml = format_cwl(&yaml).map_err(|e| anyhow!("Could not format yaml: {e}"))?;
    let old = fs::read_to_string(&filename)?;
    let mut file = fs::File::create(&filename)?;
    file.write_all(yaml.as_bytes())?;
    info!("✔️  Updated Workflow {filename}!");
    print_diff(&old, &yaml);

    Ok(())
}
