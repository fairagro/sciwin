use commonwl::{files::FileOrDirectory, inputs::DefaultValue};
use miette::Diagnostic;
use thiserror::Error;

pub mod io;
pub mod parser;
pub mod tool;

pub type AuthoringResult<T> = Result<T, AuthoringError>;

#[derive(Error, Diagnostic, Debug)]
pub enum AuthoringError {
    #[error(transparent)]
    #[diagnostic(code = "io::Error")]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_json::Error")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_saphyr::Error")]
    Yaml(#[from] serde_saphyr::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_saphyr::ser::Error")]
    SaphyrSer(#[from] serde_saphyr::ser::Error),

    #[error(transparent)]
    #[diagnostic(code = "git2::Error")]
    Git(#[from] git2::Error),

    #[error(transparent)]
    #[diagnostic(code = "repository::RepositoryError")]
    Repository(#[from] crate::repository::RepositoryError),

    #[error(transparent)]
    #[diagnostic(code = "authoring::Unknown")]
    Unknown(#[from] anyhow::Error),
}

/// `anyhow::Context`-style helper for `AuthoringResult`.
pub(crate) trait AuthoringContext<T> {
    fn with_context<F, M>(self, f: F) -> AuthoringResult<T>
    where
        F: FnOnce() -> M,
        M: std::fmt::Display;
}

impl<T, E> AuthoringContext<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_context<F, M>(self, f: F) -> AuthoringResult<T>
    where
        F: FnOnce() -> M,
        M: std::fmt::Display,
    {
        self.map_err(|e| AuthoringError::Unknown(anyhow::anyhow!("{}: {e}", f())))
    }
}

pub fn default_to_string(default: &DefaultValue) -> String {
    match default {
        DefaultValue::FileOrDirectory(FileOrDirectory::File(f)) => f
            .location
            .clone()
            .unwrap_or_else(|| f.path.clone().unwrap()),
        DefaultValue::FileOrDirectory(FileOrDirectory::Directory(d)) => d
            .location
            .clone()
            .unwrap_or_else(|| d.path.clone().unwrap()),
        DefaultValue::Any(value) => value.as_str().unwrap_or_default().to_string(),
    }
}
