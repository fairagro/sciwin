use keyring::Entry;
use std::path::PathBuf;
use crate::components::files::Node;
use async_trait::async_trait;
use dioxus::prelude::*;
use secrecy::SecretString;
use sciwin::cwl::engine::{InputObject, load_input_file_from_file};
use sciwin::execution::{ReanaRunner, WorkflowRunner};
use sciwin::reana::api::client::ReanaClient;
use sciwin::reana::auth::{AuthError, TokenProvider};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::task::spawn_blocking;

/// Reads the REANA API token from the OS keychain on demand -- backs [`ReanaClient`] with the
/// same credential storage [`get_reana_credentials`] uses, instead of a bare static string.
struct KeyringTokenProvider;

#[async_trait]
impl TokenProvider for KeyringTokenProvider {
    async fn get_token(&self) -> Result<SecretString, AuthError> {
        spawn_blocking(|| Entry::new("reana", "token")?.get_password())
            .await
            .map_err(|_| AuthError)?
            .map(|token| SecretString::new(token.into()))
            .map_err(|_| AuthError)
    }
}

/// Builds a [`ReanaRunner`] against `instance_url`, authenticating with whatever token is
/// currently stored in the OS keychain.
pub fn build_reana_runner(instance_url: &str) -> anyhow::Result<ReanaRunner> {
    let server_url = reqwest::Url::parse(instance_url)?;
    let client = ReanaClient::new(server_url.join("api")?, Arc::new(KeyringTokenProvider));
    Ok(ReanaRunner::new(client))
}

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

/// Where [`save_workflow_name`]/[`get_saved_workflows`] persist "which workflow names were
/// submitted to which REANA instance", so the terminal's Download/RO-Crate buttons can find the
/// most recent run without the user re-typing its name. Frontend-local session bookkeeping --
/// there's no equivalent in `sciwin::execution`, which builds a fresh runner per call and keeps
/// no history across process restarts.
fn status_file_path() -> PathBuf {
    std::env::temp_dir().join("workflow_status_list.json")
}

fn save_workflow_name(instance_url: &str, name: &str) -> anyhow::Result<()> {
    let file_path = status_file_path();
    let mut workflows: HashMap<String, Vec<String>> = if file_path.exists() {
        let content = std::fs::read_to_string(&file_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let entry = workflows.entry(instance_url.to_string()).or_default();
    if !entry.contains(&name.to_string()) {
        entry.push(name.to_string());
    }
    std::fs::write(&file_path, serde_json::to_string_pretty(&workflows)?)?;
    Ok(())
}

fn get_saved_workflows(instance_url: &str) -> Vec<String> {
    let file_path = status_file_path();
    if !file_path.exists() {
        return vec![];
    }
    let content = std::fs::read_to_string(&file_path).unwrap_or_default();
    let workflows: HashMap<String, Vec<String>> = serde_json::from_str(&content).unwrap_or_default();
    workflows.get(instance_url).cloned().unwrap_or_default()
}

async fn log_msg(sender: &Option<Sender<String>>, message: &str) {
    if let Some(tx) = sender {
        let _ = tx.send(format!("{message}\n")).await;
    } else {
        eprintln!("{message}");
    }
}

async fn run_reana_async(
    runner: ReanaRunner,
    instance_url: String,
    cwl_file: PathBuf,
    inputs: InputObject,
    log_sender: Option<Sender<String>>,
) -> anyhow::Result<()> {
    log_msg(&log_sender, "🚀 Submitting workflow to REANA...").await;

    // `execution::reana_compat::compatibility_adjustments` isn't wired in here, yet -- see the
    // same gap noted in crates/cli/commands/execute.rs's `execute_remote_start`.
    let run_id = runner
        .submit(&cwl_file, inputs, None)
        .await
        .map_err(|e| anyhow::anyhow!("Submit failed: {e}"))?;

    log_msg(&log_sender, &format!("Submitted workflow run '{run_id}'")).await;
    save_workflow_name(&instance_url, &run_id)?;
    log_msg(&log_sender, "▶️ Workflow started successfully!").await;

    Ok(())
}

pub async fn execute_reana_workflow(
    item: Node,
    working_dir: PathBuf,
    mut show_settings: Signal<bool>,
    log_sender: Option<Sender<String>>,
) -> anyhow::Result<()> {
    log_msg(&log_sender, "🔹 Initializing REANA execution...").await;
    let (instance, _token) = match get_reana_credentials() {
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
    let inputs = if input_file.exists() {
        match load_input_file_from_file(input_file, working_dir.clone()) {
            Ok(inputs) => inputs,
            Err(e) => {
                log_msg(&log_sender, &format!("❌ Failed to load inputs: {e}")).await;
                return Ok(());
            }
        }
    } else {
        InputObject::default()
    };

    let runner = match build_reana_runner(&instance) {
        Ok(runner) => runner,
        Err(e) => {
            log_msg(&log_sender, &format!("❌ Failed to connect to REANA: {e}")).await;
            return Ok(());
        }
    };

    let cwl_file = item.path.clone();
    let log_sender_clone = log_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = run_reana_async(runner, instance, cwl_file, inputs, log_sender_clone.clone()).await {
            if let Some(tx) = log_sender_clone {
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