use crate::reana::{auth::login_reana, compatibility::{compatibility_adjustments}, status::status_file_path};
use reana_ext::{
    api::{create_workflow, ping_reana, upload_files_parallel, start_workflow},
    parser::generate_workflow_json_from_cwl,
    reana::Reana,
};
use s4n_core::config;
use std::{collections::HashMap, fs, path::{PathBuf, Path}, sync::Arc};
use anyhow::{Result, anyhow};
use std::env;

pub async fn execute_remote_start(file: &Path, input_file: &Option<PathBuf>) -> Result<String> {
    let config_path = PathBuf::from("workflow.toml");
    let config: Option<config::Config> = if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)?; 
        Some(toml::from_str(&contents)?)
    } else {
        None
    };
    let workflow_name = derive_workflow_name(file, config.as_ref());

    // Get credentials
    let (reana_instance, reana_token) = login_reana()
        .map_err(|e| anyhow!("Failed to login to REANA: {e}"))?;
    let reana = Reana::new(reana_instance.clone(), reana_token.clone());

    // Ping server
    let ping_status = ping_reana(&reana)
        .map_err(|e| anyhow!("Failed to ping REANA server: {e}"))?;
    if ping_status.get("status").and_then(|s| s.as_str()) != Some("200") {
        return Err(anyhow!("⚠️ Unexpected response from REANA server: {ping_status:?}"));
    }

    let mut workflow_json = generate_workflow_json_from_cwl(file, input_file)
        .map_err(|e| anyhow!("Failed to generate workflow JSON: {e}"))?;

    compatibility_adjustments(&mut workflow_json, None).await
    .map_err(|e| anyhow!("❌ Compatibility adjustment failed: {e}"))?;

    let workflow_json_value = serde_json::to_value(&workflow_json)
        .map_err(|e| anyhow!("Failed to convert workflow to JSON value: {e}"))?;
    let converted_yaml: serde_yaml::Value = serde_json::from_value(workflow_json_value.clone())
        .map_err(|e| anyhow!("Failed to convert JSON to YAML: {e}"))?;

    let workflow_name_clone = workflow_name.clone();
    let create_response = create_workflow(&reana, &workflow_json_value, Some(&workflow_name_clone))
        .map_err(|e| anyhow!("Failed to create workflow: {e}"))?;
    let workflow_name_str = create_response["workflow_name"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing workflow_name in response"))?;

    let working_dir = env::current_dir()
        .map_err(|e| anyhow!("Failed to get current directory: {e}"))?;

    // Upload files
    let reana = Arc::new(reana);
    upload_files_parallel(reana.clone(), input_file, file, workflow_name_str, &workflow_json_value, Some(&working_dir))
    .await
    .map_err(|e| anyhow!("Failed to upload files: {e}"))?;

    // Start workflow
    start_workflow(&reana, workflow_name_str, None, None, false, &converted_yaml)
        .map_err(|e| anyhow!("Failed to start workflow: {e}"))?;

    eprintln!("✅ Started workflow execution of '{workflow_name_str}'.");
    eprintln!("You can check its status using: s4n execute remote status '{workflow_name_str}' or use 's4n execute remote status' to check all workflows.");

    // Save workflow name
    save_workflow_name(&reana_instance, workflow_name_str)
        .await
        .map_err(|e| anyhow!("Failed to save workflow name: {e}"))?;

    Ok(workflow_name_str.to_owned())
}

pub fn analyze_workflow_logs(logs_str: &str) {
    let logs: serde_json::Value = serde_json::from_str(logs_str).expect("Invalid logs JSON");
    let mut found_failure = false;
    for (_job_id, job_info) in logs.as_object().unwrap() {
        let status = job_info["status"].as_str().unwrap_or("unknown");
        let job_name = job_info["job_name"].as_str().unwrap_or("unknown");
        let logs_text = job_info["logs"].as_str().unwrap_or("");
        if status == "failed" {
            eprintln!("❌ Workflow execution failed at step {job_name}:");
            eprintln!("Logs:\n{logs_text}\n");
            found_failure = true;
        }
    }
    // sometimes a workflow step fails but it is marked as finished, search for errors and suggest as failed step
    if !found_failure {
        for (_job_id, job_info) in logs.as_object().unwrap() {
            let job_name = job_info["job_name"].as_str().unwrap_or("unknown");
            let logs_text = job_info["logs"].as_str().unwrap_or("");
            //search for error etc in logs of steps
            if logs_text.contains("Error")
                || logs_text.contains("Exception")
                || logs_text.contains("Traceback")
                || logs_text.to_lowercase().contains("failed")
            {
                eprintln!("❌ Workflow execution failed. Workflow step {job_name} may have encountered an error:");
                eprintln!("Logs:\n{logs_text}\n");
            }
        }
    }
}

pub async fn save_workflow_name(instance_url: &str, name: &str) -> std::io::Result<()> {
    let file_path = status_file_path();
    let mut workflows: HashMap<String, Vec<String>> = if file_path.exists() {
        let content = fs::read_to_string(&file_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let entry = workflows.entry(instance_url.to_string()).or_default();
    if !entry.contains(&name.to_string()) {
        entry.push(name.to_string());
    }
    fs::write(&file_path, serde_json::to_string_pretty(&workflows)?)?;
    Ok(())
}

pub fn get_saved_workflows(instance_url: &str) -> Vec<String> {
    let file_path = status_file_path();
    if !file_path.exists() {
        return vec![];
    }
    let content = fs::read_to_string(&file_path).unwrap_or_default();
    let workflows: HashMap<String, Vec<String>> = serde_json::from_str(&content).unwrap_or_default();
    workflows.get(instance_url).cloned().unwrap_or_default()
}

fn derive_workflow_name(file: &std::path::Path, config: Option<&config::Config>) -> String {
    let file_stem = file.file_stem().unwrap_or_default().to_string_lossy();
    config
        .as_ref()
        .map_or_else(|| file_stem.to_string(), |c| format!("{} - {}", c.workflow.name, file_stem))
}
