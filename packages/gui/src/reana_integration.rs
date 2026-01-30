use anyhow::anyhow;
use keyring::Entry;
use std::path::PathBuf;
use tokio::task;
use crate::components::files::Node;
use remote_execution::compatibility_adjustments;
 use dioxus::prelude::WritableExt;

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

pub fn store_reana_credentials(instance: &str, token: &str) -> Result<(), keyring::Error> {
    Entry::new("reana", "instance")?.set_password(instance)?;
    Entry::new("reana", "token")?.set_password(token)?;
    Ok(())
}

pub fn normalize_inputs(workflow_json: &mut serde_json::Value, prefix: &str) -> anyhow::Result<()> {
    if let Some(inputs) = workflow_json.get_mut("inputs").and_then(|v| v.as_object_mut())
        && let Some(serde_json::Value::Array(dir_list)) = inputs.get_mut("directories")
    {
        let normalized: Vec<serde_json::Value> = dir_list
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| {
                let mut path = s.to_string();
                if path.starts_with("../") {
                    path = path.trim_start_matches("../").to_string();
                }
                if path.starts_with(prefix) {
                    path = path.trim_start_matches(prefix).to_string();
                }
                serde_json::Value::String(path)
            })
            .collect();
        *dir_list = normalized;
    }
    Ok(())
}

pub async fn execute_reana_workflow(
    item: Node,
    working_dir: PathBuf,
    mut show_settings: dioxus::prelude::Signal<bool>,
) {
    let (instance, token) = match get_reana_credentials() {
        Ok(Some(creds)) => creds,
        Ok(None) => {
            show_settings.set(true);
            return;
        }
        Err(e) => {
            eprintln!("❌ Failed to retrieve REANA credentials: {e}");
            return;
        }
    };
    let input_file = working_dir.join("inputs.yml");
    let cwl_file = item.path.clone();
    let mut workflow = match reana::parser::generate_workflow_json_from_cwl(&cwl_file, &Some(input_file)) {
        Ok(wf) => wf,
        Err(e) => {
            eprintln!("❌ Failed to generate workflow JSON: {e}");
            return;
        }
    };
    if let Err(e) = compatibility_adjustments(&mut workflow) {
        eprintln!("❌ Compatibility adjustment failed: {e}");
        return;
    }
    let mut workflow_value = match serde_json::to_value(&workflow) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Failed to serialize workflow: {e}");
            return;
        }
    };
    if let Err(e) = normalize_inputs(&mut workflow_value, working_dir.to_str().unwrap_or("")) {
        eprintln!("❌ Input normalization failed: {e}");
        return;
    }
    let workflow = match serde_json::from_value(workflow_value) {
        Ok(wf) => wf,
        Err(e) => {
            eprintln!("❌ Failed to deserialize normalized workflow: {e}");
            return;
        }
    };
    let workflow_name = std::path::Path::new(&item.name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&item.name)
        .to_string();
    let reana = reana::reana::Reana::new(instance, token);
    let result = task::spawn_blocking(move || {
        run_reana_blocking(reana, workflow_name, workflow, working_dir, item.path)
    })
    .await;
    match result {
        Ok(Ok(())) => println!("✅ Workflow started successfully"),
        Ok(Err(e)) => eprintln!("❌ Workflow failed: {e}"),
        Err(e) => eprintln!("❌ Task join error: {e}"),
    }
}

fn run_reana_blocking(reana: reana::reana::Reana, workflow_name: String, workflow_json: serde_json::Value, working_dir: PathBuf, file_name: PathBuf) -> anyhow::Result<()> {
    reana::api::create_workflow(&reana, &workflow_json, Some(&workflow_name)).map_err(|e| anyhow!("Create workflow failed: {e}"))?;
    reana::api::upload_files(&reana, &None, &file_name, &workflow_name, &workflow_json, Some(&working_dir)).map_err(|e| anyhow!("Upload files failed: {e}"))?;
    let yaml: serde_yaml::Value = serde_json::from_value(workflow_json).map_err(|e| anyhow!("JSON to YAML conversion failed: {e}"))?;
    reana::api::start_workflow(&reana, &workflow_name, None, None, false, &yaml).map_err(|e| anyhow!("Start workflow failed: {e}"))?;
    Ok(())
}