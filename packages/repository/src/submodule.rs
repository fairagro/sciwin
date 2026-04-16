use git2::{Error, Repository, build::RepoBuilder};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{commit, ini, stage_all};

/// Returns a list of paths of all submodules in the repository.
pub fn get_submodule_paths(repo: &Repository) -> Result<Vec<PathBuf>, Error> {
    let submodules = repo.submodules()?;
    let paths = submodules.iter().map(|s| s.path().to_path_buf()).collect();
    Ok(paths)
}

/// Adds a submodule to the current repository, stages the changes, and commits them.
pub fn add_submodule(repo: &mut Repository, url: &str, branch: &Option<String>, path: &Path) -> Result<(), Error> {
    //clone and initialize submodule
    if let Some(branch) = branch {
        RepoBuilder::new().branch(branch).clone(url, path)?;
    } else {
        RepoBuilder::new().clone(url, path)?;
    }

    let repo_base_path = repo
        .path()
        .join("../")
        .canonicalize()
        .map_err(|e| git2::Error::from_str(&e.to_string()))?;
    let relative_path = path.strip_prefix(repo_base_path).unwrap_or(path);

    let mut module = repo.submodule(url, relative_path, false)?;

    //set correct branch to submodule
    if let Some(branch) = branch {
        let mut repo = Repository::open(repo.path())?;
        repo.submodule_set_branch(module.name().unwrap(), branch)?;
        module.sync()?;
    }

    //commit
    module.add_finalize()?;
    let name = module.name().unwrap_or("");
    commit(repo, &format!("📦 Installed Package {}", name.strip_prefix("packages/").unwrap_or(name)))?;
    Ok(())
}

/// Removes a submodule from the current repository, stages the changes, and commits them.
pub fn remove_submodule(repo: &Repository, name: &str) -> Result<(), Error> {
    let module = repo.find_submodule(name)?;
    let repo_base_path = repo
        .path()
        .join("../")
        .canonicalize()
        .map_err(|e| git2::Error::from_str(&e.to_string()))?;
    let path = repo_base_path.join(module.path());

    fs::remove_dir_all(path).ok();

    //remove ksubmodule config
    let prefix = format!("submodule \"{name}\"");
    ini::remove_section(repo.path().join("config"), &prefix).map_err(|_| git2::Error::from_str("Could not delete config entry"))?;
    ini::remove_section(repo_base_path.join(".gitmodules"), &prefix).map_err(|_| git2::Error::from_str("Could not delete .gitmodulesg entry"))?;

    //stage and commit
    stage_all(repo)?;
    commit(repo, &format!("📦 Removed Package {}", name.strip_prefix("packages/").unwrap_or(name)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fstest::fstest;
    use fstest::serial_test;
    use std::env;

    #[fstest(repo = true)]
    fn test_add_remove_submodule() {
        let current_dir = env::current_dir().unwrap_or(PathBuf::from("."));
        let mut repo = Repository::init(&current_dir).unwrap();

        let result = add_submodule(
            &mut repo,
            "https://github.com/JensKrumsieck/PorphyStruct",
            &Some("docs".to_string()),
            Path::new("ps"),
        );
        assert!(result.is_ok());

        //check whether a file is present
        assert!(fs::exists("ps/LICENSE").unwrap());

        let result = remove_submodule(&repo, "ps");
        assert!(result.is_ok());

        //check whether a file is absent
        assert!(!fs::exists("ps/LICENSE").unwrap());
    }
}
