use chrono::Utc;
use std::collections::HashMap;
use rocraters::ro_crate::{constraints::{DataType, EntityValue, License, Id}, data_entity::DataEntity, root::RootDataEntity,
    metadata_descriptor::MetadataDescriptor};


pub fn create_root_dataset_entity(
    conforms_to: &[&str],
    license_str: &str,
    name: &str,
    description: &str,
    parts: &[&str],
    mentions: &str,
    workflow_file: &str,
) -> RootDataEntity {
    let license = License::Description(license_str.to_string());
    let has_part: Vec<EntityValue> = parts.iter()
        .map(|id| EntityValue::EntityId(Id::Id(id.to_string())))
        .collect();
    let conforms_to_vec: Vec<EntityValue> = conforms_to.iter()
        .map(|id| EntityValue::EntityId(Id::Id(id.to_string())))
        .collect();

    let mut dynamic_entity = HashMap::new();
    dynamic_entity.insert("hasPart".to_string(), EntityValue::EntityVec(has_part));
    dynamic_entity.insert("conformsTo".to_string(), EntityValue::EntityVec(conforms_to_vec));
    dynamic_entity.insert("mainEntity".to_string(), EntityValue::EntityId(Id::Id(workflow_file.to_string())));
    dynamic_entity.insert("mentions".to_string(), EntityValue::EntityId(Id::Id(mentions.to_string())));

    let now = Utc::now();
    let date_published = now.to_rfc3339();

    RootDataEntity {
        id: "./".to_string(),
        type_: DataType::Term("Dataset".to_string()),
        name: name.to_string(),
        description: description.to_string(),
        date_published,
        license,
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_creative_work(id: &str) -> DataEntity {
    let version = id.rsplit('/').next().unwrap_or(id).to_string();
    let name = if id.contains("process") {
        "Process Run Crate"
    } else if id.contains("workflow/0.5") {
        "Workflow Run Crate"
    } else if id.contains("provenance") {
        "Provenance Run Crate"
    } else if id.contains("workflow-ro-crate") {
        "Workflow RO-Crate"
    } else {
        "Unknown Crate"
    };

    let mut dynamic_entity = HashMap::new();
    dynamic_entity.insert("name".to_string(), EntityValue::EntityString(name.to_string()));
    dynamic_entity.insert("version".to_string(), EntityValue::EntityString(version));

    DataEntity {
        id: id.to_string(),
        type_: DataType::Term("CreativeWork".to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_ro_crate_metadata(
    id: &str,
    about_id: Option<&str>,
    conforms_to_ids: &[&str],
) -> MetadataDescriptor {
    let about_id = about_id.unwrap_or("./");
    let type_ = DataType::Term("CreativeWork".to_string());
    let about = Id::Id(about_id.to_string());
    let conforms_to_values: Vec<EntityValue> = conforms_to_ids
        .iter()
        .map(|uri| EntityValue::EntityId(Id::Id(uri.to_string())))
        .collect();

    let primary_conform = conforms_to_ids
        .first()
        .map(|s| Id::Id(s.to_string()))
        .unwrap_or_else(|| Id::Id("https://w3id.org/ro/crate/1.1".to_string()));

    let mut dynamic_entity = HashMap::new();
    dynamic_entity.insert("conformsTo".to_string(), EntityValue::EntityVec(conforms_to_values));

    MetadataDescriptor {
        id: id.to_string(),
        type_,
        conforms_to: primary_conform,
        about,
        dynamic_entity: Some(dynamic_entity),
    }
}

pub fn create_formal_parameter(
    id: &str,
    additional_type: &str,
    default_value: Option<&str>,
    workflow_file: &str,
) -> DataEntity {
    let fixed_id = if id.starts_with(workflow_file) {
        id.to_string()
    } else {
        format!("{workflow_file}{}", id)
    };
    let name = id.rsplit('/').next().unwrap_or(id);
    let mut dynamic_entity = HashMap::from([
        ("additionalType".to_string(), EntityValue::EntityString(additional_type.to_string())),
        ("name".to_string(), EntityValue::EntityString(name.to_string())), 
    ]);
    if let Some(default) = default_value {
        dynamic_entity.insert("defaultValue".to_string(), EntityValue::EntityString(default.to_string()));
    }
    DataEntity {
        id: fixed_id,
        type_: DataType::Term("FormalParameter".to_string()),
        dynamic_entity: Some(dynamic_entity),
    }
}