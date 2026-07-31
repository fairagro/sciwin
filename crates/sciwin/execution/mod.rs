//! Running CWL documents, locally or on a remote REANA instance.
//!
//! [`WorkflowRunner`] is the one interface both backends implement -- submit, poll status,
//! stream logs, cancel, collect outputs -- so a frontend writes the same code either way:
//!
//! - [`TaskRunner`] executes through [`commonwl::engine`] on this machine
//! - [`ReanaRunner`] submits to a REANA cluster through the `reana` client
//!
//! Progress is reported by streaming, not printing: [`LogStream`] yields [`LogLine`]s as they
//! arrive, and an internal cursor tracks how far a consumer has read so polling only ever
//! yields what is new.

use commonwl::{engine::InputObject, inputs::DefaultValue};
use futures::Stream;
use miette::Diagnostic;
use miette::IntoDiagnostic;
use reana::{api::response::WorkflowStatus, logs::ReanaLogMessage};
use std::io;
use std::{
    collections::HashMap,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use thiserror::Error;
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{Level, info};

mod task_runner;
pub use task_runner::TaskRunner;
mod reana_runner;
pub use reana_runner::{JobFailure, ReanaRunner, find_failures};
pub mod reana_compat;

pub type RunnerResult<T> = Result<T, RunnerError>;

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

    #[diagnostic(code = "serde_json::Error")]
    #[error(transparent)]
    JSON(#[from] serde_json::Error),

    #[diagnostic(code = "sciwin::error::RunnerError::NotSupported")]
    #[error("Unsupported")]
    NotSupported(&'static str),

    #[diagnostic(
        code = "sciwin::error::RunnerError::DockerUnavailable",
        help = "start Docker, or skip REANA compatibility adjustments"
    )]
    #[error("Docker is not running or accessible")]
    DockerUnavailable,

    #[diagnostic(code = "sciwin::error::RunnerError::DockerCommandFailed")]
    #[error("docker {command} failed: {stderr}")]
    DockerCommandFailed { command: String, stderr: String },

    //add Runner Error in commonwl: https://github.com/fairagro/commonwl/issues/15
    #[diagnostic(code = "anyhow::Error")]
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
}

pub type RunId = String;

#[derive(Debug, Default, PartialEq, Clone)]
pub enum RunStatus {
    Created,
    Running,
    Finished,
    Failed,
    Stopped,
    Cancelled,
    #[default]
    Queued,
}

impl RunStatus {
    fn is_terminal(&self) -> bool {
        matches!(
            &self,
            RunStatus::Finished | RunStatus::Failed | RunStatus::Cancelled | RunStatus::Stopped
        )
    }
}

impl From<WorkflowStatus> for RunStatus {
    fn from(value: WorkflowStatus) -> Self {
        match value {
            WorkflowStatus::Created => RunStatus::Created,
            WorkflowStatus::Running => RunStatus::Running,
            WorkflowStatus::Finished => RunStatus::Finished,
            WorkflowStatus::Failed => RunStatus::Failed,
            WorkflowStatus::Stopped => RunStatus::Stopped,
            WorkflowStatus::Queued => RunStatus::Queued,
            WorkflowStatus::Pending => RunStatus::Queued,
        }
    }
}

#[derive(Debug)]
struct JobHandle {
    cancel: CancellationToken,
    status: watch::Sender<RunStatus>,
    #[allow(dead_code)]
    task: JoinHandle<()>,
    outputs: Arc<Mutex<Option<HashMap<String, DefaultValue>>>>,
}
pub struct LogStream(Pin<Box<dyn Stream<Item = RunnerResult<LogLine>> + Send>>);
impl LogStream {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = RunnerResult<LogLine>> + Send + 'static,
    {
        Self(Box::pin(stream))
    }
}

impl Stream for LogStream {
    type Item = RunnerResult<LogLine>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.as_mut().poll_next(cx)
    }
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub timestamp: Option<chrono::NaiveDateTime>,
    pub level: Level,
    pub step: Option<String>,
    pub message: String,
}

struct LogCursor {
    workflow_lines_seen: usize,
    job_lines_seen: HashMap<String, usize>,
}

impl LogCursor {
    fn diff(&mut self, msg: &ReanaLogMessage) -> Vec<LogLine> {
        let mut new_lines = Vec::new();

        let wf_lines: Vec<&str> = msg.workflow_logs.lines().collect();
        if wf_lines.len() > self.workflow_lines_seen {
            new_lines.extend(
                wf_lines[self.workflow_lines_seen..]
                    .iter()
                    .map(|l| LogLine {
                        timestamp: None,
                        level: Level::INFO,
                        step: None,
                        message: l.to_string(),
                    }),
            );
            self.workflow_lines_seen = wf_lines.len();
        }

        for job in msg.job_logs.values() {
            let seen = self.job_lines_seen.entry(job.job_name.clone()).or_insert(0);
            let job_lines: Vec<&str> = job.logs.lines().collect();
            if job_lines.len() > *seen {
                new_lines.extend(job_lines[*seen..].iter().map(|l| LogLine {
                    timestamp: job.finished_at,
                    level: Level::INFO,
                    step: Some(job.job_name.clone()),
                    message: l.to_string(),
                }));
                *seen = job_lines.len();
            }
        }
        new_lines
    }
}

#[async_trait::async_trait]
pub trait WorkflowRunner {
    async fn submit(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        out_dir: Option<&Path>,
    ) -> RunnerResult<RunId>;
    async fn status(&self, id: &RunId) -> RunnerResult<RunStatus>;
    async fn logs(&self, id: &RunId) -> RunnerResult<LogStream>;
    async fn cancel(&self, id: &RunId) -> RunnerResult<()>;
    async fn outputs(
        &self,
        id: &RunId,
        out_dir: Option<&Path>,
    ) -> RunnerResult<Option<HashMap<String, DefaultValue>>>;
    async fn wait_for_completion(&self, id: &RunId) -> RunnerResult<RunStatus>;
    async fn run_workflow(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        out_dir: Option<&Path>,
    ) -> miette::Result<()> {
        let run_id = self.submit(cwlfile, inputs, out_dir).await?;

        let log_task = match self.logs(&run_id).await {
            Ok(mut stream) => Some(tokio::spawn(async move {
                use futures::StreamExt;
                while let Some(line) = stream.next().await {
                    if let Ok(l) = line {
                        info!(
                            "[{}] {}",
                            l.step.as_deref().unwrap_or("workflow"),
                            l.message
                        );
                    }
                }
            })),
            Err(RunnerError::NotSupported(_)) => None, // backend has no separate log stream — fine
            Err(e) => return Err(e.into()),
        };

        let status = self.wait_for_completion(&run_id).await?;
        if let Some(task) = log_task {
            task.abort();
        }

        #[allow(clippy::disallowed_macros)]
        if matches!(status, RunStatus::Finished)
            && let Some(outputs) = self.outputs(&run_id, out_dir).await?
        {
            println!(
                "{}",
                serde_json::to_string_pretty(&outputs).into_diagnostic()?
            );
            Ok(())
        } else {
            miette::bail!("workflow ended with status {status:?}")
        }
    }
}
