use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use cwl_annotation::{annotate_license, annotate_field};
use toml_edit::{DocumentMut, Item, Table, value};
use std::fs;
use std::path::{Path, PathBuf};
use std::error::Error;
use anyhow::anyhow;
use crate::utils::{prompt};
use anyhow::Result as anyhowResult;

#[derive(Debug, Clone)]
pub struct ScriptStep {
    pub id: String,
    pub inputs: Vec<(String, String)>,
    pub outputs: Vec<(String, String)>,
    pub docker: Option<String>,
}

fn extract_io(e: &Value, key: &str, val_key: &str) -> Vec<(String, String)> {
    e.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_str()?.to_string();
                    let field = if key == "inputs" { "location" } else { "glob" };
                    let val = item.get(val_key)?.get(field)?.as_str()?.to_string();
                    Some((id, val))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_docker_images(graph: &[Value]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for tool in graph.iter().filter(|e| e.get("class") == Some(&"CommandLineTool".into())) {
        if let Some(id) = tool.get("id").and_then(Value::as_str) 
        && let Some(reqs) = tool.get("requirements").and_then(Value::as_array) {
            for req in reqs {
                if req.get("class") == Some(&"DockerRequirement".into()) &&
                    let Some(image) = req.get("dockerFile").or(req.get("dockerPull")).and_then(Value::as_str) {
                    map.insert(id.to_string(), image.to_string());
                    
                }
            }
        }
    }
    map
}

fn extract_workflow_inputs(workflow_json: &Value) -> Vec<(String, String)> {
    extract_io(workflow_json, "inputs", "default")
}

fn extract_workflow_outputs(workflow_json: &Value) -> Vec<(String, String)> {
    extract_io(workflow_json, "outputs", "outputBinding")
}

pub fn get_workflow_structure(workflow_json: &Value) -> HashMap<String, ScriptStep> {
    let graph = workflow_json
        .get("$graph")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let docker_map = extract_docker_images(&graph);
    let mut steps: HashMap<String, ScriptStep> = graph
        .iter()
        .filter(|e| e.get("class") == Some(&Value::String("CommandLineTool".into())))
        .filter_map(|e| {
            let id = e.get("id")?.as_str()?.to_string();
            let inputs = extract_io(e, "inputs", "default");
            let outputs = extract_io(e, "outputs", "outputBinding");
            let docker = docker_map.get(&id).cloned();

            if inputs.is_empty() && outputs.is_empty() {
                return None;
            }

            Some((id.clone(), ScriptStep { id, inputs, outputs, docker }))
        })
        .collect();
    let wf_inputs = extract_workflow_inputs(workflow_json);
    let wf_outputs = extract_workflow_outputs(workflow_json);
    steps.insert(
        "#main".into(),
        ScriptStep {
            id: "#main".into(),
            inputs: wf_inputs,
            outputs: wf_outputs,
            docker: None,
        },
    );
    steps
}

pub fn generate_connections(script_structure: &HashMap<String, ScriptStep>, workflow_file: &str) -> Vec<(String, String, String)> {
    let mut connections = Vec::new();
    let output_file_map: HashMap<String, (String, String)> = script_structure
    .values()
    .flat_map(|step| {
        let producer_id = &step.id;
        step.outputs.iter().filter_map(move |(output_id, output_file)| {
            Path::new(output_file)
                .file_name()
                .and_then(|f| f.to_str())
                .map(|file_name| (file_name.to_string(), (output_id.clone(), producer_id.clone())))
        })
    })
    .collect();
    for step in script_structure.values() {
        for (input_id, input_file) in &step.inputs {
            if let Some(file_name) = Path::new(input_file).file_name().and_then(|f| f.to_str()) &&
            let Some((output_id, _)) = output_file_map.get(file_name) {
                let source = format!("{}#{}", workflow_file, output_id.trim_start_matches('#'));
                let target = format!("{}#{}", workflow_file, input_id.trim_start_matches('#'));
                connections.push((source, target, generate_id_with_hash()));
            }
        }
    }
    let main_step = script_structure.get("#main");
    if let Some(main) = main_step {
        let step_ports: Vec<(String, String)> = script_structure
            .values()
            .filter(|s| s.id != "#main")
            .flat_map(|s| {
                let step_id = s.id.trim_start_matches('#');
                s.inputs.iter().chain(s.outputs.iter()).map(move |(port, path)| {
                    let port_name = port.rsplit('/').next().unwrap_or(port);
                    (format!("{}#{step_id}/{port_name}", workflow_file), path.clone())
                })
            })
            .collect();
        for (name, path, is_output) in main.inputs.iter().map(|(n, p)| (n, p, false))
            .chain(main.outputs.iter().map(|(n, p)| (n, p, true)))
        {
            let main_id = format!("{}#main/{}", workflow_file, name.trim_start_matches("#main/"));
            for (step_port_id, step_path) in &step_ports {
                if path.ends_with(step_path) || step_path.ends_with(path) {
                    let (source, target) = if is_output {
                        (step_port_id.clone(), main_id.clone())
                    } else {
                        (main_id.clone(), step_port_id.clone())
                    };
                    connections.push((source, target, generate_id_with_hash()));
                }
            }
        }
    }
    connections
}

pub fn generate_id_with_hash() -> String {
    format!("#{}", Uuid::new_v4())
}

pub fn extract_parts(script_structure: &HashMap<String, ScriptStep>, workflow_file: &str, current_dir: &Path) -> Vec<String> {
    let mut parts: HashSet<String> = script_structure
        .values()
        .flat_map(|step| step.inputs.iter().chain(step.outputs.iter()))
        .filter_map(|(_, path)| {
            if path.contains("$(") {
                return None;
            }
            let cleaned = path.trim_start_matches("file://");
            let absolute = PathBuf::from(cleaned);
            let relative = absolute.strip_prefix(current_dir).unwrap_or(&absolute);
            let rel_str = relative.to_string_lossy().replace('\\', "/");
            if Path::new(&rel_str).extension().is_some() || rel_str.contains('/') {
                Some(rel_str)
            } else {
                None
            }
        })
        .collect();
    parts.insert(workflow_file.to_string());
    let mut parts_vec: Vec<String> = parts.into_iter().collect();
    parts_vec.sort();
    let parts_clone = parts_vec.clone();
    parts_vec.retain(|path| !parts_clone.iter().any(|other| other != path && path.ends_with(other)));
    parts_vec
}


pub fn classify_and_prefix_params(id: &str, inputs: &[(String, String)], outputs: &[(String, String)]) -> Vec<(String, String, String)> {
    inputs
        .iter()
        .chain(outputs.iter())
        .map(|(input_id, loc)| {
            let new_id = if input_id.starts_with(&format!("{id}/")) {
                input_id.clone()
            } else {
                format!("{id}/{input_id}")
            };
            let path = std::path::Path::new(loc);
            let classification = if path.extension().is_some() || path.is_file() {
                "File"
            } else if path.is_dir() {
                "Directory"
            } else {
                "String"
            };
            (new_id, classification.to_string(), loc.clone())
        })
        .collect()
}

//license not working 
pub async fn extract_or_prompt_metadata(
    toml_str: &str,
    _working_dir: &Path,
    workflow_name: &str
) -> Result<(String, String, String), Box<dyn Error>> {
    let mut doc: DocumentMut = toml_str.parse().unwrap_or_else(|_| "[workflow]".parse().unwrap());
    let wf_table = doc["workflow"].or_insert(Item::Table(Table::new())).as_table_mut().unwrap();
    let (mut name, mut description, mut license) = extract_metadata_from_cwl_file(workflow_name).unwrap_or((None, None, None));
    let mut updated = false;
    let mut prompt_if_missing = |key: &str, val: &mut Option<String>, fallback: &str| {
        if val.is_none() {
            let input = prompt(&format!("Enter workflow {key}: "));
            let final_val = if input.trim().is_empty() { fallback.to_string() } else { input };
            wf_table[key] = value(&final_val);
            updated = true;
            *val = Some(final_val);
        }
    };
    prompt_if_missing("name", &mut name, &format!("run of {workflow_name}"));
    prompt_if_missing("description", &mut description, &format!("run of {workflow_name}"));
    prompt_if_missing("license", &mut license, "not specified");
     if let Err(e) = annotate_field(workflow_name, "label", &name.clone().expect("name should be set")) {
        eprintln!("Warning: failed to annotate label: {:?}", e);
    }
    if let Err(e) = annotate_field(workflow_name, "doc", &description.clone().expect("description should be set")) {
        eprintln!("Warning: failed to annotate doc: {:?}", e);
    }
    annotate_license(workflow_name, &license).await?;

    if updated {
        fs::write("workflow.toml", doc.to_string()).expect("❌ Failed to write workflow.toml");
    }
    Ok((name.unwrap(), description.unwrap(), license.unwrap()))
}
    
pub fn extract_metadata_from_cwl_file(
    cwl_file: &str,
) -> anyhowResult<(Option<String>, Option<String>, Option<String>)> {
    let cwl_content = std::fs::read_to_string(cwl_file)
        .map_err(|e| anyhow!("Failed to read CWL: {}", e))?;

    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&cwl_content)
            .map_err(|e| anyhow!("Failed to parse YAML: {}", e))?;

    let label = yaml
        .get("label")
        .and_then(serde_yaml::Value::as_str)
        .map(|s| s.to_string());

    let doc = yaml
        .get("doc")
        .and_then(serde_yaml::Value::as_str)
        .map(|s| s.to_string());

    let license = match yaml.get("s:license") {
        Some(serde_yaml::Value::Sequence(seq)) => {
            let licenses: Vec<String> = seq
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(|s| s.to_string())
                .collect();
            if licenses.is_empty() { None } else { Some(licenses.join(", ")) }
        }
        _ => None,
    };
    Ok((label, doc, license))
}

pub fn extract_workflow_steps(packed_cwl: &Value, workflow_file: &str) -> Vec<(String, String)> {
    let graph: Vec<&Value> = packed_cwl
        .get("$graph")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().collect())
        .unwrap_or_default();
    let mut steps: Vec<(String, String)> = graph
        .iter()
        .filter(|node| node.get("class") == Some(&Value::String("CommandLineTool".into())))
        .filter_map(|node| {
            let id = node.get("id")?.as_str()?.to_string();
            Some((id.clone(), id))
        })
        .collect();
    if let Some(workflow_node) = graph.iter().find(|n| n.get("class") == Some(&Value::String("Workflow".into()))) {
        if let Some(id) = workflow_node.get("id").and_then(Value::as_str) {
            steps.push((id.to_string(), "#main".to_string()));
        }
    } else {
        steps.push((workflow_file.to_string(), "#main".to_string()));
    }
    steps
}

