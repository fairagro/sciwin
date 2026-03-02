use crate::config::Config;
use anyhow::{Context, Result};
use repository::Repository;
use repository::{commit, get_modified_files, initial_commit, stage_all};
use std::env;
use std::{
    fs,
    path::{Path, PathBuf},
};
use std::{fs::File, io::Write};

pub fn initialize_project(folder: &Path, arc: bool) -> anyhow::Result<()> {
    let folder = verify_base_dir(folder)?;

    let repo = if is_git_repo(&folder) {
        Repository::open(&folder).with_context(|| format!("Could not open Repository at {folder:?}"))?
    } else {
        init_git_repo(&folder)?
    };

    if arc {
        create_arc_folder_structure(&folder)?;
    }

    create_minimal_folder_structure(&folder)?;

    write_config(&folder)?;

    let files = get_modified_files(&repo);
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

fn write_config(dir: &Path) -> anyhow::Result<()> {
    // create workflow toml
    let mut cfg = Config::default();
    cfg.workflow.name = dir.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    fs::write(dir.join("workflow.toml"), toml::to_string_pretty(&cfg)?)?;

    Ok(())
}

fn is_git_repo(path: &Path) -> bool {
    // Determine the base directory from the provided path or use the current directory
    Repository::open(path).is_ok()
}

const GITIGNORE_CONTENT: &str = include_str!("../resources/default.gitignore");

fn init_git_repo(base_dir: &Path) -> anyhow::Result<Repository> {
    if !base_dir.exists() {
        fs::create_dir_all(base_dir).with_context(|| format!("Could not create Repository at {base_dir:?}"))?;
    }
    let repo = Repository::init(base_dir).with_context(|| format!("Could not init Repository at {base_dir:?}"))?;

    let gitignore_path = base_dir.join(PathBuf::from(".gitignore"));
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, GITIGNORE_CONTENT).with_context(|| format!("Could not create .gitignore file in {base_dir:?}"))?;
    }

    //append .s4n folder to .gitignore, whatever it may contains
    let mut gitignore = fs::OpenOptions::new().append(true).open(gitignore_path)?;
    writeln!(gitignore, "\n.s4n")?;

    Ok(repo)
}

fn create_minimal_folder_structure(base_dir: &Path) -> anyhow::Result<()> {
    // Create the base directory
    if !base_dir.exists() {
        fs::create_dir_all(base_dir)?;
    }

    // Check and create subdirectories
    let workflows_dir = base_dir.join("workflows");
    if !workflows_dir.exists() {
        fs::create_dir_all(&workflows_dir)?;
    }
    File::create(workflows_dir.join(".gitkeep"))?;

    Ok(())
}

fn verify_base_dir(folder: &Path) -> Result<PathBuf> {
    if let Some(parent) = folder.parent()
        && parent.exists()
    {
        let parent = parent.canonicalize().with_context(|| format!("Could not canonicalize {parent:?}"))?;
        let foldername = folder.file_name().unwrap_or_default();
        Ok(parent.join(foldername))
    } else {
        Ok(env::current_dir()?.join(folder))
    }
}

fn create_arc_folder_structure(base_dir: &Path) -> anyhow::Result<()> {
    // Create the base directory
    if !base_dir.exists() {
        fs::create_dir_all(base_dir).with_context(|| format!("Could not create folder at {base_dir:?}"))?;
    }

    create_investigation_excel_file(base_dir.to_str().unwrap_or(""))?;
    // Check and create subdirectories
    let dirs = vec!["studies", "assays", "runs"];
    for dir_name in dirs {
        let dir = base_dir.join(dir_name);
        if !dir.exists() {
            fs::create_dir_all(&dir).with_context(|| format!("Could not create folder at {dir:?}"))?;
        }
        File::create(dir.join(".gitkeep")).with_context(|| format!("Could not create .gitkeep at {dir:?}"))?;
    }

    Ok(())
}

fn create_investigation_excel_file(directory: &str) -> anyhow::Result<()> {
    // Construct the full path for the Excel file
    let excel_path = PathBuf::from(directory).join("isa_investigation.xlsx");

    //read binary file from resources
    let bin = include_bytes!("../resources/isa.investigation.xlsx");

    // Create the directory if it doesn't exist
    fs::create_dir_all(excel_path.parent().unwrap()).with_context(|| format!("Could not create folder for {excel_path:?}"))?;
    // Write the binary content to the file
    fs::write(&excel_path, bin).with_context(|| format!("Could not create {excel_path:?}"))?;

    Ok(())
}

pub fn git_cleanup(folder_name: Option<String>) -> Result<()> {
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
    use calamine::{Reader, Xlsx, open_workbook};
    use serial_test::serial;
    use std::{env, path::Path};
    use tempfile::{Builder, NamedTempFile, tempdir};
    use test_utils::check_git_user;

    #[test]
    #[serial]
    fn test_init_git_repo() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_folder = temp_dir.path().join("my_repo");

        let result = init_git_repo(&base_folder);
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

        let _ = init_git_repo(repo_dir_pa);
        let result = is_git_repo(repo_dir_pa);
        // Assert that directory is a git repo
        assert!(result, "Expected directory to be a git repo true, got false");
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

        let result = create_minimal_folder_structure(base_folder);

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
        let base_folder = temp_file.path();

        eprintln!("Base folder path: {base_folder:?}");
        //path to file instead of a directory, assert that it fails
        let result = create_minimal_folder_structure(base_folder);
        assert!(result.is_err(), "Expected failed initialization");
    }

    #[test]
    #[serial]
    fn test_init_s4n_minimal() {
        let temp_dir = Builder::new().prefix("init_without_arc_test").tempdir().unwrap();
        check_git_user().unwrap();

        let base_folder = temp_dir.path();

        //call method with temp dir
        let result = initialize_project(base_folder, false);
        eprintln!("{result:#?}");
        assert!(result.is_ok(), "Expected successful initialization");

        //check if directories were created
        let expected_dirs = vec!["workflows"];
        //check that other directories are not created
        let unexpected_dirs = vec!["assays", "studies", "runs"];

        //assert minimal folders do exist
        for dir in &expected_dirs {
            let full_path = PathBuf::from(temp_dir.path()).join(dir);
            assert!(full_path.exists(), "Directory {dir} does not exist");
        }
        //assert other arc folders do not exist
        for dir in &unexpected_dirs {
            let full_path = PathBuf::from(temp_dir.path()).join(dir);
            assert!(!full_path.exists(), "Directory {dir} does exist, but should not exist");
        }
    }

    #[test]
    #[serial]
    fn test_create_investigation_excel_file() {
        //create directory
        let temp_dir = tempdir().unwrap();
        let directory = temp_dir.path().to_str().unwrap();

        //call the function
        assert!(
            create_investigation_excel_file(directory).is_ok(),
            "Unexpected function create_investigation_excel fail"
        );

        //verify file exists
        let excel_path = PathBuf::from(directory).join("isa_investigation.xlsx");
        assert!(excel_path.exists(), "Excel file does not exist");

        let workbook: Xlsx<_> = open_workbook(excel_path).expect("Cannot open file");

        let sheets = workbook.sheet_names();

        //verify sheet name
        assert_eq!(sheets[0], "isa_investigation", "Worksheet name is incorrect");
    }

    #[test]
    #[serial]
    fn test_create_arc_folder_structure() {
        let temp_dir = tempdir().unwrap();

        let base_folder = temp_dir.path();

        let result = create_arc_folder_structure(base_folder);

        assert!(result.is_ok(), "Expected successful initialization");

        let expected_dirs = vec!["assays", "studies", "runs"];
        //assert that folders are created
        for dir in &expected_dirs {
            let full_path = PathBuf::from(temp_dir.path()).join(dir);
            assert!(full_path.exists(), "Directory {dir} does not exist");
        }
    }

    #[test]
    #[serial]
    fn test_create_arc_folder_structure_invalid() {
        //this test only gives create_arc_folder_structure a file instead of a directory
        let temp_file = NamedTempFile::new().unwrap();
        let base_path = temp_file.path();

        let result = create_arc_folder_structure(base_path);
        //result should not be okay because of invalid input
        assert!(result.is_err(), "Expected failed initialization");
    }

    #[test]
    #[serial]
    fn test_cleanup_no_folder() {
        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        eprintln!("Temporary directory: {temp_dir:?}");
        check_git_user().unwrap();
        // Create a subdirectory in the temporary directory
        std::fs::create_dir_all(&temp_dir).expect("Failed to create test directory");

        // Change to the temporary directory
        env::set_current_dir(&temp_dir).unwrap();
        eprintln!("Current directory changed to: {}", env::current_dir().unwrap().display());

        let git_folder = ".git";
        std::fs::create_dir(git_folder).unwrap();
        assert!(Path::new(git_folder).exists());

        git_cleanup(None).unwrap();
        assert!(!Path::new(git_folder).exists());
    }

    #[test]
    #[serial]
    fn test_cleanup_failed_init() {
        let temp_dir = tempdir().unwrap();
        let test_folder = temp_dir.path().join("my_repo");
        let result = initialize_project(test_folder.as_path(), false);
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
        temp_dir.close().unwrap();
    }
}
