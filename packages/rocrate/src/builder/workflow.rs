use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use rocraters::ro_crate::{data_entity::DataEntity, graph_vector::GraphVector};
use crate::model::{Action, create_container_image, create_action};
use crate::model::{create_workflow_entity, create_software_application, create_parameter_connection, create_instrument, create_cwl_entity};
use crate::model::{create_root_dataset_entity, create_creative_work, create_ro_crate_metadata, create_formal_parameter};
use util::is_cwl_file;
use std::path::{Path};
use crate::utils::StepTimestamp;
use super::metadata::{generate_id_with_hash, generate_connections, extract_or_prompt_metadata, extract_workflow_steps, get_workflow_structure, extract_parts, classify_and_prefix_params};

const DEFAULT_WORKFLOW_RUN_CONFORMS_TO: &[&str] = &[
    "https://w3id.org/ro/wfrun/process/0.5",
    "https://w3id.org/ro/wfrun/workflow/0.5",
    "https://w3id.org/workflowhub/workflow-ro-crate/1.0",
];

const DEFAULT_WORKFLOW_RO_CONFORMS_TO: &[&str] = &[
    "https://w3id.org/workflowhub/workflow-ro-crate/1.0",
];

pub async fn create_ro_crate_metadata_json_from_graph_workflow_runcrate(
    graph_json: &[Value],
    workflow_toml: &str,
    rocrate_dir: &Path,
    workflow_file: &str,
    timestamps: Option<&StepTimestamp>,
    working_dir: &Path,
    cwl_file: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    if graph_json.is_empty() {
        return Ok(json!({
            "@context": "https://w3id.org/ro/crate/1.1/context",
            "@graph": []
        }));
    }
    let packed_cwl = json!({ "$graph": graph_json });
    let steps = extract_workflow_steps(&packed_cwl, workflow_file);
    let step_ids: Vec<&str> = steps.iter().map(|(_, step)| step.as_str()).collect();
    let step_files: Vec<&str> = steps.iter().map(|(file, _)| file.as_str()).collect();
    let script_structure = get_workflow_structure(&packed_cwl);
    let parts = extract_parts(&script_structure, workflow_file, rocrate_dir);
    let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
    let connections = generate_connections(&script_structure, workflow_file);
    let (name, description, license) =
        extract_or_prompt_metadata(workflow_toml, working_dir, cwl_file).await?;
        let ro_crate_metadata = create_ro_crate_metadata(
        "ro-crate-metadata.json",
        Some("./"),
        &[
            "https://w3id.org/ro/crate/1.1",
            "https://w3id.org/ro/wfrun/process/0.5",
            "https://w3id.org/ro/wfrun/workflow/0.5",
            "https://w3id.org/workflowhub/workflow-ro-crate/1.0",
        ],
    );
    let cwl_entity = create_cwl_entity(
        "https://w3id.org/workflowhub/workflow-ro-crate#cwl",
        "ComputerLanguage",
        "CWL",
        "https://w3id.org/cwl/v1.2/",
        "Common Workflow Language",
        "https://www.commonwl.org/",
        "v1.2",
    );
    let creative_works: Vec<DataEntity> =
        DEFAULT_WORKFLOW_RUN_CONFORMS_TO.iter().map(|id| create_creative_work(id)).collect();
    let parameter_connections: Vec<DataEntity> = connections
        .iter()
        .map(|(src, tgt, id)| create_parameter_connection(id, src, tgt))
        .collect();
    let mut graph: Vec<GraphVector> = Vec::new();
    graph.push(GraphVector::MetadataDescriptor(ro_crate_metadata));
    graph.push(GraphVector::DataEntity(cwl_entity));
    graph.extend(creative_works.into_iter().map(GraphVector::DataEntity));
    let mut formal_params: Vec<DataEntity> = Vec::new();
    let mut organize_obj_ids = Vec::new();
    let mut organize_res_ids = Vec::new();
    let empty_timestamps: StepTimestamp = HashMap::new();
    let timestamps = timestamps.unwrap_or(&empty_timestamps);
    for step in script_structure.values() {
        let id = &step.id;
        let inputs = &step.inputs;
        let outputs = &step.outputs;
        let docker = &step.docker;
        let create_id = generate_id_with_hash();
        let control_id = generate_id_with_hash();
        let mut docker_id = None;
        if is_cwl_file(id) {
            organize_obj_ids.push(control_id.clone());
            if let Some(docker_pull) = docker {
                let parts: Vec<&str> = docker_pull.split(':').collect();
                let (image_name, image_tag) = match parts.len() {
                    2 => (parts[0], Some(parts[1])),
                    1 => (parts[0], None),
                    _ => return Err("Invalid Docker image format".into()),
                };
                docker_id = Some(generate_id_with_hash());
                let docker_entity = create_container_image(
                    docker_id.as_ref().unwrap(),
                    image_name,
                    image_tag.unwrap_or("latest"),
                    "docker.io",
                );
                graph.push(GraphVector::DataEntity(docker_entity));
            }
            let modified_params = classify_and_prefix_params(id, inputs, outputs);
            let input_names: Vec<String> = inputs.iter().map(|(i, _)| i.clone()).collect();
            let output_names: Vec<String> = outputs.iter().map(|(i, _)| i.clone()).collect();
            let software_application = create_software_application(
                id.trim_start_matches('#'),
                &input_names,
                &output_names,
                workflow_file,
            );
            graph.push(GraphVector::DataEntity(software_application));
            let mut existing_ids: HashSet<String> =
                formal_params.iter().map(|p| p.id.clone()).collect();
            for (new_id, classification, loc) in &modified_params {
                if !existing_ids.contains(new_id) {
                    formal_params.push(create_formal_parameter(
                        new_id,
                        classification,
                        Some(loc.as_str()),
                        workflow_file,
                    ));
                    existing_ids.insert(new_id.clone());
                }
            }
            let step_opt = steps.iter().find(|(_, s)| *s == *id).map(|(f, _)| f.to_string());
            let key = id.trim_start_matches('#').trim_end_matches(".cwl").to_string();
            let (start, end) = timestamps.get(&key).cloned().unwrap_or((None, None));
            let input_file_names: Vec<String> = inputs
                .iter()
                .filter_map(|(_, path)| {
                    let clean = path.strip_prefix("file://").unwrap_or(path);
                    Path::new(clean).file_name()?.to_str().map(String::from)
                })
                .collect();
            let output_refs: Vec<&str> = outputs.iter().map(|(_, v)| v.as_str()).collect();
            let instrument_id = format!("{workflow_file}{id}");
            let create = Action {
                action_type: "CreateAction",
                id: &create_id,
                name: &format!(
                    "Run of {workflow_file}{}",
                    step_opt.as_deref().unwrap_or("")
                ),
                instrument_id: &instrument_id,
                object_ids: input_file_names,
                result_ids: Some(output_refs),
                start_time: start.as_deref(),
                end_time: end.as_deref(),
                container_image_id: docker_id.as_deref(),
            };
            graph.push(GraphVector::DataEntity(create_action(create)));
        } else if id == "#main" || *id == format!("{workflow_file}#main") {
            let modified_params = classify_and_prefix_params(id, inputs, outputs);
            for (input_id, classification, _) in &modified_params {
                formal_params.push(create_formal_parameter(
                    input_id,
                    classification,
                    None,
                    workflow_file,
                ));
            }
            let input_ids: Vec<String> = inputs
                .iter()
                .map(|(i, _)| format!("{workflow_file}{id}/{i}"))
                .collect();
            let workflow_entity = create_workflow_entity(
                &connections,
                &step_ids,
                &input_ids,
                outputs,
                &step_files,
                workflow_file,
            );
            graph.push(GraphVector::DataEntity(workflow_entity.clone()));
            organize_res_ids.push(create_id.clone());
            let root_dataset = create_root_dataset_entity(
                DEFAULT_WORKFLOW_RUN_CONFORMS_TO,
                &license,
                &name,
                &description,
                &parts_ref,
                &create_id,
                workflow_file,
            );
            graph.push(GraphVector::RootDataEntity(root_dataset));
        }
    }
    graph.extend(formal_params.into_iter().map(GraphVector::DataEntity));
    graph.extend(parameter_connections.into_iter().map(GraphVector::DataEntity));
    let instrument_id = generate_id_with_hash();
    let version = "cwltool 3.1.20210628163208";
    let instrument = create_instrument(&instrument_id, "SoftwareApplication", version);
    graph.push(GraphVector::DataEntity(instrument));
    if let Some(main_step) = script_structure
        .values()
        .find(|s| s.id == "#main" || s.id == format!("{workflow_file}#main"))
    {
        let workflow_create_id = generate_id_with_hash();
        let main_inputs: Vec<String> =
            main_step.inputs.iter().map(|(i, _)| i.clone()).collect();
        let main_outputs: Vec<&str> =
            main_step.outputs.iter().map(|(_, v)| v.as_str()).collect();
        let (start, end) = timestamps
            .get("workflow")
            .cloned()
            .unwrap_or((None, None));
        let workflow_create_action = Action {
            action_type: "CreateAction",
            id: &workflow_create_id,
            name: &format!("Run of {workflow_file}#main"),
            instrument_id: workflow_file,
            object_ids: main_inputs,
            result_ids: Some(main_outputs),
            start_time: start.as_deref(),
            end_time: end.as_deref(),
            container_image_id: None,
        };
        graph.push(GraphVector::DataEntity(create_action(workflow_create_action)));
    }
    let graph_values: Vec<Value> = graph
        .into_iter()
        .map(|g| match g {
            GraphVector::MetadataDescriptor(md) => serde_json::to_value(&md).unwrap(),
            GraphVector::RootDataEntity(rde) => serde_json::to_value(&rde).unwrap(),
            GraphVector::DataEntity(de) => serde_json::to_value(&de).unwrap(),
            GraphVector::ContextualEntity(ce) => serde_json::to_value(&ce).unwrap(),
        })
        .collect();
    Ok(json!({
        "@context": [
            "https://w3id.org/ro/crate/1.1/context",
            "https://w3id.org/ro/terms/workflow-run/context"
        ],
        "@graph": graph_values
    }))
}


pub async fn create_ro_crate_metadata_json_from_graph_workflow_rocrate(
    graph_json: &[Value],
    workflow_toml: &str,
    rocrate_dir: &Path,
    workflow_file: &str,
    _timestamps: Option<&StepTimestamp>,
    working_dir: &Path,
    cwl_file: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    if graph_json.is_empty() {
        return Ok(json!({
            "@context": "https://w3id.org/ro/crate/1.1/context",
            "@graph": []
        }));
    }
    let packed_cwl = json!({ "$graph": graph_json });
    let steps = extract_workflow_steps(&packed_cwl, workflow_file);
    let step_ids: Vec<&str> = steps.iter().map(|(_, step)| step.as_str()).collect();
    let step_files: Vec<&str> = steps.iter().map(|(file, _)| file.as_str()).collect();
    let script_structure = get_workflow_structure(&packed_cwl);
    let parts = extract_parts(&script_structure, workflow_file, rocrate_dir);
    let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
    let connections = generate_connections(&script_structure, workflow_file);
    let (name, description, license) =
        extract_or_prompt_metadata(workflow_toml, working_dir, cwl_file).await?;
        let ro_crate_metadata = create_ro_crate_metadata(
        "ro-crate-metadata.json",
        Some("./"),
        &[
            "https://w3id.org/ro/crate/1.1",
            "https://w3id.org/workflowhub/workflow-ro-crate/1.0",
        ],
    );
    let cwl_entity = create_cwl_entity(
        "https://w3id.org/workflowhub/workflow-ro-crate#cwl",
        "ComputerLanguage",
        "CWL",
        "https://w3id.org/cwl/v1.2/",
        "Common Workflow Language",
        "https://www.commonwl.org/",
        "v1.2",
    );
    let creative_works: Vec<DataEntity> =
        DEFAULT_WORKFLOW_RO_CONFORMS_TO.iter().map(|id| create_creative_work(id)).collect();
    let _parameter_connections: Vec<DataEntity> = connections
        .iter()
        .map(|(src, tgt, id)| create_parameter_connection(id, src, tgt))
        .collect();
    let mut graph: Vec<GraphVector> = Vec::new();
    graph.push(GraphVector::MetadataDescriptor(ro_crate_metadata));
    graph.push(GraphVector::DataEntity(cwl_entity));
    graph.extend(creative_works.into_iter().map(GraphVector::DataEntity));
    let mut formal_params: Vec<DataEntity> = Vec::new();
   // let empty_timestamps: StepTimestamp = HashMap::new();
    for step in script_structure.values() {
        let id = &step.id;
        let inputs = &step.inputs;
        let outputs = &step.outputs;
        let create_id = generate_id_with_hash();
        let _control_id = generate_id_with_hash();
        if is_cwl_file(id) {
            let modified_params = classify_and_prefix_params(id, inputs, outputs);
            let input_names: Vec<String> = inputs.iter().map(|(i, _)| i.clone()).collect();
            let output_names: Vec<String> = outputs.iter().map(|(i, _)| i.clone()).collect();
            let software_application = create_software_application(
                id.trim_start_matches('#'),
                &input_names,
                &output_names,
                workflow_file,
            );
            graph.push(GraphVector::DataEntity(software_application));
            let mut existing_ids: HashSet<String> =
                formal_params.iter().map(|p| p.id.clone()).collect();
            for (new_id, classification, loc) in &modified_params {
                if !existing_ids.contains(new_id) {
                    formal_params.push(create_formal_parameter(
                        new_id,
                        classification,
                        Some(loc.as_str()),
                        workflow_file,
                    ));
                    existing_ids.insert(new_id.clone());
                }
            }
        } else if id == "#main" || *id == format!("{workflow_file}#main") {
            let modified_params = classify_and_prefix_params(id, inputs, outputs);
            for (input_id, classification, _) in &modified_params {
                formal_params.push(create_formal_parameter(
                    input_id,
                    classification,
                    None,
                    workflow_file,
                ));
            }
            let input_ids: Vec<String> = inputs
                .iter()
                .map(|(i, _)| format!("{workflow_file}{id}/{i}"))
                .collect();
            let workflow_entity = create_workflow_entity(
                &connections,
                &step_ids,
                &input_ids,
                outputs,
                &step_files,
                workflow_file,
            );
            graph.push(GraphVector::DataEntity(workflow_entity.clone()));
            let root_dataset = create_root_dataset_entity(
                DEFAULT_WORKFLOW_RO_CONFORMS_TO,
                &license,
                &name,
                &description,
                &parts_ref,
                &create_id,
                workflow_file,
            );
            graph.push(GraphVector::RootDataEntity(root_dataset));
        }
    }
    graph.extend(formal_params.into_iter().map(GraphVector::DataEntity));
    let instrument_id = generate_id_with_hash();
    let version = "cwltool 3.1.20210628163208";
    let instrument = create_instrument(&instrument_id, "SoftwareApplication", version);
    graph.push(GraphVector::DataEntity(instrument));
  
    let graph_values: Vec<Value> = graph
        .into_iter()
        .map(|g| match g {
            GraphVector::MetadataDescriptor(md) => serde_json::to_value(&md).unwrap(),
            GraphVector::RootDataEntity(rde) => serde_json::to_value(&rde).unwrap(),
            GraphVector::DataEntity(de) => serde_json::to_value(&de).unwrap(),
            GraphVector::ContextualEntity(ce) => serde_json::to_value(&ce).unwrap(),
        })
        .collect();
    Ok(json!({
        "@context": [
            "https://w3id.org/ro/crate/1.1/context",
        ],
        "@graph": graph_values
    }))
}