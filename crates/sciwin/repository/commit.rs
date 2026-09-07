use crate::{paths::TrustedPathExt, repository::RepositoryResult};
use git2::{Commit, IndexAddOption, Repository, Status, StatusOptions, build::CheckoutBuilder};
use std::{fs, iter, path::Path};

pub fn get_modified_files(repo: &Repository) -> RepositoryResult<Vec<String>> {
    modified_files(repo, false)
}

pub fn get_modified_files_recursive(repo: &Repository) -> RepositoryResult<Vec<String>> {
    modified_files(repo, true)
}

fn modified_files(repo: &Repository, recurse_untracked_dirs: bool) -> RepositoryResult<Vec<String>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(recurse_untracked_dirs);

    let mut files = vec![];

    let statuses = repo.statuses(Some(&mut opts))?;
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

    // `repo.workdir()` is resolved through libgit2, which follows symlinks
    let relative = repo
        .workdir()
        .and_then(|workdir| dunce::canonicalize(workdir).ok())
        .zip(dunce::canonicalize(path).ok())
        .and_then(|(workdir, path)| path.strip_prefix(workdir).map(Path::to_path_buf).ok())
        .unwrap_or_else(|| path.to_path_buf());

    let mut index = repo.index()?;
    index.add_path(&relative)?;
    Ok(index.write()?)
}

pub fn stage_dir(repo: &Repository, path: impl AsRef<Path>) -> RepositoryResult<()> {
    let paths = std::fs::read_dir(path)?;
    let workdir = repo.workdir().ok_or(git2::Error::new(
        git2::ErrorCode::Directory,
        git2::ErrorClass::None,
        "No Workdir",
    ))?;

    for entry in paths {
        let entry = entry?;
        let file_path = entry.path();
        let file_path = workdir.canonicalize_trusted(&file_path)?;

        if file_path.is_file() {
            stage_file(repo, file_path)?;
        }
    }
    Ok(())
}

/// Discards uncommitted changes to `relative_path` (as returned by
/// [`get_modified_files_recursive`]): restores a tracked file to its `HEAD`
/// content, or deletes it outright if it was untracked. Returns whether the
/// file still exists afterwards.
pub fn discard_file(repo: &Repository, relative_path: &str) -> RepositoryResult<bool> {
    let status = repo.status_file(Path::new(relative_path))?;

    if status.contains(Status::WT_NEW) {
        let workdir = repo.workdir().ok_or_else(|| {
            git2::Error::new(git2::ErrorCode::Directory, git2::ErrorClass::None, "No Workdir")
        })?;
        let full_path = workdir.join(relative_path);
        if full_path.exists() {
            fs::remove_file(full_path)?;
        }
        let mut index = repo.index()?;
        let _ = index.remove_path(Path::new(relative_path));
        index.write()?;
        return Ok(false);
    }

    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    checkout.path(relative_path);
    repo.checkout_head(Some(&mut checkout))?;
    Ok(true)
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
