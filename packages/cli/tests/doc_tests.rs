#![allow(clippy::disallowed_macros)]
use commonwl::execution::io::copy_dir;
///This file contains all examples described here: <https://fairagro.github.io/sciwin/examples/tool-creation>/
use commonwl::{
    Command, Entry, load_tool, load_workflow,
    requirements::{Requirement, WorkDirItem},
};
use s4n::commands::*;
use s4n_core::project::initialize_project;
use serial_test::serial;
use std::{env, fs, path::PathBuf, vec};
use tempfile::{TempDir, tempdir};
use test_utils::{check_git_user, setup_python};

fn setup() -> (PathBuf, TempDir) {
    let dir = tempdir().unwrap();

    //copy docs dit to tmp
    let test_folder = "../../testdata/docs";
    copy_dir(test_folder, dir.path()).unwrap();

    let current = env::current_dir().unwrap();
    env::set_current_dir(dir.path()).unwrap();

    //init
    check_git_user().unwrap();
    initialize_project(dir.path(), false).expect("Could not init s4n");

    (current, dir)
}

fn cleanup(current: PathBuf, dir: TempDir) {
    env::set_current_dir(current).unwrap();
    dir.close().unwrap();
}

#[test]
#[serial]
#[cfg_attr(target_os = "windows", ignore)]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-echo
pub fn test_wrapping_echo() {
    let (current, dir) = setup();

    let command = &["echo", "\"Hello World\""];

    let args = &CreateArgs {
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join("workflows/echo/echo.cwl");
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Single("echo".to_string()));
    assert_eq!(tool.inputs.len(), 1);

    //test if is executable
    execute_local(&LocalExecuteArgs {
        file: tool_path,
        ..Default::default()
    })
    .unwrap();

    cleanup(current, dir);
}

#[test]
#[serial]
#[cfg_attr(target_os = "windows", ignore)]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-echo
pub fn test_wrapping_echo_2() {
    let (current, dir) = setup();

    let command = &["echo", "\"Hello World\"", ">", "hello.yaml"];

    let name = "echo2";
    let args = &CreateArgs {
        name: Some(name.to_string()),
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Single("echo".to_string()));
    assert_eq!(tool.inputs.len(), 1);
    assert_eq!(tool.outputs.len(), 1);
    assert_eq!(tool.stdout, Some("hello.yaml".to_string()));

    //test if is executable
    execute_local(&LocalExecuteArgs {
        file: tool_path,
        ..Default::default()
    })
    .unwrap();

    cleanup(current, dir);
}

#[test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-a-python-script
pub fn test_wrapping_python_script() {
    let (current, dir) = setup();

    let command = &["python", "echo.py", "--message", "SciWIn rocks!", "--output-file", "out.txt"];

    let name = "echo_python";
    let args = &CreateArgs {
        name: Some(name.to_string()),
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Multiple(vec!["python".to_string(), "echo.py".to_string()]));
    assert_eq!(tool.inputs.len(), 2);
    assert_eq!(tool.outputs.len(), 1);

    //test if is executable
    execute_local(&LocalExecuteArgs {
        file: tool_path,
        ..Default::default()
    })
    .unwrap();

    cleanup(current, dir);
}

#[test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-a-long-running-script
pub fn test_wrapping_a_long_running_script() {
    let (current, dir) = setup();

    let command = &["python", "sleep.py"];

    let name = "sleep";
    let args = &CreateArgs {
        no_run: true,
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Multiple(vec!["python".to_string(), "sleep.py".to_string()]));
    assert_eq!(tool.inputs.len(), 0);
    assert_eq!(tool.outputs.len(), 0);

    //test if is executable
    execute_local(&LocalExecuteArgs {
        file: tool_path,
        ..Default::default()
    })
    .unwrap();

    cleanup(current, dir);
}

#[test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-a-long-running-script
pub fn test_wrapping_a_long_running_script2() {
    let (current, dir) = setup();

    let command = &["python", "sleep.py"];

    let name = "sleep2";
    let args = &CreateArgs {
        name: Some(name.to_string()),
        no_run: true,
        outputs: Some(vec!["sleep.txt".to_string()]),
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Multiple(vec!["python".to_string(), "sleep.py".to_string()]));
    assert_eq!(tool.inputs.len(), 0);
    assert_eq!(tool.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        execute_local(&LocalExecuteArgs {
            file: tool_path,
            ..Default::default()
        })
        .unwrap();
    }

    cleanup(current, dir);
}

#[test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#implicit-inputs-hardcoded-files
pub fn test_implicit_inputs_hardcoded_files() {
    let (current, dir) = setup();

    let command = &["python", "load.py"];

    let name = "load";
    let args = &CreateArgs {
        inputs: Some(vec!["file.txt".to_string()]),
        outputs: Some(vec!["out.txt".to_string()]),
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Multiple(vec!["python".to_string(), "load.py".to_string()]));
    assert_eq!(tool.inputs.len(), 1);
    assert_eq!(tool.outputs.len(), 1);

    assert_eq!(tool.requirements.len(), 1);

    if let Requirement::InitialWorkDirRequirement(initial) = &tool.requirements[0] {
        assert_eq!(initial.listing.len(), 2);
        assert!(matches!(initial.listing[0], WorkDirItem::Dirent(_)));
        assert!(matches!(initial.listing[1], WorkDirItem::Dirent(_)));
        if let WorkDirItem::Dirent(dirent) = &initial.listing[0] {
            assert_eq!(dirent.entryname, Some("load.py".to_string()));
        }
        if let WorkDirItem::Dirent(dirent) = &initial.listing[1] {
            assert_eq!(dirent.entryname, Some("file.txt".to_string()));
            assert_eq!(dirent.entry, Entry::Source("$(inputs.file_txt)".into()));
        }
    } else {
        panic!("InitialWorkDirRequirement not found!");
    }

    //test if is executable
    if !cfg!(target_os = "macos") {
        execute_local(&LocalExecuteArgs {
            file: tool_path,
            ..Default::default()
        })
        .unwrap();
    }

    cleanup(current, dir);
}

#[test]
#[serial]
#[cfg_attr(target_os = "windows", ignore)]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#piping
pub fn test_piping() {
    let (current, dir) = setup();

    let command = &["cat", "speakers.csv", "|", "head", "-n", "5", ">", "speakers_5.csv"];

    let name = "cat";
    let args = &CreateArgs {
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };
    assert!(create_tool(args).is_ok());

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(tool.base_command, Command::Single("cat".to_string()));
    assert_eq!(tool.inputs.len(), 1);
    assert_eq!(tool.outputs.len(), 1);
    assert!(tool.arguments.is_some());
    assert_eq!(tool.arguments.unwrap().len(), 6);

    //test if is executable
    if !cfg!(target_os = "macos") {
        execute_local(&LocalExecuteArgs {
            file: tool_path,
            ..Default::default()
        })
        .unwrap();
    }

    cleanup(current, dir);
}

#[test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#pulling-containers
pub fn test_pulling_containers() {
    let (current, dir) = setup();

    let command = &[
        "python",
        "calculation.py",
        "--population",
        "population.csv",
        "--speakers",
        "speakers_revised.csv",
    ];

    let name = "calculation";
    let args = &CreateArgs {
        container_image: Some("pandas/pandas:pip-all".to_string()),
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };

    //setup python env
    let (newpath, restore) = setup_python(dir.path().to_str().unwrap());
    unsafe {
        env::set_var("PATH", newpath);
    }
    assert!(create_tool(args).is_ok());

    //restore path
    unsafe {
        env::set_var("PATH", restore);
    }

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(
        tool.base_command,
        Command::Multiple(vec!["python".to_string(), "calculation.py".to_string()])
    );
    assert_eq!(tool.inputs.len(), 2);
    assert_eq!(tool.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        execute_local(&LocalExecuteArgs {
            file: tool_path,
            ..Default::default()
        })
        .unwrap();
    }

    cleanup(current, dir);
}

#[test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#building-custom-containers
pub fn test_building_custom_containers() {
    let (current, dir) = setup();

    let command = &[
        "python",
        "calculation.py",
        "--population",
        "population.csv",
        "--speakers",
        "speakers_revised.csv",
    ];

    let name = "calculation";
    let args = &CreateArgs {
        container_image: Some("Dockerfile".to_string()),
        container_tag: Some("my-docker".to_string()),
        command: command.iter().map(|&s| s.to_string()).collect(),
        ..Default::default()
    };

    //setup python env
    let (newpath, restore) = setup_python(dir.path().to_str().unwrap());
    unsafe {
        env::set_var("PATH", newpath);
    }

    assert!(create_tool(args).is_ok());

    //restore path
    unsafe {
        env::set_var("PATH", restore);
    }

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());

    let tool = load_tool(&tool_path).unwrap();
    assert_eq!(
        tool.base_command,
        Command::Multiple(vec!["python".to_string(), "calculation.py".to_string()])
    );
    assert_eq!(tool.inputs.len(), 2);
    assert_eq!(tool.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        execute_local(&LocalExecuteArgs {
            file: tool_path,
            ..Default::default()
        })
        .unwrap();
    }

    cleanup(current, dir);
}

#[test]
#[serial]
/// see https://fairagro.github.io/sciwin/getting-started/example/
//docker not working on MacOS Github Actions
#[cfg_attr(target_os = "macos", ignore)]
pub fn test_example_project() {
    //set up environment
    let dir = tempdir().unwrap();
    let dir_str = &dir.path().to_string_lossy();
    let test_folder = "../../testdata/hello_world";
    copy_dir(test_folder, dir.path()).unwrap();

    //delete all cwl files as we want to generate
    fs::remove_dir_all(dir.path().join("workflows/main")).unwrap();
    fs::remove_file(dir.path().join("workflows/plot/plot.cwl")).unwrap();
    fs::remove_file(dir.path().join("workflows/calculation/calculation.cwl")).unwrap();

    let current = env::current_dir().unwrap();
    env::set_current_dir(dir.path()).unwrap();
    let (newpath, restore) = setup_python(dir_str);

    //modify path variable
    unsafe {
        env::set_var("PATH", newpath);
    }

    check_git_user().unwrap();

    //init project
    initialize_project(dir.path(), false).expect("Could not init s4n");

    //create calculation tool
    create_tool(&CreateArgs {
        command: [
            "python".to_string(),
            "workflows/calculation/calculation.py".to_string(),
            "--speakers".to_string(),
            "data/speakers_revised.csv".to_string(),
            "--population".to_string(),
            "data/population.csv".to_string(),
        ]
        .to_vec(),
        container_image: Some("pandas/pandas:pip-all".to_string()),
        ..Default::default()
    })
    .expect("Could not create calculation tool");
    assert!(fs::exists("workflows/calculation/calculation.cwl").unwrap());

    //create calculation tool
    create_tool(&CreateArgs {
        command: [
            "python".to_string(),
            "workflows/plot/plot.py".to_string(),
            "--results".to_string(),
            "results.csv".to_string(),
        ]
        .to_vec(),
        container_image: Some("workflows/plot/Dockerfile".to_string()),
        container_tag: Some("matplotlib".to_string()),
        ..Default::default()
    })
    .expect("Could not create plot tool");
    assert!(fs::exists("workflows/plot/plot.cwl").unwrap());

    //list files
    handle_list_command(&Default::default()).expect("Could not list cwl files");

    //create workflow
    let name = "test_workflow".to_string();
    let create_args = CreateArgs {
        name: Some(name.clone()),
        ..Default::default()
    };
    create_workflow(&create_args).expect("Could not create workflow");

    //add connections to inputs
    connect_workflow_nodes(&ConnectWorkflowArgs {
        name: name.clone(),
        from: "@inputs/population".to_string(),
        to: "calculation/population".to_string(),
    })
    .expect("Could not add input to calculation/population");

    connect_workflow_nodes(&ConnectWorkflowArgs {
        name: name.clone(),
        from: "@inputs/speakers".to_string(),
        to: "calculation/speakers".to_string(),
    })
    .expect("Could not add input to calculation/speakers");

    //connect second step
    connect_workflow_nodes(&ConnectWorkflowArgs {
        name: name.clone(),
        from: "calculation/results".to_string(),
        to: "plot/results".to_string(),
    })
    .expect("Could not add input to plot/results");

    //connect output
    connect_workflow_nodes(&ConnectWorkflowArgs {
        name: name.clone(),
        from: "plot/o_results".to_string(),
        to: "@outputs/out".to_string(),
    })
    .expect("Could not add input to output/out");

    let save_args = SaveArgs { name };
    //save workflow
    save_workflow(&save_args).expect("Could not save workflow");
    let wf_path = PathBuf::from("workflows/test_workflow/test_workflow.cwl");
    assert!(fs::exists(&wf_path).unwrap());

    let workflow = load_workflow(&wf_path).unwrap();
    assert!(workflow.has_input("speakers"));
    assert!(workflow.has_input("population"));
    assert!(workflow.has_output("out"));
    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));
    assert!(workflow.has_step_input("speakers"));
    assert!(workflow.has_step_input("population"));
    assert!(workflow.has_step_input("calculation/results"));
    assert!(workflow.has_step_output("calculation/results"));
    assert!(workflow.has_step_output("plot/o_results"));

    //workflow status
    handle_list_command(&ListCWLArgs {
        file: Some(wf_path.clone()),
        ..Default::default()
    })
    .expect("Could not print status");

    //remove outputs
    fs::remove_file("results.csv").unwrap();
    fs::remove_file("results.svg").unwrap();

    assert!(!fs::exists("results.csv").unwrap());
    assert!(!fs::exists("results.svg").unwrap());

    //execute workflow
    execute_local(&LocalExecuteArgs {
        is_quiet: false,
        file: wf_path,
        args: vec!["inputs.yml".to_string()],
        ..Default::default()
    })
    .expect("Could not execute Workflow");

    //check that only svg file is there now!
    assert!(!fs::exists("results.csv").unwrap());
    assert!(fs::exists("results.svg").unwrap());

    unsafe {
        env::set_var("PATH", restore);
    }
    env::set_current_dir(current).unwrap();
}
