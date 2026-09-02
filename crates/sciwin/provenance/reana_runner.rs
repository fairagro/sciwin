//! The REANA adapter: turns a finished REANA run into a [`CrateInputs`], and packages it into an
//! RO-Crate on disk. [`fold_run_record`] is the pure fold (REANA's status/log responses ->
//! [`RunRecord`]).

use crate::project::config::WorkflowConfig;
use crate::provenance::{
    ProvenanceError, ProvenanceResult, Written,
    builder::build_validated,
    inputs::{CrateInputs, Engine, PayloadFile, RunRecord, StepRun, WorkflowLayout},
    write_crate,
};
use chrono::{DateTime, Utc};
use reana::{
    api::{
        client::ReanaClient,
        response::{WorkflowProgressDates, WorkflowStatus},
    },
    logs::{ReanaLogMessage, engine_version},
};
use rocrate::validate::Validation;
use std::{collections::HashMap, path::Path, sync::Arc};

/// Folds a REANA run's progress dates and logs into a [`RunRecord`].
#[must_use]
pub fn fold_run_record(dates: &WorkflowProgressDates, logs: &ReanaLogMessage) -> RunRecord {
    let engine = engine_version(logs)
        .map(|(name, version)| Engine {
            name,
            version: Some(version),
        })
        .unwrap_or_default();

    // REANA always packs a submitted workflow with "#main" as the root id (see
    // `reana::client::create2`)
    let steps = logs
        .job_logs
        .values()
        .map(|job| {
            (
                format!("#main/{}", job.job_name),
                StepRun {
                    started_at: job.started_at.map(|t| t.and_utc()),
                    ended_at: job.finished_at.map(|t| t.and_utc()),
                    container_image: Some(job.docker_img.clone()),
                },
            )
        })
        .collect();

    RunRecord {
        engine,
        started_at: dates.run_started_at.map(|t| t.and_utc()),
        ended_at: dates.run_finished_at.map(|t| t.and_utc()),
        steps,
    }
}

/// Fetches everything [`build_crate`] needs for `workflow_id` from REANA.
///
/// # Errors
/// The request fails, the run is not [`WorkflowStatus::Finished`], or the response bodies don't
/// parse.
pub async fn fetch(
    client: Arc<ReanaClient>,
    workflow_id: &str,
    metadata: WorkflowConfig,
    date_published: DateTime<Utc>,
) -> ProvenanceResult<CrateInputs> {
    let status = reana::client::status_full(client.clone(), workflow_id).await?;
    if status.status != WorkflowStatus::Finished {
        return Err(ProvenanceError::NotFinished {
            run: workflow_id.to_string(),
            status: status.status.into(),
        });
    }

    let specification = reana::client::specification(client.clone(), workflow_id).await?;
    let logs_response = reana::client::logs(client.clone(), workflow_id).await?;
    let logs: ReanaLogMessage = serde_json::from_str(&logs_response.logs)?;
    let run = fold_run_record(&status.progress.dates, &logs);

    let payload = match reana::client::workspace(client.clone(), workflow_id).await {
        Ok(response) => response
            .items
            .into_iter()
            .map(|item| PayloadFile {
                name: item.name,
                size: (item.size.raw >= 0).then_some(item.size.raw as u64),
                checksum: None,
                source: None,
            })
            .collect(),
        Err(error) => {
            tracing::warn!("[{workflow_id}] could not list workspace: {error}");
            Vec::new()
        }
    };

    Ok(CrateInputs::builder()
        .workflow(specification.specification.workflow.specification)
        .metadata(metadata)
        .run(run)
        .date_published(date_published)
        .payload(payload)
        .build())
}

/// Builds and writes the RO-Crate for `workflow_id` into `directory`.
///
/// A crate that breaks a claimed profile's `Must` rules is still written, not rejected. The
/// [`Validation`] comes back alongside [`Written`] so the caller 
/// decides how to surface it.
///
/// # Errors
/// See [`fetch`] and [`write_crate`]; a crate entity that has no matching REANA workspace file
/// is not fatal -- it lands in [`Written::missing`] instead.
pub async fn export(
    client: Arc<ReanaClient>,
    workflow_id: &str,
    metadata: WorkflowConfig,
    directory: &Path,
    date_published: DateTime<Utc>,
) -> ProvenanceResult<(Written, Validation)> {
    let inputs = fetch(client.clone(), workflow_id, metadata, date_published).await?;
    let (crate_, validation) = build_validated(&inputs)?;

    // `fetch` only ever builds a `Packed` layout -- REANA hands back one packed specification,
    // never the original file tree.
    let WorkflowLayout::Packed { file_name } = &inputs.layout else {
        unreachable!("provenance::reana_runner::fetch always builds a Packed layout")
    };

    let download_dir = tempfile::tempdir()?;
    let mut sources = HashMap::new();
    for part in crate_.local_parts() {
        // Written directly from `inputs.workflow` by `write_crate`, not downloaded.
        if part == file_name {
            continue;
        }
        match reana::client::download_file(client.clone(), workflow_id, part, download_dir.path())
            .await
        {
            Ok(path) => {
                sources.insert(part.to_string(), path);
            }
            Err(error) => tracing::warn!("[{workflow_id}] could not download {part}: {error}"),
        }
    }

    let written = write_crate(
        &crate_,
        directory,
        Some((file_name.as_str(), &inputs.workflow)),
        &sources,
    )?;

    Ok((written, validation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_logs() -> ReanaLogMessage {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/rocrate/reana_logs.json"
        );
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn test_fold_run_record_engine_and_steps() {
        let logs = fixture_logs();
        let dates = WorkflowProgressDates {
            run_started_at: Some(
                chrono::NaiveDate::from_ymd_opt(2026, 7, 23)
                    .unwrap()
                    .and_hms_opt(12, 21, 26)
                    .unwrap(),
            ),
            run_finished_at: Some(
                chrono::NaiveDate::from_ymd_opt(2026, 7, 23)
                    .unwrap()
                    .and_hms_opt(12, 21, 37)
                    .unwrap(),
            ),
            run_stopped_at: None,
        };

        let run = fold_run_record(&dates, &logs);

        assert_eq!(run.engine.name, "reana");
        assert_eq!(
            run.engine.version.as_deref(),
            Some("0.9.4 with cwltool 3.1.20210628163208")
        );
        assert!(run.started_at.is_some());
        assert!(run.ended_at.is_some());

        assert_eq!(run.steps.len(), 2);
        let calculation = run.steps.get("#main/calculation").unwrap();
        assert_eq!(
            calculation.container_image.as_deref(),
            Some("pandas/pandas:pip-all")
        );
        assert!(calculation.started_at.is_some());
        assert!(calculation.ended_at.is_some());

        let plot = run.steps.get("#main/plot").unwrap();
        assert_eq!(
            plot.container_image.as_deref(),
            Some("sciwin/python-datascience")
        );
    }

    #[test]
    fn test_fold_run_record_handles_missing_engine_marker() {
        let logs = ReanaLogMessage {
            workflow_logs: "no engine marker here".to_string(),
            job_logs: HashMap::new(),
            engine_specific: None,
        };
        let dates = WorkflowProgressDates::default();

        let run = fold_run_record(&dates, &logs);

        assert_eq!(run.engine.name, "");
        assert_eq!(run.engine.version, None);
        assert!(run.steps.is_empty());
    }

    /// This test needs a valid REANA instance running and a token defined by .env file. 
    /// Ignored in CI, run manually.
    #[tokio::test]
    #[ignore]
    async fn test_export_reana() {
        use crate::execution::WorkflowRunner;

        dotenvy::dotenv().unwrap();

        let token = Arc::new(reana::auth::ReanaAccessToken::new(
            std::env::var("REANA_ACCESS_TOKEN").unwrap(),
        ));
        let server_url = url::Url::parse(&std::env::var("REANA_SERVER_URL").unwrap()).unwrap();
        let client = ReanaClient::new(server_url.join("api").unwrap(), token);
        let runner = crate::execution::ReanaRunner::new(client);

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let specification_path = root.join("../../testdata/hello_world/workflows/main/main.cwl");
        let base_path = specification_path.parent().unwrap();
        let inputs_path = root.join("../../testdata/hello_world/inputs.yml");
        let inputs =
            commonwl::engine::load_input_file_from_file(inputs_path, base_path).unwrap();

        let run_id = runner
            .submit(&specification_path, inputs, None)
            .await
            .unwrap();
        let status = runner.wait_for_completion(&run_id).await.unwrap();
        assert_eq!(status, crate::execution::RunStatus::Finished);

        let metadata = WorkflowConfig {
            name: "hello_s4n".to_string(),
            license: Some("https://spdx.org/licenses/CC-BY-4.0.html".to_string()),
            ..Default::default()
        };

        let dir = tempfile::tempdir().unwrap();
        let (written, validation) = export(
            runner.get_client(),
            &run_id,
            metadata,
            dir.path(),
            Utc::now(),
        )
        .await
        .unwrap();

        assert!(written.metadata.exists());
        assert!(dir.path().join("workflow.json").exists());
        assert!(
            validation.is_conformant(),
            "{:#?}",
            validation.errors().collect::<Vec<_>>()
        );
    }
}
