use crate::repository::RepositoryResult;
use git2::{Commit, IndexAddOption, Repository, Status, StatusOptions};
use std::{iter, path::Path};

pub fn get_modified_files(repo: &Repository) -> RepositoryResult<Vec<String>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true);

    let mut files = vec![];

    let statuses = repo.statuses(Some(&mut opts))?;
    // Print the status of the repository
    for entry in statuses.iter() {
        let status = entry.status();
        let path = entry.path().unwrap_or("unknown").to_owned();
        if status.contains(Status::WT_MODIFIED) || status.contains(Status::WT_NEW) {
            files.push(path);
        }
    }

    Ok(files)
}

/// Stages a single file. `path` may be absolute or already relative to the work tree --
/// `git2` only accepts the latter, so an absolute path under the work tree is stripped down
/// to it first.
pub fn stage_file(repo: &Repository, path: impl AsRef<Path>) -> RepositoryResult<()> {
    let path = path.as_ref();
    let relative = repo
        .workdir()
        .and_then(|workdir| path.strip_prefix(workdir).ok())
        .unwrap_or(path);

    let mut index = repo.index()?;
    index.add_path(relative)?;
    Ok(index.write()?)
}

pub fn stage_dir(repo: &Repository, path: impl AsRef<Path>) -> RepositoryResult<()> {
    let paths = std::fs::read_dir(path)?;
    for entry in paths {
        let entry = entry?;
        let file_path = entry.path();
        if file_path.is_file() {
            stage_file(repo, file_path)?;
        }
    }
    Ok(())
}

pub fn stage_all(repo: &Repository) -> RepositoryResult<()> {
    let mut index = repo.index()?;
    index.add_all(iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
    Ok(index.write()?)
}

pub fn commit(repo: &Repository, message: &str) -> RepositoryResult<()> {
    let head = repo.head()?;
    let parent = repo.find_commit(head.target().unwrap())?;
    commit_impl(repo, message, &[&parent])
}

pub fn initial_commit(repo: &Repository) -> RepositoryResult<()> {
    commit_impl(repo, "Initial Commit", &[])
}

fn commit_impl(repo: &Repository, message: &str, parents: &[&Commit<'_>]) -> RepositoryResult<()> {
    let mut index = repo.index()?;
    let new_oid = index.write_tree()?;
    let new_tree = repo.find_tree(new_oid)?;
    let author = repo.signature()?;
    repo.commit(Some("HEAD"), &author, &author, message, &new_tree, parents)?;
    Ok(())
}
