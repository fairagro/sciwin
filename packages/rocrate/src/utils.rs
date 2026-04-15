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


/// Find the main CWL file inside a RO-Crate folder
pub fn find_cwl_in_rocrate(crate_root: &Path) -> Result<PathBuf> {
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

    Ok(cwl_path)
}

pub fn find_cwl_and_yaml_in_rocrate(crate_root: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
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
    let mut yaml_path: Option<PathBuf> = None;
    let walker = walkdir::WalkDir::new(crate_root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter();
    for entry in walker {
        let entry = entry.with_context(|| "Failed to read directory entry")?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(ext) = path.extension() {
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
    file_path: Option<String>,
}

fn parse_repo_url(url: &str) -> RepoInfo {
    // GitLab raw: /-/raw/<ref>/<path>
    if let Some((repo, rest)) = url.split_once("/-/raw/") {
        let mut parts = rest.splitn(2, '/');
        let reference = parts.next().map(|s| s.to_string());
        let file_path = parts.next().map(|s| s.to_string());

        return RepoInfo {
            repo_url: format!("{}.git", repo),
            reference,
            file_path,
        };
    }
    if let Some((repo, rest)) = url.split_once("/blob/") {
        let mut parts = rest.splitn(2, '/');

        return RepoInfo {
            repo_url: format!("{}.git", repo),
            reference: parts.next().map(|s| s.to_string()),
            file_path: parts.next().map(|s| s.to_string()),
        };
    }
    // GitHub raw: raw.githubusercontent.com/<org>/<repo>/<ref>/<path>
    if url.contains("raw.githubusercontent.com") {
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() > 6 {
            return RepoInfo {
                repo_url: format!("https://github.com/{}/{}.git", parts[3], parts[4]),
                reference: Some(parts[5].to_string()),
                file_path: Some(parts[6..].join("/")),
            };
        }
    }
    // Bitbucket raw: /raw/<ref>/<path>
    if let Some((repo, rest)) = url.split_once("/raw/") {
        let mut parts = rest.splitn(2, '/');
        let reference = parts.next().map(|s| s.to_string());
        let file_path = parts.next().map(|s| s.to_string());

        return RepoInfo {
            repo_url: repo.to_string(),
            reference,
            file_path,
        };
    }
    RepoInfo {
        repo_url: url.to_string(),
        reference: None,
        file_path: None,
    }
}
 
pub fn clone_from_rocrate_or_cwl(
    ro_crate_meta: &Path,
    cwl_path: &Path,
) -> Result<(TempDir, Option<PathBuf>, Option<PathBuf>)> {
    let meta_json: Value = serde_json::from_str(
        &fs::read_to_string(ro_crate_meta)
            .with_context(|| format!("Failed to read {:?}", ro_crate_meta))?
    ).context("Invalid RO-Crate JSON")?;
    let graph = meta_json
        .get("@graph")
        .and_then(|v| v.as_array())
        .context("Missing @graph in RO-Crate")?;
    let root = graph.iter()
        .find(|item| item.get("@id").and_then(|v| v.as_str()) == Some("./"));
    let git_url =
        root.and_then(|r| r.get("isBasedOn"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        .or_else(|| {
            let main_entity_id = root
                .and_then(|r| r.get("mainEntity"))
                .and_then(|me| me.get("@id"))
                .and_then(|id| id.as_str())?;
            graph.iter()
                .find(|item| item.get("@id").and_then(|v| v.as_str()) == Some(main_entity_id))
                .and_then(|node| node.get("url"))
                .and_then(|url| url.as_str())
                .map(str::to_owned)
        })
        .or_else(|| {
            if !cwl_path.exists() {
                return None;
            }
            fs::read_to_string(cwl_path).ok().and_then(|content| {
                content.lines()
                    .find_map(|l| {
                        l.trim_start()
                            .strip_prefix("s:codeRepository:")
                            .map(|v| v.trim().to_string())
                    })
            })
        })
        .context("No repository URL found in RO-Crate or CWL file")?;
    let repo = parse_repo_url(&git_url);
    eprintln!("📦 Cloning {}", repo.repo_url);
    let temp = tempfile::tempdir().context("Failed to create temp dir")?;
    let repo_path = temp.path();
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
    cmd.arg(&repo.repo_url)
        .arg(repo_path);
    let status = cmd.status()
        .with_context(|| format!("Git clone failed: {}", repo.repo_url))?;
    if !status.success() {
        anyhow::bail!("❌ Git clone failed: {}", repo.repo_url);
    }
    let _ = fs::remove_dir_all(repo_path.join(".git"));
    let (_cwl_candidate, mut inputs_yaml_candidate) =
        if let Some(path) = repo.file_path {
            (Some(repo_path.join(path)), None)
        } else {
            find_cwl_and_inputs(repo_path, cwl_path)
        };
    if inputs_yaml_candidate.is_none() {
        inputs_yaml_candidate = find_yaml_file(repo_path);
    }

    Ok((temp, Some(cwl_path.to_path_buf()), inputs_yaml_candidate))
}

//todo change this to search for neeeded params in case multiple files
fn find_yaml_file(root: &Path) -> Option<PathBuf> {
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
            } else {
                let ext = path.extension().and_then(|e| e.to_str());
                if let Some(ext) = ext && (ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml")) {
                    return Some(path);
                }
            }
        }
    }
    None
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