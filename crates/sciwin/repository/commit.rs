use std::{iter, path::Path};

use git2::{Commit, Error, IndexAddOption, Repository, Status, StatusOptions};

pub fn get_modified_files(repo: &Repository) -> Vec<String> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true);

    let mut files = vec![];

    match repo.statuses(Some(&mut opts)) {
        Ok(statuses) => {
            // Print the status of the repository
            for entry in statuses.iter() {
                let status = entry.status();
                let path = entry.path().unwrap_or("unknown").to_owned();
                if status.contains(Status::WT_MODIFIED) || status.contains(Status::WT_NEW) {
                    files.push(path);
                }
            }
        }
        Err(e) => panic!("❌ Failed to get repository status: {e}"),
    }
    files
}

pub fn stage_file(repo: &Repository, path: impl AsRef<Path>) -> Result<(), Error> {
    let mut index = repo.index()?;
    index.add_path(path.as_ref())?;
    index.write()
}

pub fn stage_dir(repo: &Repository, path: impl AsRef<Path>) -> anyhow::Result<()> {
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

pub fn stage_all(repo: &Repository) -> Result<(), Error> {
    let mut index = repo.index()?;
    index.add_all(iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
    index.write()
}

pub fn commit(repo: &Repository, message: &str) -> Result<(), Error> {
    let head = repo.head()?;
    let parent = repo.find_commit(head.target().unwrap())?;
    commit_impl(repo, message, &[&parent])
}

pub fn initial_commit(repo: &Repository) -> Result<(), Error> {
    commit_impl(repo, "Initial Commit", &[])
}

fn commit_impl(repo: &Repository, message: &str, parents: &[&Commit<'_>]) -> Result<(), Error> {
    let mut index = repo.index()?;
    let new_oid = index.write_tree()?;
    let new_tree = repo.find_tree(new_oid)?;
    let author = repo.signature()?;
    repo.commit(Some("HEAD"), &author, &author, message, &new_tree, parents)?;
    Ok(())
}
