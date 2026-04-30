use clap::{CommandFactory, Parser};
use log::{LevelFilter, error};
use s4n::{
    ExitCode,
    cli::{Cli, Commands, generate_completions},
    commands::{
        check_git_config,
        connect_workflow_nodes,
        disconnect_workflow_nodes,
        //handle_annotation_command,
        handle_create_command,
        handle_execute_commands,
        handle_init_command,
        handle_list_command,
        handle_remove_command,
        install_package,
        remove_package,
        save_workflow,
        visualize,
    },
    logger::LOGGER,
};
use std::process::exit;

#[tokio::main]
async fn main() {
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .unwrap();

    if let Err(e) = run().await {
        error!("{e}");
        if let Some(src) = e.source() {
            error!("Caused by: {src}");
        }
        let code = e.downcast_ref::<ExitCode>().unwrap_or(&ExitCode(1));
        exit(code.0)
    }
    exit(0);
}

async fn run() -> anyhow::Result<()> {
    let args = Cli::parse();
    check_git_config()?;
    match &args.command {
        Commands::Init(args) => handle_init_command(args),
        Commands::Execute { command } => handle_execute_commands(command).await,
        Commands::Install(args) => install_package(&args.identifier, &args.branch),
        Commands::Uninstall(args) => remove_package(&args.identifier),
        //Commands::Annotate { command, tool_name } => {
        //    handle_annotation_command(command, tool_name).map_err(|e| anyhow::anyhow!("{e:#}"))?;
        //    Ok(())
        //}
        Commands::Completions { shell } => generate_completions(*shell, &mut Cli::command()),
        Commands::List(args) => handle_list_command(args),
        Commands::Remove(args) => handle_remove_command(args),
        Commands::Create(args) => handle_create_command(args).await,
        Commands::Connect(args) => connect_workflow_nodes(args),
        Commands::Disconnect(args) => disconnect_workflow_nodes(args),
        Commands::Visualize(args) => visualize(&args.filename, &args.renderer, args.no_defaults),

        Commands::Save(name) => save_workflow(name),
    }
}
