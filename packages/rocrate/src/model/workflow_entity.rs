use rocraters::ro_crate::{constraints::{DataType, EntityValue, Id}, data_entity::DataEntity};
use std::collections::{HashMap, HashSet};

pub fn create_workflow_entity(
    connections: &[(String, String, String)],
    step_ids: &[&str],
    input_ids: &[String],
    output_ids: &[(String, String)],
    tool_ids: &[&str],
    workflow_file: &str,
) -> DataEntity {
    let output_param_set: HashSet<String> = output_ids
        .iter()
        .map(|(param_id, _)| format!("{workflow_file}{param_id}"))
        .collect();

    let output_connections: Vec<EntityValue> = connections
        .iter()
        .filter_map(|(_, target, conn_id)| {
            if output_param_set.contains(target) {
                Some(EntityValue::EntityId(Id::Id(conn_id.clone())))
            } else {
                None
            }
        })
        .collect();

    let has_part: Vec<EntityValue> = step_ids
        .iter()
        .filter(|id| !id.trim_start_matches('#').starts_with("main"))
        .filter_map(|id| id.rsplit('/').next())
        .map(|id| EntityValue::EntityId(Id::Id(format!("{workflow_file}{id}"))))
        .collect();

    let inputs: Vec<EntityValue> = input_ids
        .iter()
        .map(|id| EntityValue::EntityId(Id::Id(id.clone())))
        .collect();

    let outputs: Vec<EntityValue> = output_ids
        .iter()
        .map(|(id, _)| EntityValue::EntityId(Id::Id(format!("{workflow_file}#{}", id.trim_start_matches('#').trim_end_matches(".cwl")))))
        .collect();

    let steps: Vec<EntityValue> = tool_ids
        .iter()
        .map(|id| EntityValue::EntityId(Id::Id(format!("{workflow_file}#main/{}", id.trim_start_matches('#').trim_end_matches(".cwl")))))
        .collect();

    let mut dynamic_entity = HashMap::new();
    if !output_connections.is_empty() {
        dynamic_entity.insert("connection".into(), EntityValue::EntityVec(output_connections));
    }
    dynamic_entity.insert("hasPart".into(), EntityValue::EntityVec(has_part));
    dynamic_entity.insert("input".into(), EntityValue::EntityVec(inputs));
    dynamic_entity.insert("output".into(), EntityValue::EntityVec(outputs));
    dynamic_entity.insert("step".into(), EntityValue::EntityVec(steps));
    dynamic_entity.insert("name".into(), EntityValue::EntityString(workflow_file.to_string()));

    let programming_language = EntityValue::EntityId(Id::Id(
        "https://w3id.org/workflowhub/workflow-ro-crate#cwl".to_string(),
    ));
    dynamic_entity.insert("programmingLanguage".into(), programming_language);

    let types = vec![
        EntityValue::EntityDataType(DataType::Term("File".to_string())),
        EntityValue::EntityDataType(DataType::Term("SoftwareSourceCode".to_string())),
        EntityValue::EntityDataType(DataType::Term("ComputationalWorkflow".to_string())),
        EntityValue::EntityDataType(DataType::Term("HowTo".to_string())),
    ];

    DataEntity {
        id: workflow_file.to_string(),
        type_: DataType::Term("WorkflowEntity".to_string()),
        dynamic_entity: {
            let mut de = dynamic_entity;
            de.insert("@type".into(), EntityValue::EntityVec(types));
            Some(de)
        },
    }
}

pub fn create_software_application(
    id: &str,
    inputs: &[String],
    outputs: &[String],
    workflow_file: &str,
) -> DataEntity {
    let formatted_inputs: Vec<EntityValue> = inputs
        .iter()
        .map(|input| EntityValue::EntityId(Id::Id(format!("{workflow_file}{}", input))))
        .collect();
    let formatted_outputs: Vec<EntityValue> = outputs
        .iter()
        .map(|output| EntityValue::EntityId(Id::Id(format!("{workflow_file}{}", output))))
        .collect();

    let mut dynamic_entity = HashMap::new();
    dynamic_entity.insert("name".into(), EntityValue::EntityString(id.to_string()));
    dynamic_entity.insert("input".into(), EntityValue::EntityVec(formatted_inputs));
    dynamic_entity.insert("output".into(), EntityValue::EntityVec(formatted_outputs));

    DataEntity {
        id: format!("{workflow_file}#{id}"),
        type_: DataType::Term("SoftwareApplication".to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_parameter_connection(id: &str, source: &str, target: &str) -> DataEntity {
    let dynamic_entity = HashMap::from([
        ("sourceParameter".to_string(), EntityValue::EntityId(Id::Id(source.to_string()))),
        ("targetParameter".to_string(), EntityValue::EntityId(Id::Id(target.to_string()))),
    ]);
    DataEntity {
        id: id.to_string(),
        type_: DataType::Term("ParameterConnection".to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_cwl_entity(
    id: &str,
    type_str: &str,
    alt_name: &str,
    identifier: &str,
    name: &str,
    url: &str,
    version: &str,
) -> DataEntity {
    let dynamic_entity = HashMap::from([
        ("alternateName".to_string(), EntityValue::EntityString(alt_name.to_string())),
        ("identifier".to_string(), EntityValue::EntityId(Id::Id(identifier.to_string()))),
        ("name".to_string(), EntityValue::EntityString(name.to_string())),
        ("url".to_string(), EntityValue::EntityId(Id::Id(url.to_string()))),
        ("version".to_string(), EntityValue::EntityString(version.to_string())),
    ]);
    DataEntity {
        id: id.to_string(),
        type_: DataType::Term(type_str.to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_instrument(id: &str, type_str: &str, name: &str) -> DataEntity {
    let dynamic_entity = HashMap::from([("name".to_string(), EntityValue::EntityString(name.to_string()))]);
    DataEntity {
        id: id.to_string(),
        type_: DataType::Term(type_str.to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}