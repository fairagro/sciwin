use serde_json::{json, to_value};
use rocraters::ro_crate::data_entity::DataEntity;
use rocraters::ro_crate::metadata_descriptor::MetadataDescriptor;

pub fn serialize_to_jsonld(
    entities: &[DataEntity],
    metadata: &MetadataDescriptor,
) -> String {
    let mut doc = json!({
        "@context": "https://w3id.org/ro/crate/1.1/context.jsonld",
        "@graph": []
    });

    let mut graph = Vec::new();

    for entity in entities {
        graph.push(to_value(entity).unwrap());
    }

    graph.push(to_value(metadata).unwrap());

    doc["@graph"] = json!(graph);
    serde_json::to_string_pretty(&doc).unwrap()
}