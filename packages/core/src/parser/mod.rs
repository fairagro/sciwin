use commonwl::{prelude::*, requirements::WorkDirItem};
use std::{fs, path::Path};

mod inputs;
mod outputs;
mod postprocess;
pub(crate) use inputs::*;
pub(crate) use outputs::*;
pub(crate) use postprocess::post_process_cwl;

//TODO complete list
pub static SCRIPT_EXECUTORS: &[&str] = &["python", "python3", "R", "Rscript", "node"];
pub static SCRIPT_MODIFIERS: &[&str] = &["-e", "-m"];

pub(crate) static BAD_WORDS: &[&str] = &["sql", "postgres", "mysql", "password"];

pub(crate) fn parse_command_line(commands: &[&str]) -> CommandLineTool {
    let base_command = get_base_command(commands);

    let remainder = match &base_command {
        Command::Single(_) => &commands[1..],
        Command::Multiple(vec) => &commands[vec.len()..],
    };
    let mut tool = CommandLineTool::default().with_base_command(base_command.clone());

    if !remainder.is_empty() {
        let (cmd, piped) = split_vec_at(remainder, &"|");

        let stdout_pos = cmd.iter().position(|i| *i == ">").unwrap_or(cmd.len());
        let stderr_pos = cmd.iter().position(|i| *i == "2>").unwrap_or(cmd.len());
        let first_redir_pos = usize::min(stdout_pos, stderr_pos);

        let stdout = handle_redirection(&cmd[stdout_pos..]);
        let stderr = handle_redirection(&cmd[stderr_pos..]);

        let inputs = get_inputs(&cmd[..first_redir_pos]);

        let args = collect_arguments(&piped, &inputs);

        tool = tool.with_inputs(inputs).with_stdout(stdout).with_stderr(stderr).with_arguments(args);
    }

    //add working dir items
    tool = match base_command {
        Command::Single(cmd) => {
            //if command is an existing file, add to requirements
            if fs::exists(&cmd).unwrap_or_default() {
                return tool.with_requirements(vec![Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement::from_file(&cmd))]);
            }
            tool
        }
        Command::Multiple(ref vec) => {
            //usual command `pyton script-file.py`
            if fs::exists(&vec[1]).unwrap_or_default() && Path::new(&vec[1]).is_file() {
                return tool.with_requirements(vec![Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement::from_file(
                    &vec[1],
                ))]);
            }
            //command with `R -e script.R`
            if vec.len() > 2 && SCRIPT_MODIFIERS.contains(&vec[1].as_str()) && fs::exists(&vec[2]).unwrap_or_default() && Path::new(&vec[2]).is_file() {
                return tool.with_requirements(vec![Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement::from_file(
                    &vec[2],
                ))]);
            }
            //command with `python -m folder`
            if vec.len() > 2 && SCRIPT_MODIFIERS.contains(&vec[1].as_str()) && fs::exists(&vec[2]).unwrap_or_default() && Path::new(&vec[2]).is_dir() {
                let mut tool = tool;
                tool.inputs.push(CommandInputParameter::default().with_id("module").with_type(CWLType::Directory).with_default_value(DefaultValue::Directory(Directory::from_location(&vec[2]))));
                return tool.with_requirements(vec![Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement { listing: vec![WorkDirItem::Expression("$(inputs.module)".to_string())] })]);
            }
            tool
        }
    };

    if tool.arguments.is_some() {
        tool = tool.append_requirement(Requirement::ShellCommandRequirement);
    }
    tool
}

pub(crate) fn get_base_command(command: &[&str]) -> Command {
    if command.is_empty() {
        return Command::Single(String::new());
    }

    let mut base_command = vec![command[0].to_string()];

    if SCRIPT_EXECUTORS.iter().any(|&exec| command[0].starts_with(exec)) {
        if SCRIPT_MODIFIERS.iter().any(|&modif| command[1].starts_with(modif)) {
            base_command.push(command[1].to_string()); //the modifier
            base_command.push(command[2].to_string()); //the package
        } else {
            base_command.push(command[1].to_string());
        }
    }

    match base_command.len() {
        1 => Command::Single(command[0].to_string()),
        _ => Command::Multiple(base_command),
    }
}

fn handle_redirection(remaining_args: &[&str]) -> Option<String> {
    if remaining_args.is_empty() {
        return None;
    }
    //hopefully? most cases are only `some_command > some_file.out`
    //remdirect comes at pos 0, discard that
    let out_file = remaining_args[1];
    Some(out_file.to_string())
}

fn collect_arguments(piped: &[&str], inputs: &[CommandInputParameter]) -> Option<Vec<Argument>> {
    if piped.is_empty() {
        return None;
    }

    let piped_args = piped.iter().enumerate().map(|(i, &x)| {
        Argument::Binding(CommandLineBinding {
            position: Some((inputs.len() + i).try_into().unwrap_or_default()),
            value_from: Some(x.to_string()),
            ..Default::default()
        })
    });

    let mut args = vec![Argument::Binding(CommandLineBinding {
        position: Some(inputs.len().try_into().unwrap_or_default()),
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

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::Path;
    use super::*;
    use commonwl::execution::io::copy_dir;
    use commonwl::execution::{environment::RuntimeEnvironment, runner::command::run_command};
    use commonwl::{CWLType, DefaultValue};
    use rstest::rstest;
    use serde_yaml::Value;
    use serial_test::serial;
    use tempfile::tempdir;
    use test_utils::with_temp_repository;

    fn parse_command(command: &str) -> CommandLineTool {
        let cmd = shlex::split(command).unwrap();
        parse_command_line(&cmd.iter().map(|s| s.as_str()).collect::<Vec<_>>())
    }

    #[rstest]
    #[case("python script.py --arg1 hello", Command::Multiple(vec!["python".to_string(), "script.py".to_string()]))]
    #[case("echo 'Hello World!'", Command::Single("echo".to_string()))]
    #[case("Rscript lol.R", Command::Multiple(vec!["Rscript".to_string(), "lol.R".to_string()]))]
    #[case("", Command::Single(String::new()))]
    pub fn test_get_base_command(#[case] command: &str, #[case] expected: Command) {
        let args = shlex::split(command).unwrap();
        let args_slice: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        let result = get_base_command(&args_slice);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("python script.py", CommandLineTool::default()
            .with_base_command(Command::Multiple(vec!["python".to_string(), "script.py".to_string()]))
        )]
    #[case("Rscript script.R", CommandLineTool::default()
            .with_base_command(Command::Multiple(vec!["Rscript".to_string(), "script.R".to_string()]))
    )]
    #[case("python script.py --option1 value1", CommandLineTool::default()
            .with_base_command(Command::Multiple(vec!["python".to_string(), "script.py".to_string()]))
            .with_inputs(vec![CommandInputParameter::default()
                .with_id("option1")
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_prefix("--option1"))
                .with_default_value(DefaultValue::Any(Value::String("value1".to_string())))])
    )]
    #[case("python script.py --option1 \"value with spaces\"", CommandLineTool::default()
            .with_base_command(Command::Multiple(vec!["python".to_string(), "script.py".to_string()]))
            .with_inputs(vec![CommandInputParameter::default()
                .with_id("option1")
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_prefix("--option1"))
                .with_default_value(DefaultValue::Any(Value::String("value with spaces".to_string())))])
    )]
    #[case("python script.py positional1 --option1 value1",  CommandLineTool::default()
            .with_base_command(Command::Multiple(vec!["python".to_string(), "script.py".to_string()]))
            .with_inputs(vec![
                CommandInputParameter::default()
                    .with_id("positional1")
                    .with_default_value(DefaultValue::Any(Value::String("positional1".to_string())))
                    .with_type(CWLType::String)
                    .with_binding(CommandLineBinding::default().with_position(0)),
                CommandInputParameter::default()
                    .with_id("option1")
                    .with_type(CWLType::String)
                    .with_binding(CommandLineBinding::default().with_prefix("--option1"))
                    .with_default_value(DefaultValue::Any(Value::String("value1".to_string())))
            ])
            
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
    pub fn test_parse_redirect_stderr() {
        let tool = parse_command("cat tests/test_data/inputtxt 2\\> err.txt");
        assert!(tool.stderr == Some("err.txt".to_string()));
    }

    #[test]
    pub fn test_parse_pipe_op() {
        let tool = parse_command("df \\| grep --line-buffered tmpfs \\> df.log");

        assert!(tool.arguments.is_some());
        assert!(tool.has_shell_command_requirement());

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
    #[cfg_attr(target_os = "windows", ignore)]
    pub fn test_cwl_execute_command_single() {
        let cwl = parse_command("ls -la .");
        assert!(run_command(&cwl, &mut RuntimeEnvironment::default()).is_ok());
    }

    #[test]
    pub fn test_badwords() {
        let tool = parse_command("pg_dump postgres://postgres:password@localhost:5432/test \\> dump.sql");
        assert!(BAD_WORDS.iter().any(|&word| tool.inputs.iter().any(|i| !i.id.contains(word))));
    }

    #[test]
    #[serial]
    pub fn test_cwl_execute_command_multiple() {
        with_temp_repository(|dir| {
            let cwl = parse_command("python3 scripts/echo.py --test data/input.txt");
            assert!(run_command(&cwl, &mut RuntimeEnvironment::default()).is_ok());

            let output_path = dir.path().join(Path::new("results.txt"));
            assert!(output_path.exists());
        });
    }

    #[test]
    #[serial]
    pub fn test_python_module() {
        let args = shlex::split("python3 -m my_module").unwrap();
        let args_slice: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        let result = get_base_command(&args_slice);
        assert_eq!(
            result,
            Command::Multiple(vec!["python3".to_string(), "-m".to_string(), "my_module".to_string()])
        );
    }
 
    #[test]
    #[serial]
    pub fn test_python_module_creation() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        let root = env::var("CARGO_MANIFEST_DIR").unwrap();
        copy_dir(Path::new(&root).join("../../testdata/module"), path.join("module")).unwrap();

        let current = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();

        let cwl = parse_command("python3 -m module --what ever");
        //make sure it runs
        
        assert!(run_command(&cwl, &mut RuntimeEnvironment::default()).is_ok());   
        assert_eq!(cwl.base_command, Command::Multiple(vec!["python3".to_string(), "-m".to_string(),  "module".to_string() ]));
        env::set_current_dir(current).unwrap();
    }
}
