#![allow(clippy::disallowed_macros)]

use commonwl::execution::io::create_and_write_file;
use commonwl::{load_workflow, requirements::Requirement};
use s4n::commands::*;
use s4n_core::project::initialize_project;
use serial_test::serial;
use std::{env, fs, path::Path};
use tempfile::tempdir;
use test_utils::check_git_user;

#[test]
#[serial]
pub fn test_create_workflow() {
    let dir = tempdir().unwrap();
    let current = env::current_dir().unwrap();

    env::set_current_dir(dir.path()).unwrap();
    let args = CreateArgs {
        name: Some("test".to_string()),
        ..Default::default()
    };
    let result = create_workflow(&args);
    assert!(result.is_ok());

    let path = "workflows/test/test.cwl";
    assert!(Path::new(path).exists());

    env::set_current_dir(current).unwrap();
}

#[test]
#[serial]
pub fn test_remove_workflow() {
    check_git_user().unwrap();

    let dir = tempdir().unwrap();
    let current = env::current_dir().unwrap();
    env::set_current_dir(dir.path()).unwrap();

    initialize_project(dir.path(), false).unwrap();
    create_workflow(&CreateArgs {
        name: Some("test".to_string()),
        ..Default::default()
    })
    .unwrap();

    let target = "workflows/test/test.cwl";
    assert!(fs::exists(target).unwrap());

    handle_list_command(&ListCWLArgs { file: None, list_all: true }).unwrap();
    handle_remove_command(&RemoveCWLArgs { file: target.to_string() }).unwrap();

    assert!(!fs::exists(target).unwrap());
    env::set_current_dir(current).unwrap();
}

#[test]
#[serial]
pub fn test_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir().unwrap();
    let current = env::current_dir().unwrap();

    env::set_current_dir(dir.path()).unwrap();
    initialize_project(dir.path(), false).unwrap();

    create_and_write_file("workflows/calculation/calculation.cwl", CALCULATION_FILE).unwrap();
    create_and_write_file("workflows/plot/plot.cwl", PLOT_FILE).unwrap();

    let args = CreateArgs {
        name: Some("test".to_string()),
        ..Default::default()
    };
    let result = create_workflow(&args);
    assert!(result.is_ok());

    let connect_args = vec![
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "@inputs/speakers".to_string(),
            to: "calculation/speakers".to_string(),
        },
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "@inputs/pop".to_string(),
            to: "calculation/population".to_string(),
        },
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "calculation/results".to_string(),
            to: "plot/results".to_string(),
        },
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "plot/results".to_string(),
            to: "@outputs/out".to_string(),
        },
    ];
    for c in &connect_args {
        let result = connect_workflow_nodes(c);
        eprintln!("{result:?}");
        assert!(result.is_ok());
    }

    //connect to another dummy workflow to check subworkflows work
    create_workflow(&CreateArgs {
        name: Some("dummy".to_string()),
        ..Default::default()
    })?;

    let dummy_connect_args = ConnectWorkflowArgs {
        name: "dummy".to_string(),
        from: "@inputs/speakers".to_string(),
        to: "test/speakers".to_string(),
    };
    let result = connect_workflow_nodes(&dummy_connect_args);
    assert!(result.is_ok());

    let wf = load_workflow("workflows/dummy/dummy.cwl").unwrap();
    assert!(wf.requirements.iter().any(|r| matches!(r, Requirement::SubworkflowFeatureRequirement)));

    let workflow = load_workflow("workflows/test/test.cwl").unwrap();

    assert!(workflow.has_input("speakers"));
    assert!(workflow.has_input("pop"));
    assert!(workflow.has_output("out"));

    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

    assert!(workflow.has_step_input("speakers"));
    assert!(workflow.has_step_input("pop"));
    assert!(workflow.has_step_input("calculation/results"));
    assert!(workflow.has_step_output("plot/results"));

    for c in connect_args {
        let result = disconnect_workflow_nodes(&c);
        assert!(result.is_ok());
    }

    // Reload the workflow and validate disconnections
    let workflow = load_workflow("workflows/test/test.cwl").unwrap();

    assert!(!workflow.has_input("speakers"));
    assert!(!workflow.has_input("pop"));
    assert!(!workflow.has_output("out"));

    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

    assert!(!workflow.has_step_input("speakers"));
    assert!(!workflow.has_step_input("pop"));
    assert!(!workflow.has_step_input("calculation/results"));
    assert!(workflow.has_step_output("plot/results")); 

    env::set_current_dir(current).unwrap();

    Ok(())
}

#[test]
#[serial]
pub fn test_workflow_optional_flags() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir().unwrap();
    let current = env::current_dir().unwrap();

    env::set_current_dir(dir.path()).unwrap();
    initialize_project(dir.path(), false).unwrap();

    create_and_write_file("workflows/calculation/calculation.cwl", CALCULATION_FILE).unwrap();
    create_and_write_file("workflows/plot/plot.cwl", PLOT_FILE).unwrap();

    let connect_args = vec![
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "speakers".to_string(),
            to: "calculation/speakers".to_string(),
        },
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "pop".to_string(),
            to: "calculation/population".to_string(),
        },
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "calculation/results".to_string(),
            to: "plot/results".to_string(),
        },
        ConnectWorkflowArgs {
            name: "test".to_string(),
            from: "plot/results".to_string(),
            to: "out".to_string(),
        },
    ];
    for c in &connect_args {
        let result = connect_workflow_nodes(c);
        eprintln!("{result:?}");
        assert!(result.is_ok());
    }

    //connect to another dummy workflow to check subworkflows work
    create_workflow(&CreateArgs {
        name: Some("dummy".to_string()),
        ..Default::default()
    })?;

    let dummy_connect_args = ConnectWorkflowArgs {
        name: "dummy".to_string(),
        from: "speakers".to_string(),
        to: "test/speakers".to_string(),
    };
    let result = connect_workflow_nodes(&dummy_connect_args);
    assert!(result.is_ok());

    let wf = load_workflow("workflows/dummy/dummy.cwl").unwrap();
    assert!(wf.requirements.iter().any(|r| matches!(r, Requirement::SubworkflowFeatureRequirement)));

    let workflow = load_workflow("workflows/test/test.cwl").unwrap();

    assert!(workflow.has_input("speakers"));
    assert!(workflow.has_input("pop"));
    assert!(workflow.has_output("out"));

    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

    assert!(workflow.has_step_input("speakers"));
    assert!(workflow.has_step_input("pop"));
    assert!(workflow.has_step_input("calculation/results"));
    assert!(workflow.has_step_output("plot/results"));

    for c in connect_args {
        let result = disconnect_workflow_nodes(&c);
        assert!(result.is_ok());
    }

    // Reload the workflow and validate disconnections
    let workflow = load_workflow("workflows/test/test.cwl").unwrap();

    assert!(!workflow.has_input("speakers"));
    assert!(!workflow.has_input("pop"));
    assert!(!workflow.has_output("out"));

    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

    assert!(!workflow.has_step_input("speakers"));
    assert!(!workflow.has_step_input("pop"));
    assert!(!workflow.has_step_input("calculation/results"));
    assert!(workflow.has_step_output("plot/results"));

    env::set_current_dir(current).unwrap();

    Ok(())
}

const CALCULATION_FILE: &str = r"#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: calculation.py
    entry:
      $include: ../../calculation.py

inputs:
- id: population
  type: File
  default:
    class: File
    location: ../../population.csv
  inputBinding:
    prefix: --population
- id: speakers
  type: File
  default:
    class: File
    location: ../../speakers_revised.csv
  inputBinding:
    prefix: --speakers

outputs:
- id: results
  type: File
  outputBinding:
    glob: results.csv

baseCommand:
- python
- calculation.py
";

const PLOT_FILE: &str = r"#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: plot.py
    entry:
      $include: ../../plot.py

inputs:
- id: results
  type: File
  default:
    class: File
    location: ../../results.csv
  inputBinding:
    prefix: --results

outputs:
- id: results
  type: File
  outputBinding:
    glob: results.svg

baseCommand:
- python
- plot.py
";
