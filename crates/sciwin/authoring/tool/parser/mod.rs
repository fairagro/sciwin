//! Turning raw command-line tokens into a CWL `CommandLineTool`.
//!
//! `parse_command_line` is the entry point and orchestrates the sub-modules: `command` finds
//! the base command, `shell` handles pipes and redirections, `inputs` and `outputs` derive
//! the parameters, and `staging` declares files the tool needs in its working directory.
//! `postprocess` then runs over the assembled tool.
//!
//! Only [`guess_type`] is public; the rest is reached through
//! [`crate::authoring::tool::create_tool`].

use crate::{
    authoring::{
        AuthoringResult,
        tool::parser::command::{get_base_command, matches_script_modifier},
    },
    paths::TrustedPathExt,
};
use commonwl::{
    OneOrMany,
    documents::CommandLineTool,
    files::{Directory, FileOrDirectory},
    inputs::{CommandInputParameter, DefaultValue},
    requirements::{
        InitialWorkDirRequirement, ShellCommandRequirement, ToolRequirements, WorkDirItems,
    },
    types::CWLType,
};
use std::path::Path;

pub(crate) mod command;
mod edam;
pub(super) mod inputs;
pub(super) mod outputs;
mod shell;
mod staging;

pub use inputs::guess_type;

pub(crate) static BAD_WORDS: &[&str] = &["sql", "postgres", "mysql", "password"];

pub(crate) async fn parse_command_line(
    commands: &[&str],
    base: &Path,
) -> AuthoringResult<CommandLineTool> {
    let base_command = get_base_command(commands);

    let remainder = match &base_command {
        OneOrMany::One(_) => &commands[1..],
        OneOrMany::Many(vec) => &commands[vec.len()..],
    };
    let tool = CommandLineTool::builder().base_command(base_command.clone());

    let mut tool = if remainder.is_empty() {
        tool.build()
    } else {
        let (cmd, piped) = shell::split_at_first(remainder, "|");

        let stdout_pos = cmd.iter().position(|i| *i == ">").unwrap_or(cmd.len());
        let stderr_pos = cmd.iter().position(|i| *i == "2>").unwrap_or(cmd.len());
        let first_redir_pos = usize::min(stdout_pos, stderr_pos);

        let stdout = shell::handle_redirection(&cmd[stdout_pos..]);
        let stderr = shell::handle_redirection(&cmd[stderr_pos..]);

        let inputs = inputs::build_inputs(&cmd[..first_redir_pos], base).await?;
        let args = shell::collect_arguments(piped, &inputs);

        tool.inputs(inputs)
            .maybe_stdout(stdout)
            .maybe_stderr(stderr)
            .maybe_arguments(args)
            .build()
    };

    stage_base_command(&mut tool, &base_command, base)?;

    if tool.arguments.is_some() {
        tool.append_requirement_mut(ToolRequirements::ShellCommandRequirement(
            ShellCommandRequirement,
        ));
    }
    Ok(tool)
}

/// Declares whatever the base command runs -- a script file, or a module directory -- as
/// staged, so it exists inside the tool's working directory at runtime.
fn stage_base_command(
    tool: &mut CommandLineTool,
    base_command: &OneOrMany<String>,
    base: &Path,
) -> AuthoringResult<()> {
    let tokens = match base_command {
        //if command is an existing file, add to requirements
        OneOrMany::One(cmd) => std::slice::from_ref(cmd),
        OneOrMany::Many(vec) => vec.as_slice(),
    };

    //usual command `python script-file.py`, or a bare `./script.sh`
    let script = if tokens.len() > 1 {
        &tokens[1]
    } else {
        &tokens[0]
    };
    if let Ok(Some(req)) = staging::iwdr_for_existing_file(script, base) {
        tool.append_requirement_mut(req);
    }

    //command with `R -e script.R`
    let Some(payload) = tokens.get(2) else {
        return Ok(());
    };
    if !matches_script_modifier(&tokens[1]) {
        return Ok(());
    }
    if let Ok(Some(req)) = staging::iwdr_for_existing_file(payload, base) {
        tool.append_requirement_mut(req);
    }

    //command with `python -m folder`
    if base.join_trusted_checked(payload)?.is_dir() {
        tool.inputs.push(
            CommandInputParameter::builder()
                .id("module")
                .r#type(CWLType::Directory)
                .default(DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                    Directory::builder().location(payload).build(),
                )))
                .build(),
        );
        tool.append_requirement_mut(ToolRequirements::InitialWorkDirRequirement(
            InitialWorkDirRequirement {
                listing: WorkDirItems::Expression("$(inputs.module)".to_string()),
            },
        ));
    }

    Ok(())
}

pub(crate) fn sanitize_id(input: &str) -> String {
    let trimmed = input.trim_start_matches(|c: char| !c.is_alphabetic());
    // an all-non-alphabetic name (e.g. a bare "123") trims to "" — fall back to
    // the untrimmed input rather than emitting an empty/invalid id
    let base = if trimmed.is_empty() { input } else { trimmed };
    base.trim_end_matches('/')
        .replace(['.', '/'], "_")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::{
        documents::Argument,
        inputs::{CommandInputParameter, CommandLineBinding},
    };
    use rstest::rstest;
    use serde_json::Value;

    async fn parse_command(command: &str) -> CommandLineTool {
        let cmd = shlex::split(command).unwrap();
        parse_command_line(
            &cmd.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            Path::new("."),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    #[rstest]
    #[case("python script.py", CommandLineTool::builder().
            base_command(OneOrMany::Many(vec!["python".to_string(), "script.py".to_string()])).build()
        )]
    #[case("Rscript script.R", CommandLineTool::builder()
            .base_command(OneOrMany::Many(vec!["Rscript".to_string(), "script.R".to_string()])).build()
    )]
    #[case("python script.py --option1 value1", CommandLineTool::builder()
            .base_command(OneOrMany::Many(vec!["python".to_string(), "script.py".to_string()]))
            .inputs(vec![CommandInputParameter::builder()
                .id("option1")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().prefix("--option1").build())
                .default(DefaultValue::Any(Value::String("value1".to_string()))).build()]).build()
    )]
    #[case("python script.py --option1 \"value with spaces\"", CommandLineTool::builder()
            .base_command(OneOrMany::Many(vec!["python".to_string(), "script.py".to_string()]))
            .inputs(vec![CommandInputParameter::builder()
                .id("option1")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().prefix("--option1").build())
                .default(DefaultValue::Any(Value::String("value with spaces".to_string()))).build()]).build()
    )]
    #[case("python script.py positional1 --option1 value1",  CommandLineTool::builder()
            .base_command(OneOrMany::Many(vec!["python".to_string(), "script.py".to_string()]))
            .inputs(vec![
                CommandInputParameter::builder()
                    .id("positional1")
                    .default(DefaultValue::Any(Value::String("positional1".to_string())))
                    .r#type(CWLType::String)
                    .input_binding(CommandLineBinding::builder().position(0).build()).build(),
                CommandInputParameter::builder()
                    .id("option1")
                    .r#type(CWLType::String)
                    .input_binding(CommandLineBinding::builder().prefix("--option1").build())
                    .default(DefaultValue::Any(Value::String("value1".to_string()))).build()
            ]).build()

    )]
    pub async fn test_parse_command_line(#[case] input: &str, #[case] expected: CommandLineTool) {
        let result = parse_command(input).await;
        assert_eq!(result, expected);
    }

    #[tokio::test]
    pub async fn test_parse_redirect() {
        let tool = parse_command("cat tests/test_data/input.txt \\> output.txt").await;
        assert!(tool.stdout == Some("output.txt".to_string()));
    }

    #[tokio::test]
    pub async fn test_parse_dangling_redirect_no_panic() {
        // a trailing `>` with nothing after it must not index-panic
        let tool = parse_command("echo hello \\>").await;
        assert!(tool.stdout.is_none());
    }

    #[tokio::test]
    pub async fn test_parse_redirect_stderr() {
        let tool = parse_command("cat tests/test_data/inputtxt 2\\> err.txt").await;
        assert!(tool.stderr == Some("err.txt".to_string()));
    }

    #[tokio::test]
    pub async fn test_parse_pipe_op() {
        let tool = parse_command("df \\| grep --line-buffered tmpfs \\> df.log").await;

        assert!(tool.arguments.is_some());
        assert!(tool.has_requirement::<ShellCommandRequirement>());

        if let Some(args) = tool.arguments {
            if let Argument::Binding(pipe) = &args[0] {
                assert!(pipe.value_from == Some("|".to_string()));
            } else {
                panic!();
            }
        }

        assert!(tool.stdout.is_none()); //as it is in args!
    }

    #[tokio::test]
    pub async fn test_badwords() {
        let tool =
            parse_command("pg_dump postgres://postgres:password@localhost:5432/test \\> dump.sql")
                .await;
        // no generated input id should leak a bad word — it must have been redacted to "secret_*"
        assert!(tool.inputs.iter().all(|i| {
            let id = i.id.as_ref().unwrap().to_lowercase();
            !BAD_WORDS.iter().any(|&word| id.contains(word))
        }));
        let secret_input = tool
            .inputs
            .iter()
            .find(|i| i.id.as_ref().unwrap().starts_with("secret_"))
            .expect("a secret_* input must be generated");
        // the credential itself must not be written into the tool file
        assert!(secret_input.default.is_none());
    }
    #[test]
    pub fn test_sanitize_id_numeric_only() {
        // an all-numeric name must not produce an empty id
        assert_eq!(sanitize_id("123"), "123");
    }

    #[test]
    pub fn test_sanitize_id_trailing_slash() {
        assert_eq!(sanitize_id("results/"), "results");
    }
}
