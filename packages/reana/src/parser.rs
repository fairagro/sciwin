use crate::utils::{build_inputs_cwl, build_inputs_yaml, get_all_outputs};
use anyhow::{Context, Result};
use commonwl::{
    load_doc,
    packed::{PackedCWL, pack_workflow},
    prelude::*,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_yaml::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowOutputs {
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowJson {
    pub inputs: WorkflowInputs,
    pub outputs: WorkflowOutputs,
    pub version: String,
    pub workflow: WorkflowSpec,
}

#[derive(Debug, Serialize)]
pub struct WorkflowSpec {
    pub file: String,
    pub specification: PackedCWL,
    #[serde(rename = "type")]
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowInputs {
    directories: Vec<String>,
    files: Vec<String>,
    parameters: serde_yaml::Value,
}

#[derive(Serialize, Clone, Debug)]
pub struct Parameter {
    pub r#class: String,
    pub location: String,
}

impl<'de> Deserialize<'de> for Parameter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            #[serde(rename = "class")]
            r#class: String,
            location: Option<String>,
            path: Option<String>,
        }

        let helper = Helper::deserialize(deserializer)?;
        let location = helper
            .location
            .or(helper.path)
            .ok_or_else(|| de::Error::missing_field("location or path"))?;

        Ok(Parameter {
            r#class: helper.r#class,
            location,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ParameterValue {
    Structured(Parameter),
    Scalar(String),
}

pub fn generate_workflow_json_from_cwl(file: &Path, input_file: &Option<PathBuf>) -> Result<WorkflowJson> {
    let cwl_path = file.to_str().with_context(|| format!("Invalid UTF-8 in CWL file path: {file:?}"))?;

    let inputs_yaml_data = match input_file {
        Some(yaml_file) => build_inputs_yaml(cwl_path, yaml_file).with_context(|| format!("Failed to build inputs YAML from file {yaml_file:?}"))?,
        None => build_inputs_cwl(cwl_path, None).with_context(|| format!("Failed to build inputs from CWL file '{cwl_path}'"))?,
    };

    let cwl_document = load_doc(file).map_err(|e| anyhow::anyhow!("Could not load file {file:?}: {e}"))?;
    let CWLDocument::Workflow(workflow) = cwl_document else {
        anyhow::bail!("Document is not of kind CWL Workflow {file:?}");
    };
    let specification = pack_workflow(&workflow, file, None).map_err(|e| anyhow::anyhow!("Could not pack file {file:?}: {e}"))?;

    let mut inputs_value = serde_yaml::from_value::<WorkflowInputs>(Value::Mapping(inputs_yaml_data.clone()))
        .context("Failed to deserialize inputs YAML into WorkflowInputs")?;

    let mut params = HashMap::new();
    for node in &specification.graph {
        for input in &node.inputs {
            let id = input.id.trim_start_matches('#');
            if params.contains_key(id) || matches!(input.type_, CWLType::File | CWLType::Directory) {
                continue;
            } else if let Some(default_value) = &input.default {
                params.insert(id, default_value.as_value_string());
            }
        }
    }
    if !params.is_empty()
        && let Value::Mapping(values) = &mut inputs_value.parameters
    {
        for (k, v) in params {
            if !values.contains_key(k) {
                values.insert(k.into(), v.into());
            }
        }
    }
    let output_files: Vec<String> = get_all_outputs(
        &workflow,
        &specification,
    )
    .with_context(|| {
        format!("Failed to get all outputs from CWL file '{cwl_path}'")
    })?
    .into_iter()
    .map(|(_, glob)| glob)
    .collect();

    let outputs = WorkflowOutputs { files: output_files };

    Ok(WorkflowJson {
        inputs: inputs_value,
        outputs,
        version: "0.9.4".to_string(),
        workflow: WorkflowSpec {
            file: cwl_path.to_string(),
            specification,
            r#type: "cwl".to_string(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_generate_workflow_json_from_cwl_minimal() {
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cwl_path = base_dir.join("../../testdata/hello_world/workflows/main/main.cwl");
        assert!(Path::new(&cwl_path).exists(), "Test cwl file does not exists");
        let result = generate_workflow_json_from_cwl(&cwl_path, &None);

        assert!(result.is_ok(), "Expected generation to succeed");
        let json = serde_json::to_value(result.unwrap()).unwrap();

        // Basic assertions
        assert_eq!(json["version"], "0.9.4");
        assert_eq!(json["workflow"]["type"], "cwl");
        assert_eq!(json["workflow"]["file"], cwl_path.to_str().unwrap());

        let inputs = &json["inputs"];
        assert!(inputs.is_object(), "Inputs should be an object");

        // Check 'directories'
        assert!(inputs["directories"].is_array(), "directories should be an array");
        assert_eq!(inputs["directories"].as_array().unwrap().len(), 0);

        // Check 'files'
        assert!(inputs["files"].is_array(), "files should be an array");

        // Check parameters
        let parameters = &inputs["parameters"];
        assert!(parameters.is_object(), "parameters should be an object");

        assert_eq!(parameters["population"]["class"], "File");

        // Try 'location' key, fallback to 'path'
        let population_path_value = parameters["population"].get("location").or_else(|| parameters["population"].get("path"));
        let population_path = population_path_value
            .and_then(|v| v.as_str())
            .expect("Expected parameters['population'] to have 'location' or 'path' as a string");

        assert_eq!(normalize_path(population_path), "data/population.csv");

        assert_eq!(parameters["speakers"]["class"], "File");

        let speakers_path_value = parameters["speakers"].get("location").or_else(|| parameters["speakers"].get("path"));
        let speakers_path = speakers_path_value
            .and_then(|v| v.as_str())
            .expect("Expected parameters['speakers'] to have 'location' or 'path' as a string");

        assert_eq!(normalize_path(speakers_path), "data/speakers_revised.csv");

        // Check outputs
        let outputs = &json["outputs"];
        assert!(outputs.is_object(), "Outputs should be an object");
        assert!(outputs["files"].is_array(), "outputs.files should be an array");
        assert_eq!(outputs["files"].as_array().unwrap().len(), 1);
        assert_eq!(outputs["files"][0], "results.svg");

        // Check workflow steps
        let graph = &json["workflow"]["specification"]["$graph"];
        let main = graph
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == serde_json::Value::String("#main".to_string()))
            .unwrap();
        let steps = &main["steps"];
        assert!(steps.is_array(), "Steps should be an array");

        assert!(!steps.as_array().unwrap().is_empty(), "Steps array should not be empty");

        let calculation_exists = steps.as_array().unwrap().iter().any(|step| step["id"] == "#main/calculation");
        assert!(calculation_exists, "'calculation' step is missing");

        let plot_exists = steps.as_array().unwrap().iter().any(|step| step["id"] == "#main/plot");
        assert!(plot_exists, "'plot' step is missing");
    }

    fn normalize_path(path: &str) -> String {
        Path::new(path).to_str().unwrap_or_default().replace("\\", "/")
    }

    #[test]
    fn test_generate_workflow_json_from_cwl_with_inputs_yaml() {
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../");
        let cwl_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        let inputs_yaml_path = base_dir.join("testdata/hello_world/inputs.yml");

        assert!(cwl_path.exists(), "CWL file not found at {cwl_path:?}");
        assert!(inputs_yaml_path.exists(), "Inputs YAML file not found at {inputs_yaml_path:?}");

        let result = generate_workflow_json_from_cwl(&cwl_path, &Some(inputs_yaml_path));

        assert!(result.is_ok(), "Expected generation to succeed");
        let json = serde_json::to_value(result.unwrap()).unwrap();

        assert_eq!(json["version"], "0.9.4");
        assert_eq!(json["workflow"]["type"], "cwl");
        assert_eq!(json["workflow"]["file"], cwl_path.to_str().unwrap());

        let inputs = &json["inputs"];
        assert!(inputs.is_object(), "Inputs should be an object");

        let parameters = &inputs["parameters"];
        assert!(parameters.is_object(), "parameters should be an object");
        assert_eq!(parameters["population"]["class"], "File");
        assert_eq!(
            normalize_path(parameters["population"]["location"].as_str().unwrap()),
            "data/population.csv"
        );
        assert_eq!(parameters["speakers"]["class"], "File");
        assert_eq!(
            normalize_path(parameters["speakers"]["location"].as_str().unwrap()),
            "data/speakers_revised.csv"
        );
        let outputs = &json["outputs"];
        assert!(outputs.is_object(), "Outputs should be an object");
        assert!(outputs["files"].is_array(), "outputs.files should be an array");
        assert_eq!(outputs["files"].as_array().unwrap().len(), 1);
        assert_eq!(outputs["files"][0], "results.svg");

        let cwl_files = &json["workflow"]["specification"]["$graph"];
        assert!(cwl_files.is_array(), "Steps should be an array");
        assert_eq!(cwl_files.as_array().unwrap().len(), 3);

        let graph = &json["workflow"]["specification"]["$graph"];
        let main = graph
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == serde_json::Value::String("#main".to_string()))
            .unwrap();
        let steps = &main["steps"];
        assert!(steps.is_array(), "Steps should be an array");

        assert!(!steps.as_array().unwrap().is_empty(), "Steps array should not be empty");

        let calculation_exists = steps.as_array().unwrap().iter().any(|step| step["id"] == "#main/calculation");
        assert!(calculation_exists, "'calculation' step is missing");

        let plot_exists = steps.as_array().unwrap().iter().any(|step| step["id"] == "#main/plot");
        assert!(plot_exists, "'plot' step is missing");
    }
}