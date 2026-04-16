use cwl_core::{CWLDocument, CommandLineTool, DefaultValue, load_tool};
use cwl_execution::{
    environment::RuntimeEnvironment,
    runner::{command::run_command, tool::run_tool},
};
use serial_test::serial;
use std::{collections::HashMap, fs, path::PathBuf};
use tempfile::tempdir;
use test_utils::with_temp_repository;

#[test]
#[serial]
pub fn test_run_command_simple() {
    with_temp_repository(|dir| {
        let cwl = r#"
#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

inputs:
- id: message
  type: string
  default: "Hello CWL"
  inputBinding:
    position: 0

baseCommand: echo

stdout: output.txt

outputs: 
- id: output
  type: File
  glob: output.txt

"#;
        let tool: CommandLineTool = serde_yaml::from_str(cwl).expect("Tool parsing failed");
        assert!(run_command(&tool, &mut RuntimeEnvironment::default()).is_ok());

        let output = dir.path().join("output.txt");
        assert!(output.exists());
        let contents = fs::read_to_string(output).expect("Could not read output");
        assert_eq!(contents.trim(), "Hello CWL");
    });
}

#[test]
#[serial]
pub fn test_run_command_simple_with_args() {
    with_temp_repository(|dir| {
        let cwl = r#"
#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

inputs:
- id: message
  type: string
  default: "Hello CWL"
  inputBinding:
    position: 0

baseCommand: echo

stdout: output.txt

outputs: 
- id: output
  type: File
  glob: output.txt

"#;

        let yml = "message: \"Hello World\"";

        let inputs = serde_yaml::from_str(yml).expect("Input parsing failed");
        let mut runtime = RuntimeEnvironment {
            inputs,
            ..Default::default()
        };
        let tool: CommandLineTool = serde_yaml::from_str(cwl).expect("Tool parsing failed");
        assert!(run_command(&tool, &mut runtime).is_ok());

        let output = dir.path().join("output.txt");
        assert!(output.exists());
        let contents = fs::read_to_string(output).expect("Could not read output");
        assert_eq!(contents.trim(), "Hello World");
    });
}

#[test]
#[serial]
pub fn test_run_command_mismatching_args() {
    with_temp_repository(|_| {
        let cwl = r#"
#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

inputs:
- id: message
  type: string
  default: "Hello CWL"
  inputBinding:
    position: 0

baseCommand: echo

stdout: output.txt

outputs: 
- id: output
  type: File
  glob: output.txt
"#;

        let yml = r"
message:
  class: File
  location: whale.txt
  ";

        let inputs: HashMap<String, DefaultValue> = serde_yaml::from_str(yml).expect("Input parsing failed");
        let mut runtime = RuntimeEnvironment {
            inputs,
            ..Default::default()
        };
        let tool: CommandLineTool = serde_yaml::from_str(cwl).expect("Tool parsing failed");

        let result = run_command(&tool, &mut runtime);
        assert!(result.is_err());
    });
}

#[test]
#[serial]
pub fn test_run_commandlinetool() {
    let cwl = r"
#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: testdata/echo.py
    entry:
      $include: ../../testdata/echo.py

inputs:
- id: test
  type: File
  default:
    class: File
    location: ../../testdata/input.txt
  inputBinding:
    prefix: '--test'

outputs:
- id: results
  type: File
  outputBinding:
    glob: results.txt

baseCommand:
- python3
- testdata/echo.py
";

    let mut tool: CWLDocument = serde_yaml::from_str(cwl).expect("Tool parsing failed");
    let result = run_tool(&mut tool, &Default::default(), &PathBuf::default(), None);
    assert!(result.is_ok());
    //delete results.txt
    let _ = fs::remove_file("results.txt");
    match result {
        Ok(_) => eprintln!("success!"),
        Err(e) => eprintln!("{e:?}"),
    }
}

#[test]
#[serial]
pub fn test_run_commandlinetool_array_glob() {
    let dir = tempdir().unwrap();
    let mut tool = CWLDocument::CommandLineTool(load_tool("../../testdata/array_test.cwl").expect("Tool parsing failed"));
    let result = run_tool(
        &mut tool,
        &Default::default(),
        &PathBuf::default(),
        Some(dir.path().to_string_lossy().into_owned()),
    );
    assert!(result.is_ok(), "{result:?}");
}
