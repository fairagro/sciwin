#![allow(clippy::disallowed_macros)]
mod common;

use commonwl::requirements::SubworkflowFeatureRequirement;
use common::{load_workflow, tool_path, with_workflow};
use sciwin::authoring::workflow::{
    WorkflowSlot, add_workflow_input_connection, add_workflow_output_connection,
    add_workflow_step_connection, create_workflow, remove_workflow_input_connection,
    remove_workflow_output_connection, remove_workflow_step_connection,
};
use sciwin::project::initialize_project;
use serial_test::serial;
use std::{env, fs, io::Write, path::Path};
use tempfile::tempdir;

fn create_and_write_file(path: impl AsRef<Path>, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

#[test]
#[serial]
pub fn test_create_workflow() {
    let dir = tempdir().unwrap();
    let current = env::current_dir().unwrap();

    env::set_current_dir(dir.path()).unwrap();
    let result = create_workflow("test", None, false);
    assert!(result.is_ok());

    let path = "workflows/test/test.cwl";
    assert!(Path::new(path).exists());

    env::set_current_dir(current).unwrap();
}

#[test]
#[serial]
pub fn test_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir().unwrap();
    let current = env::current_dir().unwrap();

    env::set_current_dir(dir.path()).unwrap();
    initialize_project("").unwrap();

    create_and_write_file("workflows/calculation/calculation.cwl", CALCULATION_FILE).unwrap();
    create_and_write_file("workflows/plot/plot.cwl", PLOT_FILE).unwrap();

    create_workflow("test", None, false)?;

    let test_path = tool_path("test");
    let calculation = tool_path("calculation");
    let plot = tool_path("plot");

    with_workflow("test", |wf| {
        add_workflow_input_connection(
            wf,
            &test_path,
            "speakers",
            WorkflowSlot::new(&calculation, "calculation", "speakers"),
        )
        .unwrap();
        add_workflow_input_connection(
            wf,
            &test_path,
            "pop",
            WorkflowSlot::new(&calculation, "calculation", "population"),
        )
        .unwrap();
        add_workflow_step_connection(
            wf,
            &test_path,
            WorkflowSlot::new(&calculation, "calculation", "results"),
            WorkflowSlot::new(&plot, "plot", "results"),
        )
        .unwrap();
        add_workflow_output_connection(
            wf,
            &test_path,
            WorkflowSlot::new(&plot, "plot", "results"),
            "out",
        )
        .unwrap();
    });

    //connect to another dummy workflow to check subworkflows work
    create_workflow("dummy", None, false)?;
    let dummy_path = tool_path("dummy");
    with_workflow("dummy", |wf| {
        add_workflow_input_connection(
            wf,
            &dummy_path,
            "speakers",
            WorkflowSlot::new(&test_path, "test", "speakers"),
        )
        .unwrap();
    });

    let wf = load_workflow("dummy");
    assert!(
        wf.get_requirement::<SubworkflowFeatureRequirement>()
            .is_some()
    );

    let workflow = load_workflow("test");
    assert!(workflow.has_input("speakers"));
    assert!(workflow.has_input("pop"));
    assert!(workflow.has_output("out"));
    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

    with_workflow("test", |wf| {
        remove_workflow_input_connection(wf, "speakers", "calculation", "speakers", true)
            .unwrap();
        remove_workflow_input_connection(wf, "pop", "calculation", "population", true).unwrap();
        remove_workflow_step_connection(wf, "calculation", "results", "plot", "results").unwrap();
        remove_workflow_output_connection(wf, "plot", "results", "out", true).unwrap();
    });

    // Reload the workflow and validate disconnections
    let workflow = load_workflow("test");
    assert!(!workflow.has_input("speakers"));
    assert!(!workflow.has_input("pop"));
    assert!(!workflow.has_output("out"));
    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

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
