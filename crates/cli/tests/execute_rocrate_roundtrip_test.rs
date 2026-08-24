use s4n::commands::{ContainerRuntime, ExecutionEngine, RocrateLayout, RunArgs, execute_run};
use sciwin::rocrate::RoCrate;
use serial_test::serial;
use std::path::PathBuf;

#[tokio::test]
#[serial] // mutates the process cwd for the export step
#[cfg_attr(target_os = "macos", ignore)] // docker used, MACOS CI Issues
async fn test_execute_rocrate_export_then_rerun_roundtrip() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let hello_world_src = root.join("../../testdata/hello_world");

    let project_dir = tempfile::tempdir().unwrap();
    dircpy::copy_dir(&hello_world_src, project_dir.path()).unwrap();
    std::fs::write(
        project_dir.path().join("workflow.toml"),
        "[workflow]\nname = \"hello_world\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let rocrate_dir = tempfile::tempdir().unwrap();
    let export_out_dir = tempfile::tempdir().unwrap();

    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(project_dir.path()).unwrap();
    let export_result = execute_run(&RunArgs {
        engine: ExecutionEngine::Local,
        runtime: ContainerRuntime::Docker,
        out_dir: Some(export_out_dir.path().to_path_buf()),
        file: PathBuf::from("workflows/main/main.cwl"),
        input_file: Some(PathBuf::from("inputs.yml")),
        rocrate: Some(RocrateLayout::Files),
        rocrate_dir: rocrate_dir.path().to_path_buf(),
        detach: false,
    })
    .await;
    std::env::set_current_dir(&original_cwd).unwrap();
    export_result.expect("exporting the run crate failed");

    // Step-to-step intermediates have no real file to copy in; stand in empty placeholders so
    // `write_zip` doesn't refuse a crate missing a declared part.
    let crate_ = RoCrate::from_directory(rocrate_dir.path()).expect("crate should parse");
    for part in crate_.missing_parts(rocrate_dir.path()) {
        std::fs::write(rocrate_dir.path().join(part), b"").unwrap();
    }

    let zip_path = project_dir.path().join("run.crate.zip");
    crate_
        .write_zip(&zip_path, rocrate_dir.path())
        .expect("zipping the crate should succeed now nothing is missing");

    let rerun_out_dir = tempfile::tempdir().unwrap();
    let rerun_result = execute_run(&RunArgs {
        engine: ExecutionEngine::Local,
        runtime: ContainerRuntime::Docker,
        out_dir: Some(rerun_out_dir.path().to_path_buf()),
        file: zip_path,
        input_file: None,
        rocrate: None,
        rocrate_dir: PathBuf::from("./rocrate"),
        detach: false,
    })
    .await;
    rerun_result.expect("re-running the exported crate failed");
}
