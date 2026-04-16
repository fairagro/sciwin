use inputs::{CommandInputParameter, deserialize_inputs};
use requirements::{FromRequirement, Requirement, deserialize_hints, deserialize_requirements};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::fmt::{self, Display};
use std::{
    error::Error,
    fmt::Debug,
    fs,
    ops::{Deref, DerefMut},
    path::Path,
};
pub mod deserialize;
pub mod format;
pub mod inputs;
mod io;
pub mod outputs;
pub mod packed;
pub mod prelude;
pub mod requirements;

mod clt;
mod et;
mod types;
mod wf;
pub use clt::*;
pub use et::*;
pub use types::*;
pub use wf::*;

use crate::outputs::CommandOutputParameter;

/// Represents a CWL (Common Workflow Language) document, which can be one of the following types:
/// - `CommandLineTool`: A CWL CommandLineTool document.
/// - `Workflow`: A CWL Workflow document.
/// - `ExpressionTool`: A CWL ExpressionTool document.
///
/// This enum supports automated type detection during deserialization, allowing it to handle any CWL document type seamlessly.
///
/// # Examples
///
/// ```
/// use cwl_core::CWLDocument;
/// use serde_yaml;
///
/// let yaml = r#"---
/// class: CommandLineTool
/// cwlVersion: v1.0
/// inputs: []
/// outputs: []
/// baseCommand: echo
/// "#;
///
/// let document: CWLDocument = serde_yaml::from_str(yaml).unwrap();
/// assert!(matches!(document, CWLDocument::CommandLineTool(_)));
/// ```
#[derive(Serialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum CWLDocument {
    CommandLineTool(CommandLineTool),
    Workflow(Workflow),
    ExpressionTool(ExpressionTool),
}

impl Deref for CWLDocument {
    type Target = DocumentBase;

    fn deref(&self) -> &Self::Target {
        match self {
            CWLDocument::CommandLineTool(clt) => &clt.base,
            CWLDocument::Workflow(wf) => &wf.base,
            CWLDocument::ExpressionTool(et) => &et.base,
        }
    }
}

impl DerefMut for CWLDocument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            CWLDocument::CommandLineTool(clt) => &mut clt.base,
            CWLDocument::Workflow(wf) => &mut wf.base,
            CWLDocument::ExpressionTool(et) => &mut et.base,
        }
    }
}

impl<'de> Deserialize<'de> for CWLDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: Value = Deserialize::deserialize(deserializer)?;
        let class = value
            .get("class")
            .ok_or_else(|| serde::de::Error::missing_field("class"))?
            .as_str()
            .ok_or_else(|| serde::de::Error::missing_field("class must be of type string"))?;

        match class {
            "CommandLineTool" => serde_yaml::from_value(value)
                .map(CWLDocument::CommandLineTool)
                .map_err(serde::de::Error::custom),
            "ExpressionTool" => serde_yaml::from_value(value)
                .map(CWLDocument::ExpressionTool)
                .map_err(serde::de::Error::custom),
            "Workflow" => serde_yaml::from_value(value).map(CWLDocument::Workflow).map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(format!("Unknown variant of CWL file: {class}"))),
        }
    }
}

impl CWLDocument {
    /// Returns the List of CommandOutputParameter.id of the `CWLDocument`.
    pub fn get_output_ids(&self) -> Vec<String> {
        match self {
            CWLDocument::CommandLineTool(clt) => clt.outputs.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
            CWLDocument::Workflow(wf) => wf.outputs.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
            CWLDocument::ExpressionTool(et) => et.outputs.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
        }
    }

    /// Returns the type of a specific output by its ID.
    pub fn get_output_type(&self, output_id: &str) -> Option<CWLType> {
        match self {
            CWLDocument::CommandLineTool(clt) => clt.outputs.iter().find(|o| o.id == output_id).map(|o| o.type_.clone()),
            CWLDocument::Workflow(wf) => wf.outputs.iter().find(|o| o.id == output_id).map(|o| o.type_.clone()),
            CWLDocument::ExpressionTool(et) => et.outputs.iter().find(|o| o.id == output_id).map(|o| o.type_.clone()),
        }
    }
}

/// Base struct used by all CWL Documents (CommandLineTool, ExpressionTool and Workflow) defining common fields.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocumentBase {
    pub class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwl_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(deserialize_with = "deserialize_inputs")]
    pub inputs: Vec<CommandInputParameter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_requirements")]
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_hints")]
    #[serde(default)]
    pub hints: Vec<Requirement>,
}

impl DocumentBase {
    /// Checks whether Document has a specific Requirement attached and returns an option to it
    pub fn get_requirement<T>(&self) -> Option<&T>
    where
        Requirement: FromRequirement<T>,
    {
        self.requirements.iter().chain(self.hints.iter()).find_map(|req| Requirement::get(req))
    }

    /// Checks whether Document has a specific Requirement attached and returns an option to a mutable version of it
    pub fn get_requirement_mut<T>(&mut self) -> Option<&mut T>
    where
        Requirement: FromRequirement<T>,
    {
        self.requirements
            .iter_mut()
            .chain(self.hints.iter_mut())
            .find_map(|req| Requirement::get_mut(req))
    }

    pub fn has_requirement(&self, target: &Requirement) -> bool {
        self.requirements.iter().chain(self.hints.iter()).any(|r| r == target)
    }
}

pub trait Operation: DerefMut<Target = DocumentBase> {
    fn outputs_mut(&mut self) -> &mut Vec<CommandOutputParameter>;
    fn outputs(&self) -> &Vec<CommandOutputParameter>;
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum StringOrNumber {
    String(String),
    Integer(u64),
    Decimal(f64),
}
impl fmt::Display for StringOrNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StringOrNumber::String(s) => write!(f, "{s}"),
            StringOrNumber::Integer(i) => write!(f, "{i}"),
            StringOrNumber::Decimal(d) => write!(f, "{d}"),
        }
    }
}

/// Loads a CWL Document from a YAML file on disk.
///
/// This function reads the specified file, parses its contents as YAML, and attempts to deserialize it into a `CWLDocument` object.
///
/// # Arguments
/// * `filename` - A path to the YAML file containing the CWL Docu definition.
///
/// # Returns
/// * `Ok(CWLDocument)` if the file is successfully read and parsed.
/// * `Err` if the file does not exist or cannot be parsed.
///
/// # Examples
/// ```
/// use cwl_core::load_doc;
///
/// let tool = load_doc("../../testdata/default.cwl");
/// assert!(tool.is_ok());
/// ```
pub fn load_doc<P: AsRef<Path> + Debug>(filename: P) -> Result<CWLDocument, Box<dyn Error>> {
    let path = filename.as_ref();
    if !path.exists() {
        return Err(format!("❌ File {filename:?} does not exist.").into());
    }
    let contents = fs::read_to_string(path)?;
    let doc: CWLDocument = serde_yaml::from_str(&contents).map_err(|e| format!("❌ Could not read CWL {filename:?}: {e}"))?;

    Ok(doc)
}

/// Loads a CWL CommandLineTool from a YAML file on disk.
///
/// This function reads the specified file, parses its contents as YAML, and attempts to deserialize it into a `CommandLineTool` object.
///
/// # Arguments
/// * `filename` - A path to the YAML file containing the CommandLineTool definition.
///
/// # Returns
/// * `Ok(CommandLineTool)` if the file is successfully read and parsed.
/// * `Err` if the file does not exist or cannot be parsed.
///
/// # Examples
/// ```
/// use cwl_core::load_tool;
///
/// let tool = load_tool("../../testdata/default.cwl");
/// assert!(tool.is_ok());
/// ```
pub fn load_tool<P: AsRef<Path> + Debug>(filename: P) -> Result<CommandLineTool, Box<dyn Error>> {
    let path = filename.as_ref();
    if !path.exists() {
        return Err(format!("❌ Tool {filename:?} does not exist.").into());
    }
    let contents = fs::read_to_string(path)?;
    let tool: CommandLineTool = serde_yaml::from_str(&contents).map_err(|e| format!("❌ Could not read CommandLineTool {filename:?}: {e}"))?;

    Ok(tool)
}

/// Loads a CWL ExpressionTool from a YAML file on disk.
///
/// This function reads the specified file, parses its contents as YAML, and attempts to deserialize it into an `ExpressionTool` object.
///
/// # Arguments
/// * `filename` - A path to the YAML file containing the ExpressionTool definition.
///
/// # Returns
/// * `Ok(ExpressionTool)` if the file is successfully read and parsed.
/// * `Err` if the file does not exist or cannot be parsed.
///
/// # Examples
/// ```
/// use cwl_core::load_expression_tool;
///
/// let expr_tool = load_expression_tool("../../testdata/test_expr.cwl");
/// assert!(expr_tool.is_ok());
/// ```
pub fn load_expression_tool<P: AsRef<Path> + Debug>(filename: P) -> Result<ExpressionTool, Box<dyn Error>> {
    let path = filename.as_ref();
    if !path.exists() {
        return Err(format!("❌ ExpressionTool {filename:?} does not exist.").into());
    }
    let contents = fs::read_to_string(path)?;
    let tool: ExpressionTool = serde_yaml::from_str(&contents).map_err(|e| format!("❌ Could not read ExpressionTool {filename:?}: {e}"))?;

    Ok(tool)
}

/// Loads a CWL Workflow from a YAML file on disk.
///
/// This function reads the specified file, parses its contents as YAML, and attempts to deserialize it into a `Workflow` object.
///
/// # Arguments
/// * `filename` - A path to the YAML file containing the Workflow definition.
///
/// # Returns
/// * `Ok(Workflow)` if the file is successfully read and parsed.
/// * `Err` if the file does not exist or cannot be parsed.
///
/// # Examples
/// ```
/// use cwl_core::load_workflow;
///
/// let workflow = load_workflow("../../testdata/wf_inout.cwl");
/// assert!(workflow.is_ok());
/// ```
pub fn load_workflow<P: AsRef<Path> + Debug>(filename: P) -> Result<Workflow, Box<dyn Error>> {
    let path = filename.as_ref();
    if !path.exists() {
        return Err(format!("❌ Workflow {filename:?} does not exist, yet!").into());
    }
    let contents = fs::read_to_string(path)?;
    let workflow: Workflow = serde_yaml::from_str(&contents).map_err(|e| format!("❌ Could not read Workflow {filename:?}: {e}"))?;
    Ok(workflow)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum SingularPlural<T> {
    Singular(T),
    Plural(Vec<T>),
}

impl<T: Clone> SingularPlural<T> {
    pub fn into_vec(&self) -> Vec<T> {
        match self {
            SingularPlural::Singular(item) => vec![item.clone()],
            SingularPlural::Plural(items) => items.to_vec(),
        }
    }

    pub fn into_singular(&self) -> T {
        match self {
            SingularPlural::Singular(item) => item.clone(),
            SingularPlural::Plural(vec) => vec[0].clone(),
        }
    }
}

impl Display for SingularPlural<String> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SingularPlural::Singular(s) => write!(f, "{s}"),
            SingularPlural::Plural(v) => write!(f, "{:?}", v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("../../testdata/default.cwl")]
    #[case("../../testdata/echo.cwl")]
    #[case("../../testdata/mkdir.cwl")]
    #[case("../../testdata/hello_world/workflows/calculation/calculation.cwl")]
    #[case("../../testdata/hello_world/workflows/plot/plot.cwl")]

    fn test_load_multiple_tools(#[case] filename: &str) {
        let tool = load_tool(filename);
        assert!(tool.is_ok());
    }

    #[test]
    #[should_panic]
    fn test_load_tool_fails() {
        let _ = load_tool("this is not valid").unwrap();
    }

    #[rstest]
    #[case("../../testdata/mkdir_wf.cwl")]
    #[case("../../testdata/test-wf.cwl")]
    #[case("../../testdata/test-wf_features.cwl")]
    #[case("../../testdata/test-wf_features_alt.cwl")]
    #[case("../../testdata/wf_inout.cwl")]
    #[case("../../testdata/wf_inout_dir.cwl")]
    #[case("../../testdata/wf_inout_file.cwl")]
    #[case("../../testdata/hello_world/workflows/main/main.cwl")]
    fn test_load_multiple_wfs(#[case] filename: &str) {
        let workflow = load_workflow(filename);
        assert!(workflow.is_ok());
    }

    #[test]
    #[should_panic]
    fn test_load_wf_fails() {
        let _ = load_workflow("this is not valid").unwrap();
    }
}
