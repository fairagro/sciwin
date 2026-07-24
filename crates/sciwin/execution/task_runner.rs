use crate::{
    error::RunnerError,
    execution::{JobHandle, LogStream, RunId, RunStatus, WorkflowRunner},
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
use tracing::Instrument;

#[derive(Debug)]
pub struct TaskRunner<T: TaskBackend> {
    jobs: Arc<Mutex<HashMap<RunId, JobHandle>>>,
    backend: Arc<T>,
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

    async fn logs(&self, _id: &RunId) -> Result<LogStream, RunnerError> {
        todo!()
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

    #[tokio::test]
    async fn test_execution_commonwl() {
        let storage = Arc::new(StorageBackend::new());
        let data_store = StoragePath::from_local(&env::temp_dir());
        let backend = Arc::new(LocalBackend::new(
            ContainerEngine::default(),
            storage,
            data_store,
        ));

        let runner = TaskRunner::<LocalBackend> {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            backend,
        };

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
        let base_path = specification_path.parent().unwrap();
        let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

        let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

        //dumpster for outputs
        let tmpdir = tempdir().unwrap();

        let run_id = runner
            .submit(&specification_path, inputs, Some(tmpdir.path()))
            .await
            .unwrap();
        let status = runner.wait_for_completion(&run_id).await.unwrap();

        assert!(matches!(status, RunStatus::Finished));
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

        let runner = TaskRunner::<LocalBackend> {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            backend,
        };

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
