#![allow(clippy::disallowed_macros)]
///This file contains all examples described here: <https://fairagro.github.io/sciwin/examples/tool-creation>/
mod common;

use common::{
    check_git_user, copy_dir, load_workflow, local_runner, setup_python, tool_path, with_workflow,
};
use commonwl::{OneOrMany, engine::InputObject};
use sciwin::authoring::tool::{ContainerInfo, ToolCreationOptions, create_tool};
use sciwin::authoring::workflow::{
    WorkflowSlot, add_workflow_input_connection, add_workflow_output_connection,
    add_workflow_step_connection, create_workflow,
};
use sciwin::execution::WorkflowRunner;
use sciwin::project::initialize_project;
use sciwin::repository::{Repository, commit, stage_file};
use serial_test::serial;
use std::{env, fs, path::PathBuf};
use tempfile::{TempDir, tempdir};

fn setup() -> (PathBuf, TempDir) {
    let dir = tempdir().unwrap();

    //copy docs dir to tmp
    let test_folder = "../../testdata/docs";
    copy_dir(test_folder, dir.path());

    let current = env::current_dir().unwrap();
    let canonicalized = dunce::canonicalize(dir.path()).unwrap();
    env::set_current_dir(&canonicalized).unwrap();

    //init
    check_git_user().unwrap();
    initialize_project("").expect("Could not init s4n");

    (current, dir)
}

fn cleanup(current: PathBuf, dir: TempDir) {
    env::set_current_dir(current).unwrap();
    dir.close().unwrap();
}

#[tokio::test]
#[serial]
#[cfg_attr(target_os = "windows", ignore)]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-echo
pub async fn test_wrapping_echo() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let options = ToolCreationOptions::builder()
        .command(vec!["echo".to_string(), "\"Hello World\"".to_string()])
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(&root, &options).await.unwrap();

    let tool_path = dir.path().join("workflows/echo/echo.cwl");
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(created.document.base_command, Some(OneOrMany::One("echo".to_string())));
    assert_eq!(created.document.inputs.len(), 1);

    //test if is executable
    local_runner()
        .run_workflow(&root.join(&created.path), InputObject::default(), None)
        .await
        .unwrap();

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
#[cfg_attr(target_os = "windows", ignore)]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-echo
pub async fn test_wrapping_echo_2() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "echo2";
    let options = ToolCreationOptions::builder()
        .command(vec![
            "echo".to_string(),
            "\"Hello World\"".to_string(),
            ">".to_string(),
            "hello.yaml".to_string(),
        ])
        .name(name.to_string())
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(&root, &options).await.unwrap();

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(created.document.base_command, Some(OneOrMany::One("echo".to_string())));
    assert_eq!(created.document.inputs.len(), 1);
    assert_eq!(created.document.outputs.len(), 1);
    assert_eq!(created.document.stdout, Some("hello.yaml".to_string()));

    //test if is executable
    local_runner()
        .run_workflow(&root.join(&created.path), InputObject::default(), None)
        .await
        .unwrap();

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-a-python-script
pub async fn test_wrapping_python_script() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "echo_python3";
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "echo.py".to_string(),
            "--message".to_string(),
            "SciWIn rocks!".to_string(),
            "--output-file".to_string(),
            "out.txt".to_string(),
        ])
        .name(name.to_string())
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(&root, &options).await.unwrap();

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::Many(vec![
            "python3".to_string(),
            "echo.py".to_string()
        ]))
    );
    assert_eq!(created.document.inputs.len(), 2);
    assert_eq!(created.document.outputs.len(), 1);

    //test if is executable
    local_runner()
        .run_workflow(&root.join(&created.path), InputObject::default(), None)
        .await
        .unwrap();

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-a-long-running-script
pub async fn test_wrapping_a_long_running_script() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "sleep";
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "sleep.py".to_string()])
        .no_run(true)
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(&root, &options).await.unwrap();

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::Many(vec![
            "python3".to_string(),
            "sleep.py".to_string()
        ]))
    );
    assert_eq!(created.document.inputs.len(), 0);
    assert_eq!(created.document.outputs.len(), 0);

    //test if is executable
    local_runner()
        .run_workflow(&root.join(&created.path), InputObject::default(), None)
        .await
        .unwrap();

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#wrapping-a-long-running-script
pub async fn test_wrapping_a_long_running_script2() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "sleep2";
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "sleep.py".to_string()])
        .name(name.to_string())
        .no_run(true)
        .outputs(vec!["sleep.txt".to_string()])
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(&root, &options).await.unwrap();

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::Many(vec![
            "python3".to_string(),
            "sleep.py".to_string()
        ]))
    );
    assert_eq!(created.document.inputs.len(), 0);
    assert_eq!(created.document.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        local_runner()
            .run_workflow(&root.join(&created.path), InputObject::default(), None)
            .await
            .unwrap();
    }

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#implicit-inputs-hardcoded-files
pub async fn test_implicit_inputs_hardcoded_files() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "load";
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "load.py".to_string()])
        .inputs(vec!["file.txt".to_string()])
        .outputs(vec!["out.txt".to_string()])
        .save(true)
        .commit(true)
        .build();
    let result = create_tool(&root, &options).await;
    dbg!(&result);
    let created = result.unwrap();

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::Many(vec![
            "python3".to_string(),
            "load.py".to_string()
        ]))
    );
    assert_eq!(created.document.inputs.len(), 1);
    assert_eq!(created.document.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        local_runner()
            .run_workflow(&root.join(&created.path), InputObject::default(), None)
            .await
            .unwrap();
    }

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
#[cfg_attr(target_os = "windows", ignore)]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#piping
pub async fn test_piping() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "cat";
    let options = ToolCreationOptions::builder()
        .command(vec![
            "cat".to_string(),
            "speakers.csv".to_string(),
            "|".to_string(),
            "head".to_string(),
            "-n".to_string(),
            "5".to_string(),
            ">".to_string(),
            "speakers_5.csv".to_string(),
        ])
        .save(true)
        .commit(true)
        .build();
    let created = create_tool(&root, &options).await.unwrap();

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    dbg!(&created.yaml);
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::One("cat".to_string()))
    );
    assert_eq!(created.document.inputs.len(), 1);
    assert_eq!(created.document.outputs.len(), 1);
    assert!(created.document.arguments.is_some());
    assert_eq!(created.document.arguments.as_ref().unwrap().len(), 6);

    //test if is executable
    if !cfg!(target_os = "macos") {
        local_runner()
            .run_workflow(&root.join(&created.path), InputObject::default(), None)
            .await
            .unwrap();
    }

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#pulling-containers
pub async fn test_pulling_containers() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "calculation";
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "calculation.py".to_string(),
            "--population".to_string(),
            "population.csv".to_string(),
            "--speakers".to_string(),
            "speakers_revised.csv".to_string(),
        ])
        .container(ContainerInfo::builder().image("pandas/pandas:pip-all").build())
        .save(true)
        .commit(true)
        .build();

    //setup python env
    let (newpath, restore) = setup_python(dir.path().to_str().unwrap());
    unsafe {
        env::set_var("PATH", newpath);
    }
    let created = create_tool(&root, &options).await.unwrap();

    //restore path
    unsafe {
        env::set_var("PATH", restore);
    }

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::Many(vec![
            "python3".to_string(),
            "calculation.py".to_string()
        ]))
    );
    assert_eq!(created.document.inputs.len(), 2);
    assert_eq!(created.document.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        local_runner()
            .run_workflow(&root.join(&created.path), InputObject::default(), None)
            .await
            .unwrap();
    }

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
///see https://fairagro.github.io/sciwin/examples/tool-creation/#building-custom-containers
pub async fn test_building_custom_containers() {
    let (current, dir) = setup();
    let root = env::current_dir().unwrap();

    let name = "calculation";
    let options = ToolCreationOptions::builder()
        .command(vec![
            "python3".to_string(),
            "calculation.py".to_string(),
            "--population".to_string(),
            "population.csv".to_string(),
            "--speakers".to_string(),
            "speakers_revised.csv".to_string(),
        ])
        .container(
            ContainerInfo::builder()
                .image("Dockerfile")
                .tag("my-docker")
                .build(),
        )
        .save(true)
        .commit(true)
        .build();

    //setup python env
    let (newpath, restore) = setup_python(dir.path().to_str().unwrap());
    unsafe {
        env::set_var("PATH", newpath);
    }

    let created = create_tool(&root, &options).await.unwrap();

    //restore path
    unsafe {
        env::set_var("PATH", restore);
    }

    let tool_path = dir.path().join(format!("workflows/{name}/{name}.cwl"));
    assert!(fs::exists(&tool_path).unwrap());
    assert_eq!(
        created.document.base_command,
        Some(OneOrMany::Many(vec![
            "python3".to_string(),
            "calculation.py".to_string()
        ]))
    );
    assert_eq!(created.document.inputs.len(), 2);
    assert_eq!(created.document.outputs.len(), 1);

    //test if is executable
    if !cfg!(target_os = "macos") {
        local_runner()
            .run_workflow(&root.join(&created.path), InputObject::default(), None)
            .await
            .unwrap();
    }

    cleanup(current, dir);
}

#[tokio::test]
#[serial]
/// see https://fairagro.github.io/sciwin/getting-started/example/
//docker not working on MacOS Github Actions
#[cfg_attr(target_os = "macos", ignore)]
pub async fn test_example_project() {
    //set up environment
    let dir = tempdir().unwrap();
    let dir_str = &dir.path().to_string_lossy();
    let test_folder = "../../testdata/hello_world";
    copy_dir(test_folder, dir.path());

    //delete all cwl files as we want to generate
    fs::remove_dir_all(dir.path().join("workflows/main")).unwrap();
    fs::remove_file(dir.path().join("workflows/plot/plot.cwl")).unwrap();
    fs::remove_file(dir.path().join("workflows/calculation/calculation.cwl")).unwrap();

    let current = env::current_dir().unwrap();
    env::set_current_dir(dir.path()).unwrap();
    let root = env::current_dir().unwrap();
    let (newpath, restore) = setup_python(dir_str);

    //modify path variable
    unsafe {
        env::set_var("PATH", newpath);
    }

    check_git_user().unwrap();

    //init project
    initialize_project("").expect("Could not init s4n");

    //create calculation tool
    create_tool(
        &root,
        &ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "workflows/calculation/calculation.py".to_string(),
                "--speakers".to_string(),
                "data/speakers_revised.csv".to_string(),
                "--population".to_string(),
                "data/population.csv".to_string(),
            ])
            .container(ContainerInfo::builder().image("pandas/pandas:pip-all").build())
            .save(true)
            .commit(true)
            .build(),
    )
    .await
    .expect("Could not create calculation tool");
    assert!(fs::exists("workflows/calculation/calculation.cwl").unwrap());

    //create plot tool
    create_tool(
        &root,
        &ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "workflows/plot/plot.py".to_string(),
                "--results".to_string(),
                "results.csv".to_string(),
            ])
            .container(
                ContainerInfo::builder()
                    .image("workflows/plot/Dockerfile")
                    .tag("matplotlib")
                    .build(),
            )
            .save(true)
            .commit(true)
            .build(),
    )
    .await
    .expect("Could not create plot tool");
    assert!(fs::exists("workflows/plot/plot.cwl").unwrap());

    //create workflow
    let name = "test_workflow";
    create_workflow(name, None, false).expect("Could not create workflow");

    //add connections to inputs
    let wf_path = tool_path(name);
    let calculation = tool_path("calculation");
    let plot = tool_path("plot");

    with_workflow(name, |wf| {
        add_workflow_input_connection(
            wf,
            &wf_path,
            "population",
            WorkflowSlot::new(&calculation, "calculation", "population"),
        )
        .unwrap();
        add_workflow_input_connection(
            wf,
            &wf_path,
            "speakers",
            WorkflowSlot::new(&calculation, "calculation", "speakers"),
        )
        .unwrap();

        //connect second step
        add_workflow_step_connection(
            wf,
            &wf_path,
            WorkflowSlot::new(&calculation, "calculation", "results_csv"),
            WorkflowSlot::new(&plot, "plot", "results"),
        )
        .unwrap();

        //connect output
        add_workflow_output_connection(
            wf,
            &wf_path,
            WorkflowSlot::new(&plot, "plot", "results_svg"),
            "out",
        )
        .unwrap();
    });

    //save workflow
    assert!(fs::exists(&wf_path).unwrap());

    let repo = Repository::open(&root).expect("Could not open repository");
    stage_file(&repo, &wf_path).expect("Could not stage workflow");
    commit(&repo, &format!("✅ Saved workflow {name}")).expect("Could not commit workflow");

    let workflow = load_workflow(name);
    assert!(workflow.has_input("speakers"));
    assert!(workflow.has_input("population"));
    assert!(workflow.has_output("out"));
    assert!(workflow.has_step("calculation"));
    assert!(workflow.has_step("plot"));

    //remove outputs
    fs::remove_file("results.csv").unwrap();
    fs::remove_file("results.svg").unwrap();

    assert!(!fs::exists("results.csv").unwrap());
    assert!(!fs::exists("results.svg").unwrap());

    //execute workflow
    let wf_path = root.join(&wf_path);
    let inputs_yml = dir.path().join("inputs.yml").canonicalize().unwrap();
    let inputs = commonwl::engine::load_input_file_from_file(&inputs_yml, wf_path.parent().unwrap())
        .expect("Could not load inputs.yml");

    local_runner()
        .run_workflow(&wf_path, inputs, None)
        .await
        .expect("Could not execute Workflow");

    //check that only svg file is there now!
    assert!(!fs::exists("results.csv").unwrap());
    assert!(fs::exists("results.svg").unwrap());

    unsafe {
        env::set_var("PATH", restore);
    }
    env::set_current_dir(current).unwrap();
}
