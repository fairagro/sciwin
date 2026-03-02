use clap::{CommandFactory, Parser};
use commonwl::execution::error::{CommandError, ExitCode};
use log::{LevelFilter, error};
use s4n::{
    cli::{Cli, Commands, generate_completions},
    commands::{
        check_git_config, connect_workflow_nodes, disconnect_workflow_nodes, handle_annotation_command, handle_create_command,
        handle_execute_commands, handle_init_command, handle_list_command, handle_remove_command, install_package, remove_package, save_workflow,
        visualize,
    },
    logger::LOGGER,
};
use std::process::exit;

fn main() {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info)).unwrap();

    if let Err(e) = run() {
        error!("{e}");
        if let Some(src) = e.source() {
            error!("Caused by: {src}");
        }
        if let Some(cmd_err) = e.downcast_ref::<CommandError>() {
            exit(cmd_err.exit_code());
        } else {
            exit(1);
        }
    }
    exit(0);
}

fn run() -> anyhow::Result<()> {
    let args = Cli::parse();
    check_git_config()?;
    match &args.command {
        Commands::Init(args) => Ok(handle_init_command(args)?),
        Commands::Execute { command } => {
            handle_execute_commands(command).map_err(|e| anyhow::anyhow!("{e:#}"))?;
            Ok(())
        }
        Commands::Install(args) => Ok(install_package(&args.identifier, &args.branch)?),
        Commands::Uninstall(args) => Ok(remove_package(&args.identifier)?),
        Commands::Annotate { command, tool_name } => {
            handle_annotation_command(command, tool_name).map_err(|e| anyhow::anyhow!("{e:#}"))?;
            Ok(())
        }
        Commands::Completions { shell } => Ok(generate_completions(*shell, &mut Cli::command())?),
        Commands::List(args) => Ok(handle_list_command(args)?),
        Commands::Remove(args) => Ok(handle_remove_command(args)?),
        Commands::Create(args) => Ok(handle_create_command(args)?),
        Commands::Connect(args) => Ok(connect_workflow_nodes(args)?),
        Commands::Disconnect(args) => Ok(disconnect_workflow_nodes(args)?),
        Commands::Visualize(args) => Ok(visualize(&args.filename, &args.renderer, args.no_defaults)?),
        Commands::Save(name) => Ok(save_workflow(name)?),
    }
}
