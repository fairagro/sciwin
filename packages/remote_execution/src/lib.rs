use std::path::{PathBuf, Path};
use reana_ext::parser::WorkflowJson;
use anyhow::Result;
mod reana;
use anyhow::anyhow;
use tokio::sync::mpsc::Sender;

pub fn schedule_run(file: &Path, input_file: &Option<PathBuf>) -> Result<String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| anyhow!("Failed to create tokio runtime: {e}"))?;
    rt.block_on(reana::execute_remote_start(file, input_file))
}
pub fn check_status(workflow_name: &Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::check_remote_status(workflow_name)
}

pub fn download_results(workflow_name: &str, all: bool, output_dir: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::download_remote_results(workflow_name, all, output_dir)
}

pub fn export_rocrate(workflow_name: &str, output_dir: Option<&String>, working_dir: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::export_rocrate(workflow_name, output_dir, working_dir)
}

pub fn logout() -> Result<(), Box<dyn std::error::Error>> {
    reana::logout_reana()
}

pub fn watch(workflow_name: &str, rocrate: bool) -> Result<(), Box<dyn std::error::Error>> {
    reana::watch(workflow_name, rocrate)
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