mod commit;
mod ini;
pub mod submodule;

pub use commit::*;
// Re-export git2::Repository for external use
pub use git2::Config;
pub use git2::Repository;
use miette::Diagnostic;
use thiserror::Error;

pub type RepositoryResult<T> = Result<T, RepositoryError>;

#[derive(Error, Diagnostic, Debug)]
pub enum RepositoryError {
    #[error(transparent)]
    #[diagnostic(code = "git2::Error")]
    Git(#[from] git2::Error),

    #[error(transparent)]
    #[diagnostic(code = "io::Error")]
    IO(#[from] std::io::Error),
}
