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

/// `s4n execute FILE` is shorthand for `s4n execute run FILE`. clap can't express "fall back to
/// this subcommand's own positional args when no subcommand name matches" directly -- a required
/// positional on the default subcommand and the subcommand-name check both want first claim on
/// the same token, and clap always resolves that in favor of the positional, breaking dispatch
/// to the *other* named subcommands (`execute reana ...`/`execute run ...` themselves stopped
/// working under that approach). So instead rewrite argv before parsing: insert `run` right
/// after `execute`/`ex` whenever the following token isn't `-h`/`--help` or one of `execute`'s
/// own subcommand names.
fn default_to_execute_run(args: impl Iterator<Item = String>) -> Vec<String> {
    const EXECUTE_SUBCOMMANDS: &[&str] = &["run", "r", "reana", "make-template", "help"];
    // `Cli`'s own global boolean flags -- skipped over so e.g. `s4n --debug execute FILE` is
    // recognized too, not just `s4n execute FILE`.
    const GLOBAL_FLAGS: &[&str] = &["-q", "--quiet", "--debug"];
    let mut args: Vec<String> = args.collect();

    // args[0] is the binary name; the top-level subcommand is the first arg after that which
    // isn't one of Cli's own global flags.
    let Some(execute_pos) = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, a)| !GLOBAL_FLAGS.contains(&a.as_str()))
        .map(|(i, _)| i)
    else {
        return args;
    };
    if args[execute_pos] != "execute" && args[execute_pos] != "ex" {
        return args;
    }

    let needs_default = match args.get(execute_pos + 1) {
        Some(next) => {
            next != "-h" && next != "--help" && !EXECUTE_SUBCOMMANDS.contains(&next.as_str())
        }
        None => true, // bare `s4n execute` -> let `run`'s own "FILE required" error fire
    };
    if needs_default {
        args.insert(execute_pos + 1, "run".to_string());
    }
    args
}

async fn run() -> miette::Result<()> {
    // Only --engine reana/tes need REANA_URL/REANA_TOKEN or TES_URL/TES_STORAGE/TES_TOKEN; every
    // other command works fine with no `.env` file at all, so a missing file is not an error.
    dotenvy::dotenv().ok();

    let args = Cli::parse_from(default_to_execute_run(std::env::args()));
    if args.quiet {
        init_logger(LevelFilter::ERROR);
    } else if args.debug {
        init_logger(LevelFilter::DEBUG);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn to_strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn injects_run_for_a_bare_file() {
        let out =
            default_to_execute_run(to_strings(&["s4n", "execute", "wf.cwl", "in.yml"]).into_iter());
        assert_eq!(out, to_strings(&["s4n", "execute", "run", "wf.cwl", "in.yml"]));
    }

    #[test]
    fn leaves_explicit_subcommands_alone() {
        for subcommand in ["run", "r", "reana", "make-template", "help"] {
            let out = default_to_execute_run(to_strings(&["s4n", "execute", subcommand]).into_iter());
            assert_eq!(out, to_strings(&["s4n", "execute", subcommand]));
        }
    }

    #[test]
    fn leaves_help_flags_alone() {
        for flag in ["-h", "--help"] {
            let out = default_to_execute_run(to_strings(&["s4n", "execute", flag]).into_iter());
            assert_eq!(out, to_strings(&["s4n", "execute", flag]));
        }
    }

    #[test]
    fn injects_run_for_bare_execute() {
        let out = default_to_execute_run(to_strings(&["s4n", "execute"]).into_iter());
        assert_eq!(out, to_strings(&["s4n", "execute", "run"]));
    }

    #[test]
    fn injects_run_after_a_leading_global_flag() {
        let out =
            default_to_execute_run(to_strings(&["s4n", "--debug", "execute", "wf.cwl"]).into_iter());
        assert_eq!(out, to_strings(&["s4n", "--debug", "execute", "run", "wf.cwl"]));
    }

    #[test]
    fn leaves_other_commands_untouched() {
        let out = default_to_execute_run(to_strings(&["s4n", "init", "-p", "demo"]).into_iter());
        assert_eq!(out, to_strings(&["s4n", "init", "-p", "demo"]));
    }
}
