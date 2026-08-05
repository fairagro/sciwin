use keyring::Entry;
use std::path::PathBuf;
use crate::components::files::Node;
use dioxus::prelude::*;
use tokio::sync::mpsc::Sender;
use tokio::task::spawn_blocking;
use std::sync::Arc;
use std::path::Path;
use remote_execution::{get_saved_workflows, save_workflow_name};
use serde_json::Value;

pub fn get_reana_credentials() -> Result<Option<(String, String)>, keyring::Error> {
    let instance_entry = Entry::new("reana", "instance")?;
    let token_entry = Entry::new("reana", "token")?;

    let instance = instance_entry.get_password();
    let token = token_entry.get_password();

    match (instance, token) {
        (Ok(i), Ok(t)) => Ok(Some((i, t))),
        _ => Ok(None),
    }
}

pub fn delete_reana_credentials() -> Result<(), keyring::Error> {
    let instance_entry = Entry::new("reana", "instance")?;
    let token_entry = Entry::new("reana", "token")?;
    // Remove stored credentials
    let _ = instance_entry.delete_credential();
    let _ = token_entry.delete_credential();

    Ok(())
}

pub fn store_reana_credentials(instance: &str, token: &str) -> Result<(), keyring::Error> {
    Entry::new("reana", "instance")?.set_password(instance)?;
    Entry::new("reana", "token")?.set_password(token)?;
    Ok(())
}

pub fn sanitize_path(path: &str) -> String {
    let path = Path::new(path.trim());
    let mut sanitized_path = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                sanitized_path.pop();
            }
            std::path::Component::CurDir => {
            }
            _ => {
                sanitized_path.push(comp.as_os_str());
            }
        }
    }
    sanitized_path
        .to_string_lossy()
        .replace("\\", std::path::MAIN_SEPARATOR_STR)
}


pub fn normalize_inputs(workflow_json: &mut Value, prefix: &str) -> Result<()> {
    let clean_prefix = sanitize_path(prefix);
    let prefix_tail = Path::new(&clean_prefix)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if let Some(inputs) = workflow_json.get_mut("inputs").and_then(|v| v.as_object_mut())
        && let Some(Value::Array(dir_list)) = inputs.get_mut("directories") {
            let normalized: Vec<Value> = dir_list
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| {
                    let mut path = sanitize_path(s);
                    while path.starts_with("../") {
                        path = path.trim_start_matches("../").to_string();
                    }
                    if path.starts_with(&clean_prefix) {
                        path = path[clean_prefix.len()..].trim_start_matches(['/', '\\']).to_string();
                    }
                    if let Some(idx) = path.find(&prefix_tail) {
                        path = path[idx + prefix_tail.len()..]
                            .trim_start_matches(['/', '\\'])
                            .to_string();
                    }
                    Value::String(path)
                })
                .collect();
            *dir_list = normalized;
    }
    Ok(())
}

async fn log_msg(sender: &Option<Sender<String>>, message: &str) {
    if let Some(tx) = sender {
        let _ = tx.send(format!("{message}\n")).await;
    } else {
        eprintln!("{message}");
    }
}

pub async fn run_reana_async(
    reana: reana::reana::Reana,
    workflow_name: String,
    workflow_json: serde_json::Value,
    working_dir: PathBuf,
    file_name: PathBuf,
    log_sender: Option<Sender<String>>,
) -> anyhow::Result<()> {
    let reana = Arc::new(reana);
    let creds = get_reana_credentials()?;
    let instance_url = if let Some((instance, _token)) = creds {
        instance
    } else {
        return Err(anyhow::anyhow!("No REANA credentials found"));
    };
    log_msg(&log_sender, "🚀 Starting REANA workflow setup...").await;
    log_msg(&log_sender, "📁 Creating workflow...").await;

    let workflow_name_str = {
        let reana = reana.clone();
        let workflow_json = workflow_json.clone();
        let workflow_name = workflow_name.clone();
        spawn_blocking(move || -> anyhow::Result<String> {
            let create_response = reana::api::create_workflow(&reana, &workflow_json, Some(&workflow_name))
                .map_err(|e| anyhow::anyhow!("Create workflow failed: {e}"))?;

            let workflow_name_str = create_response["workflow_name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing workflow_name in response"))?;

            Ok(workflow_name_str.to_string())
        })
        .await??
    };
    log_msg(&log_sender, &format!("Created workflow '{}'", workflow_name_str)).await;
    save_workflow_name(&instance_url, &workflow_name_str).await
        .map_err(|e| anyhow::anyhow!("Saving workflow failed: {e}"))?;
    log_msg(&log_sender, "📤 Uploading input files...").await;
    reana::api::upload_files_parallel(reana.clone(), &None, &file_name, &workflow_name, &workflow_json, Some(&working_dir))
    .await
    .map_err(|e| anyhow::anyhow!("Upload files failed: {e}"))?;
    let yaml: serde_yaml::Value = serde_json::from_value(workflow_json)
        .map_err(|e| anyhow::anyhow!("JSON to YAML conversion failed: {e}"))?;
    log_msg(&log_sender, &format!("▶️ Starting workflow execution for '{}'", workflow_name_str)).await;
    spawn_blocking(move || {
        reana::api::start_workflow(&reana, &workflow_name, None, None, false, &yaml)
            .map_err(|e| anyhow::anyhow!("Start workflow failed: {e}"))
    })
    .await??;
    log_msg(&log_sender, "✅ Workflow started successfully!").await;

    Ok(())
}

pub async fn execute_reana_workflow(
    item: Node,
    working_dir: PathBuf,
    mut show_settings: Signal<bool>,
    log_sender: Option<Sender<String>>,
) -> Result<()> {
    log_msg(&log_sender, "🔹 Initializing REANA execution...").await;
    let (instance, token) = match get_reana_credentials() {
        Ok(Some(creds)) => creds,
        Ok(None) => {
            log_msg(&log_sender, "⚠️ No REANA credentials found. Opening settings...").await;
            show_settings.set(true);
            return Ok(());
        }
        Err(e) => {
            log_msg(&log_sender, &format!("❌ Failed to get REANA credentials: {e}")).await;
            return Ok(());
        }
    };
    let input_file = working_dir.join("inputs.yml");
    let inputs_file_option = if input_file.exists() {
        Some(input_file)
    } else {
        None
    };
    let cwl_file = item.path.clone();
    let mut workflow = match reana::parser::generate_workflow_json_from_cwl(&cwl_file, &inputs_file_option) {
        Ok(wf) => wf,
        Err(e) => {
            log_msg(&log_sender, &format!("❌ Failed to generate workflow JSON: {e}")).await;
            return Ok(());
        }
    };
    if let Err(e) = remote_execution::compatibility_adjustments(&mut workflow, log_sender.clone()).await {
        log_msg(&log_sender, &format!("❌ Compatibility adjustments failed: {e}")).await;
        return Ok(());
    }
    let mut workflow_value = match serde_json::to_value(&workflow) {
        Ok(v) => v,
        Err(e) => {
            log_msg(&log_sender, &format!("❌ Failed to serialize workflow: {e}")).await;
            return Ok(());
        }
    };
    if let Err(e) = normalize_inputs(&mut workflow_value, working_dir.to_str().unwrap_or("")) {
        log_msg(&log_sender, &format!("❌ Input normalization failed: {e}")).await;
        return Ok(());
    }
    let workflow: serde_json::Value = match serde_json::from_value(workflow_value) {
        Ok(wf) => wf,
        Err(e) => {
            log_msg(&log_sender, &format!("❌ Failed to deserialize normalized workflow: {e}")).await;
            return Ok(());
        }
    };
    let workflow_name = PathBuf::from(&item.name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&item.name)
        .to_string();
    let reana = reana::reana::Reana::new(instance, token);
    let log_sender_clone = log_sender.clone();
    let working_dir_clone = working_dir.clone();
    let item_path_clone = item.path.clone();
    let workflow_clone = workflow.clone();
    let workflow_name_clone = workflow_name.clone();
    tokio::spawn(async move {
        if let Err(e) = run_reana_async(
            reana,
            workflow_name_clone,
            workflow_clone,
            working_dir_clone,
            item_path_clone,
            log_sender_clone,
        )
        .await
        {
            if let Some(tx) = log_sender {
                let _ = tx.send(format!("❌ Workflow execution failed: {e}\n")).await;
            } else {
                eprintln!("❌ Workflow execution failed: {e}");
            }
        }
    });

    Ok(())
}

pub async fn get_last_workflow_name() -> anyhow::Result<String> {
    let (instance, _token) = match get_reana_credentials() {
        Ok(Some(creds)) => creds,
        Ok(None) => {
            return Ok(String::new());
        }
        Err(_err) => {
            return Ok(String::new());
        }
    };
    let saved_workflows = get_saved_workflows(&instance);
    let last_workflow = saved_workflows
        .last()
        .ok_or_else(|| anyhow::anyhow!("No saved workflows found for this instance"))?;
    Ok(last_workflow.to_string())
}