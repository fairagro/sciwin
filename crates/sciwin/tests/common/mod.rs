#![allow(dead_code)]
use commonwl::{
    documents::{CWLDocument, Workflow},
    engine::{ContainerEngine, LocalBackend},
    load_cwl_file,
    storage::{StorageBackend, StoragePath},
};
use sciwin::{execution::TaskRunner, paths::TrustedPathExt};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// Recursively copies `src` into `dst`, creating `dst` if needed. Test-only stand-in for a
/// `copy_dir` dependency, which crates/sciwin does not otherwise need.
pub fn copy_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) {
    let (src, dst) = (src.as_ref(), dst.as_ref());
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join_trusted_unchecked(entry.file_name()).unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_dir(entry.path(), target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A `TaskRunner` backed by the local engine, same setup the CLI's `execute run` uses.
pub fn local_runner() -> TaskRunner {
    let storage = Arc::new(StorageBackend::new());
    let backend = Arc::new(LocalBackend::new(
        ContainerEngine::default(),
        storage,
        StoragePath::from_local(&env::temp_dir()),
    ));
    TaskRunner::new(backend)
}

/// Where `create_workflow`/the tests below expect a workflow named `name` to live, following
/// the same `workflows/<name>/<name>.cwl` convention `create_tool` uses.
pub fn tool_path(name: &str) -> PathBuf {
    Path::new("workflows")
        .join(name)
        .join(format!("{name}.cwl"))
}

pub fn load_workflow(name: &str) -> Workflow {
    let CWLDocument::Workflow(wf) = load_cwl_file(tool_path(name), true).unwrap() else {
        panic!("{name} is not a workflow");
    };
    wf
}

pub fn save_workflow(name: &str, workflow: Workflow) {
    sciwin::authoring::workflow::save_workflow(&CWLDocument::Workflow(workflow), &tool_path(name)).unwrap();
}

/// Loads workflow `name`, hands it to `f` to mutate with real `sciwin::authoring::workflow`
/// calls, then writes the result back -- the load/save boilerplate around a batch of
/// `add_workflow_*_connection`/`remove_workflow_*_connection` calls made with real paths and
/// slot ids, not a CLI-syntax string parser.
pub fn with_workflow(name: &str, f: impl FnOnce(&mut Workflow)) {
    let mut wf = load_workflow(name);
    f(&mut wf);
    save_workflow(name, wf);
}

/// Ensures a `user.name`/`user.email` are configured so `git2::Repository::signature()`
/// succeeds on CI runners that don't have a global git identity set. Retries because
/// parallel test binaries can race on the same global git config file.
pub fn check_git_user() -> Result<(), git2::Error> {
    let mut last_err: Option<git2::Error> = None;
    for i in 0..5 {
        match write_git_user() {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                eprintln!("git config is currently being accessed. Attempt #{i}");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    Err(last_err.expect("last_err must be set after retries are exhausted"))
}

fn write_git_user() -> Result<(), git2::Error> {
    let mut config = git2::Config::open_default()?;
    if config.get_string("user.name").is_err() {
        config.set_str("user.name", "s4n-test")?;
    }
    if config.get_string("user.email").is_err() {
        config.set_str("user.email", "s4n-test@example.com")?;
    }
    Ok(())
}

pub fn os_path(path: &str) -> String {
    if cfg!(target_os = "windows") {
        Path::new(path).to_string_lossy().replace('/', "\\")
    } else {
        path.to_string()
    }
}

pub fn setup_python(dir_str: &str) -> (String, String) {
    //windows stuff
    let ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let path_sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let venv_scripts = if cfg!(target_os = "windows") {
        "Scripts"
    } else {
        "bin"
    };

    //set up python venv
    let output = Command::new("python3")
        .arg("-m")
        .arg("venv")
        .arg(".venv")
        .output()
        .expect("Could not create venv");
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));

    let old_path = env::var("PATH").unwrap();
    let python_path = format!("{dir_str}/.venv/{venv_scripts}");
    let new_path = format!("{python_path}{path_sep}{old_path}");

    //install packages
    let req_path = format!("{dir_str}/requirements.txt");
    let output = Command::new(python_path + "/pip" + ext)
        .arg("install")
        .arg("-r")
        .arg(req_path)
        .output()
        .expect("Could not find pip");
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));

    (new_path, old_path)
}
