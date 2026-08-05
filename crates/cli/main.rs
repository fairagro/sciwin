use clap::{CommandFactory, Parser};
use s4n::{
    ExitCode,
    cli::{Cli, Commands, generate_completions},
    commands::{
        check_git_config, connect_workflow_nodes, disconnect_workflow_nodes, handle_create_command,
        handle_execute_commands, handle_init_command, handle_list_command, handle_remove_command,
        install_package, remove_package, save_workflow, visualize,
    },
    logger::init_logger,
};
use std::process::exit;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        // Printed directly rather than via `tracing::error!`: the fmt subscriber's default
        // visitor sanitizes ANSI/control bytes out of message fields (a terminal-injection
        // guard for untrusted log content), which would also neuter miette's fancy colored
        // report. This is the final fatal report right before exit, so it bypasses that layer.
        eprintln!("{e:?}");
        let code = e.downcast_ref::<ExitCode>().unwrap_or(&ExitCode(1));
        exit(code.0)
    }
    exit(0);
}

async fn run() -> miette::Result<()> {
    let args = Cli::parse();
    if args.quiet {
        init_logger(LevelFilter::ERROR);
    } else {
        init_logger(LevelFilter::INFO);
    }

    check_git_config()?;
    match &args.command {
        Commands::Init(args) => handle_init_command(args),
        Commands::Execute { command } => handle_execute_commands(command).await,
        Commands::Install(args) => install_package(&args.identifier, &args.branch),
        Commands::Uninstall(args) => remove_package(&args.identifier),
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
