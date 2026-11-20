//! Machine-readable descriptions of CWL documents: their ports with CWL types, and for a
//! workflow how its slots are wired.

use crate::authoring::{
    AuthoringError, AuthoringResult,
    workflow::{ConnectionStatus, connection_status},
};
use commonwl::{docstring, documents::CWLDocument, load_cwl_file};
use ignore::WalkBuilder;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::debug;

/// A CWL document's name, class and ports.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentSummary {
    /// File stem, the name `connect` and `remove` resolve.
    pub name: String,
    pub path: PathBuf,
    pub class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    /// Only set by [`summarize_file`] for a workflow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<ConnectionStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Port {
    pub id: String,
    /// The CWL type, as it would be written in the document.
    pub r#type: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

/// Summarizes the document at `path`, including its connection status if it is a workflow.
pub fn summarize_file(path: impl AsRef<Path>) -> AuthoringResult<DocumentSummary> {
    let path = path.as_ref();
    let doc = load_cwl_file(path, true).map_err(|e| {
        AuthoringError::Unknown(anyhow::anyhow!(
            "Could not load CWL File {}: {e}",
            path.display()
        ))
    })?;
    let mut summary = summarize(path, &doc)?;
    if let CWLDocument::Workflow(wf) = &doc {
        summary.connections = Some(connection_status(wf, path)?);
    }
    Ok(summary)
}

/// Summarizes every parseable CWL file below `dir`, honoring `.gitignore`. Paths start with
/// `dir`.
pub fn summarize_dir(dir: impl AsRef<Path>) -> AuthoringResult<Vec<DocumentSummary>> {
    let mut summaries = vec![];
    for entry in cwl_files(dir.as_ref()) {
        match load_cwl_file(entry.path(), true) {
            Ok(doc) => summaries.push(summarize(entry.path(), &doc)?),
            Err(e) => debug!("skipping {}: {e}", entry.path().display()),
        }
    }
    Ok(summaries)
}

fn summarize(path: &Path, doc: &CWLDocument) -> AuthoringResult<DocumentSummary> {
    let inputs = doc
        .get_inputs()
        .into_iter()
        .filter_map(|i| Some((i.id?, i.r#type, i.default)))
        .map(|(id, ty, default)| {
            Ok(Port {
                id,
                r#type: serde_json::to_value(&ty)?,
                default: default.as_ref().map(serde_json::to_value).transpose()?,
            })
        })
        .collect::<AuthoringResult<_>>()?;
    // output parameter types differ per class, so there is no generic accessor
    let outputs = match doc {
        CWLDocument::CommandLineTool(t) => {
            output_ports(t.outputs.iter().map(|o| (&o.id, &o.r#type)))
        }
        CWLDocument::ExpressionTool(t) => {
            output_ports(t.outputs.iter().map(|o| (&o.id, &o.r#type)))
        }
        CWLDocument::Operation(t) => output_ports(t.outputs.iter().map(|o| (&o.id, &o.r#type))),
        CWLDocument::Workflow(t) => output_ports(t.outputs.iter().map(|o| (&o.id, &o.r#type))),
    }?;
    Ok(DocumentSummary {
        name: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_path_buf(),
        class: doc.get_class().to_string(),
        label: doc.get_label().cloned(),
        doc: doc.get_doc().map(|d| docstring(d.clone())),
        inputs,
        outputs,
        connections: None,
    })
}

fn output_ports<'a, T: Serialize + 'a>(
    outputs: impl Iterator<Item = (&'a Option<String>, &'a T)>,
) -> AuthoringResult<Vec<Port>> {
    outputs
        .filter_map(|(id, ty)| Some((id.clone()?, ty)))
        .map(|(id, ty)| {
            Ok(Port {
                id,
                r#type: serde_json::to_value(ty)?,
                default: None,
            })
        })
        .collect()
}

fn cwl_files(dir: &Path) -> impl Iterator<Item = ignore::DirEntry> {
    WalkBuilder::new(dir)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "cwl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn testdata(path: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(path)
    }

    #[test]
    fn summarizes_tool_ports_with_types() {
        let path = testdata("hello_world/workflows/calculation/calculation.cwl");
        let json = serde_json::to_value(summarize_file(&path).unwrap()).unwrap();

        assert_eq!(json["name"], "calculation");
        assert_eq!(json["class"], "CommandLineTool");
        assert_eq!(json["inputs"][0]["id"], "population");
        assert_eq!(json["inputs"][0]["type"], "File");
        assert!(json["inputs"][0]["default"].is_object());
        assert_eq!(json["outputs"][0], json!({"id": "results", "type": "File"}));
        assert!(json.get("connections").is_none());
    }

    #[test]
    fn summarizes_workflow_with_connections() {
        let path = testdata("hello_world/workflows/main/main.cwl");
        let json = serde_json::to_value(summarize_file(&path).unwrap()).unwrap();

        assert_eq!(json["class"], "Workflow");
        let connections = &json["connections"];
        assert_eq!(
            connections["inputs"][0],
            json!({"id": "population", "status": "connected"})
        );
        assert_eq!(
            connections["outputs"][0],
            json!({"id": "out", "status": "connected"})
        );
        assert_eq!(connections["steps"][0]["id"], "plot");
        assert_eq!(connections["steps"][0]["run"], "../plot/plot.cwl");
    }

    #[test]
    fn summarize_dir_finds_every_cwl_file() {
        let dir = testdata("hello_world");
        let summaries = summarize_dir(&dir).unwrap();
        let mut names: Vec<_> = summaries.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["calculation", "main", "plot"]);
        assert!(summaries.iter().all(|s| s.path.starts_with(&dir)));
    }
}
