#![allow(clippy::disallowed_macros)]
use dircpy::copy_dir;
use s4n::commands::{LocalExecuteArgs, execute_local};
use serial_test::serial;
use std::{
    env,
    fs::{self},
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[tokio::test]
#[serial]
pub async fn test_execute_local() {
    let dir = tempdir().unwrap();
    let args = LocalExecuteArgs {
        file: PathBuf::from("../../testdata/echo.cwl")
            .canonicalize()
            .unwrap(),
        out_dir: Some(dir.path().to_path_buf()),
        ..Default::default()
    };

    execute_local(&args)
        .await
        .expect("Could not execute CommandLineTool");
    let file = dir.path().join("results.txt");
    assert!(file.exists());

    //check file validity
    let contents = fs::read_to_string(file).unwrap();
    let expected = include_str!("../../../testdata/input.txt");

    assert_eq!(contents, expected);
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_with_args() {
    let dir = tempdir().unwrap();
    let input_alt = PathBuf::from("../../testdata/input_alt.txt")
        .canonicalize()
        .unwrap();
    let args = LocalExecuteArgs {
        file: PathBuf::from("../../testdata/echo.cwl")
            .canonicalize()
            .unwrap(),
        out_dir: Some(dir.path().to_path_buf()),
        args: vec![
            "--test".to_string(),
            input_alt.to_string_lossy().into_owned(),
        ],
        ..Default::default()
    };

    execute_local(&args)
        .await
        .expect("Could not execute CommandLineTool");

    let file = dir.path().join("results.txt");
    assert!(file.exists());

    //check file validity
    let contents = fs::read_to_string(file).unwrap();
    let expected = include_str!("../../../testdata/input_alt.txt");

    assert_eq!(contents, expected);
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_with_file() {
    let dir = tempdir().unwrap();
    let job_file = PathBuf::from("../../testdata/echo-job.yml")
        .canonicalize()
        .unwrap();
    let args = LocalExecuteArgs {
        file: PathBuf::from("../../testdata/echo.cwl")
            .canonicalize()
            .unwrap(),
        out_dir: Some(dir.path().to_path_buf()),
        args: vec![job_file.to_string_lossy().into_owned()],
        ..Default::default()
    };

    execute_local(&args)
        .await
        .expect("Could not execute CommandLineTool");

    let file = dir.path().join("results.txt");
    assert!(file.exists());

    //check file validity
    let contents = fs::read_to_string(file).unwrap();
    let expected = include_str!("../../../testdata/input_alt.txt");

    assert_eq!(contents, expected);
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_outdir() {
    let dir = tempdir().unwrap();
    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        file: PathBuf::from("../../testdata/echo.cwl")
            .canonicalize()
            .unwrap(),
        ..Default::default()
    };

    execute_local(&args)
        .await
        .expect("Could not execute CommandLineTool");

    let file = dir.path().join("results.txt");
    assert!(file.exists());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_is_quiet() {
    let dir = tempdir().unwrap();
    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: PathBuf::from("../../testdata/echo.cwl")
            .canonicalize()
            .unwrap(),
        ..Default::default()
    };

    execute_local(&args)
        .await
        .expect("Could not execute CommandLineTool");

    //does not really test if it is quiet but rather that the process works
    let file = dir.path().join("results.txt");
    assert!(file.exists());
}

#[tokio::test]
#[serial]
//docker not working on MacOS Github Actions
#[cfg_attr(target_os = "macos", ignore)]
pub async fn test_execute_local_workflow() {
    let folder = "../../testdata/hello_world";

    let dir = tempdir().unwrap();
    copy_dir(folder, dir.path()).unwrap();

    let inputs_yml = dir.path().join("inputs.yml").canonicalize().unwrap();
    let args = LocalExecuteArgs {
        file: dir
            .path()
            .join("workflows/main/main.cwl")
            .canonicalize()
            .unwrap(),
        out_dir: Some(dir.path().to_path_buf()),
        args: vec![inputs_yml.to_string_lossy().into_owned()],
        ..Default::default()
    };
    let result = execute_local(&args).await;
    println!("{result:#?}");
    assert!(result.is_ok());

    //check if file is written which means wf ran completely
    let path = dir.path().join("results.svg");
    assert!(path.exists());
}

#[tokio::test]
#[serial]
#[cfg(not(target_os = "windows"))] //file system issues with windows
pub async fn test_execute_local_tool_default_cwl() {
    let path = PathBuf::from("../../testdata/default.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_file = format!("{}/file.wtf", &out_dir);
    let out_file2 = format!("{}/file_2.wtf", &out_dir);

    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: path.clone(),
        ..Default::default()
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let override_path = root.join("../../testdata/input.txt");
    let args_override = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: path,
        args: vec![
            "--file1".to_string(),
            override_path.to_string_lossy().into_owned(),
        ],
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(&out_file).unwrap());
    let contents = fs::read_to_string(&out_file).unwrap();
    assert_eq!(contents, "File".to_string());

    assert!(execute_local(&args_override).await.is_ok());
    assert!(fs::exists(&out_file2).unwrap());
    let contents = fs::read_to_string(&out_file2).unwrap();
    assert_eq!(contents, "Hello fellow CWL-enjoyers!".to_string());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_workflow_no_steps() {
    //has no steps, do not complain!
    let path = PathBuf::from("../../testdata/wf_inout.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_path_buf();

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
        is_quiet: true,
        file: path,
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
}

#[tokio::test]
#[serial]
#[cfg(not(target_os = "windows"))] //file system issues with windows
pub async fn test_execute_local_workflow_in_param() {
    let path = PathBuf::from("../../testdata/test-wf_features.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_file = format!("{}/file.wtf", &out_dir);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input_file_path = root.join("../../testdata/input.txt");
    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: path,
        args: vec![
            "--pop".to_string(),
            input_file_path.to_string_lossy().into_owned(),
        ],
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(&out_file).unwrap());
    let contents = fs::read_to_string(&out_file).unwrap();
    assert_eq!(contents, "Hello fellow CWL-enjoyers!".to_string());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_workflow_dir_out() {
    //has no steps, do not complain!
    let path = PathBuf::from("../../testdata/wf_inout_dir.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_path = format!("{}/test_dir", &out_dir);

    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: path,
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(format!("{out_path}/file.txt")).unwrap());
    assert!(fs::exists(format!("{out_path}/input.txt")).unwrap());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_workflow_file_out() {
    //has no steps, do not complain!
    let path = PathBuf::from("../../testdata/wf_inout_file.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_path = format!("{out_dir}/file.txt");

    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: path,
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(out_path).unwrap());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_workflow_directory_out() {
    let path = PathBuf::from("../../testdata/mkdir_wf.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_path_buf();

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
        is_quiet: true,
        file: path,
        args: vec!["--dirname".to_string(), "test_directory".to_string()],
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok()); //TODO: test fails
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_with_binary_input() {
    let path = PathBuf::from("../../testdata/read_bin.cwl")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_path = format!("{}/output.txt", &out_dir);

    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_path_buf()),
        is_quiet: true,
        file: path,
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(&out_path).unwrap());
    let contents = fs::read_to_string(&out_path).unwrap();
    assert_eq!(contents, "69420".to_string());
}
