use crate::execution::{
    LogCursor, LogStream, RunId, RunStatus, RunnerError, RunnerResult, WorkflowRunner, tail_lines,
};
use commonwl::{engine::InputObject, inputs::DefaultValue};
use futures::future::try_join_all;
use reana::{
    api::{client::ReanaClient, response::WorkflowStatus},
    client::CreatedWorkspace,
    logs::{JobLog, ReanaLogMessage, get_log_message, get_log_outputs},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tracing::{error, info, warn};

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

    /// Fetches `id`'s logs and reports which step(s) look like they failed -- see
    /// [`find_failures`] for what "look like" means. Meant to be called once a run has
    /// reached [`RunStatus::Failed`]; on a run that's still going or finished cleanly this
    /// just returns an empty list.
    pub async fn find_failures(&self, id: &RunId) -> RunnerResult<Vec<JobFailure>> {
        let logs_resp = reana::client::logs(self.client.clone(), id).await?;
        let parsed: ReanaLogMessage = serde_json::from_str(&logs_resp.logs)?;
        Ok(find_failures(&parsed))
    }
}

/// A workflow step whose logs suggest it failed.
#[derive(Debug, Clone)]
pub struct JobFailure {
    pub step: String,
    pub logs: String,
}

const FAILURE_KEYWORDS: [&str; 3] = ["Error", "Exception", "Traceback"];

/// Picks out the step(s) responsible for a failed run, from its full job logs.
pub fn find_failures(logs: &ReanaLogMessage) -> Vec<JobFailure> {
    let explicit: Vec<JobFailure> = logs
        .job_logs
        .values()
        .filter(|job| job.status == WorkflowStatus::Failed)
        .map(to_failure)
        .collect();

    if !explicit.is_empty() {
        for failure in &explicit {
            error!("step {} failed:\n{}", failure.step, failure.logs);
        }
        return explicit;
    }

    let suspects: Vec<JobFailure> = logs
        .job_logs
        .values()
        .filter(|job| looks_like_failure(job))
        .map(to_failure)
        .collect();

    for failure in &suspects {
        warn!(
            "step {} was reported as finished, but its logs look like a failure:\n{}",
            failure.step, failure.logs
        );
    }
    suspects
}

fn to_failure(job: &JobLog) -> JobFailure {
    JobFailure {
        step: job.job_name.clone(),
        logs: job.logs.clone(),
    }
}

fn looks_like_failure(job: &JobLog) -> bool {
    FAILURE_KEYWORDS.iter().any(|k| job.logs.contains(k))
        || job.logs.to_lowercase().contains("failed")
}

#[async_trait::async_trait]
impl WorkflowRunner for ReanaRunner {
    async fn submit(
        &self,
        cwlfile: &Path,
        inputs: InputObject,
        _out_dir: Option<&Path>,
    ) -> RunnerResult<RunId> {
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

    async fn status(&self, id: &RunId) -> RunnerResult<RunStatus> {
        let status = reana::client::status(self.client.clone(), id).await?;
        info!("[{id}] {:?}", status);
        Ok(status.into())
    }

    async fn logs(&self, id: &RunId) -> RunnerResult<LogStream> {
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

    async fn cancel(&self, id: &RunId) -> RunnerResult<()> {
        reana::client::stop(self.client.clone(), id).await?;
        Ok(())
    }

    async fn outputs(
        &self,
        id: &RunId,
        out_dir: Option<&Path>,
    ) -> RunnerResult<Option<HashMap<String, DefaultValue>>> {
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
        let message = get_log_message(&logs)?;
        let mut outputs = get_log_outputs(&message)?;
        if let Some(o) = outputs.as_mut() {
            update_locations(o, out_dir);
        }

        let outputs = outputs.map(serde_json::from_value).transpose()?;

        Ok(outputs)
    }

    async fn wait_for_completion(&self, id: &RunId) -> RunnerResult<RunStatus> {
        self.wait_for_completion_with_interval(id, Duration::from_secs(15))
            .await
    }

    async fn failure_detail(&self, id: &RunId) -> RunnerResult<Option<String>> {
        let failures = self.find_failures(id).await?;
        if failures.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            failures
                .iter()
                .map(|f| format!("{}: {}", f.step, tail_lines(&f.logs, 20)))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
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

        let token = Arc::new(ReanaAccessToken::new(env::var("REANA_ACCESS_TOKEN").unwrap()));
        let server_url = Url::parse(&env::var("REANA_SERVER_URL").unwrap()).unwrap();
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

        let token = Arc::new(ReanaAccessToken::new(env::var("REANA_ACCESS_TOKEN").unwrap()));
        let server_url = Url::parse(&env::var("REANA_SERVER_URL").unwrap()).unwrap();
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

    /// This test needs a valid REANA Instance running and token defined by .env file
    /// The test is ignored in CI runs and can only start manually
    #[tokio::test]
    #[ignore]
    async fn test_find_failures_reana() {
        dotenvy::dotenv().ok();

        let token = Arc::new(ReanaAccessToken::new(env::var("REANA_ACCESS_TOKEN").unwrap()));
        let server_url = Url::parse(&env::var("REANA_SERVER_URL").unwrap()).unwrap();
        let client = ReanaClient::new(server_url.join("api").unwrap(), token);
        let runner = ReanaRunner::new(client);

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/failing_tool.cwl");

        let run_id = runner
            .submit(&specification_path, InputObject::default(), None)
            .await
            .unwrap();

        let status = runner.wait_for_completion(&run_id).await.unwrap();
        assert!(
            matches!(status, RunStatus::Failed),
            "expected Failed, got {status:?}"
        );

        let failures = runner.find_failures(&run_id).await.unwrap();
        assert!(
            !failures.is_empty(),
            "expected at least one reported failure"
        );
        assert!(
            failures[0]
                .logs
                .contains("intentional failure for find_failures test")
        );
    }

    fn job_log(name: &str, status: WorkflowStatus, logs: &str) -> JobLog {
        JobLog {
            workflow_uuid: "wf".to_string(),
            job_name: name.to_string(),
            compute_backend: "kubernetes".to_string(),
            backend_job_id: "job-1".to_string(),
            docker_img: "python".to_string(),
            cmd: "python script.py".to_string(),
            status,
            logs: logs.to_string(),
            finished_at: None,
            started_at: None,
        }
    }

    fn log_message(jobs: Vec<JobLog>) -> ReanaLogMessage {
        ReanaLogMessage {
            workflow_logs: String::new(),
            job_logs: jobs.into_iter().map(|j| (j.job_name.clone(), j)).collect(),
            engine_specific: None,
        }
    }

    #[test]
    fn find_failures_prefers_jobs_explicitly_marked_failed() {
        let logs = log_message(vec![
            job_log("ok_step", WorkflowStatus::Finished, "all good"),
            job_log("bad_step", WorkflowStatus::Failed, "boom"),
        ]);

        let failures = find_failures(&logs);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].step, "bad_step");
    }

    #[test]
    fn find_failures_falls_back_to_sniffing_when_nothing_is_marked_failed() {
        let logs = log_message(vec![
            job_log("ok_step", WorkflowStatus::Finished, "all good"),
            job_log(
                "sneaky_step",
                WorkflowStatus::Finished,
                "Traceback (most recent call last):\n  raise ValueError",
            ),
        ]);

        let failures = find_failures(&logs);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].step, "sneaky_step");
    }

    #[test]
    fn find_failures_returns_empty_for_a_clean_run() {
        let logs = log_message(vec![job_log(
            "ok_step",
            WorkflowStatus::Finished,
            "all good",
        )]);
        assert!(find_failures(&logs).is_empty());
    }
}
