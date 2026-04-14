use std::path::{PathBuf, Path};
use reana_ext::parser::WorkflowJson;
use anyhow::Result;
mod reana;
use tokio::sync::mpsc::Sender;
use rocrate_ext::{RocrateArgs};

pub async fn schedule_run(file: &Path, input_file: &Option<PathBuf>) -> Result<String> {
    reana::execute_remote_start(file, input_file).await
}
pub async fn check_status(workflow_name: &Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::check_remote_status(workflow_name).await
}

pub async fn download_results(workflow_name: &str, all: bool, output_dir: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::download_remote_results(workflow_name, all, output_dir).await
}

pub fn logout() -> Result<(), Box<dyn std::error::Error>> {
    reana::logout_reana()
}

pub fn reana_login() -> Result<(String, String), Box<dyn std::error::Error>> {
    reana::login_reana()
}

pub async fn watch(workflow_name: &str, rocrate_args: &Option<RocrateArgs>) -> Result<(), Box<dyn std::error::Error>> {
    reana::watch(workflow_name, rocrate_args).await
}

pub async fn compatibility_adjustments(workflow_json: &mut WorkflowJson, log_sender: Option<Sender<String>>) -> anyhow::Result<()> {
    reana::compatibility_adjustments(workflow_json, log_sender).await
}

pub async fn save_workflow_name(instance_url: &str, name: &str) -> std::io::Result<()> {
    reana::save_workflow_name(instance_url, name).await
}

pub fn get_saved_workflows(instance_url: &str) -> Vec<String> {
    reana::get_saved_workflows(instance_url)
}