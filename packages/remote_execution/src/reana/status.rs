use crate::reana::{
    auth::login_reana,
    workflow::{analyze_workflow_logs, get_saved_workflows},
};
use rocrate_ext::{export_rocrate, RocrateArgs};
use reana_ext::{api::{get_workflow_logs, get_workflow_status, get_workflow_specification}, reana::Reana};
use std::{error::Error, path::PathBuf, sync::Arc};
use tokio::time::{sleep, Duration};

pub(super) fn status_file_path() -> PathBuf {
    std::env::temp_dir().join("workflow_status_list.json")
}

pub async fn check_remote_status(workflow_name: &Option<String>) -> Result<(), Box<dyn Error>> {
    let (reana_instance, reana_token) = login_reana()?;
    let reana = Reana::new(reana_instance.clone(), reana_token);

    if let Some(name) = workflow_name {
        evaluate_workflow_status(&reana, name, true).await?;
    } else {
        let workflows = get_saved_workflows(&reana_instance);
        if workflows.is_empty() {
            return Err(format!("No workflows saved for REANA instance '{reana_instance}'").into());
        }
        for name in workflows {
            evaluate_workflow_status(&reana, &name, false).await?;
        }
    }
    Ok(())
}

pub async fn evaluate_workflow_status(reana: &Reana, name: &str, analyze_logs: bool) -> Result<String, Box<dyn Error>> {
    let status_response = get_workflow_status(reana, name).await.map_err(|e| format!("Failed to fetch workflow status: {e}"))?;
    let status = status_response["status"].as_str().unwrap_or("unknown");
    let created = status_response["created"].as_str().unwrap_or("unknown");
    let icon = if status == "finished" {
        "✅"
    } else if status == "failed" {
        "❌"
    } else {
        "⌛"
    };
    eprintln!("{icon} {name} {status} created at {created}");
    //if single workflow failed, get step name and logs
    if status == "failed"
        && analyze_logs
        && let Some(logs_str) = status_response["logs"].as_str()
    {
        analyze_workflow_logs(logs_str);
    }
    Ok(status.to_string())
}

pub async fn watch(
    workflow_name: &str,
    rocrate_args: &Option<RocrateArgs>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (reana_instance, reana_token) = login_reana()?;
    let reana = Arc::new(Reana::new(reana_instance.clone(), reana_token.clone()));

    const POLL_INTERVAL_SECS: u64 = 5;
    const TERMINAL_STATUSES: [&str; 3] = ["finished", "failed", "deleted"];

    loop {
        let status_response =
            get_workflow_status(&reana, workflow_name).await.map_err(|e| {
                format!("Failed to fetch workflow status: {e}")
            })?;
        let workflow_status = status_response["status"].as_str().unwrap_or("unknown");
        if TERMINAL_STATUSES.contains(&workflow_status) {
            match workflow_status {
                "finished" => {
                    eprintln!("✅ Workflow finished successfully.");
                    if let Err(e) =
                        crate::reana::download_remote_results(workflow_name, false, None).await
                    {
                        eprintln!("Error downloading remote results: {e}");
                    }
                    let graph = get_workflow_specification(&reana, workflow_name).await?;
                    let graph_array = graph
                        .as_array()
                        .ok_or("Expected @graph to be array")?;

                    let logs = get_workflow_logs(&reana, workflow_name).await?;
                    let working_dir = std::env::current_dir()?;
                    if let Some(rocrate_args) = rocrate_args {
                        match export_rocrate(
                            rocrate_args.output_dir.as_ref(),
                            Some(&working_dir.to_string_lossy().to_string()),
                            workflow_name,
                            rocrate_args.run_type,
                            Some("remote"),
                            graph_array,
                            Some(&logs),
                        ).await {
                            Ok(_) => {}
                            Err(e) => eprintln!(
                                "Error trying to create a Provenance RO-Crate: {e}"
                            ),
                        }
                    }
                }
                "failed" => {
                    if let Some(logs_str) = status_response["logs"].as_str() {
                        analyze_workflow_logs(logs_str);
                    }
                }
                "deleted" => {
                    eprintln!("⚠️ Workflow was deleted before completion.");
                }
                _ => {}
            }
            break;
        }
        sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }

    Ok(())
}
