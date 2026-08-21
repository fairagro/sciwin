#![allow(clippy::disallowed_macros)]
mod common;

use common::{copy_dir, os_path};
use commonwl::{
    OneOrMany,
    documents::Argument,
    requirements::{
        DockerRequirement, Include, InitialWorkDirRequirement, NetworkAccess, StringOrInclude,
        WorkDirItems,
    },
    types::CWLType,
};
use sciwin::authoring::{AuthoringError, tool::auto_container_engine};
use sciwin::authoring::tool::{ContainerInfo, ToolCreationOptions, create_tool};
use sciwin::repository::{self, Repository};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::{TempDir, tempdir};

/// Builds an isolated project in a fresh tempdir: `files` are copied in by basename
/// (mirroring the old `#[fstest(files = [...])]`) and committed to a real git repo.
fn workspace(files: &[&str]) -> TempDir {
    let dir = tempdir().unwrap();
    for file in files {
        let src = Path::new(file);
        fs::copy(src, dir.path().join(src.file_name().unwrap())).unwrap();
    }
    fstest::create_repo_and_commit(dir.path()).unwrap();
    dir
}

fn echo_options() -> ToolCreationOptions {
    ToolCreationOptions::builder()
        .command(vec!["echo".to_string(), "hello".to_string()])
        .build()
}

/// A dirty working tree is a state a frontend acts on (offer to commit, or to re-run
/// with `no_run`), so it must arrive as a matchable variant, not a formatted string.
#[tokio::test]
pub async fn test_dirty_working_tree_is_a_typed_variant() {
    let dir = workspace(&["../../testdata/input.txt"]);
    let root = dir.path();
    fs::write(root.join("uncommitted.txt"), "scratch").unwrap();

    let err = create_tool(root, &echo_options()).await.unwrap_err();

    let AuthoringError::DirtyWorkingTree { files } = &err else {
        panic!("expected DirtyWorkingTree, got {err:?}");
    };
    assert!(files.iter().any(|f| f == "uncommitted.txt"));
}

/// Same reasoning: a missing repository is a state a frontend offers to fix.
#[tokio::test]
pub async fn test_missing_repository_is_a_typed_variant() {
    let dir = tempdir().unwrap(); // no repo here
    let err = create_tool(dir.path(), &echo_options()).await.unwrap_err();

    assert!(
        matches!(err, AuthoringError::NoRepository { .. }),
        "expected NoRepository, got {err:?}"
    );
}

/// The tool is written where `path` says, relative to the project root -- which this whole
/// file never conflates with the process's cwd (it's never touched).
#[tokio::test]
pub async fn test_saves_relative_to_project_root() {
    let dir = workspace(&["../../testdata/input.txt"]);
    let root = dir.path();

    let options = ToolCreationOptions::builder()
        .command(vec!["echo".to_string(), "hello".to_string()])
        .no_run(true)
        .save(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.path, Path::new("workflows/echo/echo.cwl"));
    assert!(root.join(&created.path).is_file());
    //the document is handed back alongside the YAML, not just the string
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::One("echo".to_string()))
    );
    assert_eq!(
        created.yaml,
        fs::read_to_string(root.join(&created.path)).unwrap()
    );
}

#[tokio::test]
pub async fn tool_create_test() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .save(true)
        .commit(true)
        .build();
    create_tool(root, &options).await.unwrap();

    assert!(root.join("results.txt").exists());
    assert!(root.join("workflows/echo/echo.cwl").exists());

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_test_inputs_outputs() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo_inline.py"]);
    let root = dir.path();

    fs::create_dir_all(root.join("data")).unwrap();
    fs::copy(root.join("input.txt"), root.join("data/input.txt")).unwrap(); //copy to data folder
    fs::remove_file(root.join("input.txt")).unwrap(); //remove original file

    let repo = Repository::open(root).unwrap();
    repository::stage_all(&repo).unwrap();

    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "echo_inline.py".to_string()])
        .inputs(vec!["data/input.txt".to_string()])
        .outputs(vec!["results.txt".to_string()])
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(
        created.path,
        Path::new("workflows/echo_inline/echo_inline.cwl")
    );
    assert!(root.join("results.txt").exists());
    assert!(root.join(&created.path).is_file());

    assert_eq!(created.document.inputs.len(), 1);
    assert_eq!(created.document.outputs.len(), 1);

    let Some(iwdr) = created
        .document
        .get_requirement::<InitialWorkDirRequirement>()
    else {
        panic!("Tool does not contain an InitialWorkDirRequirement");
    };
    let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
        panic!("InitialWorkDirRequirement does not contain listing items");
    };
    assert_eq!(listing.len(), 2);

    //no uncommitted left?
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_test_no_save() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .commit(true)
        .build();
    create_tool(root, &options).await.unwrap();

    assert!(!root.join("workflows/echo/echo.cwl").exists()); //save was not requested
    assert!(root.join("results.txt").exists());

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_test_no_commit() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .save(true) //look! no .commit(true)
        .build();
    create_tool(root, &options).await.unwrap();

    //check for files being present
    assert!(root.join("results.txt").exists());
    assert!(root.join("workflows/echo/echo.cwl").exists());

    //as we did not commit there must be files (exactly 2, the cwl file and the results.txt)
    let repo = Repository::open(root).unwrap();
    assert_eq!(repository::get_modified_files(&repo).unwrap().len(), 2);
}

#[tokio::test]
pub async fn tool_create_test_no_run() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .no_run(true)
        .save(true)
        .commit(true)
        .build();
    create_tool(root, &options).await.unwrap();

    assert!(root.join("workflows/echo/echo.cwl").exists());

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_test_no_run_explicit_inputs() {
    let dir = workspace(&[
        "../../testdata/input.txt",
        "../../testdata/echo.py",
        "../../testdata/data.bin",
    ]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .inputs(vec!["data.bin".to_string()])
        .no_run(true)
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert!(root.join("workflows/echo/echo.cwl").exists());
    assert!(
        created
            .document
            .inputs
            .iter()
            .any(|i| i.id == Some("data_bin".to_string()))
    );

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_test_no_run_explicit_inputs_string() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .inputs(vec!["wurstbrot".to_string()])
        .no_run(true)
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert!(root.join("workflows/echo/echo.cwl").exists());
    assert!(
        created
            .document
            .inputs
            .iter()
            .any(|i| i.id == Some("wurstbrot".to_string()))
    );

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_test_is_clean() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .cleanup(true)
        .save(true)
        .commit(true)
        .build();
    create_tool(root, &options).await.unwrap();

    assert!(root.join("workflows/echo/echo.cwl").exists());
    assert!(!root.join("results.txt").exists()); //no result is left as it is cleaned

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
#[cfg_attr(target_os = "macos", ignore)] //docker used, MACOS CI Issues
pub async fn tool_create_test_container_image() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .container(ContainerInfo::builder().image("python3").build())
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.requirements.as_ref().unwrap().len(), 2);

    let Some(dr) = created.document.get_requirement::<DockerRequirement>() else {
        panic!("Tool does not contain a DockerRequirement");
    };
    assert_eq!(dr.docker_pull.as_deref(), Some("python3"));

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
#[cfg_attr(target_os = "macos", ignore)] //docker used, MACOS CI Issues
pub async fn tool_create_test_dockerfile() {
    let dir = workspace(&[
        "../../testdata/Dockerfile",
        "../../testdata/input.txt",
        "../../testdata/echo.py",
    ]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .container(
            ContainerInfo::builder()
                .image("Dockerfile")
                .tag("sciwin-client")
                .build(),
        )
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.requirements.as_ref().unwrap().len(), 2);

    let Some(dr) = created.document.get_requirement::<DockerRequirement>() else {
        panic!("Tool does not contain a DockerRequirement");
    };
    let (Some(docker_file), Some(docker_image_id)) = (&dr.docker_file, &dr.docker_image_id) else {
        panic!("DockerRequirement does not contain dockerFile and dockerImageId");
    };
    assert_eq!(
        *docker_file,
        StringOrInclude::Include(Include {
            include: os_path("../../Dockerfile")
        })
    ); // as file is in root and CWL in workflows/echo
    assert_eq!(docker_image_id, "sciwin-client");

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn test_tool_magic_outputs() {
    let dir = workspace(&[]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(shlex::split("touch output.txt").unwrap())
        .cleanup(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(
        created.document.outputs[0]
            .output_binding
            .as_ref()
            .unwrap()
            .glob
            .clone()
            .unwrap()
            .as_one(),
        "$(inputs.output_txt)"
    );
}

#[tokio::test]
pub async fn test_tool_magic_stdout() {
    let dir = workspace(&["../../testdata/input.txt"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(shlex::split("wc input.txt \\> input.txt").unwrap())
        .cleanup(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.stdout.unwrap(), "$(inputs.input_txt.path)");
}

#[tokio::test]
pub async fn test_tool_magic_arguments() {
    let dir = workspace(&["../../testdata/input.txt"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(shlex::split("cat input.txt | grep -f input.txt").unwrap())
        .cleanup(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    let Argument::Binding(binding) = &created.document.arguments.unwrap()[3] else {
        panic!("expected a binding argument");
    };
    assert_eq!(
        binding.value_from,
        Some("$(inputs.input_txt.path)".to_string())
    );
}

#[tokio::test]
pub async fn test_tool_output_is_dir() {
    let dir = workspace(&["../../testdata/create_dir.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "create_dir.py".to_string()])
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.inputs.len(), 0);
    assert_eq!(created.document.outputs.len(), 1); //only folder
    assert_eq!(
        created.document.outputs[0].id,
        Some("my_directory".to_string())
    );
    assert_eq!(
        created.document.outputs[0].r#type,
        CWLType::Directory.into()
    );
}

#[tokio::test]
pub async fn test_tool_output_complete_dir() {
    let dir = workspace(&["../../testdata/create_dir.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "create_dir.py".to_string()])
        .outputs(vec![".".to_string()])
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.inputs.len(), 0);
    assert_eq!(created.document.outputs.len(), 1); //only root folder
    let Some(binding) = &created.document.outputs[0].output_binding else {
        panic!("No Binding")
    };
    assert_eq!(
        binding.glob,
        Some(OneOrMany::One("$(runtime.outdir)".to_string()))
    );
}

#[tokio::test]
#[cfg(target_os = "linux")]
pub async fn test_shell_script() {
    use commonwl::requirements::ListingItems;

    let dir = workspace(&["../../testdata/script.sh"]);
    let root = dir.path();
    std::fs::set_permissions(
        root.join("script.sh"),
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let repo = Repository::open(root).unwrap();
    repository::stage_all(&repo).unwrap();

    let options = ToolCreationOptions::builder()
        .command(vec!["./script.sh".to_string()])
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.inputs.len(), 0);
    assert_eq!(created.document.outputs.len(), 0);
    assert_eq!(created.document.requirements.as_ref().unwrap().len(), 1);

    let Some(iwdr) = created
        .document
        .get_requirement::<InitialWorkDirRequirement>()
    else {
        panic!("Tool does not contain an InitialWorkDirRequirement");
    };

    let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
        panic!("InitialWorkDirRequirement does not contain listing items");
    };
    assert_eq!(listing.len(), 1);

    let ListingItems::Dirent(dirent) = listing.first().unwrap() else {
        panic!("ListingItems is not of type Dirent");
    };
    assert_eq!(dirent.entryname, Some("./script.sh".to_string()));
}

#[tokio::test]
/// see Issue [#89](https://github.com/fairagro/sciwin/issues/89)
pub async fn test_tool_uncommitted_no_run() {
    let dir = workspace(&[]);
    let root = dir.path();
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    fs::copy(
        format!("{manifest_dir}/../../testdata/input.txt"),
        root.join("input.txt"),
    )
    .unwrap(); //repo is not in a clean state now!

    let options = ToolCreationOptions::builder()
        .command(vec!["echo".to_string(), "Hello World".to_string()])
        .no_run(true)
        .build();
    //should be ok to not commit changes, as tool does not run
    assert!(create_tool(root, &options).await.is_ok());
}

#[tokio::test]
/// see Issue [#88](https://github.com/fairagro/sciwin/issues/88)
pub async fn test_tool_output_subfolders() {
    let dir = workspace(&["../../testdata/subfolders.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "subfolders.py".to_string()])
        .build();
    //should be ok to not commit changes, as tool does not run
    assert!(create_tool(root, &options).await.is_ok());
}

#[tokio::test]
#[cfg(target_os = "linux")]
pub async fn tool_create_remote_file() {
    let dir = workspace(&[]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "cat".to_string(),
            "https://raw.githubusercontent.com/fairagro/sciwin/refs/heads/main/README.md"
                .to_string(),
            ">".to_string(),
            "README.md".to_string(),
        ])
        .build();
    let created = create_tool(root, &options).await.unwrap();

    //check file
    assert!(root.join("README.md").exists());

    //check input
    assert_eq!(created.document.inputs.len(), 1);
    assert_eq!(created.document.inputs[0].r#type, CWLType::File.into());
}

#[tokio::test]
pub async fn tool_create_test_network() {
    let dir = workspace(&["../../testdata/input.txt", "../../testdata/echo.py"]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ])
        .container(ContainerInfo::builder().image("python3").build())
        .enable_network(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert!(
        created
            .document
            .get_requirement::<NetworkAccess>()
            .is_some()
    );
}

#[tokio::test]
pub async fn tool_create_same_inout() {
    let dir = workspace(&[]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "echo".to_string(),
            "message".to_string(),
            ">".to_string(),
            "message".to_string(),
        ])
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert!(
        created
            .document
            .inputs
            .iter()
            .any(|i| i.id == Some("message".to_string()))
    );
    //is not allowed to also have same id!
    assert!(
        !created
            .document
            .outputs
            .iter()
            .any(|i| i.id == Some("message".to_string()))
    );

    //decided to just prefix the output with "o_"
    //inputs are used by name, so we do not change them
    assert!(
        created
            .document
            .outputs
            .iter()
            .any(|i| i.id == Some("o_message".to_string()))
    );
}

#[tokio::test]
pub async fn tool_create_mount() {
    let dir = workspace(&[]);
    let root = dir.path();

    //copy a dir we can mount to the working directory
    copy_dir(
        format!("{}/../../testdata/test_dir", env!("CARGO_MANIFEST_DIR")),
        root.join("test_dir"),
    );
    let repo = Repository::open(root).unwrap();
    repository::stage_all(&repo).unwrap();
    repository::commit(&repo, "message").unwrap();

    let options = ToolCreationOptions::builder()
        .command(vec![
            "ls".to_string(),
            ".".to_string(),
            ">".to_string(),
            "folder-list.txt".to_string(),
        ])
        .mounts(vec![PathBuf::from("test_dir")])
        .build();
    let created = create_tool(root, &options).await.unwrap();

    let Some(iwdr) = created
        .document
        .get_requirement::<InitialWorkDirRequirement>()
    else {
        panic!("Tool does not contain an InitialWorkDirRequirement");
    };

    let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
        panic!("InitialWorkDirRequirement does not contain listing items");
    };
    assert_eq!(listing.len(), 1);
}

#[tokio::test]
#[cfg_attr(target_os = "macos", ignore)] //docker used, MACOS CI Issues
pub async fn tool_create_auto_docker() {
    let dir = workspace(&[
        "../../testdata/hello_world/workflows/calculation/calculation.py",
        "../../testdata/hello_world/data/population.csv",
        "../../testdata/hello_world/data/speakers_revised.csv",
        "../../testdata/hello_world/requirements.txt",
    ]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "calculation.py".to_string(),
            "--speakers".to_string(),
            "speakers_revised.csv".to_string(),
            "--population".to_string(),
            "population.csv".to_string(),
        ])
        .auto_container(true)
        .run_container(auto_container_engine().unwrap())
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.requirements.as_ref().unwrap().len(), 2);

    let Some(dr) = created.document.get_requirement::<DockerRequirement>() else {
        panic!("Tool does not contain a DockerRequirement");
    };
    let Some(docker_pull) = &dr.docker_pull else {
        panic!("DockerRequirement does not contain dockerPull");
    };
    assert!(docker_pull.contains("sciwin/python-datascience")); //probably

    //no uncommitted left?
    let repo = Repository::open(root).unwrap();
    assert!(repository::get_modified_files(&repo).unwrap().is_empty());
}

#[tokio::test]
pub async fn tool_create_typehint() {
    let dir = workspace(&[]);
    let root = dir.path();
    let options = ToolCreationOptions::builder()
        .command(vec!["ls".to_string(), "s:.".to_string()]) //. would normally be a directory type. we enforce string here
        .build();
    let created = create_tool(root, &options).await.unwrap();

    assert_eq!(created.document.inputs[0].r#type, CWLType::String.into());
}
