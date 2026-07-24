use crate::{
    error::RunnerError,
    execution::{LogCursor, LogStream, RunId, RunStatus, WorkflowRunner},
};
use commonwl::{engine::InputObject, inputs::DefaultValue};
use futures::future::try_join_all;
use reana::{
    api::client::ReanaClient,
    client::CreatedWorkspace,
    logs::{ReanaLogMessage, get_log_outputs},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tracing::info;

pub struct ReanaRunner {
    client: Arc<ReanaClient>,
}

impl ReanaRunner {
    pub fn new(client: ReanaClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    pub fn get_client(&self) -> Arc<ReanaClient> {
        self.client.clone()
    }

    pub async fn wait_for_completion_with_interval(
        &self,
        id: &RunId,
        interval: std::time::Duration,
    ) -> Result<RunStatus, RunnerError> {
        loop {
            let status = self.status(id).await?;
            if status.is_terminal() {
                return Ok(status);
            }
            tokio::time::sleep(interval).await;
        }
    }
}

#[async_trait::async_trait]
impl WorkflowRunner for ReanaRunner {
    async fn submit(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        _out_dir: Option<&Path>,
    ) -> Result<RunId, RunnerError> {
        let name = "workflow"; //todo: set

        let CreatedWorkspace {
            workflow_id,
            specification,
            local_workspace,
        } = reana::client::create2(self.client.clone(), name, cwlfile, &inputs).await?;

        let mut files: HashSet<PathBuf> = specification.inputs.files.into_iter().collect();
        for item in specification.inputs.directories {
            for file in walkdir::WalkDir::new(&item) {
                let file = file.map_err(|e| e.into_io_error().unwrap())?;
                if file.file_type().is_file() {
                    files.insert(file.into_path());
                }
            }
        }

        let futures: Vec<_> = files
            .into_iter()
            .map(|f| {
                let workflow_id = workflow_id.clone();
                let client = self.client.clone();
                let location = local_workspace.join(&f);
                async move {
                    reana::client::upload_file(
                        client,
                        &workflow_id,
                        &location,
                        &f.to_string_lossy(),
                    )
                    .await
                }
            })
            .collect();

        try_join_all(futures).await?;

        reana::client::start(self.client.clone(), &workflow_id).await?;

        Ok(workflow_id)
    }

    async fn status(&self, id: &RunId) -> Result<RunStatus, RunnerError> {
        let status = reana::client::status(self.client.clone(), id).await?;
        info!("[{id}] {:?}", status);
        Ok(status.into())
    }

    async fn logs(&self, id: &RunId) -> Result<LogStream, RunnerError> {
        let client = self.client.clone();
        let id = id.clone();

        let stream = async_stream::stream! {
           let mut cursor = LogCursor { workflow_lines_seen: 0, job_lines_seen: HashMap::new() };

        loop {
            let logs_resp = match reana::client::logs(client.clone(), &id).await {
               Ok(r) => r,
               Err(e) => { yield Err(RunnerError::from(e)); return; }
            };

            let parsed: ReanaLogMessage = match serde_json::from_str(&logs_resp.logs) {
                Ok(p) => p,
                Err(e) => { yield Err(RunnerError::from(e)); return; }
            };

            for line in cursor.diff(&parsed) {
                yield Ok(line);
            }

            let status = reana::client::status(client.clone(), &id).await
                .map(|s| s.into())
                .unwrap_or(RunStatus::Running);

            if status.is_terminal() {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        };

        Ok(LogStream::new(stream))
    }

    async fn cancel(&self, id: &RunId) -> Result<(), RunnerError> {
        reana::client::stop(self.client.clone(), id).await?;
        Ok(())
    }

    async fn outputs(
        &self,
        id: &RunId,
        out_dir: Option<&Path>,
    ) -> Result<Option<HashMap<String, DefaultValue>>, RunnerError> {
        let res = reana::client::specification(self.client.clone(), id).await?;
        let outputs = res.specification.outputs;

        let mut files = vec![];
        for out in outputs.files {
            files.extend(reana::storage::glob(self.client.clone(), id, &out).await?);
        }

        let working_directory = env::current_dir()?;
        let out_dir = out_dir.unwrap_or(&working_directory);
        let futures = files.into_iter().map(|f| async move {
            reana::client::download_file(self.client.clone(), id, &f, out_dir).await
        });
        try_join_all(futures).await?;

        let logs = reana::client::logs(self.client.clone(), id).await?;
        let mut outputs = get_log_outputs(&logs)?;
        if let Some(o) = outputs.as_mut() {
            update_locations(o, out_dir);
        }

        let outputs = outputs.map(serde_json::from_value).transpose()?;

        Ok(outputs)
    }

    async fn wait_for_completion(&self, id: &RunId) -> Result<RunStatus, RunnerError> {
        self.wait_for_completion_with_interval(id, Duration::from_secs(15))
            .await
    }
}

fn update_locations(value: &mut Value, local_outputs: &Path) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(location)) = map.get_mut("location")
                && let Some((_, rel)) = location.rsplit_once("/outputs/")
            {
                *location = local_outputs
                    .join("outputs")
                    .join(rel)
                    .to_string_lossy()
                    .into_owned();
            }

            for v in map.values_mut() {
                update_locations(v, local_outputs);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                update_locations(v, local_outputs);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::engine::load_input_file_from_file;
    use reana::{api::response::WorkflowStatus, auth::ReanaAccessToken};
    use tempfile::tempdir;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    use url::Url;

    /// This test needs a valid REANA Instance running and token defined by .env file
    /// The test is ignored in CI runs and can only start manually
    #[tokio::test]
    #[ignore]
    async fn test_execution_reana() {
        dotenvy::dotenv().unwrap();

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    format!("{}=debug,reana=info,reqwest=info", env!("CARGO_CRATE_NAME")).into()
                }),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();

        let token = Arc::new(ReanaAccessToken::new(env::var("REANA_TOKEN").unwrap()));
        let server_url = Url::parse(&env::var("REANA_URL").unwrap()).unwrap();
        let client = ReanaClient::new(server_url.join("api").unwrap(), token);
        let runner = ReanaRunner::new(client);

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
        let base_path = specification_path.parent().unwrap();
        let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

        let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

        //dumpster for outputs
        let tmpdir = tempdir().unwrap();

        let res = runner
            .run_workflow(&specification_path, inputs, Some(tmpdir.path()))
            .await;
        assert!(res.is_ok());
    }

    /// This test needs a valid REANA Instance running and token defined by .env file
    /// The test is ignored in CI runs and can only start manually
    #[tokio::test]
    #[ignore]
    async fn test_cancel_reana() {
        dotenvy::dotenv().unwrap();

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    format!(
                        "{}=debug,reana=debug,reqwest=info",
                        env!("CARGO_CRATE_NAME")
                    )
                    .into()
                }),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();

        let token = Arc::new(ReanaAccessToken::new(env::var("REANA_TOKEN").unwrap()));
        let server_url = Url::parse(&env::var("REANA_URL").unwrap()).unwrap();
        let client = ReanaClient::new(server_url.join("api").unwrap(), token);
        let runner = ReanaRunner::new(client);

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
        let base_path = specification_path.parent().unwrap();
        let inputs_path = root.join("../../testdata/hello_world/inputs.yml");

        let inputs = load_input_file_from_file(inputs_path, base_path).unwrap();

        let run_id = runner
            .submit(&specification_path, inputs, None)
            .await
            .unwrap();

        let client = runner.get_client();
        //wait for run as reana only accepts cancel if running
        loop {
            let status = reana::client::status(client.clone(), &run_id)
                .await
                .unwrap();
            if status == WorkflowStatus::Running {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        runner.cancel(&run_id).await.unwrap();

        let status = runner.wait_for_completion(&run_id).await.unwrap();

        //reana has no cancel state
        assert!(matches!(status, RunStatus::Stopped));
    }
}
