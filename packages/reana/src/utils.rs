use anyhow::{Context, Result, anyhow};
use commonwl::StringOrDocument;
use commonwl::Workflow;
use serde_yaml::Mapping;
use serde_yaml::Value;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

pub fn sanitize_path(path: &str) -> String {
    let path = Path::new(path.trim());
    let mut sanitized_path = PathBuf::new();

    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                sanitized_path.pop();
            }
            _ => {
                sanitized_path.push(comp.as_os_str());
            }
        }
    }
    sanitized_path.to_string_lossy().replace("\\", std::path::MAIN_SEPARATOR_STR)
}

pub fn get_location(base_path: &str, cwl_file_path: &Path) -> Result<String> {
    let base_path = Path::new(base_path);
    let base_dir = base_path.parent().unwrap_or(base_path);

    let mut combined_path = base_dir.to_path_buf();

    for component in cwl_file_path.components() {
        match component {
            Component::Normal(name) => {
                combined_path.push(name);
            }
            Component::ParentDir => {
                combined_path = combined_path
                    .parent()
                    .map(PathBuf::from)
                    .with_context(|| format!("Cannot navigate above root from path: {}", combined_path.display()))?;
            }
            _ => {}
        }
    }
    combined_path
        .to_str()
        .map(|s| s.to_string())
        .with_context(|| format!("Failed to convert path to string: {}", combined_path.display()))
}

pub fn find_common_directory(paths: &BTreeSet<PathBuf>) -> Result<PathBuf> {
    if paths.is_empty() {
        return Err(anyhow!("No paths provided to compute common directory."));
    }

    let components: Vec<Vec<Component>> = paths.iter().map(|p| p.components().collect()).collect();

    let first = &components[0];
    let mut common_path = PathBuf::new();

    for (i, part) in first.iter().enumerate() {
        let all_match = components.iter().all(|c| c.get(i) == Some(part));

        if all_match {
            common_path.push(part.as_os_str());
        } else {
            break;
        }
    }
    if common_path.as_os_str().is_empty() {
        return Err(anyhow!("Could not determine a common directory among the given paths."));
    }

    Ok(common_path)
}

pub fn remove_files_contained_in_directories(files: &mut HashSet<String>, directories: &HashSet<String>) {
    let mut to_remove = Vec::new();

    for file in files.iter() {
        for dir in directories {
            if file.starts_with(dir) {
                to_remove.push(file.clone());
                break;
            }
        }
    }

    for file in to_remove {
        files.remove(&file);
    }
}

pub fn file_matches(requested_file: &str, candidate_path: &str) -> bool {
    Path::new(requested_file)
        .file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|file_name| candidate_path.ends_with(file_name))
}

pub fn collect_files_recursive(dir: &Path, files: &mut HashSet<String>) -> Result<()> {
    let root = dir
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize directory: {}", dir.display()))?;

    collect_files_recursive_inner(&root, &root, files)
}

fn collect_files_recursive_inner(root: &Path, dir: &Path, files: &mut HashSet<String>) -> Result<()> {
    let canonical_dir = dir
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize directory during traversal: {}", dir.display()))?;
    if !canonical_dir.starts_with(root) {
        return Err(anyhow!(
            "Attempted to traverse outside of root directory: {} (root: {})",
            canonical_dir.display(),
            root.display()
        ));
    }
    for entry in fs::read_dir(&canonical_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) && name.starts_with('.') {
                continue;
            }
            collect_files_recursive_inner(root, &path, files)?;
        } else if let Some(path_str) = path.to_str() {
            files.insert(path_str.to_string());
        }
    }
    Ok(())
}

pub fn load_cwl_yaml(base_path: &str, cwl_file_path: &Path) -> Result<Value> {
    let full_path: PathBuf = if cwl_file_path.is_absolute() {
        cwl_file_path.to_path_buf()
    } else {
        Path::new(base_path).join(cwl_file_path)
    };

    let contents = fs::read_to_string(&full_path).with_context(|| format!("Failed to read CWL file at path: {}", full_path.display()))?;

    let yaml: Value = serde_yaml::from_str(&contents).with_context(|| format!("Failed to parse CWL YAML at path: {}", full_path.display()))?;

    Ok(yaml)
}

pub fn load_yaml_file(path: &Path) -> Result<Value> {
    let contents = fs::read_to_string(path).with_context(|| format!("Failed to read YAML file at path: {}", path.display()))?;

    let yaml: Value = serde_yaml::from_str(&contents).with_context(|| format!("Failed to parse YAML content at path: {}", path.display()))?;

    Ok(yaml)
}

pub fn load_cwl_file(base_path: &str, cwl_file_path: &Path) -> Result<Value> {
    let base_path = Path::new(base_path);
    let base_path = base_path.parent().unwrap_or(base_path);

    let mut combined_path = base_path.to_path_buf();
    for component in cwl_file_path.components() {
        match component {
            std::path::Component::Normal(name) => combined_path.push(name),
            std::path::Component::ParentDir => {
                if let Some(parent) = combined_path.parent() {
                    combined_path = parent.to_path_buf();
                }
            }
            _ => {}
        }
    }

    if !combined_path.exists() {
        anyhow::bail!("CWL file not found: {}", combined_path.display());
    }

    let mut file_content = String::new();
    File::open(&combined_path)
        .with_context(|| format!("Failed to open CWL file at: {}", combined_path.display()))?
        .read_to_string(&mut file_content)
        .with_context(|| format!("Failed to read CWL file: {}", combined_path.display()))?;

    let cwl: Value = serde_yaml::from_str(&file_content).with_context(|| format!("Failed to parse CWL YAML at: {}", combined_path.display()))?;

    Ok(cwl)
}

pub fn read_file_content(file_path: &str) -> Result<String> {
    let mut file = File::open(file_path).with_context(|| format!("Failed to open file: {file_path}"))?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .with_context(|| format!("Failed to read contents of file: {file_path}"))?;

    Ok(content)
}

pub fn build_inputs_yaml(cwl_input_path: &str, input_yaml_path: &PathBuf) -> Result<Mapping> {
    let input_yaml = fs::read_to_string(input_yaml_path).with_context(|| format!("Failed to read input YAML file at '{input_yaml_path:?}'"))?;
    let input_value: Value = serde_yaml::from_str(&input_yaml).with_context(|| format!("Failed to parse input YAML at '{input_yaml_path:?}'"))?;

    let cwl_content = fs::read_to_string(cwl_input_path).with_context(|| format!("Failed to read CWL input file at '{cwl_input_path}'"))?;
    let cwl_value: Value = serde_yaml::from_str(&cwl_content).with_context(|| format!("Failed to parse CWL file at '{cwl_input_path}'"))?;

    let mut files = HashSet::new();
    let mut directories = HashSet::new();
    let mut parameters = HashMap::new();

    let main_cwl_path = Path::new(cwl_input_path);
    let main_dir = main_cwl_path.parent().unwrap_or_else(|| Path::new("."));
    let mut referenced_paths = HashSet::new();

    if let Value::Mapping(mapping) = input_value {
        for (key, value) in mapping {
            if let Value::String(key_str) = key {
                if let Value::Mapping(mut sub_mapping) = value.clone() {
                    let class = sub_mapping
                        .get(Value::String("class".to_string()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let location = sub_mapping
                        .get(Value::String("location".to_string()))
                        .or_else(|| sub_mapping.get(Value::String("path".to_string())))
                        .and_then(|v| v.as_str());

                    if let (Some(class), Some(location)) = (class, location) {
                        let sanitized_location = sanitize_path(location);
                        let key_to_update = if sub_mapping.contains_key(Value::String("location".to_string())) {
                            "location"
                        } else {
                            "path"
                        };
                        sub_mapping.insert(Value::String(key_to_update.to_string()), Value::String(sanitized_location.clone()));

                        match class.as_str() {
                            "File" => {
                                files.insert(sanitized_location);
                            }
                            "Directory" => {
                                directories.insert(sanitized_location);
                            }
                            _ => {}
                        }

                        parameters.insert(key_str, Value::Mapping(sub_mapping));
                    } else {
                        parameters.insert(key_str, Value::Mapping(sub_mapping));
                    }
                } else {
                    parameters.insert(key_str, value);
                }
            }
        }
    }

    if let Some(steps) = cwl_value.get("steps").and_then(|v| v.as_sequence()) {
        for step in steps {
            if let Some(run_path_str) = step.get("run").and_then(|v| v.as_str()) {
                let full_path = main_dir
                    .join(run_path_str)
                    .canonicalize()
                    .with_context(|| format!("Failed to resolve step run path '{run_path_str}'"))?;
                referenced_paths.insert(full_path);
            }
        }
    }

    let main_canonical = fs::canonicalize(main_cwl_path).with_context(|| format!("Failed to canonicalize CWL file path '{cwl_input_path}'"))?;
    referenced_paths.insert(main_canonical);

    if !referenced_paths.is_empty() {
        let common_root = find_common_directory(&referenced_paths.iter().cloned().collect::<BTreeSet<_>>())
            .context("Failed to determine common directory for referenced CWL paths")?;

        let relative_root = pathdiff::diff_paths(&common_root, std::env::current_dir()?).unwrap_or(common_root.clone());

        let relative_str = relative_root.to_string_lossy().to_string();
        if relative_str.is_empty() {
            let current_dir = std::env::current_dir().context("Failed to get current directory")?;
            for entry in fs::read_dir(&current_dir).context("Failed to read current directory")? {
                let entry = entry.context("Failed to read entry in current directory")?;
                let path = entry.path();

                if path.is_dir() {
                    if let Some(str_path) = path.strip_prefix(&current_dir).ok().and_then(|p| p.to_str()) {
                        directories.insert(str_path.to_string());
                    }
                } else if path.is_file()
                    && let Some(str_path) = path.strip_prefix(&current_dir).ok().and_then(|p| p.to_str())
                {
                    files.insert(str_path.to_string());
                }
            }
        } else {
            directories.insert(relative_str);
        }
    }

    remove_files_contained_in_directories(&mut files, &directories);

    let mut inputs_mapping = Mapping::new();
    inputs_mapping.insert(
        Value::String("files".to_string()),
        Value::Sequence(files.into_iter().map(Value::String).collect()),
    );
    inputs_mapping.insert(
        Value::String("directories".to_string()),
        Value::Sequence(directories.into_iter().map(Value::String).collect()),
    );
    inputs_mapping.insert(
        Value::String("parameters".to_string()),
        Value::Mapping(parameters.into_iter().map(|(k, v)| (Value::String(k), v)).collect()),
    );

    Ok(inputs_mapping)
}

pub fn build_inputs_cwl(cwl_input_path: &str, inputs_yaml: Option<&String>) -> Result<Mapping> {
    let cwl_content = fs::read_to_string(cwl_input_path).with_context(|| format!("Failed to read CWL input file at '{cwl_input_path}'"))?;
    let cwl_value: Value = serde_yaml::from_str(&cwl_content).with_context(|| format!("Failed to parse CWL file at '{cwl_input_path}'"))?;

    let mut files: HashSet<String> = HashSet::new();
    let mut directories: HashSet<String> = HashSet::new();
    let mut parameters: HashMap<String, Value> = HashMap::new();
    let mut referenced_paths: HashSet<PathBuf> = HashSet::new();

    let main_cwl_path = Path::new(cwl_input_path);
    let main_dir = main_cwl_path.parent().unwrap_or_else(|| Path::new("."));

    if let Some(inputs) = cwl_value.get("inputs").and_then(|v| v.as_sequence()) {
        for input in inputs {
            let id = input
                .get("id")
                .and_then(|v| v.as_str())
                .with_context(|| "Missing 'id' field in CWL input")?;

            let input_type_val = input.get("type").with_context(|| format!("Missing 'type' for input id '{id}'"))?;

            let input_type = input_type_val
                .as_str()
                .or_else(|| input_type_val.get("type").and_then(|t| t.as_str()))
                .unwrap_or("");

            if input_type == "File" || input_type == "Directory" {
                if let Some(default) = input.get("default") {
                    if let Value::Mapping(default_map) = default {
                        let mut sanitized_map = default_map.clone();

                        if let Some(location_val) = sanitized_map.get_mut(Value::String("location".to_string()))
                            && let Some(location) = location_val.as_str()
                        {
                            let sanitized_location = sanitize_path(location);
                            *location_val = Value::String(sanitized_location.clone());

                            match input_type {
                                "File" => {
                                    files.insert(sanitized_location);
                                }
                                "Directory" => {
                                    directories.insert(sanitized_location);
                                }
                                _ => {}
                            }
                        }
                        parameters.insert(id.to_string(), Value::Mapping(sanitized_map));
                    } else {
                        parameters.insert(id.to_string(), default.clone());
                    }
                } else {
                    let location = find_input_location(cwl_input_path, id).with_context(|| format!("Failed to find location for input id '{id}'"))?;
                    if let Some(location) = location {
                        let sanitized_location = sanitize_path(&location);
                        match input_type {
                            "File" => {
                                files.insert(sanitized_location.clone());
                            }
                            "Directory" => {
                                directories.insert(sanitized_location.clone());
                            }
                            _ => {}
                        }

                        let mut param_map = Mapping::new();
                        param_map.insert(Value::String("class".to_string()), Value::String(input_type.to_string()));
                        if input_type == "Directory" {
                            param_map.insert(Value::String("location".to_string()), Value::String(sanitized_location));
                        } else {
                            param_map.insert(Value::String("path".to_string()), Value::String(sanitized_location));
                        }
                        parameters.insert(id.to_string(), Value::Mapping(param_map));
                    }
                }
            } else if let Some(default) = input.get("default") {
                parameters.insert(id.to_string(), default.clone());
            }
        }
    }

    if let Some(steps) = cwl_value.get("steps").and_then(|v| v.as_sequence()) {
        for step in steps {
            if let Some(run_path_str) = step.get("run").and_then(|v| v.as_str()) {
                let full_path = main_dir
                    .join(run_path_str)
                    .canonicalize()
                    .with_context(|| format!("Failed to canonicalize step run path '{run_path_str}'"))?;
                referenced_paths.insert(full_path);
            }
        }
    }

    let main_canonical = fs::canonicalize(main_cwl_path).with_context(|| format!("Failed to canonicalize main CWL file path '{cwl_input_path}'"))?;
    referenced_paths.insert(main_canonical);

    if !referenced_paths.is_empty() {
        let common_root = find_common_directory(&referenced_paths.iter().cloned().collect::<BTreeSet<_>>())
            .context("Failed to find common directory for referenced paths")?;

        let relative_root = pathdiff::diff_paths(&common_root, std::env::current_dir()?).unwrap_or(common_root.clone());

        let relative_str = relative_root.to_string_lossy().to_string();
        if !relative_str.is_empty() {
            directories.insert(relative_str);
        }
    }

    if directories.is_empty() {
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        for entry in fs::read_dir(&current_dir).context("Failed to read current directory")? {
            let entry = entry.context("Failed to read entry in current directory")?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(str_path) = path.strip_prefix(&current_dir).ok().and_then(|p| p.to_str()) {
                    directories.insert(str_path.to_string());
                }
            } else if path.is_file()
                && let Some(str_path) = path.strip_prefix(&current_dir).ok().and_then(|p| p.to_str())
            {
                files.insert(str_path.to_string());
            }
        }
    }

    if let Some(yaml_path) = inputs_yaml {
        parameters.insert("inputs.yaml".to_string(), Value::String(yaml_path.to_string()));
    }

    remove_files_contained_in_directories(&mut files, &directories);

    let mut inputs_mapping = Mapping::new();
    inputs_mapping.insert(
        Value::String("files".to_string()),
        Value::Sequence(files.into_iter().map(Value::String).collect()),
    );

    inputs_mapping.insert(
        Value::String("directories".to_string()),
        Value::Sequence(directories.into_iter().map(Value::String).collect()),
    );

    let mut parameter_mapping = Mapping::new();

    for (key, value) in parameters {
        if let Some(class) = value.get("class") {
            let mut param_map = Mapping::new();
            if let Some(class_str) = class.as_str() {
                param_map.insert(Value::String("class".to_string()), Value::String(class_str.to_string()));
            }
            if let Some(location) = value.get("location") {
                param_map.insert(Value::String("location".to_string()), location.clone());
            }
            if let Some(path) = value.get("path") {
                param_map.insert(Value::String("path".to_string()), path.clone());
            }
            parameter_mapping.insert(Value::String(key), Value::Mapping(param_map));
        } else {
            parameter_mapping.insert(Value::String(key), value);
        }
    }
    inputs_mapping.insert(Value::String("parameters".to_string()), Value::Mapping(parameter_mapping));

    Ok(inputs_mapping)
}

pub fn get_all_outputs<P: AsRef<Path>>(workflow: &Workflow, path: P) -> Result<Vec<(String, String)>> {
    let main_workflow_dir = path
        .as_ref()
        .parent()
        .with_context(|| "Failed to get parent directory of main workflow file")?;
    let mut results = Vec::new();

    for output in &workflow.outputs {
        if let Some(output_source) = &output.output_source {
            let parts: Vec<&str> = output_source.split('/').collect();
            if parts.len() != 2 {
                anyhow::bail!(
                    "Invalid 'outputSource' format for output: '{}'. Expected format 'step_id/output_id'",
                    output_source
                );
            }
            let step_id = parts[0];
            let output_id = parts[1];
            let run_file_path = workflow
                .steps
                .iter()
                .find_map(|step| {
                    if step.id == step_id
                        && let StringOrDocument::String(run) = &step.run
                    {
                        Some(run)
                    } else {
                        None
                    }
                })
                .with_context(|| format!("Step with id '{step_id}' not found or missing 'run' field in main workflow"))?;

            let full_run_file_path = main_workflow_dir
                .join(run_file_path)
                .canonicalize()
                .with_context(|| format!("Failed to canonicalize run file path '{run_file_path}' for step '{step_id}'"))?;

            let tool_yaml_str =
                fs::read_to_string(&full_run_file_path).with_context(|| format!("Failed to read tool file '{}'", full_run_file_path.display()))?;
            let tool_yaml: Value = serde_yaml::from_str(&tool_yaml_str)
                .with_context(|| format!("Failed to parse YAML from tool file '{}'", full_run_file_path.display()))?;

            let tool_outputs = tool_yaml
                .get("outputs")
                .with_context(|| format!("No 'outputs' section in tool file '{run_file_path}'"))?
                .as_sequence()
                .with_context(|| format!("'outputs' section in tool file '{run_file_path}' is not a sequence"))?;

            let glob_value = tool_outputs
                .iter()
                .find_map(|tool_output| {
                    let tid = tool_output.get("id").and_then(|v| v.as_str())?;
                    if tid == output_id {
                        tool_output
                            .get("outputBinding")
                            .and_then(|binding| binding.get("glob"))
                            .and_then(|glob| glob.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .with_context(|| format!("Output '{output_id}' not found in tool file '{run_file_path}' or missing 'glob' field"))?;

            results.push((output_id.to_string(), glob_value));
        }
    }
    Ok(results)
}

pub fn find_input_location(cwl_file_path: &str, id: &str) -> Result<Option<String>> {
    let mut main_file = File::open(cwl_file_path).with_context(|| format!("Failed to open CWL file '{cwl_file_path}'"))?;
    let mut main_file_content = String::new();
    main_file
        .read_to_string(&mut main_file_content)
        .with_context(|| format!("Failed to read contents of CWL file '{cwl_file_path}'"))?;

    let main_cwl: Value =
        serde_yaml::from_str(&main_file_content).with_context(|| format!("Failed to parse YAML from CWL file '{cwl_file_path}'"))?;

    let steps = main_cwl
        .get("steps")
        .and_then(|v| v.as_sequence())
        .with_context(|| format!("Missing or invalid 'steps' section in CWL file '{cwl_file_path}'"))?;

    for step in steps {
        let inputs = step
            .get("in")
            .and_then(|v| v.as_mapping())
            .with_context(|| "Invalid or missing 'in' mapping in a step")?;

        if inputs.contains_key(Value::String(id.to_string())) {
            let run_path_str = step
                .get("run")
                .and_then(|v| v.as_str())
                .with_context(|| format!("Step with input '{id}' missing 'run' field or it is not a string"))?;

            let run_path = Path::new(run_path_str);
            let run_file = load_cwl_file(cwl_file_path, run_path)
                .with_context(|| format!("Failed to load referenced CWL file '{}' from step", run_path.display()))?;

            let inputs_section = run_file
                .get("inputs")
                .and_then(|v| v.as_sequence())
                .with_context(|| format!("Missing or invalid 'inputs' section in referenced CWL file '{}'", run_path.display()))?;

            for input in inputs_section {
                let input_id = input.get("id").and_then(|v| v.as_str()).unwrap_or_default();

                if input_id == id
                    && let Some(default) = input.get("default").and_then(|v| v.as_mapping())
                    && let Some(location_val) = default.get(Value::String("location".to_string()))
                    && let Some(location) = location_val.as_str()
                {
                    return Ok(Some(location.to_string()));
                }
            }
        }
    }

    Ok(None)
}

pub fn resolve_input_file_path(requested_file: &str, input_yaml: Option<&Value>, cwl_value: Option<&Value>) -> Result<Option<String>> {
    let requested_path = Path::new(requested_file);

    if requested_path.exists() {
        return Ok(Some(requested_file.to_string()));
    }

    // Search in input_yaml
    if let Some(Value::Mapping(mapping)) = input_yaml {
        for (_key, value) in mapping {
            if let Value::Mapping(file_entry) = value {
                for field in &["location", "path"] {
                    if let Some(Value::String(path_str)) = file_entry.get(Value::String((*field).to_string()))
                        && file_matches(requested_file, path_str)
                    {
                        return Ok(Some(path_str.to_string()));
                    }
                }
            }
        }
    }

    // Search in cwl inputs
    if let Some(cwl) = cwl_value {
        let inputs = cwl
            .get("inputs")
            .with_context(|| "Missing 'inputs' section in CWL value")?
            .as_sequence()
            .with_context(|| "'inputs' section in CWL value is not a sequence")?;

        for input in inputs {
            if let Some(Value::Mapping(default_map)) = input.get("default") {
                for field in &["location", "path"] {
                    if let Some(Value::String(loc)) = default_map.get(Value::String((*field).to_string()))
                        && file_matches(requested_file, loc)
                    {
                        return Ok(Some(loc.to_string()));
                    }
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::load_workflow;
    use serde_yaml::Value;
    use std::collections::HashSet;
    use std::fs::{self, File, create_dir_all};
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn normalize_path(path: &str) -> String {
        Path::new(path).to_str().unwrap_or_default().replace("\\", "/")
    }

    #[test]
    fn test_load_cwl_file_resolves_relative_path() {
        let temp_dir = tempdir().unwrap();
        let base_path = temp_dir.path().join("base");
        let sub_dir = base_path.join("sub");

        create_dir_all(&sub_dir).unwrap();

        let cwl_file_path = sub_dir.join("tool.cwl");

        let cwl_content = r#"
        class: CommandLineTool
        baseCommand: echo
        inputs: []
        outputs: []
        "#;

        let mut file = File::create(&cwl_file_path).unwrap();
        write!(file, "{cwl_content}").unwrap();

        let result = load_cwl_file(cwl_file_path.to_str().unwrap(), Path::new("../sub/tool.cwl"));

        assert!(result.is_ok(), "load_cwl_file failed with error: {:?}", result.err());

        let value = result.unwrap();
        assert_eq!(value["class"], serde_yaml::Value::String("CommandLineTool".to_string()));
    }

    #[test]
    fn test_find_input_location_valid_input() {
        let temp_dir = tempdir().unwrap();
        let dir_path = temp_dir.path();

        let sub_cwl_content = r#"
        class: CommandLineTool
        inputs:
        - id: population
          type: File
          default:
            class: File
            location: data/population.csv
        outputs: []
        baseCommand: echo
        "#;
        let sub_cwl_path = dir_path.join("tool.cwl");
        fs::write(&sub_cwl_path, sub_cwl_content).unwrap();

        let main_cwl_content = r#"
        class: Workflow
        inputs: []
        outputs: []
        steps:
        - id: step1
          run: tool.cwl
          in:
            population: population
            out: []
        "#;
        let main_cwl_path = dir_path.join("main.cwl");
        fs::write(&main_cwl_path, main_cwl_content).unwrap();

        let main_path_str = main_cwl_path.to_str().unwrap();

        let result = find_input_location(main_path_str, "population").unwrap();

        assert_eq!(result, Some("data/population.csv".to_string()));
    }

    #[test]
    fn test_build_inputs_yaml_real_example() {
        use serde_yaml::Value;

        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let input_yaml_path = base_dir.join("testdata/hello_world/inputs.yml");
        assert!(input_yaml_path.exists(), "Test input file does not exist");

        let cwl_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        assert!(cwl_path.exists(), "Test cwl file does not exist");

        let result = build_inputs_yaml(&cwl_path.to_string_lossy(), &input_yaml_path);
        assert!(result.is_ok(), "build_inputs_yaml failed: {result:?}");
        let mapping = result.unwrap();

        let files = mapping.get(Value::String("files".to_string())).expect("Missing 'files'");
        if let Value::Sequence(file_list) = files {
            let file_set: HashSet<_> = file_list.iter().filter_map(|v| v.as_str()).map(normalize_path).collect();
            assert!(file_set.contains("data/population.csv"), "Missing population.csv");
            assert!(file_set.contains("data/speakers_revised.csv"), "Missing speakers_revised.csv");
        } else {
            panic!("Expected 'files' to be a sequence");
        }
    }

    #[test]
    fn test_sanitize_simple_path() {
        let path = "folder/file.txt";
        let sanitized = sanitize_path(path);
        let sanitized_normalized = normalize_path(&sanitized);
        assert_eq!(
            sanitized_normalized, "folder/file.txt",
            "The sanitized path should be normalized to use forward slashes."
        );
    }

    #[test]
    fn test_sanitize_path_with_parent_dir() {
        let path = "folder/../file.txt";
        let sanitized = sanitize_path(path);
        assert_eq!(sanitized, "file.txt", "The parent directory should be removed from the path.");
    }

    #[test]
    fn test_sanitize_path_with_multiple_parent_dirs() {
        let path = "folder/../other_folder/../file.txt";
        let sanitized = sanitize_path(path);
        assert_eq!(sanitized, "file.txt", "Multiple parent directories should be removed.");
    }

    #[test]
    fn test_sanitize_empty_path() {
        let path = "";
        let sanitized = sanitize_path(path);
        assert_eq!(sanitized, "", "An empty path should return an empty string.");
    }

    #[test]
    fn test_sanitize_path_with_leading_trailing_spaces() {
        let path = "   folder/file.txt   ";
        let sanitized = sanitize_path(path);
        let sanitized_normalized = normalize_path(&sanitized);
        assert_eq!(
            sanitized_normalized, "folder/file.txt",
            "Leading and trailing spaces should be removed and slashes normalized."
        );
    }

    #[test]
    fn find_common_directory_empty_input() {
        let paths = BTreeSet::new();
        let result = find_common_directory(&paths);
        assert!(result.is_err(), "Expected error for empty input");
    }

    #[test]
    fn find_common_directory_single_path() {
        let mut paths = BTreeSet::new();
        paths.insert(PathBuf::from("/home/user/docs"));

        let result = find_common_directory(&paths).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/docs"));
    }

    #[test]
    fn find_common_directory_common_root() {
        let mut paths = BTreeSet::new();
        paths.insert(PathBuf::from("/home/user/docs/file1.txt"));
        paths.insert(PathBuf::from("/home/user/docs/file2.txt"));

        let result = find_common_directory(&paths).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/docs"));
    }

    #[test]
    fn find_common_directory_common_root_only() {
        let mut paths = BTreeSet::new();
        paths.insert(PathBuf::from("/home/user1/docs"));
        paths.insert(PathBuf::from("/home/user2/images"));

        let result = find_common_directory(&paths).unwrap();
        assert_eq!(result, PathBuf::from("/home"));
    }

    #[test]
    fn find_common_directory_different_roots() {
        let mut paths = BTreeSet::new();
        paths.insert(PathBuf::from("/var/log"));
        paths.insert(PathBuf::from("/etc/config"));

        let result = find_common_directory(&paths).unwrap();
        assert_eq!(result, PathBuf::from("/"));
    }

    #[test]
    fn find_common_directory_relative_paths() {
        let mut paths = BTreeSet::new();
        paths.insert(PathBuf::from("a/b/c"));
        paths.insert(PathBuf::from("a/b/d"));

        let result = find_common_directory(&paths).unwrap();
        assert_eq!(result, PathBuf::from("a/b"));
    }

    #[test]
    fn remove_files_contained_in_directories_data_example() {
        let directories: HashSet<String> = HashSet::from([String::from("data")]);

        let mut files: HashSet<String> = HashSet::from([
            String::from("data/population.csv"),
            String::from("data/speakers.csv"),
            String::from("workflows/main.cwl"),
        ]);

        remove_files_contained_in_directories(&mut files, &directories);

        let expected: HashSet<String> = HashSet::from([String::from("workflows/main.cwl")]);

        assert_eq!(files, expected);
    }

    #[test]
    fn file_matches_exact_filename() {
        let requested = "data/population.csv";
        let candidate = "/home/user/data/population.csv";
        assert!(file_matches(requested, candidate));
    }

    #[test]
    fn file_matches_different_path_same_filename() {
        let requested = "population.csv";
        let candidate = "backup/2020/population.csv";
        assert!(file_matches(requested, candidate));
    }

    #[test]
    fn file_matches_mismatch_filename() {
        let requested = "population.csv";
        let candidate = "data/speakers.csv";
        assert!(!file_matches(requested, candidate));
    }

    #[test]
    fn file_matches_empty_requested() {
        let requested = "";
        let candidate = "data/population.csv";
        assert!(!file_matches(requested, candidate));
    }

    #[test]
    fn file_matches_no_filename_in_requested() {
        let requested = "data/";
        let candidate = "data/population.csv";
        assert!(!file_matches(requested, candidate));
    }

    #[test]
    fn file_matches_candidate_is_filename_only() {
        let requested = "data/population.csv";
        let candidate = "population.csv";
        assert!(file_matches(requested, candidate));
    }

    #[test]
    fn test_collect_files_recursive_basic_structure() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let dir_path = temp_dir.path();

        let file1 = dir_path.join("file1.txt");
        fs::File::create(&file1).expect("Failed to create file1");

        let subdir = dir_path.join("subdir");
        fs::create_dir(&subdir).expect("Failed to create subdir");

        let file2 = subdir.join("file2.txt");
        fs::File::create(&file2).expect("Failed to create file2");

        let mut collected_files = HashSet::new();
        let result = collect_files_recursive(dir_path, &mut collected_files);

        assert!(result.is_ok());
        assert_eq!(collected_files.len(), 2);
        assert!(collected_files.iter().any(|f| f.ends_with("file1.txt")));
        assert!(collected_files.iter().any(|f| f.ends_with("file2.txt")));
    }

    #[test]
    fn test_collect_files_recursive_empty_dir() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let mut files = HashSet::new();
        let result = collect_files_recursive(temp_dir.path(), &mut files);

        assert!(result.is_ok());
        assert!(files.is_empty());
    }

    #[test]
    fn test_collect_files_recursive_nested_dirs() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let nested = temp_dir.path().join("a/b/c");
        fs::create_dir_all(&nested).expect("Failed to create nested dirs");

        let nested_file = nested.join("nested.txt");
        fs::File::create(&nested_file).expect("Failed to create nested file");

        let mut files = HashSet::new();
        let result = collect_files_recursive(temp_dir.path(), &mut files);

        assert!(result.is_ok());
        assert_eq!(files.len(), 1);
        assert!(files.iter().any(|f| f.ends_with("nested.txt")));
    }

    #[test]
    fn test_load_cwl_yaml_valid() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("workflow.cwl");

        let yaml_content = r#"
        cwlVersion: v1.0
        class: CommandLineTool
        baseCommand: echo
        inputs:
        input_file:
            type: File
            inputBinding:
            position: 1
        outputs:
        output_file:
            type: File
            outputBinding:
            glob: "*.txt"
        "#;
        let mut file = File::create(&file_path).expect("Failed to create file");
        write!(file, "{yaml_content}").expect("Failed to write CWL content");

        let base_path = temp_dir.path().to_str().unwrap();
        let result = load_cwl_yaml(base_path, &file_path);

        assert!(result.is_ok());
        let value = result.unwrap();

        assert_eq!(value["cwlVersion"], Value::from("v1.0"));
        assert_eq!(value["class"], Value::from("CommandLineTool"));
        assert_eq!(value["baseCommand"], Value::from("echo"));
    }

    #[test]
    fn test_load_cwl_yaml_nonexistent() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let fake_path = Path::new("nonexistent.cwl");

        let base_path = temp_dir.path().to_str().unwrap();
        let result = load_cwl_yaml(base_path, fake_path);

        assert!(result.is_err());
    }

    #[test]
    fn test_load_cwl_yaml_invalid_yaml() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("invalid.cwl");

        let invalid_content = "cwlVersion: v1.0\nclass: CommandLineTool\nbaseCommand echo\n";
        let mut file = File::create(&file_path).expect("Failed to create file");
        write!(file, "{invalid_content}").expect("Failed to write invalid CWL content");

        let base_path = temp_dir.path().to_str().unwrap();
        let result = load_cwl_yaml(base_path, &file_path);

        assert!(result.is_err());
    }

    #[test]
    fn test_load_cwl_yaml_relative_path() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).expect("Failed to create subdir");

        let file_path = subdir.join("workflow.cwl");

        let yaml_content = r#"
        cwlVersion: v1.0
        class: CommandLineTool
        baseCommand: echo
        "#;
        let mut file = File::create(&file_path).expect("Failed to create file");
        write!(file, "{yaml_content}").expect("Failed to write CWL content");

        let base_path = temp_dir.path().to_str().unwrap();
        let result = load_cwl_yaml(base_path, &file_path);

        assert!(result.is_ok());
        let value = result.unwrap();

        assert_eq!(value["cwlVersion"], Value::from("v1.0"));
        assert_eq!(value["class"], Value::from("CommandLineTool"));
    }

    #[test]
    fn test_load_yaml_file_valid() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("test.yaml");

        let yaml_content = r#"
        name: Test
        version: 1.0
        "#;

        let mut file = File::create(&file_path).expect("Failed to create file");
        write!(file, "{yaml_content}").expect("Failed to write YAML content");

        let result = load_yaml_file(&file_path);

        assert!(result.is_ok());
        let value = result.unwrap();

        assert_eq!(value["name"], serde_yaml::Value::from("Test"));
        assert_eq!(value["version"], serde_yaml::Value::from(1.0));
    }

    #[test]
    fn test_load_yaml_file_nonexistent() {
        let non_existent_path = Path::new("nonexistent.yaml");
        let result = load_yaml_file(non_existent_path);

        assert!(result.is_err());
    }

    #[test]
    fn test_load_cwl_file_nonexistent() {
        let base_path = "/some/base/path";
        let fake_cwl_path = Path::new("nonexistent.cwl");

        let result = load_cwl_file(base_path, fake_cwl_path);

        assert!(result.is_err());
    }

    #[test]
    fn test_load_cwl_file_invalid() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("invalid.cwl");

        let invalid_content = "cwlVersion: v1.0\nclass: CommandLineTool\nbaseCommand echo\n";
        let mut file = std::fs::File::create(&file_path).expect("Failed to create file");
        write!(file, "{invalid_content}").expect("Failed to write invalid CWL content");

        let base_path = temp_dir.path().to_str().unwrap();
        let result = load_cwl_file(base_path, &file_path);

        assert!(result.is_err());
    }

    #[test]
    fn test_read_file_content_valid() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("file.txt");

        let file_content = "This is a test file content.";
        let mut file = File::create(&file_path).expect("Failed to create file");
        write!(file, "{file_content}").expect("Failed to write file content");

        let result = read_file_content(file_path.to_str().unwrap());

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_content);
    }

    #[test]
    fn test_read_file_content_nonexistent() {
        let result = read_file_content("nonexistent.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_file_content_invalid() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("invalid.txt");

        let invalid_content = "This file might not be readable.";
        let mut file = File::create(&file_path).expect("Failed to create file");
        write!(file, "{invalid_content}").expect("Failed to write content");

        let result = read_file_content(file_path.to_str().unwrap());

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), invalid_content);
    }

    #[test]
    fn test_build_inputs_cwl_real_example() {
        use serde_yaml::Value;
        use std::collections::HashSet;

        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cwl_input_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        assert!(cwl_input_path.exists(), "Test CWL file does not exist");

        let result = build_inputs_cwl(&cwl_input_path.to_string_lossy(), None);
        assert!(result.is_ok(), "build_inputs_cwl failed: {result:?}");
        let mapping = result.unwrap();

        let files = mapping.get(Value::String("files".to_string())).expect("Missing 'files'");
        if let Value::Sequence(file_list) = files {
            let file_set: HashSet<_> = file_list.iter().filter_map(|v| v.as_str()).map(normalize_path).collect();
            assert!(file_set.contains("data/population.csv"), "Missing population.csv");
            assert!(file_set.contains("data/speakers_revised.csv"), "Missing speakers_revised.csv");
        } else {
            panic!("Expected 'files' to be a sequence");
        }
    }

    #[test]
    fn test_build_inputs_cwl() {
        use serde_yaml::Value;

        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cwl_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        assert!(cwl_path.exists(), "Test CWL file does not exist");

        let input = base_dir.join("testdata/hello_world/inputs.yml");
        assert!(input.exists(), "Test input file does not exist");

        let cwl_path_str = cwl_path.to_string_lossy().into_owned();
        let inputs_path_str = input.to_string_lossy().into_owned();

        let result = build_inputs_cwl(&cwl_path_str, Some(&inputs_path_str));
        assert!(result.is_ok(), "build_inputs_cwl failed: {result:?}");

        let mapping = result.unwrap();

        let files = mapping.get(Value::String("files".to_string())).expect("Missing 'files' section");
        if let Value::Sequence(file_list) = files {
            let file_set: HashSet<_> = file_list.iter().filter_map(|v| v.as_str()).map(normalize_path).collect();
            assert!(file_set.contains("data/population.csv"), "Missing expected file: population.csv");
            assert!(
                file_set.contains("data/speakers_revised.csv"),
                "Missing expected file: speakers_revised.csv"
            );
        } else {
            panic!("Expected 'files' to be a sequence");
        }

        let dirs = mapping
            .get(Value::String("directories".to_string()))
            .expect("Missing 'directories' section");
        if let Value::Sequence(dir_list) = dirs {
            let dir_set: HashSet<_> = dir_list.iter().filter_map(|v| v.as_str()).collect();
            assert!(
                dir_set.iter().any(|d| d.contains("workflows")),
                "Expected directory containing 'workflows' not found"
            );
        } else {
            panic!("Expected 'directories' to be a sequence");
        }

        let params = mapping
            .get(Value::String("parameters".to_string()))
            .expect("Missing 'parameters' section");
        if let Value::Mapping(param_map) = params {
            assert!(
                param_map.contains_key(Value::String("inputs.yaml".to_string())),
                "Missing 'inputs.yaml' in parameters"
            );
        } else {
            panic!("Expected 'parameters' to be a mapping");
        }
    }

    #[test]
    fn test_get_all_outputs_with_existing_file() {
        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workflow_file_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        let workflow = load_workflow(&workflow_file_path).unwrap();
        assert!(workflow_file_path.exists(), "CWL file not found at: {workflow_file_path:?}");
        let result = get_all_outputs(&workflow, &workflow_file_path);
        assert!(result.is_ok());
        let outputs = result.unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], ("results".to_string(), "results.svg".to_string()));
    }
}
