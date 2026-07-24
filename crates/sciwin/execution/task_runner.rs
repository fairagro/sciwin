use crate::{
    error::RunnerError,
    execution::{
        JobHandle, LogStream, RunId, RunStatus, WorkflowRunner,
        logging::{LogSink, RunLogLayer},
    },
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
use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

#[derive(Debug)]
pub struct TaskRunner<T: TaskBackend> {
    backend: Arc<T>,
    jobs: Arc<Mutex<HashMap<RunId, JobHandle>>>,
    log_sinks: Arc<Mutex<HashMap<RunId, LogSink>>>,
}

impl<T: TaskBackend> TaskRunner<T> {
    pub fn new(backend: Arc<T>) -> Self {
        Self {
            backend,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            log_sinks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn tracing_layer(&self) -> RunLogLayer {
        RunLogLayer {
            sinks: self.log_sinks.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<T: TaskBackend> WorkflowRunner for TaskRunner<T> {
    async fn submit(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        out_dir: Option<&Path>,
    ) -> Result<RunId, RunnerError> {
        let run_id: RunId = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        let (status_tx, _) = watch::channel(RunStatus::Queued);
        self.log_sinks.lock().unwrap().insert(
            run_id.clone(),
            LogSink {
                history: Arc::new(Mutex::new(Vec::new())),
                live: broadcast::channel(1024).0,
            },
        );

        let request = create_execution_request_with_inputs(cwlfile, inputs, out_dir, None)?;

        let backend = self.backend.clone();
        let run_span = tracing::info_span!("workflow_run", run_id = %run_id);
        let cancel_clone = cancel.clone();
        let status_task = status_tx.clone();

        let outputs_slot = Arc::new(Mutex::new(None));
        let outputs_for_task = outputs_slot.clone();

        let task = tokio::spawn(
            async move {
                let _ = status_task.send(RunStatus::Running);
                let result = execute(backend, &request, cancel_clone.clone()).await;

                let final_status = match &result {
                    Ok(r) => {
                        let code = evaluate_exitcodes(&r.exit_status, &request.specification);
                        if matches!(code, EngineStatus::Success(_)) {
                            *outputs_for_task.lock().unwrap() = Some(r.outputs.clone());
                            RunStatus::Finished
                        } else {
                            RunStatus::Failed
                        }
                    }
                    Err(_) if cancel_clone.is_cancelled() => RunStatus::Cancelled,
                    Err(_) => RunStatus::Failed,
                };
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
            },
        );

        Ok(run_id)
    }

    async fn status(&self, id: &RunId) -> Result<RunStatus, RunnerError> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.status.borrow().clone())
    }

    async fn logs(&self, id: &RunId) -> Result<LogStream, RunnerError> {
        let sink = {
            let sinks = self.log_sinks.lock().unwrap();
            sinks.get(id).ok_or(RunnerError::JobNotFound)?.clone()
        };

        let history = sink.history.lock().unwrap().clone();
        let mut rx = sink.live.subscribe();

        let stream = async_stream::stream! {
            for line in history {
                yield Ok(line);
            }
            loop {
                match rx.recv().await {
                    Ok(line) => yield Ok(line),
                    Err(broadcast::error::RecvError::Lagged(_)) => yield Err(RunnerError::LogLagged),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Ok(LogStream::new(stream))
    }

    async fn cancel(&self, id: &RunId) -> Result<(), RunnerError> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        job.cancel.cancel();

        Ok(())
    }

    async fn outputs(
        &self,
        id: &RunId,
        _out_dir: Option<&Path>,
    ) -> Result<Option<HashMap<String, DefaultValue>>, RunnerError> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(id).ok_or(RunnerError::JobNotFound)?;
        Ok(job.outputs.lock().unwrap().clone())
    }

    async fn wait_for_completion(&self, id: &RunId) -> Result<RunStatus, RunnerError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::{
        engine::{ContainerEngine, LocalBackend, load_input_file_from_file},
        storage::{StorageBackend, StoragePath},
    };
    use std::env::{self};
    use tempfile::tempdir;
    use tracing::level_filters::LevelFilter;
    use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

    #[tokio::test]
    async fn test_execution_commonwl() {
        let storage = Arc::new(StorageBackend::new());
        let data_store = StoragePath::from_local(&env::temp_dir());
        let backend = Arc::new(LocalBackend::new(
            ContainerEngine::default(),
            storage,
            data_store,
        ));

        let runner = TaskRunner::<LocalBackend>::new(backend);

        tracing_subscriber::registry()
            .with(runner.tracing_layer().with_filter(LevelFilter::DEBUG))
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    format!(
                        "{}=debug,cwl_engine=info,crankshaft=warn",
                        env!("CARGO_CRATE_NAME")
                    )
                    .into()
                }),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
        let base_path = specification_path.parent().unwrap();
        let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

        let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

        //dumpster for outputs
        let tmpdir = tempdir().unwrap();
        let result = runner
            .run_workflow(&specification_path, inputs, Some(tmpdir.path()))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_commonwl() {
        let storage = Arc::new(StorageBackend::new());
        let data_store = StoragePath::from_local(&env::temp_dir());
        let backend = Arc::new(LocalBackend::new(
            ContainerEngine::default(),
            storage,
            data_store,
        ));

        let runner = TaskRunner::<LocalBackend>::new(backend);

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
        let base_path = specification_path.parent().unwrap();
        let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

        let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

        let run_id = runner
            .submit(&specification_path, inputs, None)
            .await
            .unwrap();

        runner.cancel(&run_id).await.unwrap();
        let status = runner.wait_for_completion(&run_id).await.unwrap();

        assert!(matches!(status, RunStatus::Cancelled));
    }
}
