pub mod config;

use crate::project::config::Config;
use crate::repository::{commit, get_modified_files, initial_commit, stage_all};
use git2::Repository;
use miette::Diagnostic;
use std::path::Component;
use std::{
    fs,
    path::{Path, PathBuf},
};
use std::{fs::File, io::Write};
use thiserror::Error;

/// Result alias for this module. See [`crate::Result`] for code spanning several modules.
pub type ProjectResult<T> = Result<T, ProjectError>;

/// Anything project initialization can fail with.
#[derive(Error, Diagnostic, Debug)]
pub enum ProjectError {
    #[error("invalid project path {}: {reason}", path.display())]
    #[diagnostic(code = "project::InvalidPath")]
    InvalidPath { path: PathBuf, reason: String },

    #[error("could not canonicalize path {}", path.display())]
    #[diagnostic(code = "project::Canonicalize")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not open git repository at {}", path.display())]
    #[diagnostic(code = "project::OpenRepository")]
    OpenRepository {
        path: PathBuf,
        #[source]
        source: git2::Error,
    },

    #[error("could not initialize git repository at {}", path.display())]
    #[diagnostic(code = "project::InitRepository")]
    InitRepository {
        path: PathBuf,
        #[source]
        source: git2::Error,
    },

    #[error(transparent)]
    #[diagnostic(transparent)]
    Repository(#[from] crate::repository::RepositoryError),

    #[error(transparent)]
    #[diagnostic(code = "io::Error")]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(code = "toml::ser::Error")]
    Toml(#[from] toml::ser::Error),

    #[error(transparent)]
    #[diagnostic(code = "toml::de::Error")]
    TomlDe(#[from] toml::de::Error),
}

/// Initializes a new project in the specified folder. If no folder is provided, it initializes in the current directory.
/// `folder` may be relative (resolved against, and confined to, the current working directory) or absolute (used as-is).
/// Either way, no path-traversal components ('..' or '.') are allowed.
pub fn initialize_project(folder: impl AsRef<Path>) -> ProjectResult<()> {
    let folder = verify_base_dir(folder.as_ref())?;

    let repo = if is_git_repo(&folder) {
        Repository::open(&folder).map_err(|source| ProjectError::OpenRepository {
            path: folder.clone(),
            source,
        })?
    } else {
        init_git_repo(&folder)?
    };

    create_minimal_folder_structure(&folder)?;

    write_config(&folder)?;

    let files = get_modified_files(&repo)?;
    if !files.is_empty() {
        stage_all(&repo)?;
        if repo.head().is_ok() {
            commit(&repo, "🚀 Initialized Project")?;
        } else {
            initial_commit(&repo)?;
        }
    }

    Ok(())
}

const WORKFLOW_TOML: &str = "workflow.toml";

fn write_config(dir: &Path) -> ProjectResult<()> {
    let dir = resolve_canonical(dir)?;

    // create workflow toml
    let mut cfg = Config::default();
    cfg.workflow.name = dir
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    fs::write(dir.join(WORKFLOW_TOML), toml::to_string_pretty(&cfg)?)?;

    Ok(())
}

/// Reads `workflow.toml` out of `dir`.
///
/// # Errors
/// Path is not readable, or the file is not valid TOML for [`Config`]
pub fn read_config(dir: &Path) -> ProjectResult<Config> {
    let contents = fs::read_to_string(dir.join(WORKFLOW_TOML))?;
    Ok(toml::from_str(&contents)?)
}

/// [`read_config`], but searching `start` and each of its ancestors in turn. Bounded to `start`'s
/// own git repository when it's in one.
#[must_use]
pub fn find_config(start: &Path) -> Option<Config> {
    let boundary = Repository::discover(start)
        .ok()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf))
        .and_then(|path| canonicalize(&path).ok());

    let mut dir = Some(canonicalize(start).unwrap_or_else(|_| start.to_path_buf()));
    while let Some(d) = dir {
        if let Ok(config) = read_config(&d) {
            return Some(config);
        }
        if boundary.as_deref() == Some(d.as_path()) {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

fn is_git_repo(path: &Path) -> bool {
    // Determine the base directory from the provided path or use the current directory
    Repository::open(path).is_ok()
}

const GITIGNORE_CONTENT: &str = include_str!("../resources/default.gitignore");

fn init_git_repo(base_dir: &Path) -> ProjectResult<Repository> {
    let base_dir = resolve_canonical(base_dir)?;

    if !base_dir.exists() {
        fs::create_dir_all(&base_dir)?;
    }
    let repo = Repository::init(&base_dir).map_err(|source| ProjectError::InitRepository {
        path: base_dir.clone(),
        source,
    })?;

    let gitignore_path = base_dir.join(PathBuf::from(".gitignore"));
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, GITIGNORE_CONTENT)?;
    }

    //append .s4n folder to .gitignore, whatever it may contains
    let mut gitignore = fs::OpenOptions::new().append(true).open(gitignore_path)?;
    writeln!(gitignore, "\n.s4n")?;

    Ok(repo)
}

fn create_minimal_folder_structure(base_dir: &Path) -> ProjectResult<()> {
    let base_dir = resolve_canonical(base_dir)?;

    // Create the base directory
    if !base_dir.exists() {
        fs::create_dir_all(&base_dir)?;
    }

    // Check and create subdirectories
    let workflows_dir = base_dir.join("workflows");
    if !workflows_dir.exists() {
        fs::create_dir_all(&workflows_dir)?;
    }
    File::create(workflows_dir.join(".gitkeep"))?;

    Ok(())
}

/// Validates that `folder` contains no traversal components ('..' or '.') and no
/// stray root/prefix components in the middle of the path. Absolute paths are
/// allowed (their leading root/prefix component is expected, not stray); a
/// relative path is additionally required to stay within the current directory
/// once resolved (see `resolve_relative_to_cwd`). This is what actually defends
/// against path-traversal: a caller-supplied path that resolves outside the
/// directory it was meant to stay in.
fn verify_base_dir(folder: &Path) -> ProjectResult<PathBuf> {
    let is_absolute = folder.is_absolute();

    for component in folder.components() {
        let reason = match component {
            Component::ParentDir => Some("must not contain parent directory components ('..')"),
            Component::CurDir => Some("must not be current directory '.'"),
            Component::Prefix(_) if !is_absolute => Some("contains an invalid path prefix"),
            Component::RootDir if !is_absolute => Some("must not contain root directory components"),
            _ => None,
        };
        if let Some(reason) = reason {
            return Err(ProjectError::InvalidPath {
                path: folder.to_path_buf(),
                reason: reason.to_string(),
            });
        }
    }

    if is_absolute {
        return resolve_canonical(folder);
    }

    let cwd = canonicalize(Path::new("."))?;
    let path = cwd.join(folder);
    verify_relative_to_cwd(&path)
}

/// Resolves `path` to its canonical (symlink-free) form, whether or not it
/// exists yet, and confirms it isn't a plain file.
fn resolve_canonical(path: &Path) -> ProjectResult<PathBuf> {
    let mut path = path.to_path_buf();
    if path.exists() {
        path = canonicalize(&path)?;
    } else if let Some(parent) = path.parent() {
        let canonical_parent = canonicalize(parent)?;
        let file_name = path.file_name().ok_or_else(|| ProjectError::InvalidPath {
            path: path.clone(),
            reason: "has no valid file name".to_string(),
        })?;
        path = canonical_parent.join(file_name);
    } else {
        return Err(ProjectError::InvalidPath {
            path: path.clone(),
            reason: "has no valid parent directory".to_string(),
        });
    }

    if path.exists() && path.is_file() {
        return Err(ProjectError::InvalidPath {
            path,
            reason: "is a file, expected a directory".to_string(),
        });
    }

    Ok(path)
}

fn verify_relative_to_cwd(path: &Path) -> ProjectResult<PathBuf> {
    let cwd = canonicalize(Path::new("."))?;
    let path = resolve_canonical(path)?;

    // check whether it still starts with cwd
    if !path.starts_with(&cwd) {
        return Err(ProjectError::InvalidPath {
            path,
            reason: "escapes the current working directory".to_string(),
        });
    }

    Ok(path)
}

fn canonicalize(path: &Path) -> ProjectResult<PathBuf> {
    dunce::canonicalize(path).map_err(|source| ProjectError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

pub fn git_cleanup(folder_name: Option<String>) -> ProjectResult<()> {
    // init project in folder name failed, delete it
    if let Some(folder) = folder_name {
        std::fs::remove_dir_all(&folder)?;
    }
    // init project in current folder failed, only delete .git folder
    else {
        let git_folder = ".git";
        std::fs::remove_dir_all(git_folder)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::{env, path::Path};
    use tempfile::{Builder, NamedTempFile, tempdir};

    /// Ensures a `user.name`/`user.email` are configured so `git2::Repository::signature()`
    /// succeeds on CI runners that don't have a global git identity set. Retries because
    /// parallel test binaries can race on the same global git config file.
    fn check_git_user() -> Result<(), git2::Error> {
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

    #[test]
    #[serial]
    fn test_init_git_repo() {
        let cwd = env::current_dir().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        env::set_current_dir(&temp_dir).unwrap();

        let base_folder = temp_dir.path().join("my_repo");
        let result = init_git_repo(&base_folder);

        env::set_current_dir(cwd).unwrap();
        assert!(result.is_ok(), "Expected successful initialization");

        // Verify that the .git directory was created
        let git_dir = base_folder.join(".git");
        assert!(git_dir.exists(), "Expected .git directory to be created");
    }

    #[test]
    #[serial]
    fn test_is_git_repo() {
        let repo_dir = tempdir().unwrap();
        let repo_dir_pa = repo_dir.path();

        let cwd = env::current_dir().unwrap();
        env::set_current_dir(repo_dir_pa).unwrap();

        init_git_repo(repo_dir_pa).unwrap();
        env::set_current_dir(cwd).unwrap();

        let result = is_git_repo(repo_dir_pa);
        // Assert that directory is a git repo
        assert!(
            result,
            "Expected directory to be a git repo true, got false"
        );
    }

    #[test]
    #[serial]
    fn test_is_not_git_repo() {
        //create directory that is not a git repo
        let no_repo = tempdir().unwrap();

        let no_repo_str = no_repo.path();
        // call is_git repo_function
        let result = is_git_repo(no_repo_str);

        // assert that it is not a git repo
        assert!(!result, "Expected directory to not be a git repo");
    }

    #[test]
    #[serial]
    fn test_create_minimal_folder_structure() {
        let temp_dir = Builder::new().prefix("minimal_folder").tempdir().unwrap();

        let base_folder = temp_dir.path();

        let cwd = env::current_dir().unwrap();
        env::set_current_dir(base_folder).unwrap();

        let result = create_minimal_folder_structure(base_folder);

        env::set_current_dir(&cwd).unwrap();

        //test if result is ok
        assert!(result.is_ok(), "Expected successful initialization");

        let expected_dirs = vec!["workflows"];
        //assert that folders exist
        for dir in &expected_dirs {
            let full_path = PathBuf::from(temp_dir.path()).join(dir);
            assert!(full_path.exists(), "Directory {dir} does not exist");
        }
    }

    #[test]
    #[serial]
    fn test_create_minimal_folder_structure_invalid() {
        //create an invalid file input
        let temp_file = NamedTempFile::new().unwrap();
        let base_folder_invalid = temp_file.path();
        let base_dir = base_folder_invalid.parent().unwrap();

        eprintln!("Base folder path: {base_folder_invalid:?}");

        let cwd = env::current_dir().unwrap();
        env::set_current_dir(base_dir).unwrap();

        //path to file instead of a directory, assert that it fails
        let result = create_minimal_folder_structure(base_folder_invalid);

        env::set_current_dir(&cwd).unwrap();

        assert!(result.is_err(), "Expected failed initialization");
    }

    #[test]
    #[serial]
    fn test_init_s4n_minimal() {
        let temp_dir = Builder::new()
            .prefix("init_without_arc_test")
            .tempdir()
            .unwrap();
        check_git_user().unwrap();

        let cwd = env::current_dir().unwrap();
        env::set_current_dir(&temp_dir).unwrap();

        //call method with temp dir
        let result = initialize_project("");
        eprintln!("{result:#?}");
        assert!(result.is_ok(), "Expected successful initialization");

        env::set_current_dir(&cwd).unwrap();

        //check if directories were created
        let expected_dirs = vec!["workflows"];

        //assert minimal folders do exist
        for dir in &expected_dirs {
            let full_path = PathBuf::from(temp_dir.path()).join(dir);
            assert!(full_path.exists(), "Directory {dir} does not exist");
        }
    }

    #[test]
    fn test_init_s4n_absolute_path() {
        // a GUI caller (no meaningful cwd of its own) passes an absolute path directly
        let temp_dir = Builder::new()
            .prefix("init_absolute_test")
            .tempdir()
            .unwrap();
        check_git_user().unwrap();

        let target = temp_dir.path().join("my_project");
        let result = initialize_project(&target);
        eprintln!("{result:#?}");
        assert!(result.is_ok(), "Expected absolute-path initialization to succeed");

        assert!(target.join("workflow.toml").exists());
        assert!(target.join("workflows").exists());
        assert!(target.join(".git").exists());
    }

    #[test]
    fn test_verify_base_dir_rejects_traversal_in_relative_path() {
        assert!(verify_base_dir(Path::new("foo/../bar")).is_err());
    }

    #[test]
    fn test_verify_base_dir_rejects_traversal_in_absolute_path() {
        assert!(verify_base_dir(Path::new("/tmp/foo/../bar")).is_err());
    }

    #[test]
    fn test_verify_base_dir_accepts_plain_absolute_path() {
        let temp_dir = tempdir().unwrap();
        let target = temp_dir.path().join("project");
        let result = verify_base_dir(&target);
        assert!(result.is_ok(), "Expected a clean absolute path to be accepted: {result:?}");
    }

    #[test]
    #[serial]
    fn test_read_config_roundtrip() {
        let cwd = env::current_dir().unwrap();
        let temp_dir = tempdir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        write_config(temp_dir.path()).unwrap();
        let cfg = read_config(temp_dir.path());

        env::set_current_dir(cwd).unwrap();

        let cfg = cfg.unwrap();
        assert_eq!(
            cfg.workflow.name,
            temp_dir
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        );
    }

    #[test]
    #[serial]
    fn test_read_config_missing() {
        let temp_dir = tempdir().unwrap();

        let result = read_config(temp_dir.path());
        assert!(result.is_err(), "Expected missing workflow.toml to fail");
    }

    #[test]
    #[serial]
    fn test_cleanup_no_folder() {
        let cwd = env::current_dir().unwrap();
        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        eprintln!("Temporary directory: {temp_dir:?}");
        check_git_user().unwrap();
        // Create a subdirectory in the temporary directory
        std::fs::create_dir_all(&temp_dir).expect("Failed to create test directory");

        // Change to the temporary directory
        env::set_current_dir(&temp_dir).unwrap();
        eprintln!(
            "Current directory changed to: {}",
            env::current_dir().unwrap().display()
        );

        let git_folder = ".git";
        std::fs::create_dir(git_folder).unwrap();
        assert!(Path::new(git_folder).exists());

        git_cleanup(None).unwrap();
        assert!(!Path::new(git_folder).exists());
        env::set_current_dir(&cwd).unwrap();
    }

    #[test]
    #[serial]
    fn test_cleanup_failed_init() {
        check_git_user().unwrap();
        let cwd = env::current_dir().unwrap();

        let temp_dir = tempdir().unwrap();
        env::set_current_dir(&temp_dir).unwrap();
        let test_folder = temp_dir.path().join("my_repo");
        let result = initialize_project("my_repo");
        if let Err(e) = &result {
            eprintln!("Error initializing git repo: {}", e);
        }
        assert!(result.is_ok(), "Expected successful initialization");
        assert!(Path::new(&test_folder).exists());
        let git_dir = test_folder.join(".git");
        assert!(git_dir.exists(), "Expected .git directory to be created");
        git_cleanup(Some(test_folder.display().to_string())).unwrap();
        assert!(!Path::new(&test_folder).exists());
        assert!(!git_dir.exists(), "Expected .git directory to be deleted");

        env::set_current_dir(cwd).unwrap();
        temp_dir.close().unwrap();
    }

    #[test]
    #[serial]
    fn test_find_config_walks_up_outside_a_repo() {
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(WORKFLOW_TOML),
            "[workflow]\nname = \"found-me\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();
        let nested = temp_dir.path().join("workflows/main");
        fs::create_dir_all(&nested).unwrap();

        let config = find_config(&nested).expect("should find workflow.toml above nested");
        assert_eq!(config.workflow.name, "found-me");
    }

    #[test]
    #[serial]
    fn test_find_config_does_not_cross_a_repo_boundary() {
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(WORKFLOW_TOML),
            "[workflow]\nname = \"outside-the-repo\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();
        let repo_root = temp_dir.path().join("repo");
        let nested = repo_root.join("workflows/main");
        fs::create_dir_all(&nested).unwrap();
        Repository::init(&repo_root).unwrap();

        assert!(find_config(&nested).is_none());
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn test_find_config_boundary_survives_a_symlinked_start_path() {
        // Mimics macOS, where `/tmp` (a common `start`) is itself a symlink (`/tmp` ->
        // `/private/tmp`) -- `git2`'s `workdir()` returns the resolved form, so comparing it
        // against an un-resolved walk path would never match.
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(WORKFLOW_TOML),
            "[workflow]\nname = \"outside-the-repo\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();
        let real_root = temp_dir.path().join("real_root");
        let repo_root = real_root.join("repo");
        let nested = repo_root.join("workflows/main");
        fs::create_dir_all(&nested).unwrap();
        Repository::init(&repo_root).unwrap();

        let symlinked_root = temp_dir.path().join("symlinked_root");
        std::os::unix::fs::symlink(&real_root, &symlinked_root).unwrap();
        let nested_via_symlink = symlinked_root.join("repo/workflows/main");

        assert!(find_config(&nested_via_symlink).is_none());
    }
}
