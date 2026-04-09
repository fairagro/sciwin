#![allow(clippy::disallowed_macros)]
use commonwl::documents::{Argument, CWLDocument, CommandLineTool};
use commonwl::requirements::{
    DockerRequirement, Include, InitialWorkDirRequirement, ListingItems, NetworkAccess,
    StringOrInclude, WorkDirItems,
};
use commonwl::types::CWLType;
use commonwl::{OneOrMany, load_cwl_file};
use dircpy::copy_dir;
use fstest::fstest;
use repository::Repository;
use repository::{commit, get_modified_files, stage_all};
use s4n::{cli::Commands, commands::*};
use std::{
    env,
    fs::{self, read_to_string},
    path::Path,
};
use test_utils::os_path;

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test() {
    let tool_create_args = CreateArgs {
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    //check for files being present
    let output_paths = vec![
        Path::new("results.txt"),
        Path::new("workflows/echo/echo.cwl"),
    ];
    for output_path in output_paths {
        assert!(output_path.exists());
    }

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo_inline.py"])]
pub async fn tool_create_test_inputs_outputs() {
    fs::create_dir_all("data").unwrap();
    fs::copy("input.txt", "data/input.txt").unwrap(); //copy to data folder
    fs::remove_file("input.txt").unwrap(); //remove original file

    let repo = Repository::open(".").unwrap();
    stage_all(&repo).unwrap();

    let script = "echo_inline.py".to_string();
    let input = "data/input.txt".to_string();

    let tool_create_args = CreateArgs {
        inputs: Some(vec![input.clone()]),
        outputs: Some(vec!["results.txt".to_string()]),
        command: vec!["python".to_string(), script.clone()],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    let tool_path = Path::new("workflows/echo_inline/echo_inline.cwl");

    //check for files being present
    let output_paths = vec![Path::new("results.txt"), tool_path];

    for output_path in output_paths {
        assert!(output_path.exists());
    }

    //check tool props
    let CWLDocument::CommandLineTool(tool) = load_cwl_file(tool_path, true).unwrap() else {
        panic!("Could not load tool");
    };

    assert_eq!(tool.inputs.len(), 1);
    assert_eq!(tool.outputs.len(), 1);

    let Some(iwdr) = tool.get_requirement::<InitialWorkDirRequirement>() else {
        panic!("Tool does not contain an InitialWorkDirRequirement");
    };
    let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
        panic!("InitialWorkDirRequirement does not contain listing items");
    };
    assert!(listing.as_many().len() == 2);

    //no uncommitted left?
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_is_raw() {
    let tool_create_args = CreateArgs {
        is_raw: true,
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }
    assert!(!Path::new("workflows/echo/echo.cwl").exists()); //no cwl file as it is outputted to stdout
    assert!(Path::new("results.txt").exists());

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_no_commit() {
    let tool_create_args = CreateArgs {
        no_commit: true, //look!
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    //check for files being present
    let output_paths = vec![
        Path::new("results.txt"),
        Path::new("workflows/echo/echo.cwl"),
    ];
    for output_path in output_paths {
        assert!(output_path.exists());
    }
    //as we did not commit there must be files (exactly 2, the cwl file and the results.txt)
    let repo = Repository::open(".").unwrap();
    assert_eq!(get_modified_files(&repo).len(), 2);
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_no_run() {
    let tool_create_args = CreateArgs {
        no_run: true,
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }
    assert!(Path::new("workflows/echo/echo.cwl").exists());

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py", "../../testdata/data.bin"])]
pub async fn tool_create_test_no_run_explicit_inputs() {
    let tool_create_args = CreateArgs {
        no_run: true,
        inputs: Some(vec!["data.bin".to_string()]),
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }
    assert!(Path::new("workflows/echo/echo.cwl").exists());

    let CWLDocument::CommandLineTool(tool) =
        load_cwl_file("workflows/echo/echo.cwl", true).unwrap()
    else {
        panic!("Could not load tool");
    };
    assert!(
        tool.inputs
            .iter()
            .any(|i| i.id == Some("data_bin".to_string()))
    );

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_no_run_explicit_inputs_string() {
    let tool_create_args = CreateArgs {
        no_run: true,
        inputs: Some(vec!["wurstbrot".to_string()]),
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }
    assert!(Path::new("workflows/echo/echo.cwl").exists());

    let CWLDocument::CommandLineTool(tool) =
        load_cwl_file("workflows/echo/echo.cwl", true).unwrap()
    else {
        panic!("Could not load tool");
    };
    assert!(
        tool.inputs
            .iter()
            .any(|i| i.id == Some("wurstbrot".to_string()))
    );

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_is_clean() {
    let tool_create_args = CreateArgs {
        is_clean: true,
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }
    assert!(Path::new("workflows/echo/echo.cwl").exists());
    assert!(!Path::new("results.txt").exists()); //no result is left as it is cleaned

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_container_image() {
    let tool_create_args = CreateArgs {
        container_image: Some("python".to_string()),
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    //read file
    let cwl_file = Path::new("workflows/echo/echo.cwl");
    let cwl_contents = read_to_string(cwl_file).expect("Could not read CWL File");
    let cwl: CommandLineTool = serde_yaml::from_str(&cwl_contents).expect("Could not convert CWL");
    assert_eq!(cwl.requirements.as_ref().unwrap().len(), 2);

    let Some(dr) = cwl.get_requirement::<DockerRequirement>() else {
        panic!("Tool does not contain a DockerRequirement");
    };
    if let Some(image) = &dr.docker_pull {
        assert_eq!(image, "python");
    } else {
        panic!("DockerRequirement does not contain a dockerPull");
    }

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/Dockerfile", "../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_dockerfile() {
    let tool_create_args = CreateArgs {
        container_image: Some("Dockerfile".to_string()),
        container_tag: Some("sciwin-client".to_string()),
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }
    //read file
    let cwl_file = Path::new("workflows/echo/echo.cwl");
    let cwl_contents = read_to_string(cwl_file).expect("Could not read CWL File");
    let cwl: CommandLineTool = serde_yaml::from_str(&cwl_contents).expect("Could not convert CWL");

    assert_eq!(cwl.requirements.as_ref().unwrap().len(), 2);

    let Some(dr) = cwl.get_requirement::<DockerRequirement>() else {
        panic!("Tool does not contain a DockerRequirement");
    };
    if let (Some(docker_file), Some(docker_image_id)) = (&dr.docker_file, &dr.docker_image_id) {
        assert_eq!(
            *docker_file,
            StringOrInclude::Include(Include {
                include: os_path("../../Dockerfile")
            })
        ); // as file is in root and CWL in workflows/echo
        assert_eq!(docker_image_id, "sciwin-client");
    } else {
        panic!("DockerRequirement does not contain dockerFile and dockerImageId");
    }

    //no uncommitted left?
    let repo = Repository::open(".").unwrap();
    assert!(get_modified_files(&repo).is_empty());
}

#[fstest(repo = true, tokio = true)]
pub async fn test_tool_magic_outputs() {
    let str = "touch output.txt";
    let args = CreateArgs {
        no_commit: true,
        is_clean: true,
        command: shlex::split(str).unwrap(),
        ..Default::default()
    };

    assert!(create_tool(&args).await.is_ok());

    let tool = load_cwl_file("workflows/touch/touch.cwl", true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };

    assert_eq!(
        tool.outputs[0]
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

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
pub async fn test_tool_magic_stdout() {
    let str = "wc input.txt \\> input.txt";
    let args = CreateArgs {
        no_commit: true,
        is_clean: true,
        command: shlex::split(str).unwrap(),
        ..Default::default()
    };

    assert!(create_tool(&args).await.is_ok());

    let tool = load_cwl_file("workflows/wc/wc.cwl", true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };
    assert!(tool.stdout.unwrap() == *"$(inputs.input_txt.path)");
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
pub async fn test_tool_magic_arguments(_dir: &Path) {
    let str = "cat input.txt | grep -f input.txt";
    let args = CreateArgs {
        no_commit: true,
        is_clean: true,
        command: shlex::split(str).unwrap(),
        ..Default::default()
    };

    assert!(create_tool(&args).await.is_ok());

    let tool = load_cwl_file("workflows/cat/cat.cwl", true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };
    if let Argument::Binding(binding) = &tool.arguments.unwrap()[3] {
        assert!(binding.value_from == Some("$(inputs.input_txt.path)".to_string()));
    } else {
        panic!()
    }
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/create_dir.py"])]
pub async fn test_tool_output_is_dir() {
    let name = "create_dir";
    let command = &["python", "create_dir.py"];
    let args = CreateArgs {
        command: command.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        ..Default::default()
    };

    assert!(create_tool(&args).await.is_ok());

    let tool = load_cwl_file(format!("workflows/{name}/{name}.cwl"), true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };
    assert_eq!(tool.inputs.len(), 0);
    assert_eq!(tool.outputs.len(), 1); //only folder
    assert_eq!(tool.outputs[0].id, Some("my_directory".to_string()));
    assert_eq!(tool.outputs[0].r#type, CWLType::Directory.into());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/create_dir.py"])]
pub async fn test_tool_output_complete_dir() {
    let name = "create_dir";
    let command = &["python", "create_dir.py"];
    let args = CreateArgs {
        outputs: Some(vec![".".into()]), //
        command: command.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        ..Default::default()
    };

    assert!(create_tool(&args).await.is_ok());

    let tool = load_cwl_file(format!("workflows/{name}/{name}.cwl"), true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };
    assert_eq!(tool.inputs.len(), 0);
    assert_eq!(tool.outputs.len(), 1); //only root folder
    if let Some(binding) = &tool.outputs[0].output_binding {
        assert_eq!(
            binding.glob,
            Some(OneOrMany::One("$(runtime.outdir)".to_string()))
        );
    } else {
        panic!("No Binding")
    }

    println!("{:#?}", tool.outputs);
}

#[fstest(repo= true, tokio = true, files=["../../testdata/script.sh"])]
#[cfg(target_os = "linux")]
pub async fn test_shell_script() {
    use repository::stage_all;

    std::fs::set_permissions(
        "script.sh",
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let repo = Repository::open(".").unwrap();
    stage_all(&repo).unwrap();

    let name = "script";
    let command = &["./script.sh"];
    let args = CreateArgs {
        command: command.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        ..Default::default()
    };

    let result = create_tool(&args).await;
    println!("{result:#?}");
    assert!(result.is_ok());

    let tool = load_cwl_file(format!("workflows/{name}/{name}.cwl"), true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };
    assert_eq!(tool.inputs.len(), 0);
    assert_eq!(tool.outputs.len(), 0);

    assert_eq!(tool.requirements.as_ref().unwrap().len(), 1);

    let Some(iwdr) = tool.get_requirement::<InitialWorkDirRequirement>() else {
        panic!("Tool does not contain an InitialWorkDirRequirement");
    };

    let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
        panic!("InitialWorkDirRequirement does not contain listing items");
    };
    assert!(listing.as_many().len() == 1);

    let items = listing.as_many();
    let item = items.first().unwrap();
    let ListingItems::Dirent(dirent) = item else {
        panic!("ListingItems is not of type Dirent");
    };
    assert_eq!(dirent.entryname, Some("./script.sh".to_string()));
}

#[fstest(repo = true, tokio = true)]
/// see Issue [#89](https://github.com/fairagro/sciwin/issues/89)
pub async fn test_tool_uncommitted_no_run() {
    let root = env!("CARGO_MANIFEST_DIR");
    fs::copy(format!("{root}/../../testdata/input.txt"), "input.txt").unwrap(); //repo is not in a clean state now!
    let args = CreateArgs {
        command: ["echo".to_string(), "Hello World".to_string()].to_vec(),
        no_run: true,
        ..Default::default()
    };
    //should be ok to not commit changes, as tool does not run
    assert!(create_tool(&args).await.is_ok());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/subfolders.py"])]
/// see Issue [#88](https://github.com/fairagro/sciwin/issues/88)
pub async fn test_tool_output_subfolders() {
    let args = CreateArgs {
        command: ["python".to_string(), "subfolders.py".to_string()].to_vec(),
        ..Default::default()
    };
    //should be ok to not commit changes, as tool does not run
    assert!(create_tool(&args).await.is_ok());
}

#[fstest(repo = true, tokio = true)]
#[cfg(target_os = "linux")]
pub async fn tool_create_remote_file() {
    let tool_create_args = CreateArgs {
        command: vec![
            "wget".to_string(),
            "https://raw.githubusercontent.com/fairagro/sciwin/refs/heads/main/README.md"
                .to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    //check file
    assert!(Path::new("README.md").exists());

    //check input
    let tool_path = Path::new("workflows/wget/wget.cwl");
    let tool = load_cwl_file(tool_path, true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };
    assert_eq!(tool.inputs.len(), 1);
    assert_eq!(tool.inputs[0].r#type, CWLType::File.into());
}

#[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
pub async fn tool_create_test_network() {
    let tool_create_args = CreateArgs {
        command: vec![
            "python".to_string(),
            "echo.py".to_string(),
            "--test".to_string(),
            "input.txt".to_string(),
        ],
        container_image: Some("python".to_string()),
        enable_network: true,
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    let tool_path = Path::new("workflows/echo/echo.cwl");
    let tool = load_cwl_file(tool_path, true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };

    assert!(tool.get_requirement::<NetworkAccess>().is_some());
}

#[fstest(repo = true, tokio = true)]
pub async fn tool_create_same_inout() {
    let tool_create_args = CreateArgs {
        command: vec![
            "echo".to_string(),
            "message".to_string(),
            ">".to_string(),
            "message".to_string(),
        ],
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    let tool_path = Path::new("workflows/echo/echo.cwl");
    let tool = load_cwl_file(tool_path, true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };

    assert!(
        tool.inputs
            .iter()
            .any(|i| i.id == Some("message".to_string()))
    );
    //is not allowed to also have same id!
    assert!(
        !tool
            .outputs
            .iter()
            .any(|i| i.id == Some("message".to_string()))
    );

    //decided to just prefix the output with "o_"
    //inputs are used by name, so we do not change them
    assert!(
        tool.outputs
            .iter()
            .any(|i| i.id == Some("o_message".to_string()))
    );
}

#[fstest(repo = true, tokio = true)]
pub async fn tool_create_mount() {
    //copy a dir we can mount to the working directory
    copy_dir(
        format!("{}/../../testdata/test_dir", env!("CARGO_MANIFEST_DIR")),
        "test_dir",
    )
    .unwrap();
    let repo = Repository::open(".").unwrap();
    stage_all(&repo).unwrap();
    commit(&repo, "message").unwrap();

    let tool_create_args = CreateArgs {
        command: vec![
            "ls".to_string(),
            ".".to_string(),
            ">".to_string(),
            "folder-list.txt".to_string(),
        ],
        mount: Some(vec!["test_dir".into()]),
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    let tool_path = Path::new("workflows/ls/ls.cwl");
    let tool = load_cwl_file(tool_path, true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };

    let Some(iwdr) = tool.get_requirement::<InitialWorkDirRequirement>() else {
        panic!("Tool does not contain an InitialWorkDirRequirement");
    };

    let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
        panic!("InitialWorkDirRequirement does not contain listing items");
    };
    assert!(listing.as_many().len() == 1);
}

#[fstest(repo = true, tokio = true)]
pub async fn tool_create_typehint() {
    let tool_create_args = CreateArgs {
        command: vec!["ls".to_string(), "s:.".to_string()], //. would normally be a directory type. we enforce string here
        ..Default::default()
    };
    let cmd = Commands::Create(tool_create_args);
    if let Commands::Create(ref args) = cmd {
        assert!(handle_create_command(args).await.is_ok());
    }

    let tool_path = Path::new("workflows/ls/ls.cwl");
    let tool = load_cwl_file(tool_path, true).unwrap();
    let CWLDocument::CommandLineTool(tool) = tool else {
        panic!("Could not load tool");
    };

    let input = &tool.inputs[0];
    assert_eq!(input.r#type, CWLType::String.into());
}
