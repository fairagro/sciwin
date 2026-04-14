use crate::run_type::{RocrateRunType};
use serde_json::Value;
use std::path::{Path, PathBuf};
use crate::builder::create_ro_crate_metadata_json_from_graph_provenance_runcrate;
use crate::builder::create_ro_crate_metadata_json_from_graph_workflow_runcrate;
use crate::builder::create_ro_crate_metadata_json_from_graph_workflow_processcrate;
use crate::builder::create_ro_crate_metadata_json_from_graph_workflow_rocrate;
use crate::utils::{extract_times_from_logs};
use crate::utils::zip_dir;
use std::fs;
use serde_json::json;
use std::collections::HashMap;

pub async fn export_rocrate(
    ro_crate_dir: Option<&String>,
    working_dir: Option<&String>,
    cwl_file: &str,
    run_type: RocrateRunType,
    execution_type: Option<&str>,
    graph_json: &[Value],
    logs: Option<&serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let execution_type = execution_type.unwrap_or("local");
    let workflow_file = match execution_type {
        "remote" => "workflow.json",
        "local" => "packed.cwl",
        _ => return Err("Unknown execution type".into()),
    };
    let working_dir = working_dir
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let rocrate_dir = ro_crate_dir
        .cloned()
        .unwrap_or_else(|| "rocrate".to_string());
    let crate_dir = working_dir.join(&rocrate_dir);
    std::fs::create_dir_all(&crate_dir)?;
    let workflow_toml = {
        let config_path = working_dir.join("workflow.toml");
        if config_path.exists() {
            fs::read_to_string(config_path)?
        } else {
            eprintln!("⚠️ workflow.toml not found. Using defaults.");
            "{}".to_string()
        }
    };
    let timestamps = if execution_type == "remote" {
        extract_times_from_logs(&serde_json::to_string_pretty(&logs)?)?
    } else {
        HashMap::new()
    };
    let mut ro_crate_metadata_json: Value = match run_type {
        RocrateRunType::WorkflowRun =>
            create_ro_crate_metadata_json_from_graph_workflow_runcrate(
                graph_json,
                &workflow_toml,
                &crate_dir,
                workflow_file,
                if execution_type == "remote" { Some(&timestamps) } else { None },
                &working_dir,
                cwl_file,
            ).await?,

        RocrateRunType::ProcessRun =>
            create_ro_crate_metadata_json_from_graph_workflow_processcrate(
                graph_json,
                &workflow_toml,
                &crate_dir,
                workflow_file,
                if execution_type == "remote" { Some(&timestamps) } else { None },
                &working_dir,
                cwl_file,
            ).await?,

        RocrateRunType::WorkflowROCrate =>
            create_ro_crate_metadata_json_from_graph_workflow_rocrate(
                graph_json,
                &workflow_toml,
                &crate_dir,
                workflow_file,
                None,
                &working_dir,
                cwl_file,
            ).await?,

        RocrateRunType::ProvenanceRun =>
            create_ro_crate_metadata_json_from_graph_provenance_runcrate(
                graph_json,
                &workflow_toml,
                &crate_dir,
                workflow_file,
                if execution_type == "remote" { Some(&timestamps) } else { None },
                &working_dir,
                cwl_file,
            ).await?,
    };
    let metadata_path = crate_dir.join("ro-crate-metadata.json");
    std::fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&ro_crate_metadata_json)?
    )?;
    let graph_value = graph_json
        .first()
        .ok_or("graph_json slice is empty")?;
    let graph_path = crate_dir.join(workflow_file);
    std::fs::write(
        graph_path,
        serde_json::to_string_pretty(graph_value)?
    )?;
    if let Some(graph) = ro_crate_metadata_json
    .get_mut("@graph")
    .and_then(|g: &mut Value| g.as_array_mut())
    {
        for entity in graph {
            if let Some(default_value) = entity.get_mut("defaultValue") &&
                let Some(path_str) = default_value.as_str() &&
                    let Some(stripped) = path_str.strip_prefix("file://") {
                        let src_path = Path::new(stripped);

                        if src_path.exists() && src_path.is_file() {
                            let file_name = src_path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .to_string();

                            let dest_path = crate_dir.join(&file_name);

                            if src_path != dest_path {
                                std::fs::copy(src_path, &dest_path)?;
                            }

                            *default_value = json!(file_name);
                        }
            }
        }
    }
    let zip_path = working_dir.join(format!("{}.zip", rocrate_dir));
    zip_dir(&crate_dir, &zip_path)?;

    std::fs::remove_dir_all(&crate_dir)?;

    Ok(())
}