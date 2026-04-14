#![allow(clippy::disallowed_macros)]
use commonwl::execution::io::copy_dir;
use s4n::commands::{LocalExecuteArgs, execute_local};
use serial_test::serial;
use std::{
    env,
    fs::{self},
    iter,
    path::{Path, PathBuf},
};
use tempfile::tempdir;
use test_utils::repository;

#[tokio::test]
#[serial]
pub async fn test_execute_local() {
    let dir = tempdir().unwrap();
    let current = repository(dir.path())
        .copy_file("testdata/echo.cwl", "echo.cwl")
        .copy_file("testdata/echo.py", "echo.py")
        .copy_file("testdata/input.txt", "input.txt")
        .finalize()
        .enter();

    let args = LocalExecuteArgs {
        file: PathBuf::from("echo.cwl"),
        ..Default::default()
    };

    execute_local(&args).await.expect("Could not execute CommandLineTool");

    let file = Path::new("results.txt");
    assert!(file.exists());

    //check file validity
    let contents = fs::read_to_string(file).unwrap();
    let expected = include_str!("../../../testdata/input.txt");

    assert_eq!(contents, expected);
    env::set_current_dir(current).unwrap();
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_with_args() {
    let dir = tempdir().unwrap();
    let current = repository(dir.path())
        .copy_file("testdata/echo.cwl", "echo.cwl")
        .copy_file("testdata/echo.py", "echo.py")
        .copy_file("testdata/input_alt.txt", "input_alt.txt")
        .finalize()
        .enter();

    let args = LocalExecuteArgs {
        file: PathBuf::from("echo.cwl"),
        args: ["--test", "input_alt.txt"].iter().map(ToString::to_string).collect::<Vec<_>>(),
        ..Default::default()
    };

    execute_local(&args).await.expect("Could not execute CommandLineTool");

    let file = Path::new("results.txt");
    assert!(file.exists());

    //check file validity
    let contents = fs::read_to_string(file).unwrap();
    let expected = include_str!("../../../testdata/input_alt.txt");

    assert_eq!(contents, expected);
    env::set_current_dir(current).unwrap();
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_with_file() {
    let dir = tempdir().unwrap();
    let current = repository(dir.path())
        .copy_file("testdata/echo.cwl", "echo.cwl")
        .copy_file("testdata/echo.py", "echo.py")
        .copy_file("testdata/echo-job.yml", "echo-job.yml")
        .copy_file("testdata/input_alt.txt", "input_alt.txt")
        .finalize()
        .enter();

    let args = LocalExecuteArgs {
        file: PathBuf::from("echo.cwl"),
        args: iter::once(&"echo-job.yml").map(ToString::to_string).collect::<Vec<_>>(),
        ..Default::default()
    };

    execute_local(&args).await.expect("Could not execute CommandLineTool");

    let file = Path::new("results.txt");
    assert!(file.exists());

    //check file validity
    let contents = fs::read_to_string(file).unwrap();
    let expected = include_str!("../../../testdata/input_alt.txt");

    assert_eq!(contents, expected);
    env::set_current_dir(current).unwrap();
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_outdir() {
    let dir = tempdir().unwrap();
    let current = repository(dir.path())
        .copy_file("testdata/echo.cwl", "echo.cwl")
        .copy_file("testdata/echo.py", "echo.py")
        .copy_file("testdata/input.txt", "input.txt")
        .finalize()
        .enter();

    let dir = tempdir().unwrap();
    let args = LocalExecuteArgs {
        out_dir: Some(dir.path().to_string_lossy().into_owned()),
        file: PathBuf::from("echo.cwl"),
        ..Default::default()
    };

    execute_local(&args).await.expect("Could not execute CommandLineTool");

    let file = dir.path().join("results.txt");

    assert!(file.exists());
    env::set_current_dir(current).unwrap();
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_is_quiet() {
    let dir = tempdir().unwrap();
    let current = repository(dir.path())
        .copy_file("testdata/echo.cwl", "echo.cwl")
        .copy_file("testdata/echo.py", "echo.py")
        .copy_file("testdata/input.txt", "input.txt")
        .finalize()
        .enter();

    //does not really test if it is quiet but rather that the process works
    let args = LocalExecuteArgs {
        is_quiet: true,
        file: PathBuf::from("echo.cwl"),
        ..Default::default()
    };

    execute_local(&args).await.expect("Could not execute CommandLineTool");

    let file = Path::new("results.txt");

    assert!(file.exists());
    env::set_current_dir(current).unwrap();
}

#[tokio::test]
#[serial]
//docker not working on MacOS Github Actions
#[cfg_attr(target_os = "macos", ignore)]
pub async fn test_execute_local_workflow() {
    let folder = "../../testdata/hello_world";

    let dir = tempdir().unwrap();
    let dir_str = &dir.path().to_string_lossy();
    copy_dir(folder, dir.path()).unwrap();

    let current_dir = env::current_dir().unwrap();
    env::set_current_dir(dir.path()).unwrap();
    //execute workflow
    let args = LocalExecuteArgs {
        file: PathBuf::from(format!("{dir_str}/workflows/main/main.cwl")),
        args: vec!["inputs.yml".to_string()],
        ..Default::default()
    };
    let result = execute_local(&args).await;
    println!("{result:#?}");
    assert!(result.is_ok());

    //check if file is written which means wf ran completely
    let results_url = format!("{dir_str}/results.svg");
    let path = Path::new(&results_url);
    assert!(path.exists());

    env::set_current_dir(current_dir).unwrap();
}

#[tokio::test]
#[serial]
#[cfg(not(target_os = "windows"))] //file system issues with windows
pub async fn test_execute_local_tool_default_cwl() {
    let path = PathBuf::from("../../testdata/default.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_file = format!("{}/file.wtf", &out_dir);

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir.clone()),
        is_quiet: true,
        file: path.clone(),
        ..Default::default()
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let override_path = root.join("../../testdata/input.txt");
    let args_override = LocalExecuteArgs {
        out_dir: Some(out_dir),
        is_quiet: true,
        file: path,
        args: vec!["--file1".to_string(), override_path.to_string_lossy().into_owned()],
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(&out_file).unwrap());
    let contents = fs::read_to_string(&out_file).unwrap();
    assert_eq!(contents, "File".to_string());
    
    assert!(execute_local(&args_override).await.is_ok());
    assert!(fs::exists(&out_file).unwrap());
    let contents = fs::read_to_string(&out_file).unwrap();
    assert_eq!(contents, "Hello fellow CWL-enjoyers!".to_string());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_workflow_no_steps() {
    //has no steps, do not complain!
    let path = PathBuf::from("../../testdata/wf_inout.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();

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
    let path = PathBuf::from("../../testdata/test-wf_features.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_file = format!("{}/file.wtf", &out_dir);

    
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input_file_path = root.join("../../testdata/input.txt");
    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
        is_quiet: true,
        file: path,
        args: vec!["--pop".to_string(), input_file_path.to_string_lossy().into_owned()],
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
    let path = PathBuf::from("../../testdata/wf_inout_dir.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_path = format!("{}/test_dir", &out_dir);

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
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
    let path = PathBuf::from("../../testdata/wf_inout_file.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_path = format!("{out_dir}/file.txt");

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
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
    let path = PathBuf::from("../../testdata/mkdir_wf.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
        is_quiet: true,
        file: path,
        args: vec!["--dirname".to_string(), "test_directory".to_string()],
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
}

#[tokio::test]
#[serial]
pub async fn test_execute_local_with_binary_input() {
    let path = PathBuf::from("../../testdata/read_bin.cwl");
    let dir = tempdir().unwrap();
    let out_dir = dir.path().to_string_lossy().into_owned();
    let out_path = format!("{}/output.txt", &out_dir);

    let args = LocalExecuteArgs {
        out_dir: Some(out_dir),
        is_quiet: true,
        file: path,
        ..Default::default()
    };

    assert!(execute_local(&args).await.is_ok());
    assert!(fs::exists(&out_path).unwrap());
    let contents = fs::read_to_string(&out_path).unwrap();
    assert_eq!(contents, "69420".to_string());
}
