use crate::repository::RepositoryResult;
use git2::{BranchType, Repository};

/// The shorthand name of the branch `HEAD` currently points to, or `None` for
/// a detached `HEAD` (e.g. mid-rebase, or a checked-out tag/commit).
pub fn current_branch(repo: &Repository) -> RepositoryResult<Option<String>> {
    let Ok(head) = repo.head() else {
        return Ok(None);
    };
    if !head.is_branch() {
        return Ok(None);
    }
    Ok(head.shorthand()?.to_owned().into())
}

/// Names of all local branches, sorted alphabetically.
pub fn list_branches(repo: &Repository) -> RepositoryResult<Vec<String>> {
    let mut branches: Vec<String> = repo
        .branches(Some(BranchType::Local))?
        .filter_map(|entry| entry.ok())
        .filter_map(|(branch, _)| branch.name().ok().flatten().map(str::to_owned))
        .collect();
    branches.sort();
    Ok(branches)
}

/// Switches `HEAD` and the working tree to the tip of the local branch `name`.
/// Uses libgit2's safe checkout, which errors out rather than overwriting
/// files with uncommitted changes.
pub fn checkout_branch(repo: &Repository, name: &str) -> RepositoryResult<()> {
    let refname = format!("refs/heads/{name}");
    let object = repo.revparse_single(&refname)?;
    repo.checkout_tree(&object, None)?;
    repo.set_head(&refname)?;
    Ok(())
}
