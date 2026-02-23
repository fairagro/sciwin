use crate::api::download_files;
use crate::reana::Reana;
use chrono::Utc;
use fancy_regex::Regex;
use keyring::Entry;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use toml_edit::{DocumentMut, Item, Table, value};
use util::is_cwl_file;
use uuid::Uuid;

type ScriptStep = (String, Vec<(String, String)>, Vec<(String, String)>, Option<String>);
type StepTimestamp = HashMap<String, (Option<String>, Option<String>)>;

pub fn create_root_dataset_entity(conforms_to: &[&str], license: &str, name: &str, description: &str, parts: &[&str], mentions: &str) -> Value {
    let has_part: Vec<Value> = parts.iter().map(|id| json!({ "@id": id })).collect();
    let now = Utc::now();
    let timestamp = now.to_rfc3339();
    json!({
        "@id": "./",
        "@type": "Dataset",
        "datePublished": timestamp,
        "description": description,
        "conformsTo": conforms_to.iter().map(|id| json!({ "@id": id })).collect::<Vec<_>>(),
        "hasPart": has_part,
        "license": license,
        "mainEntity": { "@id": "workflow.json" },
        "name": name,
        "mentions": { "@id": mentions },
    })
}

fn extract_workflow_steps(json_data: &Value) -> Vec<(String, String)> {
    let mut steps = Vec::new();
    if let Some(graph) = json_data.pointer("/workflow/specification/$graph").and_then(|v| v.as_array()) {
        for item in graph {
            if item.get("class").and_then(Value::as_str) == Some("Workflow")
                && let Some(step_array) = item.get("steps").and_then(|v| v.as_array())
            {
                for step in step_array {
                    if let (Some(id), Some(run)) = (step.get("id").and_then(Value::as_str), step.get("run").and_then(Value::as_str)) {
                        steps.push((id.to_string(), run.to_string()));
                    }
                }
            }
        }
    }
    steps
}

pub fn create_workflow_entity(
    connections: &[(String, String, String)],
    step_ids: &[&str],
    input_ids: &[String],
    output_ids: &[(String, String)],
    tool_ids: &[&str],
) -> Value {
    let output_param_set: HashSet<String> = output_ids.iter().map(|(param_id, _)| format!("workflow.json{param_id}")).collect();
    let output_connections = connections
        .iter()
        .filter_map(|(_, target, conn_id)| {
            if output_param_set.contains(target) {
                Some(json!({ "@id": conn_id }))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let has_part = step_ids
        .iter()
        .filter_map(|id| id.rsplit('/').next())
        .map(|id| json!({"@id": format!("workflow.json{id}")}))
        .collect::<Vec<_>>();
    let inputs = input_ids.iter().map(|id| json!({ "@id": id })).collect::<Vec<_>>();
    let outputs = output_ids
        .iter()
        .map(|(id, _)| json!({ "@id": format!("workflow.json{id}")}))
        .collect::<Vec<_>>();
    let steps = tool_ids
        .iter()
        .map(|id| json!({ "@id": format!("workflow.json{id}") }))
        .collect::<Vec<_>>();

    json!({
        "@id": "workflow.json",
        "@type": [
            "File",
            "SoftwareSourceCode",
            "ComputationalWorkflow",
            "HowTo"
        ],
        "connection": output_connections,
        "hasPart": has_part,
        "input": inputs,
        "output": outputs,
        "name": "workflow.json",
        "programmingLanguage": {
            "@id": "https://w3id.org/workflowhub/workflow-ro-crate#cwl"
        },
        "step": steps
    })
}

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

pub fn create_action(a: Action) -> Value {
    let mut action = json!({
        "@id": a.id,
        "@type": a.action_type,
        "name": a.name,
        "instrument": { "@id": a.instrument_id }
    });
    if !a.object_ids.is_empty() {
        let objects: Vec<Value> = a.object_ids.iter().map(|id| json!({ "@id": id })).collect();
        action["object"] = if objects.len() == 1 { objects[0].clone() } else { Value::Array(objects) };
    }
    if let Some(results) = a.result_ids {
        let result_json: Vec<Value> = results.iter().map(|id| json!({ "@id": id })).collect();
        if !result_json.is_empty() {
            action["result"] = if result_json.len() == 1 {
                result_json[0].clone()
            } else {
                Value::Array(result_json)
            };
        }
    }
    if let Some(start) = a.start_time {
        action["startTime"] = json!(start);
    }
    if let Some(end) = a.end_time {
        action["endTime"] = json!(end);
    }
    if let Some(container_id) = a.container_image_id {
        action["containerImage"] = json!({ "@id": container_id });
    }
    action
}

fn create_howto_steps(steps: &[(String, String)], connections: &[(String, String, String)], id: &str) -> Vec<serde_json::Value> {
    let mut result = Vec::new();
    for (i, (step_id, step_id_match)) in steps.iter().enumerate() {
        if step_id_match == id {
            // Find connections for this step
            let step_connections: Vec<Value> = connections
                .iter()
                .filter_map(|(_, target, conn_id)| {
                    // Only include connections where target starts with this step's id
                    if target.starts_with(&format!("workflow.json{id}")) {
                        Some(json!({ "@id": conn_id }))
                    } else {
                        None
                    }
                })
                .collect();

            result.push(json!({
                "@id": format!("workflow.json{}", step_id),
                "@type": "HowToStep",
                "position": i.to_string(),
                "connection": step_connections,
                "workExample": {
                    "@id": format!("workflow.json{}", id)
                }
            }));
        }
    }
    result
}

fn create_software_application(id: &str, inputs: &[String], outputs: &[String]) -> Value {
    let formatted_inputs: Vec<Value> = inputs.iter().map(|input| json!({ "@id": format!("workflow.json{input}") })).collect();
    let formatted_outputs: Vec<Value> = outputs.iter().map(|output| json!({ "@id": format!("workflow.json{output}") })).collect();
    json!({
        "@id": format!("workflow.json#{id}"),
        "@type": "SoftwareApplication",
        "name": id,
        "input": formatted_inputs,
        "output": formatted_outputs
    })
}

fn create_parameter_connection(id: &str, source: &str, target: &str) -> Value {
    json!({
        "@id": id,
        "@type": "ParameterConnection",
        "sourceParameter": { "@id": source },
        "targetParameter": { "@id": target }
    })
}

fn create_instruments(id: &str, type_str: &str, name: &str) -> Value {
    json!({
        "@id": id,
        "@type": type_str,
        "name": name
    })
}

fn create_formal_parameter(id: &str, additional_type: &str, default_value: Option<&str>) -> Value {
    let fixed_id = if id.starts_with("workflow.json") {
        id.to_string()
    } else {
        format!("workflow.json{id}")
    };
    let name = id.rsplit('/').next().unwrap_or(id);
    let mut obj = json!({
        "@id": fixed_id,
        "@type": "FormalParameter",
        "additionalType": additional_type,
        "name": name
    });
    if let Some(default) = default_value {
        obj["defaultValue"] = json!(default);
    }
    obj
}

fn create_cwl_entity(id: &str, type_str: &str, alt_name: &str, identifier: &str, name: &str, url: &str, version: &str) -> Value {
    json!({
        "@id": id,
        "@type": type_str,
        "alternateName": alt_name,
        "identifier": { "@id": identifier },
        "name": name,
        "url": { "@id": url },
        "version": version
    })
}

//create entities for each file
pub fn create_files(connections: &[(String, String, String)], parts: &[String], graph: &[Value], rocrate_dir: &str) -> Vec<Value> {
    let mut file_entities = Vec::new();

    for (source_id, target_id, fallback_uuid) in connections {
        let name = target_id.rsplit(&['/', '#']).next().unwrap_or(target_id);
        let mut file_id = fallback_uuid.as_str();
        let mut alt_name = name;
        if let Some(part) = parts.iter().find(|p| p.contains(name)) {
            file_id = part;
            alt_name = part;
        } else if let Some(glob_or_loc) = find_glob_or_location_for_id(source_id, graph).or_else(|| find_glob_or_location_for_id(target_id, graph))
            && let Some(part) = parts.iter().find(|p| p.contains(&glob_or_loc))
        {
            file_id = part;
            alt_name = part;
        }
        // Optionally use rocrate_dir here, if needed
        let full_path = if rocrate_dir.is_empty() {
            alt_name.to_string()
        } else {
            format!("{}/{}", rocrate_dir.trim_end_matches('/'), alt_name)
        };
        let content_size = get_file_size(&full_path);
        let normalize_id = |id: &str| {
            if id.starts_with("workflow.json#") {
                id.to_string()
            } else {
                format!("workflow.json#{id}")
            }
        };
        let entity = json!({
            "@id": file_id,
            "@type": "File",
            "alternateName": alt_name,
            "contentSize": content_size,
            "exampleOfWork": [
                { "@id": normalize_id(source_id) },
                { "@id": normalize_id(target_id) }
            ],
        });
        file_entities.push(entity);
    }
    file_entities
}

//search for path of file
pub fn find_glob_or_location_for_id(target_id: &str, graph: &[Value]) -> Option<String> {
    for entry in graph {
        for key in ["outputs", "inputs"] {
            if let Some(array) = entry.get(key).and_then(|v| v.as_array()) {
                for item in array {
                    if let Some(item_id) = item.get("id").and_then(Value::as_str) {
                        let target_fragment = target_id.rsplit_once('#').map_or(target_id, |(_, frag)| frag);
                        if item_id.ends_with(target_fragment) {
                            if let Some(glob) = item.pointer("/outputBinding/glob").and_then(Value::as_str) {
                                return Some(glob.to_string());
                            }
                            if let Some(loc) = item.pointer("/default/location").and_then(Value::as_str) {
                                return Some(loc.rsplit('/').next().unwrap_or(loc).to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

//for all conforms to parts
fn create_creative_work(id: &str) -> Value {
    let version = id.rsplit('/').next().unwrap_or(id);
    let name = match id {
        s if s.contains("process") => "Process Run Crate",
        s if s.contains("workflow/0.5") => "Workflow Run Crate",
        s if s.contains("provenance") => "Provenance Run Crate",
        s if s.contains("workflow-ro-crate") => "Workflow RO-Crate",
        _ => "Unknown Crate",
    };
    json!({
        "@id": id,
        "@type": "CreativeWork",
        "name": name,
        "version": version
    })
}

fn create_ro_crate_metadata(id: &str, about_id: Option<&str>, conforms_to_ids: &[&str]) -> Value {
    let about_id = about_id.unwrap_or("./");
    let conforms_to: Vec<Value> = conforms_to_ids.iter().map(|&uri| json!({ "@id": uri })).collect();
    json!({
        "@id": id,
        "@type": "CreativeWork",
        "about": { "@id": about_id },
        "conformsTo": conforms_to
    })
}

fn generate_id_with_hash() -> String {
    format!("#{}", Uuid::new_v4())
}

pub fn extract_or_prompt_metadata(graph: &[Value], toml_str: &str) -> (String, String, String) {
    let mut doc: DocumentMut = toml_str.parse().unwrap_or_else(|_| "[workflow]".parse().unwrap());
    let workflow_table = doc
        .as_table_mut()
        .entry("workflow")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .unwrap();
    let wf_data = graph
        .iter()
        .find(|item| item.get("class").and_then(Value::as_str) == Some("Workflow") && item.get("id").and_then(Value::as_str) == Some("#main"));
    let mut updated = false;
    let mut get_or_prompt = |key: &str, fallback: &str| -> String {
        if let Some(val) = workflow_table.get(key).and_then(Item::as_str) {
            val.to_string()
        } else {
            let prompt_val = prompt(&format!("Enter workflow {key}: "));
            let final_val = if prompt_val.trim().is_empty() {
                fallback.to_string()
            } else {
                prompt_val
            };
            workflow_table[key] = value(&final_val);
            updated = true;
            final_val
        }
    };
    let name = get_or_prompt(
        "name",
        wf_data
            .and_then(|w| w.get("name").and_then(Value::as_str))
            .unwrap_or("run of workflow.json"),
    );
    let description = get_or_prompt(
        "description",
        wf_data
            .and_then(|w| w.get("description").and_then(Value::as_str))
            .unwrap_or("run of workflow.json"),
    );
    let license = get_or_prompt(
        "license",
        wf_data.and_then(|w| w.get("license").and_then(Value::as_str)).unwrap_or("not specified"),
    );
    if updated {
        std::fs::write("workflow.toml", doc.to_string()).expect("❌ Failed to write updated workflow.toml");
    }

    (name, description, license)
}

#[allow(clippy::disallowed_macros)]
fn prompt(message: &str) -> String {
    print!("{message}");
    std::io::stdout().flush().expect("Failed to flush stdout");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).expect("Failed to read input");
    input.trim().to_string()
}

//get all files
fn extract_parts(script_structure: &[ScriptStep]) -> Vec<String> {
    let mut parts = HashSet::new();
    for (_, inputs, outputs, _) in script_structure {
        for (_, path) in inputs.iter().chain(outputs.iter()) {
            if let Some(file_name) = std::path::Path::new(path).file_name().and_then(|f| f.to_str()) {
                parts.insert(file_name.to_string());
            }
        }
    }
    parts.insert("workflow.json".to_string());
    let mut parts_vec: Vec<String> = parts.into_iter().collect();
    parts_vec.sort();
    parts_vec
}

//use reana log files to extract start and end times of stepts
fn extract_times_from_logs(contents: &str) -> Result<StepTimestamp, Box<dyn std::error::Error>> {
    let re_timestamp = Regex::new(r"(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})").unwrap();
    let re_workflow_start = Regex::new(r"running workflow on context").unwrap();
    let re_workflow_end = Regex::new(r"workflow done").unwrap();
    let re_step_start = Regex::new(r"starting step (?P<step>\w+)").unwrap();
    let re_step_end = Regex::new(r"\[step (?P<step>\w+)\] completed success").unwrap();
    let re_step_start2 =
        Regex::new(r"(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3}).*\[\s*workflow\s*\]\s*starting step (?P<step>[a-zA-Z0-9_.-/]+)")?;
    let re_step_end2 =
        Regex::new(r"(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3}).*\[step (?P<step>[a-zA-Z0-9_.-/]+)\] completed success")?;

    let mut workflow_start = None;
    let mut workflow_end = None;
    let mut steps: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    for line in contents.lines() {
        if let Some(cap) = re_timestamp.captures(line)? {
            let timestamp = cap["timestamp"].to_string();
            if re_workflow_start.is_match(line)? {
                workflow_start = Some(timestamp.clone());
            }
            if re_workflow_end.is_match(line)? {
                workflow_end = Some(timestamp.clone());
            }
            if let Some(cap_step) = re_step_start.captures(line)? {
                let step = cap_step["step"].to_string();
                steps.entry(step).or_insert((None, None)).0 = Some(timestamp.clone());
            }
            if let Some(cap_step) = re_step_start2.captures(line)? {
                let step = cap_step["step"].to_string();
                steps.entry(step).or_insert((None, None)).0 = Some(timestamp.clone());
            }
            if let Some(cap_step) = re_step_end.captures(line)? {
                let step = cap_step["step"].to_string();
                steps.entry(step).or_insert((None, None)).1 = Some(timestamp.clone());
            }
            if let Some(cap_step) = re_step_end2.captures(line)? {
                let step = cap_step["step"].to_string();
                steps.entry(step).or_insert((None, None)).1 = Some(timestamp.clone());
            }
        }
    }
    steps.insert("workflow".to_string(), (workflow_start, workflow_end));
    Ok(steps)
}

fn get_file_size(path: &str) -> String {
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().to_string()
    } else {
        "unknown".to_string()
    }
}

pub fn get_workflow_structure(workflow_json: &Value) -> Vec<ScriptStep> {
    let mut results = Vec::new();
    let mut docker_map = HashMap::new();
    let elements = workflow_json
        .pointer("/workflow/specification/$graph")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // docker info for CommandLineTool
    for e in &elements {
        if e.get("class") == Some(&Value::String("CommandLineTool".into()))
            && let Some(id) = e.get("id").and_then(Value::as_str)
        {
            let docker = e.get("requirements").and_then(Value::as_array).and_then(|r| {
                r.iter().find_map(|req| {
                    (req.get("class") == Some(&Value::String("DockerRequirement".into())))
                        .then(|| req.get("dockerPull")?.as_str())
                        .flatten()
                })
            });
            if let Some(img) = docker {
                docker_map.insert(id.to_string(), img.to_string());
            }
        }
    }
    // inputs and outputs
    for e in &elements {
        if e.get("class") == Some(&Value::String("CommandLineTool".into())) {
            let id = e.get("id").and_then(Value::as_str).unwrap_or("unknown").to_string();
            let io = |key: &str, val_key: &str| {
                e.get(key)
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let k = item.get("id")?.as_str()?.to_string();
                                let v = item
                                    .get(val_key)?
                                    .get(if key == "inputs" { "location" } else { "glob" })?
                                    .as_str()?
                                    .to_string();
                                Some((k, v))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let inputs: Vec<_> = io("inputs", "default");
            let outputs = io("outputs", "outputBinding");
            let docker = docker_map.get(&id).cloned();
            if !inputs.is_empty() || !outputs.is_empty() {
                results.push((id, inputs, outputs, docker));
            }
        }
    }
    // Workflow inputs
    let wf_inputs = workflow_json
        .pointer("/inputs/parameters")
        .and_then(Value::as_object)
        .map(|p| {
            p.iter()
                .filter_map(|(k, v)| v.get("location").and_then(Value::as_str).map(|loc| (k.clone(), loc.to_string())))
                .collect()
        })
        .unwrap_or_default();
    // Workflow outputs
    let wf_outputs = workflow_json
        .pointer("/workflow/specification/$graph")
        .and_then(Value::as_array)
        .and_then(|g| g.iter().find(|n| n.get("class") == Some(&Value::String("Workflow".into()))))
        .and_then(|wf| wf.get("outputs").and_then(Value::as_array))
        .map(|outs| {
            let empty = vec![];
            let files = workflow_json.pointer("/outputs/files").and_then(Value::as_array).unwrap_or(&empty);
            outs.iter()
                .zip(files)
                .filter_map(|(o, f)| {
                    let full_id = o.get("id")?.as_str()?;
                    Some((full_id.to_string(), f.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    results.push(("#main".into(), wf_inputs, wf_outputs, None));
    results
}

fn create_container_image(id: &str, name: &str, tag: &str, registry: &str) -> Value {
    json!({
        "@id": id,
        "@type": "ContainerImage",
        "additionalType": {
            "@id": "https://w3id.org/ro/terms/workflow-run#DockerImage"
        },
        "name": name,
        "registry": registry,
        "tag": tag
    })
}

//find connections of inputs, outputs and between workflow steps/CommandLineTools
fn generate_connections(script_structure: &[ScriptStep]) -> Vec<(String, String, String)> {
    let mut connections = Vec::new();

    // Map output file names to (output_param_id, producer_id)
    let mut output_file_map: HashMap<String, (String, String)> = HashMap::new();
    for (producer_id, _, producer_outputs, _) in script_structure {
        for (output_param_id, output_file) in producer_outputs {
            if let Some(file_name) = std::path::Path::new(output_file).file_name().and_then(|f| f.to_str()) {
                output_file_map.insert(file_name.to_string(), (output_param_id.clone(), producer_id.clone()));
            }
        }
    }

    // For each input, find matching output by file name
    for (_consumer_id, consumer_inputs, _, _) in script_structure {
        for (input_param_id, input_file) in consumer_inputs {
            if let Some(input_file_name) = std::path::Path::new(input_file).file_name().and_then(|f| f.to_str())
                && let Some((output_param_id, _producer_id)) = output_file_map.get(input_file_name)
            {
                let source = format!("workflow.json#{}", output_param_id.trim_start_matches('#'));
                let target = format!("workflow.json#{}", input_param_id.trim_start_matches('#'));
                connections.push((source, target, generate_id_with_hash()));
            }
        }
    }

    // Handle main workflow inputs/outputs connections to/from steps
    let (main_steps, other_steps): (Vec<_>, Vec<_>) = script_structure.iter().partition(|(id, _, _, _)| id == "#main");
    if let Some((_, main_inputs, main_outputs, _)) = main_steps.first() {
        // Collect all step ports (inputs and outputs)
        let step_ports: Vec<(String, String)> = other_steps
            .iter()
            .flat_map(|(id, inputs, outputs, _)| {
                let step_id = id.trim_start_matches('#');
                inputs.iter().chain(outputs).map(move |(port, path)| {
                    let port_name = port.rsplit('/').next().unwrap_or(port);
                    (format!("workflow.json#{step_id}/{port_name}"), path.clone())
                })
            })
            .collect();

        // Connect main inputs to step inputs, and step outputs to main outputs
        for (name, path, is_output) in main_inputs
            .iter()
            .map(|(n, p)| (n, p, false))
            .chain(main_outputs.iter().map(|(n, p)| (n, p, true)))
        {
            let main_id = if name.starts_with("#main/") {
                format!("workflow.json#main/{}", name.trim_start_matches("#main/"))
            } else {
                format!("workflow.json#main/{name}")
            };
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

fn classify_and_prefix_params(id: &str, inputs: &[(String, String)], outputs: &[(String, String)]) -> Vec<(String, String, String)> {
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

fn get_or_prompt_credential(service: &str, key: &str, prompt_msg: &str) -> Result<String, Box<dyn std::error::Error>> {
    let entry = Entry::new(service, key)?;

    match entry.get_password() {
        Ok(val) => Ok(val),
        Err(keyring::Error::NoEntry) => {
            let value = prompt(prompt_msg);
            entry.set_password(&value)?;
            Ok(value)
        }
        Err(e) => Err(Box::new(e)),
    }
}

//create rocrate folder with ro-crate-metadata.json and other input, output and intermediate result files
pub fn create_ro_crate(
    workflow_json: &serde_json::Value,
    logs_str: &str,
    conforms_to: &[&str],
    rocrate_dir: Option<String>,
    workspace_files: &[String],
    workflow_name: &str,
    workflow_toml: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let folder_name = rocrate_dir.unwrap_or_else(|| "rocrate".to_string());
    std::fs::create_dir_all(&folder_name)?;
    // generate RO-Crate metadata
    let mut ro_crate_metadata_json = create_ro_crate_metadata_json(workflow_json, logs_str, conforms_to, workflow_toml, &folder_name)?;
    let metadata_path = std::path::Path::new(&folder_name).join("ro-crate-metadata.json");
    let mut file = std::fs::File::create(&metadata_path)?;
    let metadata_str = serde_json::to_string_pretty(&ro_crate_metadata_json)?;
    file.write_all(metadata_str.as_bytes())?;
    if let Some(graph) = ro_crate_metadata_json.get_mut("@graph").and_then(|g| g.as_array_mut()) {
        for entity in graph {
            if let Some(default_value) = entity.get_mut("defaultValue")
                && let Some(path_str) = default_value.as_str()
                && let Some(stripped) = path_str.strip_prefix("file://")
            {
                let path = std::path::Path::new(stripped);
                if path.exists()
                    && path.is_file()
                    && let Some(file_name) = path.file_name().and_then(|f| f.to_str())
                {
                    let target_path = std::path::Path::new(&folder_name).join(file_name);
                    std::fs::copy(path, &target_path)?;
                    *default_value = Value::String(file_name.to_string());
                }
            }
        }
    }
    // Collect parts
    let mut found_paths = Vec::new();
    if let Some(graph) = ro_crate_metadata_json.get("@graph").and_then(|g| g.as_array())
        && let Some(main_entity) = graph.iter().find(|e| e.get("@id") == Some(&Value::String("./".to_string())))
        && let Some(parts) = main_entity.get("hasPart").and_then(|p| p.as_array())
    {
        for entry in parts {
            if let Some(path_value) = entry.get("@id") {
                if let Some(path_str) = path_value.as_str() {
                    let local_path = std::path::PathBuf::from(&folder_name).join(path_str);
                    //already in rocrate folder, skip
                    if local_path.exists() {
                        continue;
                    //check if there is a file in the workspace that has the same ending
                    } else if let Some(matching_file) = workspace_files
                        .iter()
                        .find(|wf| std::path::Path::new(wf).file_name().is_some_and(|f| f == path_str))
                    {
                        found_paths.push(matching_file.clone());
                    }
                }
            } else {
                eprintln!("⚠️ No 'hasPart' field found in entry: {entry}");
            }
        }
    }
    let reana_instance = get_or_prompt_credential("reana", "instance", "Enter REANA instance URL: ")?;
    let reana_token = get_or_prompt_credential("reana", "token", "Enter REANA access token: ")?;
    let reana = Reana::new(reana_instance, reana_token);
    //download intermediate outputs that were found
    download_files(&reana, workflow_name, &found_paths, Some(&folder_name))?;

    Ok(())
}

//create ro_crate_metadata.json file
pub fn create_ro_crate_metadata_json(
    json_data: &serde_json::Value,
    logs: &str,
    conforms_to: &[&str],
    workflow_toml: &str,
    rocrate_dir: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let graph_json = json_data
        .get("workflow")
        .and_then(|w| w.get("specification"))
        .and_then(|s| s.get("$graph"))
        .ok_or("Missing '$graph' field in workflow specification")?
        .as_array()
        .ok_or("'$graph' must be an array")?;
    // if $graph is empty
    if graph_json.is_empty() {
        return Ok(json!({
            "@context": "https://w3id.org/ro/crate/1.1/context",
            "@graph": []
        }));
    }
    // extract connections, steps, parts, etc
    let steps = extract_workflow_steps(json_data);
    let step_ids: Vec<&str> = steps.iter().map(|(_, step)| step.as_str()).collect();
    let step_files: Vec<&str> = steps.iter().map(|(file, _)| file.as_str()).collect();
    let script_structure = get_workflow_structure(json_data);
    let parts = extract_parts(&script_structure);
    let connections = generate_connections(&script_structure);
    let connections_slice: &[(String, String, String)] = connections.as_slice();
    let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
    let (name, description, license) = extract_or_prompt_metadata(graph_json, workflow_toml);

    // create main rocrate metadata and CWL entity
    let ro_crate_metadata = create_ro_crate_metadata(
        "ro-crate-metadata.json",
        Some("./"),
        &["https://w3id.org/ro/crate/1.1", "https://w3id.org/workflowhub/workflow-ro-crate/1.0"],
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
    let creative_works: Vec<Value> = conforms_to.iter().map(|id| create_creative_work(id)).collect();
    // create parameter connections from connection triples
    let parameter_connections: Vec<Value> = connections
        .iter()
        .map(|(source, target, id)| create_parameter_connection(id, source, target))
        .collect();
    let mut graph = vec![ro_crate_metadata, cwl_entity];
    graph.extend(creative_works);
    // extract timestamps from logs for action timing
    let timestamps = extract_times_from_logs(logs)?;
    let mut organize_obj_ids = Vec::new();
    let mut organize_res_ids = Vec::new();
    let mut formal_params: Vec<Value> = Vec::new();
    for (id, inputs, outputs, docker) in &script_structure {
        let script_name = id.trim_start_matches('#').rsplit('/').next().unwrap_or(id).to_string();
        let create_id = generate_id_with_hash();
        let control_id = generate_id_with_hash();
        let mut docker_id = None;
        //if CommandLineTool
        let step_name = if is_cwl_file(id) {
            organize_obj_ids.push(control_id.clone());
            //create docker_entity
            if let Some(docker_pull) = docker {
                let parts: Vec<&str> = docker_pull.split(':').collect();
                if parts.len() != 2 {
                    return Err("Invalid Docker image format, expected 'name:tag'".into());
                }
                docker_id = Some(generate_id_with_hash());
                let docker_entity = create_container_image(docker_id.as_ref().unwrap(), parts[0], parts[1], "docker.io");
                graph.push(docker_entity);
                let modified_params = classify_and_prefix_params(id, inputs, outputs);
                let input_names: Vec<String> = inputs.iter().map(|(i, _)| i.to_string()).collect();
                let output_names: Vec<String> = outputs.iter().map(|(i, _)| i.to_string()).collect();
                let software_application = create_software_application(&script_name, &input_names, &output_names);
                graph.push(software_application);
                let mut existing_ids: HashSet<String> = formal_params
                    .iter()
                    .filter_map(|param| param.get("@id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    .collect();

                for (new_id, classification, loc) in &modified_params {
                    if !existing_ids.contains(new_id) {
                        formal_params.push(create_formal_parameter(new_id, classification, Some(loc.as_str())));
                        existing_ids.insert(new_id.clone());
                    }
                }
            }
            id.trim_start_matches('#')
                .trim_end_matches(".cwl")
                .rsplit('/')
                .next()
                .unwrap_or(id)
                .to_string()
        }
        //if main/Workflow
        else if id == "workflow.json" || id == "#main" {
            let modified_params = classify_and_prefix_params(id, inputs, outputs);
            for (input_id, classification, _) in &modified_params {
                formal_params.push(create_formal_parameter(input_id, classification, None));
            }
            let input_ids: Vec<String> = inputs.iter().map(|(i, _)| format!("workflow.json{id}/{i}")).collect();
            //create workflow_entity
            let workflow_entity = create_workflow_entity(&connections, &step_ids, &input_ids, outputs, &step_files);
            graph.push(workflow_entity);
            organize_res_ids.push(create_id.clone());
            //create root_dataset
            let root_dataset = create_root_dataset_entity(conforms_to, &license, &name, &description, &parts_ref, &create_id);
            graph.push(root_dataset);
            "workflow".to_string()
        } else {
            continue;
        };
        let how_to_steps = create_howto_steps(&steps, connections_slice, id);
        graph.extend(how_to_steps);

        let step_opt = steps.iter().find(|(_, step_id)| *step_id == *id).map(|(file, _)| file.to_string());
        let (start, end) = if step_name == "workflow" {
            timestamps.get("workflow").cloned().unwrap_or((None, None))
        } else {
            timestamps.get(&step_name).cloned().unwrap_or((None, None))
        };
        let output_refs: Vec<&str> = outputs.iter().map(|(_, v)| v.as_str()).collect();
        let input_file_names: Vec<String> = inputs
            .iter()
            .filter_map(|(_, path)| {
                let clean_path = path.strip_prefix("file://").unwrap_or(path);
                std::path::Path::new(clean_path)
                    .file_name()
                    .and_then(|os_str| os_str.to_str())
                    .map(String::from)
            })
            .collect();
        let instrument_id = if step_name == "workflow" {
            "workflow.json".to_string()
        } else {
            format!("workflow.json{id}")
        };
        //createAction
        let create = Action {
            action_type: "CreateAction",
            id: &create_id,
            name: &format!("Run of workflow.json{}", step_opt.as_deref().unwrap_or("")),
            instrument_id: &instrument_id,
            object_ids: input_file_names,
            result_ids: Some(output_refs),
            start_time: start.as_deref(),
            end_time: end.as_deref(),
            container_image_id: docker_id.as_deref(),
        };
        //ControlAction
        let create_action_obj = create_action(create);
        if let Some(step_val) = step_opt {
            let control_action = Action {
                action_type: "ControlAction",
                id: &control_id,
                name: &format!("orchestrate {}", id.strip_prefix("#").unwrap_or(id)),
                instrument_id: &format!("workflow.json{step_val}"),
                object_ids: vec![create_id.clone()],
                result_ids: None,
                start_time: None,
                end_time: None,
                container_image_id: None,
            };
            graph.push(create_action(control_action));
        }
        graph.push(create_action_obj);
    }
    graph.extend(formal_params);
    graph.extend(parameter_connections);
    // Extract instrument version from logs
    let re = Regex::new(r"cwltool (\d+\.\d+\.\d+)")?;
    let instrument_id = generate_id_with_hash();
    if let Some(caps) = re.captures(logs)? {
        let version = format!("cwltool {}", &caps[1].split_whitespace().next().unwrap_or(&caps[1]));
        //add instrument
        let instrument = create_instruments(&instrument_id, "SoftwareApplication", &version);
        graph.push(instrument.clone());
        //OrganizeAction
        let organize_id = generate_id_with_hash();
        let organize_res_str: Vec<&str> = organize_res_ids.iter().map(String::as_str).collect();
        let (start_org, end_org) = timestamps.get("workflow").cloned().unwrap_or((None, None));
        let organize_action = Action {
            action_type: "OrganizeAction",
            id: &organize_id,
            name: &format!("Run of {version}"),
            instrument_id: &instrument_id,
            object_ids: organize_obj_ids,
            result_ids: Some(organize_res_str),
            start_time: start_org.as_deref(),
            end_time: end_org.as_deref(),
            container_image_id: None,
        };
        //files entity with alternateName
        let files = create_files(connections_slice, &parts, graph_json, rocrate_dir);
        graph.extend(files);
        graph.push(create_action(organize_action));
    }
    Ok(json!({
        "@context": ["https://w3id.org/ro/crate/1.1/context","https://w3id.org/ro/terms/workflow-run"],
        "@graph": graph
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;

    //uuids and datePublished differ: remove datePublished and replace uuid by other id that keeps track of ordering
    fn normalize_uuids_and_strip_date_published(
        value: &Value,
        uuid_map: &mut HashMap<String, String>,
        uuid_re: &Regex,
        counter: &mut usize,
    ) -> Value {
        match value {
            Value::String(s) => {
                if uuid_re.is_match(s).unwrap_or(false) {
                    let entry = uuid_map.entry(s.clone()).or_insert_with(|| {
                        let label = format!("UUID-{counter}");
                        *counter += 1;
                        label
                    });
                    Value::String(entry.clone())
                } else {
                    Value::String(s.clone())
                }
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .map(|v| normalize_uuids_and_strip_date_published(v, uuid_map, uuid_re, counter))
                    .collect(),
            ),
            Value::Object(map) => {
                let new_map = map
                    .iter()
                    .filter(|(k, _)| k != &"datePublished")
                    .map(|(k, v)| (k.clone(), normalize_uuids_and_strip_date_published(v, uuid_map, uuid_re, counter)))
                    .collect();
                Value::Object(new_map)
            }
            _ => value.clone(),
        }
    }

    #[test]
    fn test_workflow_structure_similarity() -> Result<(), Box<dyn std::error::Error>> {
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workflow_path = base_dir.join("testdata/workflow.json");
        let logs_path = base_dir.join("testdata/reana_logs.txt");
        let expected_json_path = base_dir.join("testdata/ro-crate-metadata.json");
        assert!(workflow_path.exists());
        assert!(logs_path.exists());
        assert!(expected_json_path.exists());
        let workflow_json_str = std::fs::read_to_string(&workflow_path).unwrap();
        let workflow_json: Value = serde_json::from_str(&workflow_json_str).unwrap();
        let logs_str = std::fs::read_to_string(&logs_path).unwrap();
        let expected_str = std::fs::read_to_string(&expected_json_path).unwrap();
        let expected_json: Value = serde_json::from_str(&expected_str)?;

        let conforms_to = [
            "https://w3id.org/ro/wfrun/process/0.5",
            "https://w3id.org/ro/wfrun/workflow/0.5",
            "https://w3id.org/ro/wfrun/provenance/0.5",
            "https://w3id.org/workflowhub/workflow-ro-crate/1.0",
        ];
        let workflow_toml = r#"[workflow]
                            name = "hello_s4n"
                            version = "0.1.0"
                            description = "some test workflow"
                            license = "https://spdx.org/licenses/CC-BY-4.0.html"
                            [reana]
                            "#;
        let folder_name = "rocrate";
        // generate rocrate
        let result =
            create_ro_crate_metadata_json(&workflow_json, &logs_str, &conforms_to, workflow_toml, folder_name).expect("Function should return Ok");
        let generated_json: Value = serde_json::to_value(result)?;

        let uuid_re = Regex::new(r"#?[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap();

        let mut uuid_map1 = HashMap::new();
        let mut counter1 = 0;
        let normalized_expected = normalize_uuids_and_strip_date_published(&expected_json, &mut uuid_map1, &uuid_re, &mut counter1);

        let mut uuid_map2 = HashMap::new();
        let mut counter2 = 0;
        let normalized_generated = normalize_uuids_and_strip_date_published(&generated_json, &mut uuid_map2, &uuid_re, &mut counter2);

        //compare expected and generated json files with replaced uuids
        assert_eq!(normalized_expected, normalized_generated, "structures do not match");
        Ok(())
    }

    #[test]
    fn test_create_ro_crate_metadata_json_with_empty_graph() {
        let input_json = json!({
            "workflow": {
                "specification": {
                    "$graph": []
                }
            }
        });

        let logs = "";
        let conforms_to = &[];
        let workflow_toml = r#"[workflow]
                                    name = "hello_s4n"
                                    version = "0.1.0"
                                    [reana]
                                    "#;
        let folder_name = "rocrate";
        let result = create_ro_crate_metadata_json(&input_json, logs, conforms_to, workflow_toml, folder_name);

        assert!(result.is_ok(), "Function should succeed even with empty $graph");

        let output = result.unwrap();

        assert!(output.is_object(), "Output should be a JSON object");

        let graph = output.get("@graph").unwrap_or(&json!(null));
        assert!(graph.is_array(), "@graph should be an array");
    }
}
