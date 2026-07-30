use commonwl::{
    IntegerOrExpression, OneOrMany,
    documents::{Argument, CommandLineTool},
    files::{Directory, FileOrDirectory},
    inputs::{CommandInputParameter, CommandLineBinding, DefaultValue},
    requirements::{
        InitialWorkDirRequirement, ShellCommandRequirement, ToolRequirements, WorkDirItems,
    },
    types::CWLType,
};
use std::{fs, path::Path};

mod inputs;
mod outputs;
mod postprocess;
pub use inputs::*;
pub(crate) use outputs::*;
pub(crate) use postprocess::post_process_cwl;

//TODO complete list
pub static SCRIPT_EXECUTORS: &[&str] = &["python", "python3", "R", "Rscript", "node"];
pub static SCRIPT_MODIFIERS: &[&str] = &["-e", "-m"];
static SPECIAL_CHARS: &[&str] = &["|", ">", "2>"];
pub(crate) static BAD_WORDS: &[&str] = &["sql", "postgres", "mysql", "password"];

pub(crate) fn parse_command_line(commands: &[&str]) -> CommandLineTool {
    let base_command = get_base_command(commands);

    let remainder = match &base_command {
        OneOrMany::One(_) => &commands[1..],
        OneOrMany::Many(vec) => &commands[vec.len()..],
    };
    let tool = CommandLineTool::builder().base_command(base_command.clone());

    let mut tool = if !remainder.is_empty() {
        let (cmd, piped) = split_vec_at(remainder, &"|");

        let stdout_pos = cmd.iter().position(|i| *i == ">").unwrap_or(cmd.len());
        let stderr_pos = cmd.iter().position(|i| *i == "2>").unwrap_or(cmd.len());
        let first_redir_pos = usize::min(stdout_pos, stderr_pos);

        let stdout = handle_redirection(&cmd[stdout_pos..]);
        let stderr = handle_redirection(&cmd[stderr_pos..]);

        let inputs = get_inputs(&cmd[..first_redir_pos]);

        let args = collect_arguments(&piped, &inputs);

        tool.inputs(inputs)
            .maybe_stdout(stdout)
            .maybe_stderr(stderr)
            .maybe_arguments(args)
            .build()
    } else {
        tool.build()
    };

    //add working dir items
    tool = match base_command {
        OneOrMany::One(cmd) => {
            //if command is an existing file, add to requirements
            if let Some(req) = iwdr_for_existing_file(&cmd) {
                tool.append_requirement_mut(req);
            }
            tool
        }
        OneOrMany::Many(ref vec) => {
            //usual command `python script-file.py`
            if let Some(req) = iwdr_for_existing_file(&vec[1]) {
                tool.append_requirement_mut(req);
            }
            //command with `R -e script.R`
            if vec.len() > 2
                && SCRIPT_MODIFIERS.contains(&vec[1].as_str())
                && let Some(req) = iwdr_for_existing_file(&vec[2])
            {
                tool.append_requirement_mut(req);
            }
            //command with `python -m folder`
            if vec.len() > 2
                && SCRIPT_MODIFIERS.contains(&vec[1].as_str())
                && fs::exists(&vec[2]).unwrap_or_default()
                && Path::new(&vec[2]).is_dir()
            {
                tool.inputs.push(
                    CommandInputParameter::builder()
                        .id("module")
                        .r#type(CWLType::Directory)
                        .default(DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                            Directory::builder().location(&vec[2]).build(),
                        )))
                        .build(),
                );
                tool.append_requirement_mut(ToolRequirements::InitialWorkDirRequirement(
                    InitialWorkDirRequirement {
                        listing: WorkDirItems::Expression("$(inputs.module)".to_string()),
                    },
                ));
            }
            tool
        }
    };

    if tool.arguments.is_some() {
        tool.append_requirement_mut(ToolRequirements::ShellCommandRequirement(
            ShellCommandRequirement,
        ));
    }
    tool
}

/// Whether a token names a script interpreter, allowing a version suffix (`python3.11`).
pub(crate) fn matches_script_executor(token: &str) -> bool {
    SCRIPT_EXECUTORS.iter().any(|&exec| {
        token == exec
            || (token.starts_with(exec)
                && token[exec.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_alphabetic()))
    })
}

/// Whether a token is an interpreter modifier that shifts the payload one position right,
/// as in `python -m module` or `R -e script.R`.
pub(crate) fn matches_script_modifier(token: &str) -> bool {
    SCRIPT_MODIFIERS.iter().any(|&modif| token.starts_with(modif))
}

pub(crate) fn get_base_command(command: &[&str]) -> OneOrMany<String> {
    if command.is_empty() {
        return OneOrMany::One(String::new());
    }

    let mut base_command = vec![command[0].to_string()];

    if command.len() > 1 && matches_script_executor(command[0]) {
        if command.len() > 2 && matches_script_modifier(command[1]) {
            base_command.push(command[1].to_string()); //the modifier
            base_command.push(command[2].to_string()); //the package
        } else {
            base_command.push(command[1].to_string());
        }
    }

    match base_command.len() {
        1 => OneOrMany::One(command[0].to_string()),
        _ => OneOrMany::Many(base_command),
    }
}

fn handle_redirection(remaining_args: &[&str]) -> Option<String> {
    //hopefully? most cases are only `some_command > some_file.out`
    //redirect token comes at pos 0, discard that; pos 1 is the filename, if present
    //(absent e.g. for a trailing dangling `>` with nothing after it)
    remaining_args.get(1).map(|out_file| out_file.to_string())
}

fn collect_arguments(piped: &[&str], inputs: &[CommandInputParameter]) -> Option<Vec<Argument>> {
    if piped.is_empty() {
        return None;
    }

    let piped_args = piped.iter().enumerate().map(|(i, &x)| {
        let shell_quote = if SPECIAL_CHARS.contains(&x) {
            Some(false)
        } else {
            None
        };
        Argument::Binding(CommandLineBinding {
            position: Some(IntegerOrExpression::Int(
                (inputs.len() + i).try_into().unwrap(),
            )),
            value_from: Some(x.to_string()),
            shell_quote,
            ..Default::default()
        })
    });

    let mut args = vec![Argument::Binding(CommandLineBinding {
        position: Some(IntegerOrExpression::Int(
            inputs.len().try_into().unwrap_or_default(),
        )),
        value_from: Some("|".to_string()),
        shell_quote: Some(false),
        ..Default::default()
    })];
    args.extend(piped_args);

    Some(args)
}

fn split_vec_at<T: PartialEq + Clone, C: AsRef<[T]>>(vec: C, split_at: &T) -> (Vec<T>, Vec<T>) {
    let slice = vec.as_ref();
    if let Some(index) = slice.iter().position(|x| x == split_at) {
        let lhs = slice[..index].to_vec();
        let rhs = slice[index + 1..].to_vec();
        (lhs, rhs)
    } else {
        (slice.to_vec(), vec![])
    }
}

fn iwdr_for_existing_file(path: &str) -> Option<ToolRequirements> {
    (fs::exists(path).unwrap_or_default() && Path::new(path).is_file()).then(|| {
        ToolRequirements::InitialWorkDirRequirement(
            InitialWorkDirRequirement::new_from_filename_include(path),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::inputs::CommandLineBinding;
    use rstest::rstest;
    use serde_json::Value;
    use serial_test::serial;

    fn parse_command(command: &str) -> CommandLineTool {
        let cmd = shlex::split(command).unwrap();
        parse_command_line(&cmd.iter().map(|s| s.as_str()).collect::<Vec<_>>())
    }

    #[rstest]
    #[case("python script.py --arg1 hello", OneOrMany::Many(vec!["python".to_string(), "script.py".to_string()]))]
    #[case("echo 'Hello World!'", OneOrMany::One("echo".to_string()))]
    #[case("Rscript lol.R", OneOrMany::Many(vec!["Rscript".to_string(), "lol.R".to_string()]))]
    #[case("", OneOrMany::One(String::new()))]
    #[case("python", OneOrMany::One("python".to_string()))]
    #[case("Rake build", OneOrMany::One("Rake".to_string()))]
    #[case("python3.11 script.py", OneOrMany::Many(vec!["python3.11".to_string(), "script.py".to_string()]))]
    pub fn test_get_base_command(#[case] command: &str, #[case] expected: OneOrMany<String>) {
        let args = shlex::split(command).unwrap();
        let args_slice: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        let result = get_base_command(&args_slice);
        assert_eq!(result, expected);
    }

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
    pub fn test_parse_command_line(#[case] input: &str, #[case] expected: CommandLineTool) {
        let result = parse_command(input);
        assert_eq!(result, expected);
    }

    #[test]
    pub fn test_parse_redirect() {
        let tool = parse_command("cat tests/test_data/input.txt \\> output.txt");
        assert!(tool.stdout == Some("output.txt".to_string()));
    }

    #[test]
    pub fn test_parse_dangling_redirect_no_panic() {
        // a trailing `>` with nothing after it must not index-panic
        let tool = parse_command("echo hello \\>");
        assert!(tool.stdout.is_none());
    }

    #[test]
    pub fn test_parse_redirect_stderr() {
        let tool = parse_command("cat tests/test_data/inputtxt 2\\> err.txt");
        assert!(tool.stderr == Some("err.txt".to_string()));
    }

    #[test]
    pub fn test_parse_pipe_op() {
        let tool = parse_command("df \\| grep --line-buffered tmpfs \\> df.log");

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

    #[test]
    pub fn test_badwords() {
        let tool =
            parse_command("pg_dump postgres://postgres:password@localhost:5432/test \\> dump.sql");
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
    #[serial]
    pub fn test_python_module() {
        let args = shlex::split("python3 -m my_module").unwrap();
        let args_slice: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        let result = get_base_command(&args_slice);
        assert_eq!(
            result,
            OneOrMany::Many(vec![
                "python3".to_string(),
                "-m".to_string(),
                "my_module".to_string()
            ])
        );
    }
}
