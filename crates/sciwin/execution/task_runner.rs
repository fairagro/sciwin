use crate::execution::{
    ExecutionTiming, FailureDetail, JobHandle, LogStream, RunId, RunSpecification, RunStatus,
    RunnerError, RunnerResult, WorkflowRunner, tail_lines,
};
use commonwl::{
    engine::{
        EngineStatus, InputObject, TaskBackend, create_execution_request_with_inputs,
        evaluate_exitcodes, execute,
    },
    inputs::DefaultValue,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error};

#[derive(Debug)]
pub struct TaskRunner<T: TaskBackend> {
    backend: Arc<T>,
    jobs: Arc<Mutex<HashMap<RunId, JobHandle>>>,
}

impl<T: TaskBackend> TaskRunner<T> {
    pub fn new(backend: Arc<T>) -> Self {
        Self {
            backend,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The timing `id`'s run produced, once `execute` has returned a result -- `None` while the
    /// run is still going, or if it errored before producing one at all (nothing to record in
    /// that case; see `execution::RunnerError` for why it failed instead).
    ///
    /// # Errors
    /// `id` is not a run this `TaskRunner` knows about.
    pub fn execution_timing(&self, id: &RunId) -> RunnerResult<Option<ExecutionTiming>> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.timing.lock().unwrap().clone())
    }

    /// Why `id`'s run ended in [`RunStatus::Failed`] -- `None` if it's still going, ended some
    /// other way, or errored before there was anything more specific to say than that.
    ///
    /// # Errors
    /// `id` is not a run this `TaskRunner` knows about.
    pub fn failure(&self, id: &RunId) -> RunnerResult<Option<FailureDetail>> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.failure.lock().unwrap().clone())
    }

    /// The document and path `id` was [`submit`](WorkflowRunner::submit)ted with -- available
    /// from submission onward, not just after the run finishes.
    ///
    /// # Errors
    /// `id` is not a run this `TaskRunner` knows about.
    pub fn specification(&self, id: &RunId) -> RunnerResult<RunSpecification> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.specification.clone())
    }
}

#[async_trait::async_trait]
impl<T: TaskBackend> WorkflowRunner for TaskRunner<T> {
    async fn submit(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        out_dir: Option<&Path>,
    ) -> RunnerResult<RunId> {
        let run_id: RunId = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        let (status_tx, _) = watch::channel(RunStatus::Queued);

        let request = create_execution_request_with_inputs(cwlfile, inputs, out_dir, None)?;
        let specification = RunSpecification {
            document: request.specification.clone(),
            path: dunce::canonicalize(cwlfile)?,
        };

        let backend = self.backend.clone();
        let run_span = tracing::info_span!("workflow_run", run_id = %run_id);
        let cancel_clone = cancel.clone();
        let status_task = status_tx.clone();

        let outputs_slot = Arc::new(Mutex::new(None));
        let outputs_for_task = outputs_slot.clone();
        let timing_slot = Arc::new(Mutex::new(None));
        let timing_for_task = timing_slot.clone();
        let failure_slot = Arc::new(Mutex::new(None));
        let failure_for_task = failure_slot.clone();

        let task = tokio::spawn(
            async move {
                let _ = status_task.send(RunStatus::Running);
                let result = execute(backend, &request, cancel_clone.clone()).await;

                let mut failure_detail: Option<FailureDetail> = None;
                let final_status = match &result {
                    Ok(r) => {
                        *timing_for_task.lock().unwrap() = Some(ExecutionTiming {
                            started_at: r.started_at,
                            finished_at: r.finished_at,
                            step_timings: r.step_timings.clone().unwrap_or_default(),
                        });
                        let code = evaluate_exitcodes(&r.exit_status, &request.specification);
                        if let EngineStatus::Success(_) = code {
                            *outputs_for_task.lock().unwrap() = Some(r.outputs.clone());
                            RunStatus::Finished
                        } else {
                            let exit_code = match code {
                                EngineStatus::Failure(c) | EngineStatus::Undefined(c) => Some(c),
                                EngineStatus::Success(_) => unreachable!(),
                            };
                            let stderr = tail_lines(&r.stderr, 20);
                            failure_detail = Some(FailureDetail {
                                exit_code,
                                message: if stderr.is_empty() {
                                    "command produced no stderr output".to_string()
                                } else {
                                    stderr
                                },
                            });
                            RunStatus::Failed
                        }
                    }
                    Err(_) if cancel_clone.is_cancelled() => RunStatus::Cancelled,
                    Err(e) => {
                        failure_detail = Some(FailureDetail {
                            exit_code: None,
                            message: tail_lines(&format!("{e:#}"), 20),
                        });
                        RunStatus::Failed
                    }
                };
                if let Some(detail) = &failure_detail {
                    error!("run failed: {}", detail.message);
                }
                *failure_for_task.lock().unwrap() = failure_detail;
                let _ = status_task.send(final_status);
            }
            .instrument(run_span),
        );

        self.jobs.lock().unwrap().insert(
            run_id.clone(),
            JobHandle {
                cancel,
                status: status_tx,
                task,
                outputs: outputs_slot,
                timing: timing_slot,
                failure: failure_slot,
                specification,
            },
        );

        Ok(run_id)
    }

    async fn status(&self, id: &RunId) -> RunnerResult<RunStatus> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.status.borrow().clone())
    }

    async fn logs(&self, _id: &RunId) -> RunnerResult<LogStream> {
        Err(RunnerError::NotSupported(
            "local runner logs are only available live via run_workflow's console output",
        ))
    }

    async fn cancel(&self, id: &RunId) -> RunnerResult<()> {
        let mut status_rx = {
            let jobs = self.jobs.lock().unwrap();
            let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
            job.cancel.cancel();
            job.status.subscribe()
        };

        let cooperative = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            status_rx.wait_for(RunStatus::is_terminal),
        )
        .await;

        //hard exit as fallback
        if cooperative.is_err() {
            let jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get(id) {
                job.task.abort();
                let _ = job.status.send(RunStatus::Cancelled);
            }
        }
        Ok(())
    }

    async fn outputs(
        &self,
        id: &RunId,
        _out_dir: Option<&Path>,
    ) -> RunnerResult<Option<HashMap<String, DefaultValue>>> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.outputs.lock().unwrap().clone())
    }

    async fn failure_detail(&self, id: &RunId) -> RunnerResult<Option<String>> {
        Ok(self.failure(id)?.map(|f| match f.exit_code {
            Some(code) => format!("exit code {code}: {}", f.message),
            None => f.message,
        }))
    }

    async fn wait_for_completion(&self, id: &RunId) -> RunnerResult<RunStatus> {
        let mut rx = {
            let jobs = self.jobs.lock().unwrap();
            let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
            job.status.subscribe()
        };

        loop {
            let status = rx.borrow().clone();
            if status.is_terminal() {
                return Ok(status.clone());
            }
            rx.changed().await.map_err(|_| RunnerError::JobPanicked)?;
        }
    }
}

// Integration-style coverage of this module (running a real workflow end to end through the
// local engine) lives in `crates/sciwin/tests/execute_integration_test.rs`.
