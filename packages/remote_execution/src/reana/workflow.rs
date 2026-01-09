use crate::reana::{auth::login_reana, compatibility::compatibility_adjustments, status::status_file_path};
use reana_ext::{
    api::{create_workflow, ping_reana, upload_files, start_workflow},
    parser::generate_workflow_json_from_cwl,
    reana::Reana,
};
use s4n_core::config;
use std::{collections::HashMap, error::Error, fs, path::PathBuf};

pub fn execute_remote_start(file: &PathBuf, input_file: &Option<PathBuf>) -> Result<String, Box<dyn Error>> {
    let config_path = PathBuf::from("workflow.toml");
    let config: Option<config::Config> = if config_path.exists() {
        Some(toml::from_str(&fs::read_to_string(&config_path)?)?)
    } else {
        None
    };
    let workflow_name = derive_workflow_name(file, config.as_ref());

    // Get credentials
    let (reana_instance, reana_token) = login_reana()?;
    let reana = Reana::new(reana_instance.clone(), reana_token.clone());


    // Ping
    let ping_status = ping_reana(&reana)?;
    if ping_status.get("status").and_then(|s| s.as_str()) != Some("200") {
        return Err(format!("⚠️ Unexpected response from Reana server: {ping_status:?}").into());
    }
    // Generate worfklow.json
    let mut workflow_json = generate_workflow_json_from_cwl(file, input_file)?;
    compatibility_adjustments(&mut workflow_json)?;

    let workflow_json = serde_json::to_value(workflow_json)?;
    let converted_yaml: serde_yaml::Value = serde_json::from_value(workflow_json.clone())?;
    // Create workflow
    let create_response = create_workflow(&reana, &workflow_json, Some(&workflow_name))?;
    let Some(workflow_name) = create_response["workflow_name"].as_str() else {
        return Err("Missing workflow_name in response".into());
    };
    upload_files(&reana, input_file, file, workflow_name, &workflow_json)?;
    start_workflow(&reana, workflow_name, None, None, false, &converted_yaml)?;
    eprintln!("✅ Started workflow execution");

    save_workflow_name(&reana_instance, workflow_name)?;
    Ok(workflow_name.to_owned())
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

fn save_workflow_name(instance_url: &str, name: &str) -> std::io::Result<()> {
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

pub(super) fn get_saved_workflows(instance_url: &str) -> Vec<String> {
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
