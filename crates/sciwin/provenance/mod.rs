use miette::Diagnostic;
use thiserror::Error;

pub mod builder;
pub mod graph;
pub mod inputs;
pub mod reana_runner;
pub mod task_runner;

/// Result alias for this module. See [`crate::Result`] for code spanning several modules.
pub type ProvenanceResult<T> = Result<T, ProvenanceError>;

/// Anything RO-Crate provenance recording/export can fail with.
#[derive(Error, Diagnostic, Debug)]
pub enum ProvenanceError {
    #[error("the workflow specification has no Workflow in its $graph")]
    #[diagnostic(code = "sciwin::provenance::NoWorkflow")]
    NoWorkflow,

    #[error("step `{step}` runs `{run}`, which is not in the packed graph")]
    #[diagnostic(code = "sciwin::provenance::UnresolvedStep")]
    UnresolvedStep { step: String, run: String },

    #[error("workflow `{run}` is {status:?}; only a finished run can be exported")]
    #[diagnostic(code = "sciwin::provenance::NotFinished")]
    NotFinished {
        run: crate::execution::RunId,
        status: crate::execution::RunStatus,
    },

    #[error(transparent)]
    #[diagnostic(transparent)]
    Invalid(#[from] rocrate::validate::InvalidCrate),

    #[error(transparent)]
    #[diagnostic(transparent)]
    CrateIo(#[from] rocrate::io::Error),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Reana(#[from] reana::error::ClientError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Project(#[from] crate::project::ProjectError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Runner(#[from] crate::execution::RunnerError),

    #[error(transparent)]
    #[diagnostic(code = "std::io::Error")]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_json::Error")]
    JSON(#[from] serde_json::Error),
}
