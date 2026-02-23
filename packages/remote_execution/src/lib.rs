use std::path::{PathBuf, Path};
use reana_ext::parser::WorkflowJson;
use anyhow::Result;
mod reana;

pub fn schedule_run(file: &Path, input_file: &Option<PathBuf>) -> Result<String, Box<dyn std::error::Error>> {
    reana::execute_remote_start(file, input_file)
}

pub fn check_status(workflow_name: &Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::check_remote_status(workflow_name)
}

pub fn download_results(workflow_name: &str, all: bool, output_dir: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::download_remote_results(workflow_name, all, output_dir)
}

pub fn export_rocrate(workflow_name: &str, output_dir: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    reana::export_rocrate(workflow_name, output_dir)
}

pub fn logout() -> Result<(), Box<dyn std::error::Error>> {
    reana::logout_reana()
}

pub fn watch(workflow_name: &str, rocrate: bool) -> Result<(), Box<dyn std::error::Error>> {
    reana::watch(workflow_name, rocrate)
}

pub fn compatibility_adjustments(
    workflow_json: &mut WorkflowJson,
) -> Result<()> {
    crate::reana::compatibility_adjustments(workflow_json)
}
