use sciwin::rocrate::RoCrate;
use std::process::Command;

#[test]
#[cfg_attr(target_os = "macos", ignore)] // docker used, MACOS CI Issues
fn test_execute_rocrate_export_then_rerun_roundtrip() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let hello_world_src = root.join("../../testdata/hello_world");

    // A fresh project dir `--rocrate` can export from -- it needs a `workflow.toml` to read
    // crate metadata out of.
    let project_dir = tempfile::tempdir().unwrap();
    dircpy::copy_dir(&hello_world_src, project_dir.path()).unwrap();
    std::fs::write(
        project_dir.path().join("workflow.toml"),
        "[workflow]\nname = \"hello_world\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let rocrate_dir = tempfile::tempdir().unwrap();

    let export_status = Command::new(env!("CARGO_BIN_EXE_s4n"))
        .current_dir(project_dir.path())
        .args(["execute", "run", "workflows/main/main.cwl", "inputs.yml"])
        .arg("--rocrate")
        .arg("--rocrate_dir")
        .arg(rocrate_dir.path())
        .status()
        .expect("failed to spawn s4n");
    assert!(export_status.success(), "exporting the run crate failed");

    let crate_ = RoCrate::from_directory(rocrate_dir.path()).expect("crate should parse");
    for part in crate_.missing_parts(rocrate_dir.path()) {
        std::fs::write(rocrate_dir.path().join(part), b"").unwrap();
    }

    let zip_path = project_dir.path().join("run.crate.zip");
    crate_
        .write_zip(&zip_path, rocrate_dir.path())
        .expect("zipping the crate should succeed now nothing is missing");

    let rerun_cwd = tempfile::tempdir().unwrap();
    let job_file = hello_world_src.join("inputs.yml");
    let rerun_status = Command::new(env!("CARGO_BIN_EXE_s4n"))
        .current_dir(rerun_cwd.path())
        .arg("execute")
        .arg("run")
        .arg(&zip_path)
        .arg(&job_file)
        .status()
        .expect("failed to spawn s4n");
    assert!(
        rerun_status.success(),
        "re-running the exported crate failed"
    );
}
