#![allow(clippy::disallowed_macros)]
mod common;

use common::{copy_dir, local_runner};
use commonwl::{
    engine::{InputObject, load_input_file_from_file},
    files::{File, FileOrDirectory},
    inputs::DefaultValue,
};
use sciwin::execution::{RunStatus, WorkflowRunner, rocrate};
use serde_json::Value;
use std::{collections::HashMap, fs, path::PathBuf};
use tempfile::tempdir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn file_input(location: impl Into<String>) -> DefaultValue {
    DefaultValue::FileOrDirectory(FileOrDirectory::File(
        File::builder().location(location.into()).build(),
    ))
}

#[tokio::test]
pub async fn test_execute_local() {
    let dir = tempdir().unwrap();
    let runner = local_runner();
    let file = PathBuf::from("../../testdata/echo.cwl")
        .canonicalize()
        .unwrap();

    runner
        .run_workflow(&file, InputObject::default(), Some(dir.path()))
        .await
        .expect("Could not execute CommandLineTool");

    let out_file = dir.path().join("results.txt");
    assert!(out_file.exists());

    let contents = fs::read_to_string(out_file).unwrap();
    let expected = include_str!("../../../testdata/input.txt");
    assert_eq!(contents, expected);
}

#[tokio::test]
pub async fn test_execute_local_with_args() {
    let dir = tempdir().unwrap();
    let runner = local_runner();
    let file = PathBuf::from("../../testdata/echo.cwl")
        .canonicalize()
        .unwrap();
    let input_alt = PathBuf::from("../../testdata/input_alt.txt")
        .canonicalize()
        .unwrap();

    let inputs = InputObject {
        inputs: HashMap::from([(
            "test".to_string(),
            file_input(input_alt.to_string_lossy().into_owned()),
        )]),
        ..Default::default()
    };

    runner
        .run_workflow(&file, inputs, Some(dir.path()))
        .await
        .expect("Could not execute CommandLineTool");

    let out_file = dir.path().join("results.txt");
    assert!(out_file.exists());

    let contents = fs::read_to_string(out_file).unwrap();
    let expected = include_str!("../../../testdata/input_alt.txt");
    assert_eq!(contents, expected);
}

#[tokio::test]
pub async fn test_execute_local_with_file() {
    let dir = tempdir().unwrap();
    let runner = local_runner();
    let file = PathBuf::from("../../testdata/echo.cwl")
        .canonicalize()
        .unwrap();
    let job_file = PathBuf::from("../../testdata/echo-job.yml")
        .canonicalize()
        .unwrap();

    let inputs = load_input_file_from_file(&job_file, file.parent().unwrap()).unwrap();

    runner
        .run_workflow(&file, inputs, Some(dir.path()))
        .await
        .expect("Could not execute CommandLineTool");

    let out_file = dir.path().join("results.txt");
    assert!(out_file.exists());

    let contents = fs::read_to_string(out_file).unwrap();
    let expected = include_str!("../../../testdata/input_alt.txt");
    assert_eq!(contents, expected);
}

#[tokio::test]
pub async fn test_execute_local_outdir() {
    let dir = tempdir().unwrap();
    let runner = local_runner();
    let file = PathBuf::from("../../testdata/echo.cwl")
        .canonicalize()
        .unwrap();

    runner
        .run_workflow(&file, InputObject::default(), Some(dir.path()))
        .await
        .expect("Could not execute CommandLineTool");

    assert!(dir.path().join("results.txt").exists());
}

#[tokio::test]
//docker not working on MacOS Github Actions
#[cfg_attr(target_os = "macos", ignore)]
pub async fn test_execute_local_workflow() {
    let folder = "../../testdata/hello_world";

    let dir = tempdir().unwrap();
    copy_dir(folder, dir.path());

    let file = dir
        .path()
        .join("workflows/main/main.cwl")
        .canonicalize()
        .unwrap();
    let job_file = dir.path().join("inputs.yml").canonicalize().unwrap();
    let inputs = load_input_file_from_file(&job_file, file.parent().unwrap()).unwrap();

    let runner = local_runner();
    let result = runner.run_workflow(&file, inputs, Some(dir.path())).await;
    println!("{result:#?}");
    assert!(result.is_ok());

    //check if file is written which means wf ran completely
    assert!(dir.path().join("results.svg").exists());
}

#[tokio::test]
#[cfg(not(target_os = "windows"))] //file system issues with windows
pub async fn test_execute_local_tool_default_cwl() {
    let file = PathBuf::from("../../testdata/default.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_file = dir.path().join("file.wtf");
    let out_file2 = dir.path().join("file_2.wtf");

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let override_path = root.join("../../testdata/input.txt");
    let inputs = InputObject {
        inputs: HashMap::from([(
            "file1".to_string(),
            file_input(override_path.to_string_lossy().into_owned()),
        )]),
        ..Default::default()
    };

    let runner = local_runner();
    runner
        .run_workflow(&file, InputObject::default(), Some(dir.path()))
        .await
        .unwrap();
    assert!(out_file.exists());
    let contents = fs::read_to_string(&out_file).unwrap();
    assert_eq!(contents, "File".to_string());

    runner
        .run_workflow(&file, inputs, Some(dir.path()))
        .await
        .unwrap();
    assert!(out_file2.exists());
    let contents = fs::read_to_string(&out_file2).unwrap();
    assert_eq!(contents, "Hello fellow CWL-enjoyers!".to_string());
}

#[tokio::test]
pub async fn test_execute_local_workflow_no_steps() {
    //has no steps, do not complain!
    let file = PathBuf::from("../../testdata/wf_inout.cwl");
    let dir = tempdir().unwrap();

    let runner = local_runner();
    assert!(
        runner
            .run_workflow(&file, InputObject::default(), Some(dir.path()))
            .await
            .is_ok()
    );
}

#[tokio::test]
#[cfg(not(target_os = "windows"))] //file system issues with windows
pub async fn test_execute_local_workflow_in_param() {
    let file = PathBuf::from("../../testdata/test-wf_features.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_file = dir.path().join("file.wtf");

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let input_file_path = root.join("../../testdata/input.txt");
    let inputs = InputObject {
        inputs: HashMap::from([(
            "pop".to_string(),
            file_input(input_file_path.to_string_lossy().into_owned()),
        )]),
        ..Default::default()
    };

    let runner = local_runner();
    runner
        .run_workflow(&file, inputs, Some(dir.path()))
        .await
        .unwrap();
    assert!(out_file.exists());
    let contents = fs::read_to_string(&out_file).unwrap();
    assert_eq!(contents, "Hello fellow CWL-enjoyers!".to_string());
}

#[tokio::test]
pub async fn test_execute_local_workflow_dir_out() {
    let file = PathBuf::from("../../testdata/wf_inout_dir.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_path = dir.path().join("test_dir");

    let runner = local_runner();
    runner
        .run_workflow(&file, InputObject::default(), Some(dir.path()))
        .await
        .unwrap();
    assert!(out_path.join("file.txt").exists());
    assert!(out_path.join("input.txt").exists());
}

#[tokio::test]
pub async fn test_execute_local_workflow_file_out() {
    let file = PathBuf::from("../../testdata/wf_inout_file.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();

    let runner = local_runner();
    runner
        .run_workflow(&file, InputObject::default(), Some(dir.path()))
        .await
        .unwrap();
    assert!(dir.path().join("file.txt").exists());
}

#[tokio::test]
pub async fn test_execute_local_workflow_directory_out() {
    let file = PathBuf::from("../../testdata/mkdir_wf.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();

    let inputs = InputObject {
        inputs: HashMap::from([(
            "dirname".to_string(),
            DefaultValue::Any(Value::String("test_directory".to_string())),
        )]),
        ..Default::default()
    };

    let runner = local_runner();
    let res = runner.run_workflow(&file, inputs, Some(dir.path())).await;
    dbg!(&res);
    assert!(res.is_ok());
}

#[tokio::test]
pub async fn test_execute_local_with_binary_input() {
    let file = PathBuf::from("../../testdata/read_bin.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_path = dir.path().join("output.txt");

    let runner = local_runner();
    runner
        .run_workflow(&file, InputObject::default(), Some(dir.path()))
        .await
        .unwrap();
    assert!(out_path.exists());
    let contents = fs::read_to_string(&out_path).unwrap();
    assert_eq!(contents, "69420".to_string());
}

/// Moved from `execution::task_runner`'s unit tests: it runs a real workflow end to end
/// through the local engine, which is integration-test territory, not a narrow unit check.
#[tokio::test]
#[cfg_attr(target_os = "macos", ignore)] //docker used, MACOS CI Issues
async fn test_execution_commonwl() {
    let runner = local_runner();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=debug,cwl_engine=info,crankshaft=warn",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init()
        .ok();

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
    let base_path = specification_path.parent().unwrap();
    let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

    let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

    //dumpster for outputs
    let tmpdir = tempdir().unwrap();
    let result = runner
        .run_workflow(&specification_path, inputs, Some(tmpdir.path()))
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
#[cfg_attr(target_os = "macos", ignore)] //docker used, MACOS CI Issues
async fn test_execute_rocrate_zip() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let zip_path = root.join("../../testdata/test_workflow.zip");
    let job_path = root.join("../../testdata/hello_world/inputs.yml");

    let resolved =
        rocrate::resolve_target(&zip_path).expect("should resolve the Workflow RO-Crate");

    // Mirrors `execute_run`'s own merge: crate defaults first, the job file layered on top.
    let base_path = resolved.cwl_path.parent().unwrap();
    let overrides = load_input_file_from_file(job_path, base_path).unwrap();
    let mut inputs = resolved.default_inputs.clone();
    inputs.inputs.extend(overrides.inputs);
    inputs.requirements = overrides.requirements;
    inputs.hints = overrides.hints;

    let runner = local_runner();
    let tmpdir = tempdir().unwrap();
    let result = runner
        .run_workflow(&resolved.cwl_path, inputs, Some(tmpdir.path()))
        .await;

    assert!(result.is_ok(), "{:?}", result.err());
}

/// Moved from `execution::task_runner`'s unit tests alongside `test_execution_commonwl`.
#[tokio::test]
#[cfg_attr(target_os = "macos", ignore)] //docker used, MACOS CI Issues
async fn test_cancel_commonwl() {
    let runner = local_runner();

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
    let base_path = specification_path.parent().unwrap();
    let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

    let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

    let run_id = runner
        .submit(&specification_path, inputs, None)
        .await
        .unwrap();

    runner.cancel(&run_id).await.unwrap();
    let status = runner.wait_for_completion(&run_id).await.unwrap();

    assert!(matches!(status, RunStatus::Cancelled));
}
