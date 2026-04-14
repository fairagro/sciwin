use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};
use commonwl::{StringOrDocument, Workflow, CWLDocument, SingularPlural, packed::PackedCWL};
use crate::reana::Reana; 
use crate::api::{get_workflow_status, get_workflow_specification};
use std::sync::Arc;

static TEMP_DIR_REGEX: OnceLock<Regex> = OnceLock::new();

pub fn strip_temp_prefix(path: &str) -> String {
    let re = TEMP_DIR_REGEX.get_or_init(|| Regex::new(r"^/tmp/\.tmp[^/]+/").unwrap());
    re.replace(path, "").to_string()
}

pub fn sanitize_path(path: &str) -> String {
    let mut normalized = PathBuf::new();
    for comp in Path::new(path.trim()).components() {
        match comp {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(name) => normalized.push(name),
            _ => {}
        }
    }
    normalized
        .to_string_lossy()
        .replace('\\', std::path::MAIN_SEPARATOR_STR)
}

pub fn find_common_directory(paths: &BTreeSet<PathBuf>) -> Result<PathBuf> {
    if paths.is_empty() {
        return Err(anyhow!("No paths provided to compute common directory."));
    }
    let components: Vec<Vec<_>> = paths.iter().map(|p| p.components().collect()).collect();
    let mut common = PathBuf::new();
    for (i, part) in components[0].iter().enumerate() {
        if components.iter().all(|c| c.get(i) == Some(part)) {
            common.push(part.as_os_str());
        } else {
            break;
        }
    }
    if common.as_os_str().is_empty() {
        return Err(anyhow!("Could not determine a common directory among the given paths."));
    }
    Ok(common)
}

pub fn find_common_directory_with_prefix(cwl_input_path: &str, input_yaml_path: &Path) -> Result<PathBuf> {
    let main = Path::new(cwl_input_path).canonicalize()
        .with_context(|| format!("Failed to canonicalize CWL path '{cwl_input_path}'"))?;
    let input = input_yaml_path.canonicalize()
        .with_context(|| format!("Failed to canonicalize YAML path '{}'", input_yaml_path.display()))?;

    let paths = BTreeSet::from([main, input]);
    find_common_directory(&paths)
}

pub fn make_relative_to_common_dir(path: &str, common_dir: &Path) -> String {
    let rel = pathdiff::diff_paths(path, common_dir).unwrap_or_else(|| PathBuf::from(path));
    sanitize_path(&rel.to_string_lossy())
}

pub fn get_location(base_path: &str, cwl_file_path: &Path) -> Result<String> {
    let base_dir = Path::new(base_path).parent().unwrap_or_else(|| Path::new(base_path));
    let combined = base_dir.join(cwl_file_path).canonicalize()
        .with_context(|| format!("Failed to resolve CWL path '{}'", cwl_file_path.display()))?;
    combined.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Non-UTF8 path: {}", combined.display()))
}

pub fn remove_files_contained_in_directories(files: &mut HashSet<String>, directories: &HashSet<String>) {
    let to_remove: Vec<_> = files.iter()
        .filter(|file| directories.iter().any(|dir| file.starts_with(dir)))
        .cloned()
        .collect();
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
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read directory: {}", dir.display()))? {
        let entry = entry.with_context(|| format!("Failed to read entry in directory: {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, files)?;
        } else if path.is_file() {
            let path_str = path.to_str().ok_or_else(|| anyhow!("Invalid UTF-8 in file path: {}", path.display()))?;
            files.insert(path_str.to_string());
        }
    }
    Ok(())
}

pub fn load_yaml_file(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path).with_context(|| format!("Failed to read YAML file at path: {}", path.display()))?;
    let yaml: Value = serde_yaml::from_str(&content).with_context(|| format!("Failed to parse YAML content at path: {}", path.display()))?;
    Ok(yaml)
}

pub fn load_cwl_file(base_path: &str, cwl_file_path: &Path) -> Result<Value> {
    let combined_path = Path::new(base_path).join(cwl_file_path);
    if !combined_path.exists() {
        anyhow::bail!("load cwl file: CWL file not found: {}", combined_path.display());
    }
    let content = fs::read_to_string(&combined_path)
        .with_context(|| format!("Failed to read CWL file: {}", combined_path.display()))?;
    let yaml: Value = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse CWL YAML at: {}", combined_path.display()))?;
    Ok(yaml)
}

pub fn read_file_content(file_path: &str) -> Result<String> {
    let mut file = File::open(file_path).with_context(|| format!("Failed to open file: {file_path}"))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .with_context(|| format!("Failed to read contents of file: {file_path}"))?;
    Ok(content)
}

pub fn resolve_input_file_path(requested_file: &str, input_yaml: Option<&Value>, cwl_value: Option<&Value>) -> Result<Option<String>> {
    let requested_path = Path::new(requested_file);
    if requested_path.exists() {
        return Ok(Some(requested_file.to_string()));
    }
    if let Some(Value::Mapping(mapping)) = input_yaml {
        for value in mapping.values() {
            if let Value::Mapping(file_entry) = value {
                for field in &["location", "path"] {
                    if let Some(Value::String(path_str)) = file_entry.get(Value::String((*field).to_string()))
                        && file_matches(requested_file, path_str)
                    {
                        return Ok(Some(path_str.clone()));
                    }
                }
            }
        }
    }
    if let Some(cwl) = cwl_value && let Some(inputs) = cwl.get("inputs").and_then(|v| v.as_sequence()) {
            for input in inputs {
                if let Some(Value::Mapping(default_map)) = input.get("default") {
                    for field in &["location", "path"] {
                        if let Some(Value::String(loc)) = default_map.get(Value::String((*field).to_string()))
                            && file_matches(requested_file, loc)
                        {
                            return Ok(Some(loc.clone()));
                        }
                    }
                }
            
        }
    }
    Ok(None)
}

pub fn build_inputs_yaml(cwl_input_path: &str, input_yaml_path: &Path) -> Result<Mapping> {
    let input_yaml: Value = load_yaml_file(input_yaml_path)
        .with_context(|| format!("Failed to load input YAML: {}", input_yaml_path.display()))?;
    let common_dir = find_common_directory_with_prefix(cwl_input_path, input_yaml_path)
        .unwrap_or_else(|_| PathBuf::from("."));
    let mut files: HashSet<String> = HashSet::new();
    let mut directories: HashSet<String> = HashSet::new();
    let mut parameters: HashMap<String, Value> = HashMap::new();
    let mut referenced_paths: HashSet<PathBuf> = HashSet::new();
    let main_cwl_path = Path::new(cwl_input_path);
    let main_dir = main_cwl_path.parent().unwrap_or_else(|| Path::new("."));
    if let Value::Mapping(mapping) = input_yaml {
        for (key, value) in mapping {
            if let Value::String(key_str) = key {
                if let Value::Mapping(mut sub_mapping) = value.clone() {
                    let class_opt = sub_mapping
                        .get(Value::String("class".into()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let location_opt = sub_mapping
                        .get(Value::String("location".into()))
                        .or_else(|| sub_mapping.get(Value::String("path".into())))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if let (Some(class), Some(location)) = (class_opt, location_opt) {
                        let relative_path = make_relative_to_common_dir(&location, &common_dir);
                        let sanitized = strip_temp_prefix(&relative_path);
                        let has_location_field = sub_mapping.contains_key(Value::String("location".into()));
                        let field = if has_location_field { "location" } else { "path" };
                        sub_mapping.insert(Value::String(field.into()), Value::String(sanitized.clone()));
                        match class.as_str() {
                            "File" => { files.insert(sanitized); }
                            "Directory" => { directories.insert(sanitized); }
                            _ => {}
                        }
                    }
                    parameters.insert(key_str, Value::Mapping(sub_mapping));
                } else {
                    parameters.insert(key_str, value);
                }
            }
        }
    }
    let workflow_yaml = load_cwl_file(".", main_cwl_path)?;
    let workflow: Workflow = serde_yaml::from_value(workflow_yaml)
        .with_context(|| format!("Failed to deserialize CWL workflow from '{}'", main_cwl_path.display()))?;
    for step in &workflow.steps {
        if let StringOrDocument::String(run_str) = &step.run {
            let run_path = main_dir.join(run_str);
            let resolved = run_path.canonicalize().unwrap_or(run_path.clone());
            referenced_paths.insert(resolved);
        }
    }
    referenced_paths.insert(main_cwl_path.canonicalize()?);
    if !referenced_paths.is_empty() {
        let common_root = find_common_directory(&referenced_paths.iter().cloned().collect::<BTreeSet<_>>())?;
        let relative_root = make_relative_to_common_dir(&common_root.to_string_lossy(), &common_dir);
        if !relative_root.is_empty() {
            directories.insert(strip_temp_prefix(&relative_root));
        }

        for path in referenced_paths {
            if path.exists() && path.is_file() {
                let rel = make_relative_to_common_dir(&path.to_string_lossy(), &common_dir);
                files.insert(strip_temp_prefix(&rel));
            }
        }
    }
    remove_files_contained_in_directories(&mut files, &directories);
    let mut inputs_mapping = Mapping::new();
    inputs_mapping.insert(Value::String("files".into()), Value::Sequence(files.into_iter().map(Value::String).collect()));
    inputs_mapping.insert(Value::String("directories".into()), Value::Sequence(directories.into_iter().map(Value::String).collect()));
    inputs_mapping.insert(Value::String("parameters".into()), Value::Mapping(parameters.into_iter().map(|(k,v)| (Value::String(k),v)).collect()));

    Ok(inputs_mapping)
}

pub fn build_inputs_cwl(cwl_input_path: &str, inputs_yaml: Option<&String>) -> Result<Mapping> {
    let cwl_content = fs::read_to_string(cwl_input_path).with_context(|| format!("Failed to read CWL file '{cwl_input_path}'"))?;
    let cwl_value: Value = serde_yaml::from_str(&cwl_content)
        .with_context(|| format!("Failed to parse CWL file '{cwl_input_path}'"))?;
    let mut files: HashSet<String> = HashSet::new();
    let mut directories: HashSet<String> = HashSet::new();
    let mut parameters: HashMap<String, Value> = HashMap::new();
    if let Some(inputs) = cwl_value.get("inputs").and_then(|v| v.as_sequence()) {
        for input in inputs {
            if let Some(id) = input.get("id").and_then(|v| v.as_str()) {
                let input_type_val = input.get("type");
                let input_type = input_type_val
                    .and_then(|t| t.as_str())
                    .or_else(|| input_type_val.and_then(|t| t.get("type").and_then(|v| v.as_str())))
                    .unwrap_or("");
                if ["File", "Directory"].contains(&input_type) {
                    if let Some(default) = input.get("default") {
                        if let Value::Mapping(default_map) = default {
                            let mut sanitized_map = default_map.clone();
                            if let Some(Value::String(location)) = sanitized_map.get(Value::String("location".into())) {
                                let sanitized_location = sanitize_path(location);
                                sanitized_map.insert(Value::String("location".into()), Value::String(sanitized_location.clone()));
                                match input_type {
                                    "File" => { files.insert(sanitized_location); }
                                    "Directory" => { directories.insert(sanitized_location); }
                                    _ => {}
                                }
                            }
                            parameters.insert(id.to_string(), Value::Mapping(sanitized_map));
                        } else {
                            parameters.insert(id.to_string(), default.clone());
                        }
                    }
                } else if let Some(default) = input.get("default") {
                    parameters.insert(id.to_string(), default.clone());
                }
            }
        }
    }
    if let Some(yaml_path) = inputs_yaml {
        parameters.insert("inputs.yaml".to_string(), Value::String(yaml_path.to_string()));
    }
    remove_files_contained_in_directories(&mut files, &directories);
    let mut inputs_mapping = Mapping::new();
    inputs_mapping.insert(Value::String("files".into()), Value::Sequence(files.into_iter().map(Value::String).collect()));
    inputs_mapping.insert(Value::String("directories".into()), Value::Sequence(directories.into_iter().map(Value::String).collect()));
    inputs_mapping.insert(Value::String("parameters".into()), Value::Mapping(parameters.into_iter().map(|(k,v)| (Value::String(k),v)).collect()));

    Ok(inputs_mapping)
}

pub fn get_all_outputs(workflow: &Workflow, packed: &PackedCWL) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    for output in &workflow.outputs {
        let output_id = output.id.clone();
        let Some(source) = &output.output_source else {
            continue;
        };
        let mut parts = source.split('/');
        let (Some(step_id), Some(step_output_id)) = (parts.next(), parts.next()) else {
            continue;
        };
        let tool_id = format!("#{step_id}.cwl");
        let tool = packed.graph.iter().find_map(|node| {
            if let CWLDocument::CommandLineTool(tool) = node && tool.id.as_deref() == Some(tool_id.as_str()) {
                return Some(tool);
            }
            None
        });
        let Some(tool) = tool else {
            continue;
        };
        let tool_output = tool.outputs.iter().find(|o| {
            o.id.ends_with(step_output_id)
        });
        let Some(tool_output) = tool_output else {
            continue;
        };
        let glob = tool_output
            .output_binding
            .as_ref()
            .and_then(|b| {
                b.glob.as_ref().and_then(|g| match g {
                    SingularPlural::Singular(s) => Some(s.clone()),
                    SingularPlural::Plural(v) => v.first().cloned(),
                })
            });
        let Some(glob) = glob else {
            continue;
        };
        results.push((output_id, glob));
    }
    if results.is_empty() {
        anyhow::bail!("❌ No valid outputs found in workflow");
    }
    Ok(results)
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

pub async fn get_cwl_name(reana: Option<Arc<Reana>>, workflow_name: &str) -> Result<String> {
    let reana = reana.ok_or_else(|| anyhow!("REANA instance is required"))?;
    let workflow_name = workflow_name.to_string();

    get_workflow_status(&reana, &workflow_name)
        .await
        .map_err(|e| anyhow!(e.to_string()))
        .and_then(|status_json| {
            match status_json.get("status").and_then(|v| v.as_str()) {
                Some("finished") => Ok(()),
                _ => Err(anyhow!("Workflow not finished")),
            }
        })?;

    let graph = get_workflow_specification(&reana, &workflow_name)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;

    let cwl_file = graph
        .get("specification")
        .and_then(|spec| spec.get("workflow"))
        .and_then(|wf| wf.get("file"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(cwl_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::load_workflow;
    use std::collections::HashSet;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use commonwl::packed::pack_workflow;

    fn normalize_path(path: &str) -> String {
        Path::new(path).to_str().unwrap_or_default().replace("\\", "/")
    }

   #[test]
    fn test_load_cwl_file_valid_main_cwl() -> anyhow::Result<()> {
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cwl_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        assert!(cwl_path.exists(), "Expected test CWL file to exist at {:?}", cwl_path);
        let result = load_cwl_file(base_dir.to_str().unwrap(), Path::new("testdata/hello_world/workflows/main/main.cwl"));
        assert!(result.is_ok(), "load_cwl_file() failed with error: {:?}", result.as_ref().err());
        let yaml = result?;
        assert_eq!(yaml["class"], serde_yaml::Value::String("Workflow".to_string()), "Expected CWL class to be 'Workflow'");
        assert!(yaml.get("inputs").is_some(), "Expected CWL file to contain 'inputs' section");
        assert!(yaml.get("steps").is_some(), "Expected CWL file to contain 'steps' section");

        Ok(())
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
        let cwl_input_path = base_dir.join("../../testdata/hello_world/workflows/main/main.cwl");
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
    fn test_get_all_outputs_with_existing_file() -> Result<(), Box<dyn std::error::Error>> {
        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workflow_file_path = base_dir.join("testdata/hello_world/workflows/main/main.cwl");
        let workflow = load_workflow(&workflow_file_path).unwrap();
        let specification = pack_workflow(&workflow, &workflow_file_path, None).map_err(|e| anyhow::anyhow!("Could not pack file {workflow_file_path:?}: {e}"))?;
        assert!(workflow_file_path.exists(), "CWL file not found at: {workflow_file_path:?}");
        let result = get_all_outputs(&workflow, &specification);
        assert!(result.is_ok());
        let outputs = result.unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], ("out".to_string(), "results.svg".to_string()));
        Ok(())
    }
}