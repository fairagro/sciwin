use miette::Diagnostic;
use std::io;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum RunnerError {
    #[diagnostic(transparent)]
    #[error(transparent)]
    REANA(#[from] reana::error::ClientError),

    #[diagnostic(code = "sciwin::error::RunnerError::JobNotFound")]
    #[error("Could not find requested job")]
    JobNotFound,

    #[diagnostic(code = "sciwin::error::RunnerError::JobPanicked")]
    #[error("A worker got into panic")]
    JobPanicked,

    #[diagnostic(code = "std::io::Error")]
    #[error(transparent)]
    IO(#[from] io::Error),

    //add Runner Error in commonwl: https://github.com/fairagro/commonwl/issues/15
    #[diagnostic(code = "anyhow::Error")]
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
}
