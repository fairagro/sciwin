//! The crate-level error every module's error folds into.

use miette::Diagnostic;
use thiserror::Error;

/// Convenience alias for fallible operations that can fail in more than one module.
///
/// Individual modules have narrower aliases ([`crate::authoring::AuthoringResult`],
/// [`crate::execution::RunnerResult`], [`crate::repository::RepositoryResult`]); prefer those
/// inside a single area, and this one where the areas meet.
pub type Result<T> = std::result::Result<T, Error>;

/// Anything this crate can fail with.
///
/// Each variant is transparent, so the underlying diagnostic -- its code, its help text --
/// reaches the caller unchanged. `?` converts any module error into this automatically.
#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    #[error(transparent)]
    #[diagnostic(transparent)]
    Authoring(#[from] crate::authoring::AuthoringError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Execution(#[from] crate::execution::RunnerError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Repository(#[from] crate::repository::RepositoryError),
}
