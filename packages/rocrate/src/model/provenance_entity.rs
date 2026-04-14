use rocraters::ro_crate::{constraints::{DataType, EntityValue, Id}, data_entity::DataEntity};
use std::collections::HashMap;

pub struct Action<'a> {
    pub action_type: &'a str,
    pub id: &'a str,
    pub name: &'a str,
    pub instrument_id: &'a str,
    pub object_ids: Vec<String>,
    pub result_ids: Option<Vec<&'a str>>,
    pub start_time: Option<&'a str>,
    pub end_time: Option<&'a str>,
    pub container_image_id: Option<&'a str>,
}

pub fn create_action(a: Action) -> DataEntity {
    let mut dynamic_entity = HashMap::new();
    dynamic_entity.insert("name".to_string(), EntityValue::parse(a.name).unwrap());
    dynamic_entity.insert(
        "instrument".to_string(),
        EntityValue::parse(&format!(r#"{{"@id": "{}"}}"#, a.instrument_id)).unwrap(),
    );
    if !a.object_ids.is_empty() {
        let object_values: Vec<EntityValue> = a
            .object_ids
            .iter()
            .map(|id| EntityValue::parse(&format!(r#"{{"@id": "{}"}}"#, id)).unwrap())
            .collect();
        dynamic_entity.insert("object".to_string(), EntityValue::EntityVec(object_values));
    }
    if let Some(results) = a.result_ids {
        let result_values: Vec<EntityValue> = results
            .iter()
            .map(|id| EntityValue::parse(&format!(r#"{{"@id": "{}"}}"#, id)).unwrap())
            .collect();
        dynamic_entity.insert("result".to_string(), EntityValue::EntityVec(result_values));
    }
    if let Some(start) = a.start_time {
        dynamic_entity.insert("startTime".to_string(), EntityValue::parse(start).unwrap());
    }
    if let Some(end) = a.end_time {
        dynamic_entity.insert("endTime".to_string(), EntityValue::parse(end).unwrap());
    }
    if let Some(container_id) = a.container_image_id {
        dynamic_entity.insert(
            "containerImage".to_string(),
            EntityValue::parse(&format!(r#"{{"@id": "{}"}}"#, container_id)).unwrap(),
        );
    }

    DataEntity {
        id: a.id.to_string(),
        type_: DataType::Term(a.action_type.to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_container_image(
    id: &str,
    name: &str,
    tag: &str,
    registry: &str,
) -> DataEntity {
    let dynamic_entity = HashMap::from([
        (
            "additionalType".to_string(),
            EntityValue::EntityId(Id::Id(
                "https://w3id.org/ro/terms/workflow-run#DockerImage".to_string(),
            )),
        ),
        ("name".to_string(), EntityValue::EntityString(name.to_string())),
        ("registry".to_string(), EntityValue::EntityString(registry.to_string())),
        ("tag".to_string(), EntityValue::EntityString(tag.to_string())),
    ]);
    DataEntity {
        id: id.to_string(),
        type_: DataType::Term("ContainerImage".to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_howto_steps(
    steps: &[(String, String)],
    connections: &[(String, String, String)],
    id: &str,
    workflow_file: &str,
) -> Vec<DataEntity> {
    let mut result = Vec::new();
    if id == "#main" || id.ends_with("#main") {
        return result;
    }
    for (i, (step_id, step_id_match)) in steps.iter().enumerate() {
        if step_id_match == id {
            let step_connections: Vec<EntityValue> = connections
                .iter()
                .filter_map(|(_, target, conn_id)| {
                    if target.starts_with(&format!("{workflow_file}{}", id)) {
                        Some(EntityValue::parse(&format!(r#"{{"@id": "{}"}}"#, conn_id)).unwrap())
                    } else {
                        None
                    }
                })
                .collect();

            let mut dynamic_entity = HashMap::new();
            dynamic_entity.insert(
                "position".to_string(),
                EntityValue::parse(&i.to_string()).unwrap(),
            );
            dynamic_entity.insert(
                "workExample".to_string(),
                EntityValue::parse(&format!(r#"{{"@id": "{}{}"}}"#, workflow_file, id)).unwrap(),
            );
            if !step_connections.is_empty() {
                dynamic_entity.insert("connection".to_string(), EntityValue::EntityVec(step_connections));
            }

            let step_entity = DataEntity {
                id: format!(
                    "{}#main{}",
                    workflow_file,
                    step_id.replace('#', "/").trim_end_matches(".cwl")
                ),
                type_: DataType::Term("HowToStep".to_string()),
                dynamic_entity: Some(dynamic_entity),
            };
            result.push(step_entity);
        }
    }
    result
}