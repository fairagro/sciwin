use crate::{cwl::highlight_cwl, print_diff, print_list};
use clap::Args;
use colored::Colorize;
use miette::{IntoDiagnostic, bail, miette};
use sciwin::authoring::tool::CreatedTool;
use sciwin::authoring::tool::ToolCreationOptions;
use sciwin::cwl::engine::ContainerEngine;
use std::env;
use std::{path::PathBuf, str::FromStr};
use tracing::{debug, info, warn};

pub async fn handle_create_command(args: &CreateArgs) -> miette::Result<()> {
    if args.command.is_empty() && args.name.is_some() {
        debug!("no command given, only a name -- creating a workflow shell, not a tool");
        info!(
            "ℹ️  Workflow creation is optional. Creation will be triggered by adding the first connection, too!"
        );
        create_workflow(args)
    } else {
        create_tool(args).await
    }
}

#[derive(Args, Debug, Default)]
pub struct CreateArgs {
    #[arg(
        short = 'n',
        long = "name",
        help = "A name to be used for this workflow or tool"
    )]
    pub name: Option<String>,
    #[arg(
        short = 'c',
        long = "container-image",
        help = "An image to pull from e.g. docker hub or path to a Dockerfile"
    )]
    pub container_image: Option<String>,
    #[arg(
        short = 't',
        long = "container-tag",
        help = "The tag for the container when using a Dockerfile"
    )]
    pub container_tag: Option<String>,

    #[arg(
        short = 'r',
        long = "raw",
        help = "Outputs the raw CWL contents to terminal"
    )]
    pub is_raw: bool,
    #[arg(long = "no-commit", help = "Do not commit at the end of tool creation")]
    pub no_commit: bool,
    #[arg(long = "no-run", help = "Do not run given command")]
    pub no_run: bool,
    #[arg(long = "clean", help = "Deletes created outputs after usage")]
    pub is_clean: bool,
    #[arg(long = "no-defaults", help = "Removes default values from inputs")]
    pub no_defaults: bool,
    #[arg(
        long = "net",
        alias = "enable-network",
        help = "Enables network in container"
    )]
    pub enable_network: bool,
    #[arg(
        short = 'i',
        long = "inputs",
        help = "Force values to be considered as an input.",
        value_delimiter = ' '
    )]
    pub inputs: Option<Vec<String>>,
    #[arg(
        long = "run-container",
        help = "Possible container engines: docker, podman, singularity, apptainer"
    )]
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
    #[arg(
        trailing_var_arg = true,
        help = "Command line call e.g. python script.py [ARGUMENTS]"
    )]
    pub command: Vec<String>,
}

impl From<&CreateArgs> for ToolCreationOptions {
    fn from(args: &CreateArgs) -> Self {
        Self {
            command: args.command.clone(),
            outputs: args.outputs.clone().unwrap_or(vec![]),
            inputs: args.inputs.clone().unwrap_or(vec![]),
            no_run: args.no_run,
            cleanup: args.is_clean,
            commit: !args.no_commit,
            clear_defaults: args.no_defaults,
            container: args.container_image.as_ref().map(|image| {
                sciwin::authoring::tool::ContainerInfo {
                    image: image.to_owned(),
                    tag: args.container_tag.clone(),
                }
            }),
            enable_network: args.enable_network,
            run_container: args.run_container.clone().map(|r| r.0),
            mounts: args.mount.clone().unwrap_or(vec![]),
            env: args.env.clone(),
            name: args.name.clone(),
            output_dir: None,
            save: !args.is_raw,
            auto_container: false, //need to wire into CLI
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerEngineArg(pub ContainerEngine);

impl FromStr for ContainerEngineArg {
    type Err = miette::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let engine = match s.to_lowercase().as_str() {
            "docker" => ContainerEngine::Docker,
            "podman" => ContainerEngine::Podman,
            "singularity" => ContainerEngine::Singularity,
            "apptainer" => ContainerEngine::Apptainer,
            other => return Err(miette!("Unknown container engine: {other}")),
        };
        Ok(ContainerEngineArg(engine))
    }
}

pub fn create_workflow(args: &CreateArgs) -> miette::Result<()> {
    let Some(name) = &args.name else {
        return Err(miette!("❌ Workflow name is required"));
    };
    debug!("creating workflow '{name}' (force={})", args.force);

    let (path, yaml) = sciwin::authoring::workflow::create_workflow(name, None, args.force)?;

    info!("📄 Created new Workflow file: {}", path.display());
    print_diff("", &yaml);

    Ok(())
}

pub async fn create_tool(args: &CreateArgs) -> miette::Result<()> {
    if args.command.is_empty() {
        bail!("❌ Command is required to create a tool");
    }
    if args.no_run {
        warn!("User requested no execution, could not determine outputs!");
    }

    let cwd = env::current_dir().into_diagnostic()?;
    debug!("running `{}` in {cwd:?} to observe its outputs", args.command.join(" "));
    let CreatedTool {
        document,
        path,
        yaml,
    } = sciwin::authoring::tool::create_tool(&cwd, &args.into()).await?;

    info!("Found outputs:");
    let string_outputs: Vec<String> = document
        .outputs
        .iter()
        .filter_map(|o| o.output_binding.as_ref()?.glob.clone().map(|g| g.as_many()))
        .flatten()
        .collect();
    debug!("{} output(s) detected from glob bindings", string_outputs.len());

    print_list(&string_outputs);

    //save tool
    if args.is_raw {
        debug!("--raw given, printing CWL to terminal instead of saving to disk");
        highlight_cwl(&yaml);
    } else {
        info!(
            "\n📄 Created CWL file {}",
            path.to_string_lossy().green().bold()
        );
    }

    Ok(())
}
