use crate::reana::{Content, Reana, WorkflowEndpoint};
//use crate::utils::{collect_files_recursive, load_cwl_yaml, load_yaml_file, resolve_input_file_path, sanitize_path, get_location};
use anyhow::{Context, Result};
use serde_json::Value;
use serde_json::json;
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::{
    collections::HashMap,
    error::Error,
    fs,
    io::Write,
    path::PathBuf,
};
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;

pub async fn create_workflow(reana: &Reana, workflow: &Value, workflow_name: Option<&str>) -> Result<Value, Box<dyn Error>> {
    let mut params = HashMap::new();
    if let Some(name) = workflow_name {
        params.insert("workflow_name".to_string(), name.to_string());
    }
    let response = reana.post(&WorkflowEndpoint::Root, Content::Json(workflow.clone()), Some(params)).await?;
    let json_response: Value = response.json().await.context("Failed to parse JSON response from create workflow request")?;
    Ok(json_response)
}

pub async fn ping_reana(reana: &Reana) -> Result<Value> {
    let response = reana.ping().await?;
    let json_response: Value = response.json().await.with_context(|| "Failed to parse JSON from".to_string())?;

    Ok(json_response)
}

pub async fn start_workflow(
    reana: &Reana,
    workflow_name: &str,
    operational_parameters: Option<HashMap<String, Value>>,
    input_parameters: Option<HashMap<String, Value>>,
    restart: bool,
    reana_specification: &serde_yaml::Value,
) -> Result<Value> {
    let body = json!({
        "operational_options": operational_parameters.unwrap_or_default(),
        "input_parameters": input_parameters.unwrap_or_default(),
        "restart": restart,
        "reana_specification": reana_specification
    });

    let response = reana.post(&WorkflowEndpoint::Start(workflow_name), Content::Json(body), None).await?;

    let json_response: Value = response.json().await.context("Failed to parse JSON response from workflow start request")?;

    Ok(json_response)
}

pub async fn get_workflow_logs(reana: &Reana, workflow_id: &str) -> Result<Value> {
    let response = reana.get(&WorkflowEndpoint::Logs(workflow_id)).await?;
    let json_response: Value = response.json().await.context("Failed to parse JSON response from workflow logs request")?;

    Ok(json_response)
}

pub async fn get_workflow_status(reana: &Reana, workflow_id: &str) -> Result<Value> {
    let response = reana.get(&WorkflowEndpoint::Status(workflow_id)).await?;

    let status = response.status();
    let json_response: Value = response.json().await.context("Failed to parse JSON response from workflow status request")?;

    if status.is_success() {
        Ok(json_response)
    } else {
        // Return error but include JSON body
        anyhow::bail!("Server returned status {}: {}", status, json_response);
    }
}

pub async fn get_workflow_specification(reana: &Reana, workflow_id: &str) -> Result<Value> {
    let response = reana.get(&WorkflowEndpoint::Specification(workflow_id)).await?;

    let status = response.status();
    let json_response: Value = response.json().await.context("Failed to parse JSON response from workflow specification request")?;

    if status.is_success() {
        Ok(json_response)
    } else {
        anyhow::bail!("Error trying to get workflow specification. Server returned status {status}: {json_response}");
    }
}

pub async fn upload_files_parallel(
    reana: Arc<Reana>,
    input_yaml: &Option<PathBuf>,
    file: &Path,
    workflow_name: &str,
    workflow_json: &Value,
    working_dir: Option<&PathBuf>,
) -> Result<()> {
    eprintln!("📤 Collecting files to upload...");
    let input_yaml_value = if let Some(input_path) = input_yaml {
        Some(crate::utils::load_yaml_file(input_path).context("Failed to load input YAML")?)
    } else {
        None
    };
    let base_path = std::env::current_dir()?.to_string_lossy().to_string();
    let cwl_yaml = crate::utils::load_cwl_yaml(&base_path, file).context("Failed to load CWL YAML")?;
    // Collect files and directories from workflow JSON
    let mut files: HashSet<String> = HashSet::new();
    if let Some(inputs) = workflow_json.get("inputs") {
        if let Some(Value::Array(file_list)) = inputs.get("files") {
            for f in file_list.iter().filter_map(|v| v.as_str()) {
                files.insert(f.to_string());
            }
        }
        if let Some(Value::Array(dir_list)) = inputs.get("directories").or_else(|| inputs.get("directory")) {
            for dir in dir_list.iter().filter_map(|v| v.as_str()) {
                let mut path = PathBuf::from(dir);
                if !path.exists() {
                    if let Some(base) = working_dir {
                        let candidate = base.join(&path);
                        if candidate.exists() {
                            path = candidate;
                        } else if let Ok(Some(resolved_path)) = crate::utils::resolve_input_file_path(
                            path.to_string_lossy().as_ref(),
                            input_yaml_value.as_ref(),
                            Some(&cwl_yaml),
                        ) {
                            if let Some(base) = working_dir && let Ok(loc) = crate::utils::get_location(&base.to_string_lossy(), Path::new(&resolved_path)) {
                                path = PathBuf::from(loc);
                            }
                        } else {
                            eprintln!("⚠️ Directory not found: {:?}", dir);
                            continue;
                        }
                    } else {
                        eprintln!("⚠️ Directory not found: {:?} (no working_dir provided)", dir);
                        continue;
                    }
                }
                if path.is_dir() {
                    crate::utils::collect_files_recursive(&path, &mut files)
                        .context("Failed to collect files recursively")?;
                }
            }
        }
    }
    if files.is_empty() {
        eprintln!("⚠️ No files to upload found in workflow JSON.");
        return Ok(());
    }
    eprintln!("📤 Uploading {} files safely in parallel...", files.len());
    let futures: FuturesUnordered<_> = files.into_iter().map(|file_name| {
        let reana = reana.clone();
        let workflow_name = workflow_name.to_string();
        let working_dir = working_dir.cloned();
        let input_yaml_value = input_yaml_value.clone();
        let cwl_yaml = cwl_yaml.clone();
        tokio::spawn(async move {
            // Resolve file path
            let mut file_path = PathBuf::from(&file_name);
            if !file_path.exists() {
                let mut resolved = None;
                if let Some(base) = &working_dir {
                    let candidate = base.join(&file_path);
                    if candidate.exists() {
                        resolved = Some(candidate);
                    }
                }
                if resolved.is_none()
                    && let Ok(Some(resolved_path)) = crate::utils::resolve_input_file_path(
                        file_path.to_string_lossy().as_ref(),
                        input_yaml_value.as_ref(),
                        Some(&cwl_yaml),
                    )
                    && let Some(base) = &working_dir
                    && let Ok(loc) = crate::utils::get_location(&base.to_string_lossy(), Path::new(&resolved_path))
                {
                    resolved = Some(PathBuf::from(loc));
                }
                if let Some(found) = resolved {
                    file_path = found;
                } else {
                    eprintln!("⚠️ File not found: {:?}", file_path);
                    return Ok::<(), anyhow::Error>(());
                }
            }
            // Read file content asynchronously
            let file_content = tokio::fs::read(&file_path)
                .await
                .with_context(|| format!("Failed to read file '{}'", file_path.display()))?;
            // Relative path for params
            let relative = if let Some(base) = &working_dir {
                pathdiff::diff_paths(&file_path, base).unwrap_or(file_path.clone())
            } else {
                file_path.clone()
            };
            let mut params = std::collections::HashMap::new();
            params.insert("file_name".to_string(), crate::utils::sanitize_path(&relative.to_string_lossy()));
            // Upload file via REANA
            let response = reana.post(
                &WorkflowEndpoint::Workspace(&workflow_name, None),
                Content::OctetStream(file_content),
                Some(params),
            ).await?;
            if response.status().is_success() {
                eprintln!("✔️ Uploaded {}", file_name);
            } else {
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                eprintln!("❌ Failed to upload {}. Response: {}", file_name, error_text);
            }

            Ok::<(), anyhow::Error>(())
        })
    }).collect();

    futures.for_each(|res| async {
        if let Err(e) = res {
            eprintln!("❌ Upload task failed: {:?}", e);
        }
    }).await;

    Ok(())
}

pub async fn download_files(reana: &Reana, workflow_name: &str, files: &[String], folder: Option<&str>) -> Result<()> {
    if files.is_empty() {
        eprintln!("ℹ️ No files to download.");
        return Ok(());
    }
    for file_name in files {
        let response = reana.get(&WorkflowEndpoint::Workspace(workflow_name, Some(file_name.to_string()))).await?;
        if response.status().is_success() {
            // reana adds all outputs in an outputs/ folder, remove this for now
            let relative_path = file_name.strip_prefix("outputs/").unwrap_or(file_name);
            let output_path = match folder {
                Some(dir) => Path::new(dir).join(relative_path),
                None => PathBuf::from(relative_path),
            };
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("❌ Failed to create folder: {}", parent.display()))?;
            }
            let content = response.bytes().await.context("❌ Failed to read response bytes")?;
            let mut file = File::create(&output_path)
                .with_context(|| format!("❌ Failed to create file: {}", output_path.display()))?;
            file.write_all(&content)
                .with_context(|| format!("❌ Failed to write to file: {}", output_path.display()))?;
            eprintln!("✅ Downloaded: {}", output_path.display());
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            eprintln!("❌ Failed to download {file_name}. Response: {error_text}");
        }
    }

    Ok(())
}

pub async fn get_workflow_workspace(reana: &Reana, workflow_id: &str) -> Result<Value> {
    let response = reana.get(&WorkflowEndpoint::Workspace(workflow_id, None)).await?;
    let json_response: Value = response.json().await.context("❌ Failed to parse JSON response")?;

    Ok(json_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;
    //use mockito::{self, Matcher, Server};
    use serde_json::{Value, json};
    use std::fs::{create_dir_all, write};
    use tempfile::{NamedTempFile, tempdir};
    use std::sync::Arc;
    use tokio;
    use reqwest::Client;

    #[tokio::test]
    async fn test_ping_reana_success() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/api/ping");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"status":"ok"}"#);
        });

        let reana = Reana::new(server.base_url(), "test-token".into());

        let response = super::ping_reana(&reana).await.unwrap();

        assert_eq!(response["status"], "ok");
        _mock.assert();
    }


    #[tokio::test]
    async fn test_start_workflow_failure() {

        let server = MockServer::start();

        let workflow_id = "nonexistent-workflow";
        let token = "test-token";

        let expected_json = json!({
            "operational_options": {},
            "input_parameters": {},
            "restart": false,
            "reana_specification": {
                "version": "0.9.4",
                "workflow": {
                    "type": "serial",
                    "specification": {
                        "steps": []
                    }
                },
                "inputs": {},
                "outputs": {}
            }
        });

        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path(format!("/api/workflows/{workflow_id}/start"))
                .query_param("access_token", token)
                .header("authorization", "Bearer test_token")
                .header("content-type", "application/json")
                .json_body(expected_json.clone());

            then.status(404)
                .header("content-type", "application/json")
                .body(r#"{"message": "Workflow not found."}"#);
        });

        // Actual HTTP request
        let client = Client::new();
        let res = client
            .post(format!(
                "{}/api/workflows/{}/start?access_token={}",
                &server.base_url(),
                workflow_id,
                token
            ))
            .header("authorization", "Bearer test_token")
            .header("content-type", "application/json")
            .json(&expected_json)
            .send()
            .await
            .expect("request failed");

        assert_eq!(res.status(), 404);
        let json: Value = res.json().await.unwrap();
        assert_eq!(json["message"], "Workflow not found.");

        let yaml_equiv: serde_yaml::Value = serde_yaml::from_str(&expected_json.to_string()).expect("YAML conversion failed");
        let url = &server.base_url();
        let reana = Reana::new(url.to_string(), "test-token".to_string());
        let result = start_workflow(&reana, workflow_id, None, None, false, &yaml_equiv).await;

        assert!(result.is_err(), "Expected error, but got Ok.");
    }

    #[tokio::test]
    async fn test_start_workflow_success() {
        use httpmock::{MockServer, Method::POST};
        use serde_json::json;
        use serde_yaml::Value as YamlValue;

        let server = MockServer::start_async().await;
        let workflow_id = "test-workflow";
        let token = "test-token";
        let reana_spec: YamlValue = YamlValue::Null;
        let expected_json = json!({
            "operational_options": {},
            "input_parameters": {},
            "restart": false,
            "reana_specification": reana_spec
        });

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path(format!("/api/workflows/{}/start", workflow_id))
                .query_param("access_token", token)
                .json_body(expected_json.clone());   // cleaner
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "message": "Workflow started successfully",
                    "status": "started"
                }));
        });
        let reana = crate::reana::Reana::new(server.base_url(), token.to_string());
        let result = crate::api::start_workflow(
            &reana,
            workflow_id,
            None,
            None,
            false,
            &reana_spec
        )
        .await
        .expect("Expected workflow to start successfully");
        assert_eq!(result["message"], "Workflow started successfully");
        assert_eq!(result["status"], "started");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_workflow_success() {
        let server = MockServer::start_async().await;

        let workflow_payload = json!({
            "name": "test-workflow",
            "type": "serial",
            "specification": { "steps": [] }
        });

        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/api/workflows")
                .query_param("access_token", "test-token")
                .header("content-type", "application/json")
                .json_body(workflow_payload.clone());
            then.status(201)
                .json_body(json!({
                    "message": "Workflow created",
                    "workflow_id": "1234"
                }));
        });

        let reana = Reana::new(server.base_url(), "test-token".into());
        let result = super::create_workflow(&reana, &workflow_payload, None).await.unwrap();

        assert_eq!(result["message"], "Workflow created");
        assert_eq!(result["workflow_id"], "1234");
        _mock.assert();
    }

    #[tokio::test]
    async fn test_create_workflow_failure_invalid_token() {
        let server = MockServer::start_async().await;

        let workflow_payload = json!({
            "name": "fail-case",
            "type": "serial",
            "specification": { "steps": [] }
        });

        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/api/workflows");
            then.status(401)
                .json_body(json!({ "message": "Unauthorized" }));
        });

        let reana = Reana::new(server.base_url(), "invalid-token".into());
        let result = super::create_workflow(&reana, &workflow_payload, None).await;

        assert!(result.is_err());
        _mock.assert();
    }

    #[tokio::test]
    async fn test_get_workflow_status_success() {
        let server = MockServer::start_async().await;

        let workflow_id = "123";
        let token = "test-token";

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/api/workflows/{}/status", workflow_id))
                .query_param("access_token", token);
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"status":"completed"}"#);
        });

        let reana = Reana::new(server.base_url(), token.to_string());
        let result = super::get_workflow_status(&reana, workflow_id).await.unwrap();

        assert_eq!(result["status"], "completed");
        _mock.assert();
    }

    #[tokio::test]
    async fn test_get_workflow_status_failure() {
        let server = MockServer::start_async().await;

        let workflow_id = "999";
        let token = "test-token";

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/api/workflows/{}/status", workflow_id))
                .query_param("access_token", token);
            then.status(404)
                .header("content-type", "application/json")
                .body(r#"{"error":"workflow not found"}"#);
        });

        let reana = Reana::new(server.base_url(), token.to_string());
        let result = super::get_workflow_status(&reana, workflow_id).await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("404") && err_msg.contains("workflow not found"));
        _mock.assert();
    }

    #[tokio::test]
    async fn test_upload_files_parallel() {
        let server = MockServer::start_async().await;
        let reana_token = "test-token";
        let workflow_name = "my_workflow";
        let base_dir = tempdir().unwrap();
        let data_dir = base_dir.path().join("data");
        let wf_dir = base_dir.path().join("testdata/hello_world/workflows");

        create_dir_all(&data_dir).unwrap();
        create_dir_all(&wf_dir).unwrap();

        let pop_file = data_dir.join("population.csv");
        let spk_file = data_dir.join("speakers_revised.csv");
        let dir_file = wf_dir.join("hello.txt");

        write(&pop_file, "data").unwrap();
        write(&spk_file, "data").unwrap();
        write(&dir_file, "workflow file").unwrap();

        let _mock_upload = server.mock(|when, then| {
            when.method(POST)
                .path(format!("/api/workflows/{workflow_name}/workspace"))
                .query_param("access_token", reana_token)
                .query_param_exists("file_name");
            then.status(200).header("content-type", "text/plain").body("uploaded");
        });

        let workflow_json: Value = json!({
            "inputs": {
                "directories": [ wf_dir.to_str().unwrap() ],
                "files": [
                    pop_file.to_str().unwrap(),
                    spk_file.to_str().unwrap()
                ],
                "parameters": {
                    "population": { "class": "File", "location": pop_file.to_str().unwrap() },
                    "speakers": { "class": "File", "location": spk_file.to_str().unwrap() }
                }
            }
        });

        let dummy_cwl = NamedTempFile::new().unwrap();
        write(dummy_cwl.path(), "cwlVersion: v1.2").unwrap();
        let reana_url = server.base_url();
        let reana = Arc::new(Reana::new(reana_url, reana_token.to_string()));
        let result = upload_files_parallel(
            reana.clone(),
            &None,
            dummy_cwl.path(),
            workflow_name,
            &workflow_json,
            None
        ).await;

        assert!(result.is_ok(), "upload_files_parallel failed: {:?}", result.err());
        assert_eq!(_mock_upload.calls(), 3, "Expected 3 file uploads");
    }

    #[tokio::test]
    async fn test_download_files_no_files() {
        let server = MockServer::start();
        let reana_token = "test-token";
        let workflow_name = "my_workflow";

        let files = vec![];

        let url = &server.base_url();
        let reana = Reana::new(url.to_string(), reana_token.to_string());
        let result = download_files(&reana, workflow_name, &files, None).await;

        assert!(result.is_ok(), "download_files failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_download_files_success() {
        use httpmock::MockServer;
        use std::env;
        use std::fs;
        use tempfile::tempdir;

        let server = MockServer::start();
        let reana_token = "test-token";
        let workflow_name = "my_workflow";
        let test_filename = "results.svg";
        let test_content = "<svg>mock-content</svg>";

        let _mock = server.mock(|when, then| {
            when.method("GET")
                .path(format!("/api/workflows/{workflow_name}/workspace/{test_filename}"))
                .query_param("access_token", reana_token);
            then.status(200).header("content-type", "image/svg+xml").body(test_content);
        });
        let original_dir = env::current_dir().expect("Failed to get current dir");

        let temp_dir = tempdir().expect("Failed to create temp dir");
        env::set_current_dir(&temp_dir).expect("Failed to set current dir");
        let files = vec!["results.svg".to_string()];

        let url = &server.base_url();
        let reana = Reana::new(url.to_string(), reana_token.to_string());
        let result = download_files(&reana, workflow_name, &files, None).await;

        env::set_current_dir(&original_dir).expect("Failed to restore original dir");

        assert!(result.is_ok(), "download_files failed: {:?}", result.err());

        let downloaded_path = temp_dir.path().join(test_filename);
        let contents = fs::read_to_string(&downloaded_path).expect("Failed to read downloaded file");

        assert_eq!(contents, test_content);
        _mock.assert_calls(1);
    }

    #[tokio::test]
    async fn test_download_files_failure() {
        let server = MockServer::start();
        let reana_token = "test-token";
        let workflow_name = "my_workflow";
        let test_filename = "results.svg";

        let _mock = server.mock(|when, then| {
            when.method("GET")
                .path(format!("/api/workflows/{workflow_name}/workspace/{test_filename}"))
                .query_param("access_token", reana_token);
            then.status(404)
                .header("content-type", "application/json")
                .body(r#"{"error": "File not found"}"#);
        });

        let files = vec![test_filename.to_string()];
        let url = &server.base_url();
        let reana = Reana::new(url.to_string(), reana_token.to_string());
        let result = download_files(&reana, workflow_name, &files, None).await;

        assert!(result.is_ok(), "download_files failed: {:?}", result.err());
        _mock.assert_calls(1);
    }
}
