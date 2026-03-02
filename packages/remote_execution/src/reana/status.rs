use crate::reana::{
    auth::login_reana,
    export_rocrate,
    workflow::{analyze_workflow_logs, get_saved_workflows},
};
use reana_ext::{api::get_workflow_status, reana::Reana};
use std::{error::Error, path::PathBuf, thread, time::Duration};
pub(super) fn status_file_path() -> PathBuf {
    std::env::temp_dir().join("workflow_status_list.json")
}

pub fn check_remote_status(workflow_name: &Option<String>) -> Result<(), Box<dyn Error>> {
    let (reana_instance, reana_token) = login_reana()?;
    let reana = Reana::new(reana_instance.clone(), reana_token);

    if let Some(name) = workflow_name {
        evaluate_workflow_status(&reana, name, true)?;
    } else {
        let workflows = get_saved_workflows(&reana_instance);
        if workflows.is_empty() {
            return Err(format!("No workflows saved for REANA instance '{reana_instance}'").into());
        }
        for name in workflows {
            evaluate_workflow_status(&reana, &name, false)?;
        }
    }
    Ok(())
}

pub fn evaluate_workflow_status(reana: &Reana, name: &str, analyze_logs: bool) -> Result<String, Box<dyn Error>> {
    let status_response = get_workflow_status(reana, name).map_err(|e| format!("Failed to fetch workflow status: {e}"))?;
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

pub fn watch(workflow_name: &str, rocrate: bool) -> Result<(), Box<dyn Error>> {
    let (reana_instance, reana_token) = login_reana()?;
    let reana = Reana::new(reana_instance, reana_token);

    const POLL_INTERVAL_SECS: u64 = 5;
    const TERMINAL_STATUSES: [&str; 3] = ["finished", "failed", "deleted"];

    loop {
        let status_response = get_workflow_status(&reana, workflow_name).map_err(|e| format!("Failed to fetch workflow status: {e}"))?;
        let workflow_status = status_response["status"].as_str().unwrap_or("unknown");
        if TERMINAL_STATUSES.contains(&workflow_status) {
            match workflow_status {
                "finished" => {
                    eprintln!("✅ Workflow finished successfully.");
                    if let Err(e) = crate::reana::download_remote_results(workflow_name, false, None) {
                        eprintln!("Error downloading remote results: {e}");
                    }
                    if rocrate && let Err(e) = export_rocrate(workflow_name, Some(&"rocrate".to_string()), None) {
                        eprintln!("Error trying to create a Provenance RO-Crate: {e}");
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
        thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));
    }
    Ok(())
}
