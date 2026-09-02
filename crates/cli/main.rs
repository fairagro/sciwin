use clap::{Command, CommandFactory, Parser};
use s4n::{
    ExitCode,
    cli::{Cli, Commands, generate_completions},
    commands::{
        check_git_config, connect_workflow_nodes, disconnect_workflow_nodes, handle_create_command,
        handle_execute_commands, handle_init_command, handle_list_command, handle_remove_command,
        install_package, remove_package, stage_and_commit_workflow, visualize,
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
/// this subcommand's own positional args when no subcommand name matches" directly
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

/// `s4n execute --help` (or `-h`) should read like `run`'s options are `execute`'s own -- since
/// `run` is invisible to users (see `ExecuteCommands::Run`'s `hide = true`), its help would
/// otherwise be unreachable other than via `s4n execute run --help`. Detects that exact request
/// (leading global flags allowed, `execute`/`ex` immediately followed by `-h`/`--help`) so `run()`
/// can special-case it; anything else (including `s4n execute reana --help`) returns `None` and
/// falls through to clap's normal help handling.
fn execute_help_requested(args: &[String]) -> Option<bool> {
    const GLOBAL_FLAGS: &[&str] = &["-q", "--quiet", "--debug"];
    let execute_pos = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, a)| !GLOBAL_FLAGS.contains(&a.as_str()))
        .map(|(i, _)| i)?;
    if args[execute_pos] != "execute" && args[execute_pos] != "ex" {
        return None;
    }
    match args.get(execute_pos + 1).map(String::as_str) {
        Some("-h") => Some(true),
        Some("--help") => Some(false),
        _ => None,
    }
}

/// Prints `execute`'s help with `run`'s own args (engine/runtime/outdir/rocrate/.../FILE) merged
/// in, so hiding `run` from `execute`'s command list doesn't also hide its options. Pulled live
/// from `RunArgs`' derived `Arg`s rather than duplicated as text, so it can't drift out of sync.
#[allow(clippy::disallowed_macros)]
fn print_execute_help(short: bool) {
    let mut app = Cli::command();
    app.build(); // propagates global args (-q/--debug/-h) onto subcommands before we inspect them
    let run_args: Vec<clap::Arg> = app
        .find_subcommand("execute")
        .and_then(|c| c.find_subcommand("run"))
        .expect("execute/run subcommand exists")
        .get_arguments()
        .cloned()
        .collect();

    let execute_cmd = app
        .find_subcommand_mut("execute")
        .expect("execute subcommand exists");
    let existing: Vec<_> = execute_cmd
        .get_arguments()
        .map(|a| a.get_id().clone())
        .collect();
    let taken = std::mem::replace(execute_cmd, Command::new(""));
    *execute_cmd = run_args
        .into_iter()
        .filter(|a| !existing.contains(a.get_id()))
        .fold(taken, Command::arg);

    if short {
        print!("{}", execute_cmd.render_help());
    } else {
        print!("{}", execute_cmd.render_long_help());
    }
}

async fn run() -> miette::Result<()> {
    // Only --engine reana/tes need REANA_SERVER_URL/REANA_ACCESS_TOKEN or TES_URL/TES_STORAGE/TES_TOKEN; every
    // other command works fine with no `.env` file at all, so a missing file is not an error.
    dotenvy::dotenv().ok();

    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(short) = execute_help_requested(&raw_args) {
        print_execute_help(short);
        return Ok(());
    }

    let args = Cli::parse_from(default_to_execute_run(raw_args.into_iter()));
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
        Commands::Save(name) => stage_and_commit_workflow(name),
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
        assert_eq!(
            out,
            to_strings(&["s4n", "execute", "run", "wf.cwl", "in.yml"])
        );
    }

    #[test]
    fn leaves_explicit_subcommands_alone() {
        for subcommand in ["run", "r", "reana", "make-template", "help"] {
            let out =
                default_to_execute_run(to_strings(&["s4n", "execute", subcommand]).into_iter());
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
        let out = default_to_execute_run(
            to_strings(&["s4n", "--debug", "execute", "wf.cwl"]).into_iter(),
        );
        assert_eq!(
            out,
            to_strings(&["s4n", "--debug", "execute", "run", "wf.cwl"])
        );
    }

    #[test]
    fn leaves_other_commands_untouched() {
        let out = default_to_execute_run(to_strings(&["s4n", "init", "-p", "demo"]).into_iter());
        assert_eq!(out, to_strings(&["s4n", "init", "-p", "demo"]));
    }

    #[test]
    fn detects_execute_help_flags() {
        assert_eq!(
            execute_help_requested(&to_strings(&["s4n", "execute", "-h"])),
            Some(true)
        );
        assert_eq!(
            execute_help_requested(&to_strings(&["s4n", "execute", "--help"])),
            Some(false)
        );
        assert_eq!(
            execute_help_requested(&to_strings(&["s4n", "ex", "--help"])),
            Some(false)
        );
        assert_eq!(
            execute_help_requested(&to_strings(&["s4n", "--debug", "execute", "-h"])),
            Some(true)
        );
    }

    #[test]
    fn does_not_treat_subcommand_help_as_execute_help() {
        assert_eq!(
            execute_help_requested(&to_strings(&["s4n", "execute", "reana", "--help"])),
            None
        );
        assert_eq!(
            execute_help_requested(&to_strings(&["s4n", "execute", "wf.cwl"])),
            None
        );
        assert_eq!(execute_help_requested(&to_strings(&["s4n", "init"])), None);
    }
}
