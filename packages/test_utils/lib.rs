use copy_dir::copy_dir;
use core::panic;
use git2::Repository;
use serde_json::Value;
use std::{
    env::{self},
    fs::{self, create_dir_all},
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::{TempDir, tempdir};

mod git;
pub use git::*;

pub fn setup_python(dir_str: &str) -> (String, String) {
    //windows stuff
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let path_sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let venv_scripts = if cfg!(target_os = "windows") { "Scripts" } else { "bin" };

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

pub struct Repo<'a>(&'a Path);

pub fn repository(path: &Path) -> Repo<'_> {
    Repo(path)
}

impl Repo<'_> {
    pub fn dir<P: AsRef<Path>>(&self, name: P) -> &Self {
        create_dir_all(self.0.join(name)).unwrap();
        self
    }

    pub fn copy_file<P: AsRef<Path>>(&self, file: P, path: P) -> &Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let root = root.canonicalize().unwrap();
        fs::copy(root.join(file), self.0.join(path)).unwrap();
        self
    }

    pub fn copy_dir<P: AsRef<Path>>(&self, dir: P, path: P) -> &Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let root = root.canonicalize().unwrap();
        copy_dir(root.join(dir), self.0.join(path)).unwrap();

        self
    }

    pub fn finalize(&self) -> &Self {
        check_git_user().unwrap();
        let repo = Repository::init(self.0).expect("Failed to create a blank repository");
        stage_all(&repo).expect("Could not stage files");

        if repo.signature().is_err() {
            let mut cfg = repo.config().expect("Could not get config");
            cfg.set_str("user.name", "Derp").expect("Could not set name");
            cfg.set_str("user.email", "derp@google.de").expect("Could not set email");
        }
        initial_commit(&repo).expect("Could not create inital commit");

        self
    }

    pub fn enter(&self) -> PathBuf {
        let current_dir = env::current_dir().unwrap();
        env::set_current_dir(self.0).unwrap();
        current_dir
    }
}

/// Sets up a temporary repository with test data
fn set_up_repository() -> TempDir {
    let dir = tempdir().expect("Failed to create a temporary directory");
    repository(dir.path())
        .dir("scripts")
        .dir("data")
        .copy_file("testdata/echo.py", "scripts/echo.py")
        .copy_file("testdata/echo2.py", "scripts/echo2.py")
        .copy_file("testdata/echo3.py", "scripts/echo3.py")
        .copy_file("testdata/script_test.py", "scripts/script_test.py")
        .copy_file("testdata/echo_inline.py", "scripts/echo_inline.py")
        .copy_file("testdata/input.txt", "data/input.txt")
        .copy_file("testdata/input2.txt", "data/input2.txt")
        .copy_file("testdata/Dockerfile", "Dockerfile")
        .finalize();
    dir
}

/// Sets up a repository with the files in `testdata` in tmp folder.
/// You *must* specify `#[serial]` for those tests
pub fn with_temp_repository<F>(test: F)
where
    F: FnOnce(&TempDir) + panic::UnwindSafe,
{
    let dir = set_up_repository();
    let current_dir = env::current_dir().expect("Could not get current working directory");
    env::set_current_dir(dir.path()).expect("Could not set current dir");

    test(&dir);

    env::set_current_dir(current_dir).expect("Could not reset current dir");
    dir.close().unwrap();
}

pub fn os_path(path: &str) -> String {
    if cfg!(target_os = "windows") {
        Path::new(path).to_string_lossy().replace('/', "\\")
    } else {
        path.to_string()
    }
}

pub fn normalize_json_newlines(val: &mut Value) {
    match val {
        Value::String(s) => {
            *s = s.replace("\r\n", "\n");
        }
        Value::Array(arr) => {
            for item in arr {
                normalize_json_newlines(item);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                normalize_json_newlines(value);
            }
        }
        _ => {}
    }
}
