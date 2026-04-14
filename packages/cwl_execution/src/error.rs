use cwl_core::{CWLType, DefaultValue};
use reqwest::StatusCode;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub trait ExitCode {
    fn exit_code(&self) -> i32;
}

#[derive(Error, Debug)]
#[error("{message} (exit code: {exit_code})")]
pub struct CommandError {
    pub exit_code: i32,
    pub message: String,
}

impl ExitCode for CommandError {
    fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl CommandError {
    pub fn new(message: String, exit_code: i32) -> Self {
        Self { exit_code, message }
    }
}

#[derive(Error, Debug)]
#[error("YAML parsing of {file} failed: {source}")]
pub struct YAMLDeserializationError {
    file: PathBuf,
    #[source]
    source: serde_yaml::Error,
}

impl YAMLDeserializationError {
    pub fn new(path: &Path, source: serde_yaml::Error) -> Self {
        Self {
            file: path.to_path_buf(),
            source,
        }
    }
}

#[derive(Error, Debug)]
#[error("JSON parsing of {file} failed: {source}")]
pub struct JSONDeserializationError {
    file: PathBuf,
    #[source]
    source: serde_json::Error,
}

impl JSONDeserializationError {
    pub fn new(path: &Path, source: serde_json::Error) -> Self {
        Self {
            file: path.to_path_buf(),
            source,
        }
    }
}

#[derive(Error, Debug)]
#[error("Copying {from} to {to} failed: {source}")]
pub struct CopyDataError {
    from: PathBuf,
    to: PathBuf,
    #[source]
    source: std::io::Error,
}

impl CopyDataError {
    pub fn new(from: &Path, to: &Path, source: std::io::Error) -> Self {
        Self {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        }
    }
}

#[derive(Error, Debug)]
#[error("Accessing file {file} failed: {source}")]
pub struct FileSystemError {
    file: PathBuf,
    #[source]
    source: std::io::Error,
}

impl FileSystemError {
    pub fn new(file: &Path, source: std::io::Error) -> Self {
        Self {
            file: file.to_path_buf(),
            source,
        }
    }
}

#[derive(Error, Debug)]
#[error(transparent)]
pub enum ExecutionError {
    //original errors
    CommandError(#[from] CommandError),
    CopyDataError(#[from] CopyDataError),
    FileSystemError(#[from] FileSystemError),
    YAMLDeserializationError(#[from] YAMLDeserializationError),
    JSONDeserializationError(#[from] JSONDeserializationError),

    #[error("{0}")]
    CWLVersionMismatch(String),
    #[error("CWLType '{0:?}' is not matching given input type. Given input was: \n{1:#?}")]
    CWLTypeValueMismatch(CWLType, Box<DefaultValue>),
    #[error("No Input for id `{0}` found!")]
    CWLMissingInput(String),
    #[error("Could not download file from {0}: Code {1}")]
    DownloadFileError(PathBuf, StatusCode),

    //passthrough
    #[error("YAML parsing failed: {0}")]
    YAMLError(#[from] serde_yaml::Error),
    #[error("JSON parsing failed: {0}")]
    JSONError(#[from] serde_json::Error),
    IOError(#[from] std::io::Error),
    GlobPatternError(#[from] glob::PatternError),
    GlobError(#[from] glob::GlobError),
    RustScriptError(#[from] rustyscript::Error),
    TryFromIntError(#[from] std::num::TryFromIntError),
    ParseIntError(#[from] std::num::ParseIntError),
    RequestError(#[from] reqwest::Error),

    // catch-all
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl From<tokio::task::JoinError> for ExecutionError {
    fn from(e: tokio::task::JoinError) -> Self {
        ExecutionError::Any(anyhow::Error::new(e))
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ExecutionError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        ExecutionError::Any(anyhow::anyhow!(e.to_string()))
    }
}

impl From<Box<dyn std::error::Error>> for ExecutionError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        ExecutionError::Any(anyhow::Error::msg(e.to_string()))
    }
}