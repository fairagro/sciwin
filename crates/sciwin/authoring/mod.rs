use commonwl::{files::FileOrDirectory, inputs::DefaultValue};
use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

pub mod parser;
pub mod paths;
pub mod tool;

pub type AuthoringResult<T> = Result<T, AuthoringError>;

#[derive(Error, Diagnostic, Debug)]
pub enum AuthoringError {
    #[error("uncommitted changes detected: {}", files.join(", "))]
    #[diagnostic(
        code = "authoring::DirtyWorkingTree",
        help = "commit or stash the listed files, or skip the trial run"
    )]
    DirtyWorkingTree { files: Vec<String> },

    #[error("could not find a git repository at {}", path.display())]
    #[diagnostic(
        code = "authoring::NoRepository",
        help = "run this inside a git repository, or initialize one first"
    )]
    NoRepository {
        path: PathBuf,
        #[source]
        source: git2::Error,
    },

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
