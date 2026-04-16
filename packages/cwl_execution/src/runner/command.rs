use crate::Result;
use crate::error::CommandError;
use crate::{
    container_engine,
    docker::{build_docker_command, is_docker_installed},
    environment::RuntimeEnvironment,
    expression::{replace_expressions, set_self, unset_self},
    format_command,
    inputs::{evaluate_input, evaluate_input_as_string},
    io::{create_and_write_file_forced, get_random_filename, get_shell_command},
};
use cwl_core::{SingularPlural, StringOrNumber, prelude::*};
use log::{info, warn};
use serde_yaml::Value;
use std::process::Command as SystemCommand;
use std::{env, process::Stdio};
use util::handle_process;

pub fn run_command(tool: &CommandLineTool, runtime: &mut RuntimeEnvironment) -> Result<()> {
    let mut command = build_command(tool, runtime)?;

    if let Some(docker) = tool.get_docker_requirement() {
        if is_docker_installed() {
            command = build_docker_command(&mut command, docker, runtime)?;
        } else {
            eprintln!(
                "{} is not installed, can not use {} on this system!",
                container_engine(),
                container_engine()
            );
            warn!("{} is not installed, can not use", container_engine());
        }
    }

    //run
    info!("⏳ Executing Command: `{}`", format_command(&command));

    let mut child = command.spawn()?;
    let output = handle_process(&mut child, runtime.time_limit)?;

    //handle redirection of stdout
    {
        let out = &output.stdout;
        if let Some(stdout) = &tool.stdout {
            create_and_write_file_forced(stdout, out)?;
        } else if tool.has_stdout_output() {
            let output = tool.outputs.iter().filter(|o| matches!(o.type_, CWLType::Stdout)).collect::<Vec<_>>()[0];
            let filename = output
                .output_binding
                .as_ref()
                .and_then(|binding| binding.glob.clone())
                .unwrap_or_else(|| SingularPlural::Singular(get_random_filename(&format!("{}_stdout", output.id), "out")))
                .into_singular();
            create_and_write_file_forced(filename, out)?;
        }
    }
    //handle redirection of stderr
    {
        let out = &output.stderr;
        if let Some(stderr) = &tool.stderr {
            create_and_write_file_forced(stderr, out)?;
        } else if tool.has_stderr_output() {
            let output = tool.outputs.iter().filter(|o| matches!(o.type_, CWLType::Stderr)).collect::<Vec<_>>()[0];
            let filename = output
                .output_binding
                .as_ref()
                .and_then(|binding| binding.glob.clone())
                .unwrap_or_else(|| SingularPlural::Singular(get_random_filename(&format!("{}_stderr", output.id), "out")))
                .into_singular();
            create_and_write_file_forced(filename, out)?;
        }
    }

    let status_code = output.exit_code;
    runtime
        .runtime
        .insert("exitCode".to_string(), StringOrNumber::Integer(status_code as u64));

    if tool.get_sucess_code() == status_code {
        Ok(()) //fails expectedly
    } else {
        Err(CommandError::new("Command Execution failed".to_owned(), status_code).into())
    }
}

#[derive(Debug, Clone)]
struct BoundBinding {
    sort_key: Vec<SortKey>,
    command: CommandLineBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Int(i32),
    Str(String),
}

fn build_command(tool: &CommandLineTool, runtime: &RuntimeEnvironment) -> Result<SystemCommand> {
    let mut args: Vec<String> = vec![];

    //get executable
    let cmd = match &tool.base_command {
        Command::Single(cmd) => cmd,
        Command::Multiple(vec) => {
            if vec.is_empty() {
                &String::new()
            } else {
                &vec[0]
            }
        }
    };

    if !cmd.is_empty() {
        args.push(cmd.to_string());
        //append rest of base command as args
        if let Command::Multiple(vec) = &tool.base_command {
            args.extend_from_slice(&vec[1..]);
        }
    }

    let mut bindings: Vec<BoundBinding> = vec![];

    //handle arguments field...
    if let Some(arguments) = &tool.arguments {
        for (i, arg) in arguments.iter().enumerate() {
            let mut sort_key = vec![];
            match arg {
                Argument::String(str) => {
                    let binding = CommandLineBinding {
                        value_from: Some(str.clone()),
                        ..Default::default()
                    };
                    sort_key.push(SortKey::Int(0));
                    sort_key.push(SortKey::Int(i32::try_from(i)?));
                    bindings.push(BoundBinding { sort_key, command: binding });
                }
                Argument::Binding(binding) => {
                    let position = i32::try_from(binding.position.unwrap_or_default())?;
                    sort_key.push(SortKey::Int(position));
                    sort_key.push(SortKey::Int(i32::try_from(i)?));
                    bindings.push(BoundBinding {
                        sort_key,
                        command: binding.clone(),
                    });
                }
            }
        }
    }

    //handle inputs
    for input in &tool.inputs {
        if let Some(binding) = &input.input_binding {
            let mut binding = binding.clone();
            let position = binding.position.unwrap_or_default();
            let mut sort_key = vec![SortKey::Int(i32::try_from(position)?), SortKey::Str(input.id.clone())];

            let value = runtime.inputs.get(&input.id);
            set_self(&value)?;
            if let Some(value_from) = &binding.value_from {
                if let Some(val) = value {
                    if let DefaultValue::Any(Value::Null) = val {
                        continue;
                    } else {
                        binding.value_from = Some(replace_expressions(value_from).unwrap_or(value_from.to_string()));
                    }
                }
            } else if matches!(input.type_, CWLType::Array(_)) {
                let val = evaluate_input(input, &runtime.inputs)?;
                if let DefaultValue::Array(vec) = val {
                    if vec.is_empty() {
                        continue;
                    }
                    if let Some(sep) = &binding.item_separator {
                        binding.value_from = Some(vec.iter().map(|i| i.as_value_string()).collect::<Vec<_>>().join(sep).to_string());
                    } else {
                        for (i, item) in vec.iter().enumerate() {
                            binding.value_from = Some(item.as_value_string());
                            sort_key.push(SortKey::Int(i32::try_from(i)?));
                            bindings.push(BoundBinding {
                                sort_key: sort_key.clone(),
                                command: binding.clone(),
                            });
                        }
                        unset_self()?;
                        continue;
                    }
                }
            } else {
                let binding_str = evaluate_input_as_string(input, &runtime.inputs)?;
                if matches!(input.type_, CWLType::Optional(_)) && binding_str == "null" {
                    continue;
                }
                binding.value_from = Some(binding_str.replace("'", ""));
            }
            unset_self()?;
            bindings.push(BoundBinding { sort_key, command: binding });
        }
    }

    //do sorting
    bindings.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));

    //add bindings
    for input in bindings.iter().map(|b| &b.command) {
        if let Some(prefix) = &input.prefix {
            args.push(prefix.to_string());
        }
        if let Some(value) = &input.value_from {
            if tool.has_shell_command_requirement() {
                if let Some(shellquote) = input.shell_quote {
                    if shellquote {
                        args.push(format!("\"{value}\""));
                    } else {
                        args.push(value.to_string());
                    }
                } else {
                    args.push(value.to_string());
                }
            } else {
                args.push(value.to_string());
            }
        }
    }

    //remove empty args
    args.retain(|s| !s.is_empty());

    let mut command = if tool.has_shell_command_requirement() {
        let joined_args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>().join(" ");
        let mut cmd = get_shell_command();
        cmd.arg(joined_args);
        cmd
    } else {
        let mut cmd = SystemCommand::new(args[0].clone());
        for arg in &args[1..] {
            cmd.arg(arg);
        }
        cmd
    };

    //append stdin i guess?
    if let Some(stdin) = &tool.stdin {
        command.arg(stdin);
    }

    let current_dir = env::current_dir()?.to_string_lossy().into_owned();

    //set environment for run
    command.envs(runtime.environment.clone());
    command.env(
        "HOME",
        runtime
            .runtime
            .get("outdir")
            .unwrap_or(&StringOrNumber::String(current_dir.clone()))
            .to_string(),
    );
    command.env(
        "TMPDIR",
        runtime
            .runtime
            .get("tmpdir")
            .unwrap_or(&StringOrNumber::String(current_dir.clone()))
            .to_string(),
    );
    command.current_dir(
        runtime
            .runtime
            .get("outdir")
            .unwrap_or(&StringOrNumber::String(current_dir.clone()))
            .to_string(),
    );

    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::set_container_engine;
    use cwl_core::load_tool;
    use std::collections::HashMap;

    #[test]
    fn test_build_command() {
        let yaml = r"
class: CommandLineTool
cwlVersion: v1.2
inputs:
  file1: 
    type: File
    inputBinding: {position: 0}
outputs:
  output_file:
    type: File
    outputBinding: {glob: output.txt}
baseCommand: cat
stdout: output.txt";
        let tool = &serde_yaml::from_str(yaml).unwrap();

        let inputs = r#"{
    "file1": {
        "class": "File",
        "location": "hello.txt"
    }
}"#;

        let input_values = serde_json::from_str(inputs).unwrap();
        let runtime = RuntimeEnvironment {
            inputs: input_values,
            ..Default::default()
        };
        let cmd = build_command(tool, &runtime).unwrap();

        assert_eq!(format_command(&cmd), "cat hello.txt");
    }

    #[test]
    fn test_build_command_stdin() {
        let yaml = r"
class: CommandLineTool
cwlVersion: v1.2
inputs: []
outputs: []
baseCommand: [cat]
stdin: hello.txt";
        let tool = &serde_yaml::from_str(yaml).unwrap();

        let cmd = build_command(tool, &Default::default()).unwrap();

        assert_eq!(format_command(&cmd), "cat hello.txt");
    }

    #[test]
    fn test_build_command_args() {
        let yaml = r#"class: CommandLineTool
cwlVersion: v1.2
requirements:
  - class: ShellCommandRequirement
inputs:
  indir: Directory
outputs:
  outlist:
    type: File
    outputBinding:
      glob: output.txt
arguments: ["cd", "$(inputs.indir.path)",
  {shellQuote: false, valueFrom: "&&"},
  "find", ".",
  {shellQuote: false, valueFrom: "|"},
  "sort"]
stdout: output.txt"#;
        let in_yaml = r"indir:
  class: Directory
  location: testdir";
        let tool = &serde_yaml::from_str(yaml).unwrap();
        let input_values = serde_yaml::from_str(in_yaml).unwrap();
        let runtime = RuntimeEnvironment {
            inputs: input_values,
            ..Default::default()
        };
        let cmd = build_command(tool, &runtime).unwrap();

        let shell_cmd = get_shell_command();
        let shell = shell_cmd.get_program().to_string_lossy();
        let c_arg = shell_cmd.get_args().collect::<Vec<_>>()[0].to_string_lossy();

        assert_eq!(format_command(&cmd), format!("{shell} {c_arg} cd $(inputs.indir.path) && find . | sort"));
    }

    #[test]
    fn test_build_command_docker() {
        set_container_engine(crate::ContainerEngine::Docker);
        //tool has docker requirement
        let path = format!(
            "{}/../../testdata/hello_world/workflows/calculation/calculation.cwl",
            env!("CARGO_MANIFEST_DIR")
        );
        let tool = load_tool(&path).unwrap();
        let runtime = RuntimeEnvironment {
            runtime: HashMap::from([
                ("outdir".to_string(), StringOrNumber::String("testdir".to_string())),
                ("tmpdir".to_string(), StringOrNumber::String("testdir".to_string())),
            ]),
            ..Default::default()
        };

        let mut cmd = build_command(&tool, &runtime).unwrap();
        let cmd = build_docker_command(&mut cmd, tool.get_docker_requirement().unwrap(), &runtime).unwrap();
        eprint!("{}", format_command(&cmd));
        assert!(cmd.get_program().to_string_lossy().contains("docker"));
    }

    #[test]
    fn test_build_command_podman() {
        set_container_engine(crate::ContainerEngine::Podman);

        //tool has docker requirement
        let path = format!(
            "{}/../../testdata/hello_world/workflows/calculation/calculation.cwl",
            env!("CARGO_MANIFEST_DIR")
        );
        let tool = load_tool(&path).unwrap();
        let runtime = RuntimeEnvironment {
            runtime: HashMap::from([
                ("outdir".to_string(), StringOrNumber::String("testdir".to_string())),
                ("tmpdir".to_string(), StringOrNumber::String("testdir".to_string())),
            ]),
            ..Default::default()
        };

        let mut cmd = build_command(&tool, &runtime).unwrap();
        let cmd = build_docker_command(&mut cmd, tool.get_docker_requirement().unwrap(), &runtime).unwrap();
        eprint!("{}", format_command(&cmd));
        assert!(cmd.get_program().to_string_lossy().contains("podman"));
    }
}