use crate::reana::{auth::login_reana, workflow::analyze_workflow_logs};
use reana_ext::{
    api::{download_files, get_workflow_specification, get_workflow_status, get_workflow_workspace},
    reana::Reana,
};
use std::error::Error;

pub fn download_remote_results(workflow_name: &str, all: bool, output_dir: Option<&String>) -> Result<(), Box<dyn Error>> {
    let (reana_instance, reana_token) = login_reana()?;
    let reana = Reana::new(reana_instance, reana_token);

    let status_response = get_workflow_status(&reana, workflow_name).map_err(|e| format!("Failed to fetch workflow status: {e}"))?;
    let workflow_status = status_response["status"].as_str().unwrap_or("unknown");
    // Get workflow status, only download if finished?
    match workflow_status {
        "finished" => {
            // Download only outputs
            if !all {
                let workflow_json = get_workflow_specification(&reana, workflow_name)?;
                let output_files = workflow_json
                    .get("specification")
                    .and_then(|spec| spec.get("outputs"))
                    .and_then(|outputs| outputs.get("files"))
                    .and_then(|files| files.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|filename| format!("outputs/{filename}"))
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default();
                download_files(&reana, workflow_name, &output_files, output_dir.map(|x| x.as_str()))?;
            }
            // Download all files in workspace
            else {
                let workspace_json = get_workflow_workspace(&reana, workflow_name)?;
                let workspace_files: Vec<String> = workspace_json
                    .get("items")
                    .and_then(|items| items.as_array())
                    .map(|array| array.iter().filter_map(|item| item.get("name")?.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                    download_files(&reana, workflow_name, &workspace_files, output_dir.map(|x| x.as_str()))?;
            }
        }
        "failed" => {
            if let Some(logs_str) = status_response["logs"].as_str() {
                analyze_workflow_logs(logs_str);
            }
            return Err(format!("❌ Workflow '{workflow_name}' failed.").into());
        }
        "created" | "pending" | "running" | "stopped" => {
            return Err(format!("⚠️ Workflow '{workflow_name}' is in '{workflow_status}' state. Cannot export RO-Crate.").into());
        }
        unknown => {
            return Err(format!("❌ Unrecognized workflow status: {unknown}").into());
        }
    }
    Ok(())
}
