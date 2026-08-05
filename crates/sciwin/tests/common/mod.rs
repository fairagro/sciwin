#![allow(dead_code)]
use commonwl::{
    documents::{CWLDocument, Workflow},
    engine::{ContainerEngine, LocalBackend},
    format::format_cwl,
    load_cwl_file,
    storage::{StorageBackend, StoragePath},
};
use sciwin::{execution::TaskRunner, paths::TrustedPathExt};
use std::{
    env, fs,
    path::{Path, PathBuf},
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

/// A `TaskRunner` backed by the local engine, same setup the CLI's `execute local` uses.
pub fn local_runner() -> TaskRunner<LocalBackend> {
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
    let mut yaml = serde_saphyr::to_string(&CWLDocument::Workflow(workflow)).unwrap();
    yaml = format_cwl(&yaml).unwrap();
    fs::write(tool_path(name), yaml).unwrap();
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
