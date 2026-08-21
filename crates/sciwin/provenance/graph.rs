//! The typed walk over a packed CWL workflow: steps, ports and the dataflow connections
//! between them. Connections are derived from `WorkflowStepInput.source` and
//! `WorkflowOutputParameter.output_source` -- never from matching file basenames.

use std::collections::HashMap;

use commonwl::{
    Identifiable, OneOrMany,
    documents::{CWLDocument, StringOrDocument},
    inputs::{CommandInputParameterType, CommandInputType, DefaultValue, InputType},
    outputs::{CommandOutputParameterType, CommandOutputType},
    packed::PackedCWL,
    requirements::DockerRequirement,
    types::CWLType,
};

use crate::provenance::{ProvenanceError, ProvenanceResult};

/// One step of the workflow, resolved against the tool document it runs.
#[derive(Debug, Clone)]
pub struct StepNode {
    pub id: String,
    pub run: String,
    pub position: usize,
    pub container_image: Option<String>,
}

/// Whether a [`PortNode`] consumes or produces data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Input,
    Output,
}

/// One input or output port, on the main workflow or on a tool a step runs.
#[derive(Debug, Clone)]
pub struct PortNode {
    pub id: String,
    pub kind: PortKind,
    pub additional_type: Option<CWLType>,
    pub file_name: Option<String>,
}

/// A dataflow edge: the port at `.0` produces what the port at `.1` consumes.
pub type Connection = (String, String);

/// The typed shape of a packed workflow: steps, ports and the connections between them.
///
/// Ids are exactly as they appear in the packed document (e.g. `#main/population`,
/// `#calculation.cwl/results`) -- turning them into crate-relative RO-Crate entity ids is
/// `provenance::builder`'s job, not this one.
#[derive(Debug, Clone, Default)]
pub struct WorkflowGraph {
    pub workflow_id: String,
    pub steps: Vec<StepNode>,
    pub inputs: Vec<PortNode>,
    pub outputs: Vec<PortNode>,
    pub tool_ports: Vec<PortNode>,
    pub connections: Vec<Connection>,
}

impl WorkflowGraph {
    /// Walks a packed CWL document into its typed graph.
    ///
    /// # Errors
    /// The graph has no `Workflow`, or a step's `run` does not resolve inside the packed graph.
    pub fn from_packed(packed: &PackedCWL) -> ProvenanceResult<Self> {
        let workflow = packed
            .graph
            .iter()
            .find_map(|doc| match doc {
                CWLDocument::Workflow(wf) => Some(wf),
                _ => None,
            })
            .ok_or(ProvenanceError::NoWorkflow)?;

        let workflow_id = workflow.id.clone().unwrap_or_default();

        let tools: HashMap<&str, &CWLDocument> = packed
            .graph
            .iter()
            .filter_map(|doc| doc.get_id().map(|id| (id.as_str(), doc)))
            .collect();

        let mut steps = Vec::with_capacity(workflow.steps.len());
        let mut tool_ports = Vec::new();
        let mut step_runs: HashMap<String, String> = HashMap::with_capacity(workflow.steps.len());

        for (position, step) in workflow.steps.iter().enumerate() {
            let step_id = step.id.clone().unwrap_or_default();

            let (run_id, tool): (String, &CWLDocument) = match &step.run {
                StringOrDocument::String(id) => {
                    let tool = tools.get(id.as_str()).copied().ok_or_else(|| {
                        ProvenanceError::UnresolvedStep {
                            step: step_id.clone(),
                            run: id.clone(),
                        }
                    })?;
                    (id.clone(), tool)
                }
                StringOrDocument::Document(doc) => {
                    let id = doc
                        .get_id()
                        .cloned()
                        .unwrap_or_else(|| format!("{step_id}#run"));
                    (id, doc.as_ref())
                }
            };

            step_runs.insert(step_id.clone(), run_id.clone());

            let container_image = tool
                .get_requirement_or_hint::<DockerRequirement>()
                .and_then(|docker| docker.docker_pull.clone());

            tool_ports.extend(ports_of(tool));

            steps.push(StepNode {
                id: step_id,
                run: run_id,
                position,
                container_image,
            });
        }

        let inputs = workflow
            .inputs
            .iter()
            .map(|input| PortNode {
                id: input.id.clone().unwrap_or_default(),
                kind: PortKind::Input,
                additional_type: simple_input_type(&input.r#type),
                file_name: file_name_from_default(&input.default),
            })
            .collect();

        let outputs = workflow
            .outputs
            .iter()
            .map(|output| PortNode {
                id: output.id.clone().unwrap_or_default(),
                kind: PortKind::Output,
                additional_type: simple_output_type(&output.r#type),
                file_name: None,
            })
            .collect();

        let mut connections = Vec::new();
        for step in &workflow.steps {
            for step_in in &step.r#in {
                let (Some(sources), Some(port_id)) = (&step_in.source, &step_in.id) else {
                    continue;
                };
                let target = resolve(port_id, &step_runs);
                for source in sources {
                    connections.push((resolve(source, &step_runs), target.clone()));
                }
            }
        }
        for output in &workflow.outputs {
            let (Some(sources), Some(output_id)) = (&output.output_source, &output.id) else {
                continue;
            };
            let target = resolve(output_id, &step_runs);
            for source in sources {
                connections.push((resolve(source, &step_runs), target.clone()));
            }
        }

        Ok(Self {
            workflow_id,
            steps,
            inputs,
            outputs,
            tool_ports,
            connections,
        })
    }
}

/// Rewrites a step-qualified port reference (`#main/<step>/<port>`) to the id of the port on the
/// tool that step runs (`#<tool>/<port>`) -- the id the tool's own `CommandInputParameter`/
/// `CommandOutputParameter` carries. Workflow-level input/output ids (`#main/<port>`, no step
/// segment) and anything that doesn't name a known step pass through unchanged.
fn resolve(reference: &str, step_runs: &HashMap<String, String>) -> String {
    if let Some((step_part, port)) = reference.rsplit_once('/')
        && let Some(run_id) = step_runs.get(step_part)
    {
        return format!("{run_id}/{port}");
    }
    reference.to_string()
}

/// The typed ports a tool exposes. Only `CommandLineTool` is supported today -- a step running an
/// `ExpressionTool`/`Operation` still resolves and connects correctly, it just contributes no
/// enriched ports (no `additionalType`/`file_name`) to `tool_ports`.
fn ports_of(tool: &CWLDocument) -> Vec<PortNode> {
    let CWLDocument::CommandLineTool(tool) = tool else {
        return Vec::new();
    };

    let inputs = tool.inputs.iter().map(|input| PortNode {
        id: input.id.clone().unwrap_or_default(),
        kind: PortKind::Input,
        additional_type: simple_command_input_type(&input.r#type),
        file_name: file_name_from_default(&input.default),
    });
    let outputs = tool.outputs.iter().map(|output| PortNode {
        id: output.id.clone().unwrap_or_default(),
        kind: PortKind::Output,
        additional_type: simple_output_type(&output.r#type),
        file_name: output
            .output_binding
            .as_ref()
            .and_then(|binding| binding.glob.as_ref())
            .and_then(|glob| glob.iter().next())
            .map(|glob| basename(glob)),
    });
    inputs.chain(outputs).collect()
}

/// The common case only: a bare `CWLType` (`File`, `string`, ...). Array/record schemas, `stdin`
/// and `stdout` are left untyped here -- `additionalType` just won't be set for those ports.
fn simple_input_type(ty: &OneOrMany<InputType>) -> Option<CWLType> {
    match ty {
        OneOrMany::One(InputType::CWLType(cwl_type)) => Some(*cwl_type),
        _ => None,
    }
}

fn simple_command_input_type(ty: &CommandInputParameterType) -> Option<CWLType> {
    match ty {
        CommandInputParameterType::CommandInputType(OneOrMany::One(CommandInputType::CWLType(
            cwl_type,
        ))) => Some(*cwl_type),
        _ => None,
    }
}

fn simple_output_type(ty: &CommandOutputParameterType) -> Option<CWLType> {
    match ty {
        CommandOutputParameterType::CommandOutputType(OneOrMany::One(
            CommandOutputType::CWLType(cwl_type),
        )) => Some(*cwl_type),
        _ => None,
    }
}

fn file_name_from_default(default: &Option<DefaultValue>) -> Option<String> {
    let location = default.as_ref()?.as_file()?.location.as_ref()?;
    Some(basename(location))
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_packed() -> PackedCWL {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/rocrate/workflow.json"
        );
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        serde_json::from_value(raw["workflow"]["specification"].clone()).unwrap()
    }

    #[test]
    fn test_steps_and_containers() {
        let graph = WorkflowGraph::from_packed(&fixture_packed()).unwrap();
        assert_eq!(graph.workflow_id, "#main");
        assert_eq!(graph.steps.len(), 2);

        let calculation = graph
            .steps
            .iter()
            .find(|s| s.id == "#main/calculation")
            .unwrap();
        assert_eq!(calculation.run, "#calculation.cwl");
        assert_eq!(calculation.position, 0);
        assert_eq!(
            calculation.container_image.as_deref(),
            Some("pandas/pandas:pip-all")
        );

        let plot = graph.steps.iter().find(|s| s.id == "#main/plot").unwrap();
        assert_eq!(plot.run, "#plot.cwl");
        assert_eq!(plot.position, 1);
        assert_eq!(
            plot.container_image.as_deref(),
            Some("user12398/pytest:v1.0.0")
        );
    }

    #[test]
    fn test_ports() {
        let graph = WorkflowGraph::from_packed(&fixture_packed()).unwrap();

        let population = graph
            .inputs
            .iter()
            .find(|p| p.id == "#main/population")
            .unwrap();
        assert_eq!(population.additional_type, Some(CWLType::File));
        assert_eq!(population.file_name.as_deref(), Some("population.csv"));

        let out = graph.outputs.iter().find(|p| p.id == "#main/out").unwrap();
        assert_eq!(out.additional_type, Some(CWLType::File));

        let tool_population = graph
            .tool_ports
            .iter()
            .find(|p| p.id == "#calculation.cwl/population")
            .unwrap();
        assert_eq!(tool_population.file_name.as_deref(), Some("population.csv"));

        let results = graph
            .tool_ports
            .iter()
            .find(|p| p.id == "#calculation.cwl/results")
            .unwrap();
        assert_eq!(results.additional_type, Some(CWLType::File));
        assert_eq!(results.file_name.as_deref(), Some("results.csv"));
    }

    #[test]
    fn test_connections_derived_not_basename_matched() {
        let graph = WorkflowGraph::from_packed(&fixture_packed()).unwrap();

        let mut connections = graph.connections.clone();
        connections.sort();

        let mut expected = vec![
            (
                "#calculation.cwl/results".to_string(),
                "#plot.cwl/results".to_string(),
            ),
            (
                "#main/population".to_string(),
                "#calculation.cwl/population".to_string(),
            ),
            (
                "#main/speakers".to_string(),
                "#calculation.cwl/speakers".to_string(),
            ),
            (
                "#plot.cwl/results".to_string(),
                "#main/out".to_string(),
            ),
        ];
        expected.sort();

        assert_eq!(connections, expected);
    }

    #[test]
    fn test_no_workflow_errors() {
        let result = WorkflowGraph::from_packed(&PackedCWL::default());
        assert!(matches!(result, Err(ProvenanceError::NoWorkflow)));
    }
}
