use rocrate_ext::{RocrateArgs, export_rocrate};
use commonwl::{load_doc, packed::pack_workflow};
use anyhow::anyhow;
use commonwl::CWLDocument;

pub async fn handle_rocrate_command(args: RocrateArgs) -> Result<(), anyhow::Error> {
    let workflow_name = args.workflow_name.clone().unwrap_or_else(|| "unknown_workflow".to_string());
    let doc = load_doc(&workflow_name).map_err(|e| anyhow!("Failed to load CWL document: {e}"))?;
    let CWLDocument::Workflow(workflow) = doc else {
        return Err(anyhow!("CWL document is not a Workflow: {workflow_name}"));
    };
    let packed = pack_workflow(&workflow, &workflow_name, None).map_err(|e| anyhow!("Failed to pack workflow: {e}"))?;
    let packed_json = serde_json::to_value(&packed).map_err(|e| anyhow!("Failed to serialize packed workflow: {e}"))?;
    std::fs::write("packed.cwl", serde_json::to_string_pretty(&packed_json)?).map_err(|e| anyhow!("Failed to write packed.cwl: {e}"))?;
    let working_dir = std::env::current_dir().map_err(|e| anyhow!("Failed to get current directory: {e}"))?;
    let graph_json = packed_json
        .get("$graph")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("Missing or invalid '$graph' field"))?;

    export_rocrate(
        args.output_dir.as_ref(),
        Some(&working_dir.to_string_lossy().to_string()),
        &workflow_name,
        args.run_type,
        Some("local"),
        graph_json,
        None,
    ).await.map_err(|e| anyhow!("Failed to export ROCrate: {e}"))?;

    Ok(())
}