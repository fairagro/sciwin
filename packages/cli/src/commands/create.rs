use crate::{cwl::highlight_cwl, print_diff, print_list};
use anyhow::{anyhow, bail};
use clap::Args;
use colored::Colorize;
use commonwl::CommandLineTool;
use log::{info, warn};
use s4n_core::{
    io::{get_qualified_filename, get_workflows_folder},
    tool::ToolCreationOptions,
};
use std::{path::PathBuf, str::FromStr};
use commonwl::execution::docker::ContainerEngine;

pub fn handle_create_command(args: &CreateArgs) -> anyhow::Result<()> {
    if args.command.is_empty() && args.name.is_some() {
        info!("ℹ️  Workflow creation is optional. Creation will be triggered by adding the first connection, too!");
        create_workflow(args)
    } else {
        create_tool(args)
    }
}

#[derive(Args, Debug, Default)]
pub struct CreateArgs {
    #[arg(short = 'n', long = "name", help = "A name to be used for this workflow or tool")]
    pub name: Option<String>,
    #[arg(
        short = 'c',
        long = "container-image",
        help = "An image to pull from e.g. docker hub or path to a Dockerfile"
    )]
    pub container_image: Option<String>,
    #[arg(short = 't', long = "container-tag", help = "The tag for the container when using a Dockerfile")]
    pub container_tag: Option<String>,

    #[arg(short = 'r', long = "raw", help = "Outputs the raw CWL contents to terminal")]
    pub is_raw: bool,
    #[arg(long = "no-commit", help = "Do not commit at the end of tool creation")]
    pub no_commit: bool,
    #[arg(long = "no-run", help = "Do not run given command")]
    pub no_run: bool,
    #[arg(long = "clean", help = "Deletes created outputs after usage")]
    pub is_clean: bool,
    #[arg(long = "no-defaults", help = "Removes default values from inputs")]
    pub no_defaults: bool,
    #[arg(long = "net", alias = "enable-network", help = "Enables network in container")]
    pub enable_network: bool,
    #[arg(short = 'i', long = "inputs", help = "Force values to be considered as an input.", value_delimiter = ' ')]
    pub inputs: Option<Vec<String>>,
    #[arg(long = "run-container", help = "Possible container engines: docker, podman, singularity, apptainer")]
    pub run_container: Option<ContainerEngineArg>,
    #[arg(
        short = 'o',
        long = "outputs",
        help = "Force values to be considered as an output.",
        value_delimiter = ' '
    )]
    pub outputs: Option<Vec<String>>,
    #[arg(
        short = 'm',
        long = "mount",
        help = "Mounts a directory into the working directory",
        value_delimiter = ' '
    )]
    pub mount: Option<Vec<PathBuf>>,
    #[arg(long = "env", help = "Loads an .env File")]
    pub env: Option<PathBuf>,
    #[arg(short = 'f', long = "force", help = "Overwrites existing workflow")]
    pub force: bool,
    #[arg(trailing_var_arg = true, help = "Command line call e.g. python script.py [ARGUMENTS]")]
    pub command: Vec<String>,
}

impl<'a> From<&'a CreateArgs> for ToolCreationOptions<'a> {
    fn from(args: &'a CreateArgs) -> Self {
        Self {
            command: &args.command,
            outputs: args.outputs.as_deref().unwrap_or(&[]),
            inputs: args.inputs.as_deref().unwrap_or(&[]),
            no_run: args.no_run,
            cleanup: args.is_clean,
            commit: !args.no_commit,
            clear_defaults: args.no_defaults,
            container: args.container_image.as_ref().map(|image| s4n_core::tool::ContainerInfo {
                image: image.as_str(),
                tag: args.container_tag.as_deref(),
            }),
            enable_network: args.enable_network,
            run_container: args.run_container.as_ref().map(|r| r.0),
            mounts: args.mount.as_deref().unwrap_or(&[]),
            env: args.env.as_deref(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerEngineArg(pub ContainerEngine);

impl FromStr for ContainerEngineArg {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let engine = match s.to_lowercase().as_str() {
            "docker" => ContainerEngine::Docker,
            "podman" => ContainerEngine::Podman,
            "singularity" => ContainerEngine::Singularity,
            "apptainer" => ContainerEngine::Apptainer,
            other => return Err(anyhow!("Unknown container engine: {other}")),
        };
        Ok(ContainerEngineArg(engine))
    }
}

pub fn create_workflow(args: &CreateArgs) -> anyhow::Result<()> {
    let Some(name) = &args.name else {
        return Err(anyhow!("❌ Workflow name is required"));
    };

    //check if workflow already exists
    let filename = format!("{}{}/{}.cwl", get_workflows_folder(), name, name);
    let yaml = s4n_core::workflow::create_workflow(&filename, args.force)?;

    info!("📄 Created new Workflow file: {filename}");
    print_diff("", &yaml);

    Ok(())
}

pub fn create_tool(args: &CreateArgs) -> anyhow::Result<()> {
    if args.command.is_empty() {
        bail!("❌ Command is required to create a tool");
    }
    if args.no_run {
        warn!("User requested no execution, could not determine outputs!");
    }

    let yaml = s4n_core::tool::create_tool(&args.into(), args.name.clone(), !args.is_raw)?;
    let cwl: CommandLineTool = serde_yaml::from_str(&yaml)?;

    info!("Found outputs:");
    let string_outputs: Vec<String> = cwl
        .outputs
        .iter()
        .filter_map(|o| o.output_binding.as_ref()?.glob.clone().map(|g| g.into_vec()))
        .flatten()
        .collect();

    print_list(&string_outputs);

    //save tool
    if args.is_raw {
        highlight_cwl(&yaml);
    } else {
        let path = get_qualified_filename(&cwl.base_command, args.name.clone());
        info!("\n📄 Created CWL file {}", path.green().bold());
    }

    Ok(())
}
