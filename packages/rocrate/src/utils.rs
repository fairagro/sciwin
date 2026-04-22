use anyhow::{anyhow, Context, Result};
use cwl_core::{StringOrDocument, load_doc, CWLDocument};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::ZipArchive;
use std::io::Write;
use fancy_regex::Regex;
use keyring::Entry;
use serde_json::Value;
use std::collections::HashMap;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;
use std::process::Command;

pub type StepTimestamp = HashMap<String, (Option<String>, Option<String>)>;

pub fn find_cwl_and_yaml_in_rocrate(crate_root: &Path, raw_inputs: &[String]) -> Result<(PathBuf, Option<PathBuf>)> {
    let mut yaml_path = None; 
    for raw_input in raw_inputs {
        let path = Path::new(raw_input);
        if path.exists() && path.is_file() {
            yaml_path = Some(path.to_path_buf()); 
        }
    }
    let meta_path = crate_root.join("ro-crate-metadata.json");
    let json_str = fs::read_to_string(&meta_path).with_context(|| format!("Failed to read RO-Crate metadata: {meta_path:?}"))?;
    let json_value: Value = serde_json::from_str(&json_str).context("Failed to parse RO-Crate metadata JSON")?;
    let graph = json_value
        .get("@graph")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("RO-Crate metadata missing @graph"))?;
    let dataset_node = graph
        .iter()
        .find(|node| node.get("@id").and_then(|id| id.as_str()) == Some("./"))
        .ok_or_else(|| anyhow!("No Dataset node with @id './' found"))?;
    let main_entity_id = dataset_node
        .get("mainEntity")
        .and_then(|me| me.get("@id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| anyhow!("Dataset node './' missing mainEntity.@id"))?;
    let cwl_path = crate_root.join(main_entity_id);
    if !cwl_path.exists() {
        return Err(anyhow!("CWL file not found at {cwl_path:?}"));
    }
    let walker = WalkDir::new(crate_root).follow_links(false).sort_by_file_name().into_iter();
    for entry in walker {
        let entry = entry.with_context(|| "Failed to read directory entry")?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(ext) = path.extension() && yaml_path.is_none() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            if ext_str == "yml" || ext_str == "yaml" {
                yaml_path = Some(path.to_path_buf());
                break;
            }
        }
    }
    Ok((cwl_path, yaml_path))
}

/// Verify that all step files in Workflow CWL exist
pub fn verify_cwl_references(cwl_path: &Path) -> Result<bool> {
    let doc = load_doc(cwl_path).map_err(|e| anyhow::anyhow!("Failed to load CWL document: {e}"))?;
    let CWLDocument::Workflow(workflow) = doc else {
        return Err(anyhow!("CWL document is not a Workflow: {cwl_path:?}"));
    };
    let parent = cwl_path.parent().unwrap_or_else(|| Path::new("."));
    let mut all_exist = true;
    for step in &workflow.steps {
        if let StringOrDocument::String(run_str) = &step.run {
            let run_path = parent.join(run_str);
            if !run_path.exists() {
                eprintln!("⚠ Missing referenced run file: {run_path:?}");
                all_exist = false;
            }
        }
    }

    Ok(all_exist)
}

#[derive(Debug)]
struct RepoInfo {
    repo_url: String,
    reference: Option<String>,
}

fn parse_repo_url(url: &str) -> RepoInfo {
    if let Some((repo, rest)) = url.split_once("/-/raw/") {
        let mut parts = rest.splitn(2, '/');
        let reference = parts.next().map(|s| s.to_string());
        return RepoInfo {
            repo_url: format!("{}.git", repo),
            reference,
        };
    }
    if let Some((repo, rest)) = url.split_once("/blob/") {
        let mut parts = rest.splitn(2, '/');
        return RepoInfo {
            repo_url: format!("{}.git", repo),
            reference: parts.next().map(|s| s.to_string()),
        };
    }
    if url.contains("raw.githubusercontent.com") {
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() > 6 {
            return RepoInfo {
                repo_url: format!("https://github.com/{}/{}.git", parts[3], parts[4]),
                reference: Some(parts[5].to_string()),
            };
        }
    }
    if let Some((repo, rest)) = url.split_once("/raw/") {
        let mut parts = rest.splitn(2, '/');
        let reference = parts.next().map(|s| s.to_string());
        return RepoInfo {
            repo_url: repo.to_string(),
            reference,
        };
    }
    RepoInfo {
        repo_url: url.to_string(),
        reference: None,
    }
}

pub fn clone_from_rocrate_or_cwl(ro_crate_meta: &Path, cwl_path: &Path) -> Result<(TempDir, Option<PathBuf>, Option<PathBuf>)> {
    let meta_json: Value = serde_json::from_str(
        &fs::read_to_string(ro_crate_meta).with_context(|| format!("Failed to read {:?}", ro_crate_meta))?
    ).context("Invalid RO-Crate JSON")?;
    let graph = meta_json.get("@graph").and_then(|v| v.as_array()).context("Missing @graph in RO-Crate")?;
    let root = graph.iter().find(|item| item.get("@id").and_then(|v| v.as_str()) == Some("./"));
    let mut candidates: Vec<String> = Vec::new();
    if let Some(url) = root.and_then(|r| r.get("isBasedOn")).and_then(|v| v.as_str()) {
        candidates.push(url.to_string());
    }
    if let Some(main_entity_id) = root.and_then(|r| r.get("mainEntity"))
     .and_then(|me| me.get("@id")).and_then(|id| id.as_str())
     && let Some(url) = graph.iter()
        .find(|item| item.get("@id").and_then(|v| v.as_str()) == Some(main_entity_id))
        .and_then(|node| node.get("url"))
        .and_then(|url| url.as_str())
    {
        candidates.push(url.to_string());
    }
    if cwl_path.exists() && let Ok(content) = fs::read_to_string(cwl_path) {
        for line in content.lines() {
            if let Some(v) = line.trim_start().strip_prefix("s:codeRepository:") {
                candidates.push(v.trim().to_string());
            }
        }
    }
    if candidates.is_empty() {
        anyhow::bail!("No repository URL found in RO-Crate or CWL file");
    }
    let temp = tempfile::tempdir().context("Failed to create temp dir")?;
    let repo_path = temp.path();
    let mut cloned = false;
    for candidate in candidates {
        if !looks_like_git_repo(&candidate) {
            continue;
        }
        let repo = parse_repo_url(&candidate);
        let mut cmd = Command::new("git");
        cmd.arg("clone")
            .arg("--depth").arg("1")
            .arg("--single-branch")
            .arg("--no-tags")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_LFS_SKIP_SMUDGE", "1");
        if let Some(ref branch) = repo.reference {
            cmd.arg("--branch").arg(branch);
        }
        cmd.arg(&repo.repo_url).arg(repo_path);
        match cmd.status() {
            Ok(status) if status.success() => {
                eprintln!("✅ Cloned {}", repo.repo_url);
                cloned = true;
                break;
            }
            _ => {
                eprintln!("⚠️ Failed to clone {}", repo.repo_url);
            }
        }
    }
    if !cloned {
        anyhow::bail!("❌ No valid git repository found in RO-Crate or CWL");
    }
    let _ = fs::remove_dir_all(repo_path.join(".git"));
    let (cwl_candidate, mut inputs_yaml_candidate) =
        if let Some(path) = find_cwl_and_inputs(repo_path, cwl_path).0 {
            (Some(path), None)
        } else {
            find_cwl_and_inputs(repo_path, cwl_path)
        };
    if inputs_yaml_candidate.is_none() {
        inputs_yaml_candidate = find_yaml_file(repo_path, cwl_path)
            .and_then(|p| validate_yaml_paths(&p, temp.path()));
    }
    Ok((temp, cwl_candidate, inputs_yaml_candidate))
}

fn looks_like_git_repo(url: &str) -> bool {
   std::path::Path::new(url).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("git"))
        || url.contains("github.com")
        || url.contains("gitlab.")
        || url.contains("bitbucket.")
        || url.contains("/git/")
        || url.contains("git")
}

fn find_yaml_file(root: &Path, cwl_path: &Path) -> Option<PathBuf> {
    let cwl_content = fs::read_to_string(cwl_path).ok()?;
    let cwl_yaml: serde_yaml::Value = serde_yaml::from_str(&cwl_content).ok()?;
    let input_ids: Vec<String> = cwl_yaml
        .get("inputs")?
        .as_mapping()?
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect();
    if input_ids.is_empty() {
        return None;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if (ext_str == "yaml" || ext_str == "yml") &&
                 let Ok(content) = fs::read_to_string(&path) &&
                 let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(&content) &&
                 let Some(map) = yaml.as_mapping() {
                    let keys: Vec<&str> = map.keys().filter_map(|k| k.as_str()).collect();
                    if input_ids.iter().any(|id| keys.contains(&id.as_str())) {
                        return Some(path.to_path_buf());
                    }
                }
            }
        }
    }
    None
}

pub fn extract_repo_name(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).or_else(|| url.strip_prefix("git@")).unwrap_or(url);
    let url = if let Some(pos) = url.find(':') {
        &url[pos + 1..]
    } else {
        url
    };
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    let mut repo = parts.last()?.to_string();
    if std::path::Path::new(&repo).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("git")) {
        repo = std::path::Path::new(&repo).file_stem().and_then(|s| s.to_str()).unwrap_or(&repo).to_string();
    }
    Some(repo)
}

pub fn resolve_yaml_path<P: AsRef<Path>, Q: AsRef<Path>>(yaml_file: P, yaml_path: Q) -> std::io::Result<PathBuf> {
    let yaml_file = yaml_file.as_ref();
    let yaml_path = yaml_path.as_ref();
    if yaml_path.is_absolute() && yaml_path.exists() {
        return Ok(yaml_path.to_path_buf());
    }
    let yaml_dir = yaml_file.parent().ok_or_else(|| std::io::Error::other("Invalid YAML path"))?;
    let joined = yaml_dir.join(yaml_path);
    let normalized = normalize_path(&joined);

    Ok(normalized)
}

/// Normalize a path without requiring it to exist
fn normalize_path(path: &Path) -> PathBuf {
    let components = path.components().peekable();
    let mut normalized = PathBuf::new();
    for component in components {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub fn validate_yaml_paths(yaml_path: &Path, base_dir: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(yaml_path).ok()?;
    let mut yaml: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let mut missing_found = false;
    fn clean(
        value: &mut serde_yaml::Value,
        base_dir: &Path,
        missing_found: &mut bool,
        yaml_path: &Path,
    ) {
        match value {
            serde_yaml::Value::Mapping(map) => {
                let class = map
                    .get(serde_yaml::Value::String("class".into()))
                    .and_then(|v| v.as_str());
                if matches!(class, Some("File") | Some("Directory")) {
                    let key = serde_yaml::Value::String("path".into());
                    let loc = serde_yaml::Value::String("location".into());
                    let target_key = if map.contains_key(&key) {
                        Some(key.clone())
                    } else if map.contains_key(&loc) {
                        Some(loc.clone())
                    } else {
                        None
                    };
                    if let Some(k) = target_key && let Some(serde_yaml::Value::String(path_str)) = map.get(&k) {
                        let normalized = resolve_yaml_path(yaml_path, path_str)
                            .unwrap_or_else(|_| path_str.clone().into());
                        let candidate = base_dir.join(&normalized);
                        if !candidate.exists() {
                            *missing_found = true;
                            map.insert(k, serde_yaml::Value::Null);
                        } else {
                            map.insert(
                                k,
                                serde_yaml::Value::String(candidate.to_string_lossy().to_string()),
                            );
                        }
                    }
                }
                for (_, v) in map.iter_mut() {
                    clean(v, base_dir, missing_found, yaml_path);
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                for v in seq {
                    clean(v, base_dir, missing_found, yaml_path);
                }
            }
            _ => {}
        }
    }
    clean(&mut yaml, base_dir, &mut missing_found, yaml_path);
    if missing_found {
        eprintln!("\n❌ Missing files detected in YAML");
        eprintln!("No changes written. YAML preview:\n");
        if let Ok(s) = serde_yaml::to_string(&yaml) {
            eprintln!("{s}");
        }
        return None;
    }
    let updated_yaml = serde_yaml::to_string(&yaml).ok()?;
    if fs::write(yaml_path, updated_yaml).is_err() {
        return None;
    }
    Some(yaml_path.to_path_buf())
}

/// Find CWL and inputs.yaml files in a cloned repository
pub fn find_cwl_and_inputs(repo_path: &Path, cwl_path: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let cwl_file_name = cwl_path.file_name().and_then(|s| s.to_str());
    let mut cwl_candidate: Option<PathBuf> = None;
    let mut inputs_yaml_candidate: Option<PathBuf> = None;

    for entry in WalkDir::new(repo_path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if let (Some(cwl_name), Some(name)) = (cwl_file_name, path.file_name().and_then(|s| s.to_str()))
                && name == cwl_name {
                cwl_candidate = Some(path.to_path_buf());
            }
            if let Some(name) = path.file_name().and_then(|s| s.to_str())
                && (name == "inputs.yaml" || name == "inputs.yml") {
                inputs_yaml_candidate = Some(path.to_path_buf());
            }
            if cwl_candidate.is_some() && inputs_yaml_candidate.is_some() {
                break;
            }
        }
    }

    (cwl_candidate, inputs_yaml_candidate)
}

/// Unzip a RO-Crate ZIP into a directory
pub fn unzip_rocrate(zip_path: &Path, dest_dir: &Path) -> Result<PathBuf> {
    let file = fs::File::open(zip_path).with_context(|| format!("Failed to open ZIP file: {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)?;
    archive
        .extract(dest_dir)
        .with_context(|| format!("Failed to extract ZIP file: {}", zip_path.display()))?;

    let mut crate_root = dest_dir.to_path_buf();
    let entries = fs::read_dir(dest_dir).with_context(|| format!("Failed to read directory: {dest_dir:?}"))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join("ro-crate-metadata.json").exists() {
            crate_root = path;
            break;
        }
    }

    if !crate_root.join("ro-crate-metadata.json").exists() {
        anyhow::bail!("RO-Crate metadata not found in extracted ZIP: {dest_dir:?}");
    }

    Ok(crate_root)
}

pub fn zip_dir(src: &Path, dst: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(dst)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(src)?;
        // skip root dir entry
        if rel.as_os_str().is_empty() {
            continue;
        }
        let zip_path = rel.to_string_lossy();
        if path.is_file() {
            zip.start_file(zip_path, options)?;
            let mut f = std::fs::File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
        } else {
            zip.add_directory(zip_path, options)?;
        }
    }
    zip.finish()?;
    Ok(())
}

#[allow(clippy::disallowed_macros)]
pub fn prompt(message: &str) -> String {
    print!("{message}");
    std::io::stdout().flush().expect("Failed to flush stdout");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).expect("Failed to read input");
    input.trim().to_string()
}

//use reana log files to extract start and end times of stepts
pub fn extract_times_from_logs(contents: &str) -> Result<StepTimestamp, Box<dyn std::error::Error>> {
    let re_ts = Regex::new(r"(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})")?;
    let re_wf_start = Regex::new(r"running workflow on context")?;
    let re_wf_end = Regex::new(r"workflow done")?;
    let patterns = [
        (Regex::new(r"starting step (?P<step>\w+)")?, 0),
        (Regex::new(r"starting step (?P<step>[a-zA-Z0-9_.\-/]+)")?, 0),
        (Regex::new(r"\[step (?P<step>\w+)\] completed success")?, 1),
        (Regex::new(r"\[step (?P<step>[a-zA-Z0-9_.\-/]+)\] completed success")?, 1),
    ];
    let mut steps: StepTimestamp = HashMap::new();
    let mut wf_start = None;
    let mut wf_end = None;
    contents
        .lines()
        .filter_map(|line| re_ts.captures(line).ok()?.and_then(|c| c.name("ts").map(|m| (line, m.as_str().to_string()))))
        .try_for_each(|(line, ts)| -> Result<(), fancy_regex::Error> {
            if re_wf_start.is_match(line)? {
                wf_start = Some(ts.clone());
            }
            if re_wf_end.is_match(line)? {
                wf_end = Some(ts.clone());
            }
            for (re, idx) in &patterns {
                if let Some(step) = re.captures(line)?.and_then(|c| c.name("step")) {
                    let entry = steps.entry(step.as_str().to_string()).or_insert((None, None));
                    match idx {
                        0 => entry.0 = Some(ts.clone()),
                        1 => entry.1 = Some(ts.clone()),
                        _ => {}
                    }
                }
            }
            Ok(())
        })?;
    steps.insert("workflow".into(), (wf_start, wf_end));
    Ok(steps)
}

pub fn get_or_prompt_credential(service: &str, key: &str, prompt_msg: &str) -> Result<String, Box<dyn std::error::Error>> {
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