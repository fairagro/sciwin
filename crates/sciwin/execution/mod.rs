use crate::error::RunnerError;
use commonwl::{engine::InputObject, inputs::DefaultValue};
use futures::Stream;
use miette::IntoDiagnostic;
use reana::{api::response::WorkflowStatus, logs::ReanaLogMessage};
use std::{
    collections::HashMap,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{Level, info};

mod task_runner;
pub use task_runner::TaskRunner;
mod reana_runner;
pub use reana_runner::ReanaRunner;
mod logging;

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
pub struct LogStream(Pin<Box<dyn Stream<Item = Result<LogLine, RunnerError>> + Send>>);
impl LogStream {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<LogLine, RunnerError>> + Send + 'static,
    {
        Self(Box::pin(stream))
    }
}

impl Stream for LogStream {
    type Item = Result<LogLine, RunnerError>;
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
    ) -> Result<RunId, RunnerError>;
    async fn status(&self, id: &RunId) -> Result<RunStatus, RunnerError>;
    async fn logs(&self, id: &RunId) -> Result<LogStream, RunnerError>;
    async fn cancel(&self, id: &RunId) -> Result<(), RunnerError>;
    async fn outputs(
        &self,
        id: &RunId,
        out_dir: Option<&Path>,
    ) -> Result<Option<HashMap<String, DefaultValue>>, RunnerError>;
    async fn wait_for_completion(&self, id: &RunId) -> Result<RunStatus, RunnerError>;
    async fn run_workflow(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        out_dir: Option<&Path>,
    ) -> miette::Result<()> {
        let run_id = self.submit(cwlfile, inputs, out_dir).await?;

        let mut log_stream = self.logs(&run_id).await?;
        let log_task = tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(line) = log_stream.next().await {
                if let Ok(l) = line {
                    info!(
                        "[{}] {}",
                        l.step.as_deref().unwrap_or("workflow"),
                        l.message
                    );
                }
            }
        });

        let status = self.wait_for_completion(&run_id).await?;
        log_task.abort();

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
