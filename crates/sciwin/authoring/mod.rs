//! Building CWL documents.
//!
//! The headline job is turning a shell command someone just ran into a reusable
//! `CommandLineTool`. [`tool::create_tool`] drives it end to end:
//!
//! ```text
//!   ["python3", "script.py", "--out", "results.csv"]
//!            |
//!            |  parser::parse_command_line
//!            v
//!   CommandLineTool  ..... base command, inputs, outputs, redirections, pipes,
//!            |              and the files it needs staged into its working dir
//!            |  tool::probe          (skipped when `no_run` is set)
//!            v
//!   + discovered outputs  ..... run it once, diff the working tree, and attribute
//!            |                   whatever appeared to the tool
//!            |  tool::requirements
//!            v
//!   + container / network / env / mounts
//!            |
//!            |  parser::post_process_cwl
//!            v
//!   + arrays merged, CWL variables substituted, id collisions resolved
//!            |
//!            |  tool::save
//!            v
//!   formatted CWL YAML, paths rebased on where the file lands
//! ```
//!
//! # Modules
//!
//! - [`parser`] -- command line to `CommandLineTool`, and the post-processing pass over it
//! - [`paths`] -- naming tools and deciding where they belong. Pure path arithmetic
//! - [`tool`] -- the orchestration above, plus the options and result types
//!
//! # A note on inference
//!
//! Parsing a command line is guesswork: whether a token is a file, whether a value is a
//! number, what the tool writes. [`parser::guess_type`] consults the real filesystem, so the
//! same command can parse differently from a different directory. That's deliberate -- the
//! workflow is "convert a command I just ran" -- but it means results are context-dependent.

use commonwl::{files::FileOrDirectory, inputs::DefaultValue};
use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

pub mod parser;
pub mod paths;
pub mod tool;
pub mod workflow;

/// Result alias for this module. See [`crate::Result`] for code spanning several modules.
pub type AuthoringResult<T> = Result<T, AuthoringError>;

/// Anything authoring can fail with.
///
/// The first two variants are states a frontend can act on -- offer to commit, offer to
/// `git init` -- so they carry structured data instead of a formatted message.
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

/// Renders a default value as the string a CWL document would carry: a location for files
/// and directories, the scalar itself otherwise.
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
