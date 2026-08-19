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

use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

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

    #[error("Input {id} not found in {path}")]
    #[diagnostic(code = "authoring::InvalidWorkflowInput")]
    InvalidWorkflowInput { id: String, path: String },

    #[error("Input {id} not found in {path}")]
    #[diagnostic(code = "authoring::InvalidWorkflowOutput")]
    InvalidWorkflowOutput { id: String, path: String },

    #[error("Step {id} not found in workflow")]
    #[diagnostic(code = "authoring::InvalidWorkflowStep")]
    InvalidWorkflowStep { id: String },

    #[error(transparent)]
    #[diagnostic(code = "commonwl::Error")]
    CWL(#[from] commonwl::Error),

    #[diagnostic(transparent)]
    #[error(transparent)]
    CWLEngine(#[from] commonwl::engine::RunnerError),

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
    #[diagnostic(code = "reqwest::Error")]
    Net(#[from] reqwest::Error),

    #[error(transparent)]
    #[diagnostic(code = "url::ParseError")]
    UrlParse(#[from] url::ParseError),

    #[error(transparent)]
    #[diagnostic(code = "uv_requirements_txt::RequirementsTxtFileError")]
    RequirementsTxt(#[from] uv_requirements_txt::RequirementsTxtFileError),

    #[error(transparent)]
    #[diagnostic(code = "uv_pypi_types::MetadataError")]
    PyProjectToml(#[from] uv_pypi_types::MetadataError),

    #[error("could not parse `{spec}` as a version requirement")]
    #[diagnostic(code = "authoring::InvalidRequirement")]
    InvalidRequirement { spec: String },

    #[error(transparent)]
    #[diagnostic(code = "authoring::Unknown")]
    Unknown(#[from] anyhow::Error),
}
