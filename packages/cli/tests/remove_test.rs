#![allow(clippy::disallowed_macros)]
use repository::Repository;
use repository::get_modified_files;
use s4n::cli::Commands;
use s4n::commands::*;
use serial_test::serial;
use std::path::Path;
use test_utils::with_temp_repository;

#[test]
#[serial]
fn test_remove_non_existing_tool() {
    let args = RemoveCWLArgs {
        file: "non_existing_tool".to_string(),
    };

    let result = handle_remove_command(&args);
    assert!(result.is_err());
}

#[test]
#[serial]
pub fn tool_remove_test() {
    with_temp_repository(|dir: &tempfile::TempDir| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let tool_create_args = CreateArgs {
                name: Some("echo".to_string()),
                command: vec![
                    "python".to_string(),
                    "scripts/echo.py".to_string(),
                    "--test".to_string(),
                    "data/input.txt".to_string(),
                ],
                ..Default::default()
            };
            let cmd_create = Commands::Create(tool_create_args);
            if let Commands::Create(ref args) = cmd_create {
                assert!(handle_create_command(args).await.is_ok());
            }
            assert!(dir.path().join(Path::new("workflows/echo")).exists());
            let args = RemoveCWLArgs {
                file: "echo".to_string(),
            };
            let cmd_remove = handle_remove_command(&args);
            assert!(cmd_remove.is_ok(), "Removing tool should succeed");
            assert!(
                !dir.path()
                    .join(Path::new("workflows/echo/echo.cwl"))
                    .exists()
            );
            assert!(!dir.path().join(Path::new("workflows/echo")).exists());
            let repo = Repository::open(dir.path()).unwrap();
            assert!(get_modified_files(&repo).is_empty());
        });
    });
}
