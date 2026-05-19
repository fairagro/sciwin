use crate::types::{NodeInstance, PortType, Slot, VisualEdge, VisualNode};
use commonwl::{
    Identifiable,
    documents::{CWLDocument, StringOrDocument, Workflow},
    load_cwl_file,
    outputs::CommandOutputParameterType,
};
use dioxus::html::geometry::euclid::Point2D;
use petgraph::{graph::NodeIndex, prelude::*};
use rand::Rng;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
};

pub type WorkflowGraph = StableDiGraph<VisualNode, VisualEdge>;

pub fn load_workflow_graph(
    workflow: &Workflow,
    path: impl AsRef<Path>,
) -> anyhow::Result<WorkflowGraph> {
    let wgb = WorkflowGraphBuilder::from_workflow(workflow, path)?;
    Ok(wgb.graph)
}

#[derive(Default)]
struct WorkflowGraphBuilder {
    pub graph: WorkflowGraph,
    node_map: HashMap<String, NodeIndex>,
}

impl WorkflowGraphBuilder {
    fn from_workflow(workflow: &Workflow, path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut builder = Self::default();
        builder.load_workflow(workflow, path)?;
        Ok(builder)
    }

    fn load_workflow(&mut self, workflow: &Workflow, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let mut rng = rand::rng();
        let path = path.as_ref();

        for input in &workflow.inputs {
            let node_id = self.graph.add_node(VisualNode {
                id: input.id.clone().unwrap(),
                instance: NodeInstance::Input(input.clone()),
                outputs: vec![Slot {
                    id: input.id.clone().unwrap(),
                    type_: PortType::Input(input.r#type.clone()),
                }],
                inputs: vec![],
                path: None,
                position: Point2D::new(0.0, rng.random_range(0.0..=1.0)),
            });
            self.node_map.insert(input.id.clone().unwrap(), node_id);
        }

        for output in &workflow.outputs {
            let node_id = self.graph.add_node(VisualNode {
                id: output.id.clone().unwrap(),
                instance: NodeInstance::Output(output.clone()),
                inputs: vec![Slot {
                    id: output.id.clone().unwrap(),
                    type_: PortType::Output(output.r#type.clone()),
                }],

                outputs: vec![],
                path: None,
                position: Point2D::new(rng.random_range(0.0..=1.0), 1.0),
            });
            self.node_map.insert(output.id.clone().unwrap(), node_id);
        }

        // add steps sorted by execution order
        let step_ids = sort_steps(workflow).map_err(|e| anyhow::anyhow!("{e}"))?;
        for step_id in step_ids {
            let step = workflow
                .get_step(&step_id)
                .ok_or_else(|| anyhow::anyhow!("Could not find step: {step_id}"))?;
            let StringOrDocument::String(str) = &step.run else {
                anyhow::bail!("Inline Document not supported")
            };

            let step_file = path.parent().unwrap_or(path).join(str);
            let mut doc = load_cwl_file(&step_file, true).map_err(|e| anyhow::anyhow!("{e}"))?;
            if doc.get_id().is_none() {
                doc.set_id(&step_file.file_name().unwrap().to_string_lossy());
            }

            let node_id = self.graph.add_node(VisualNode {
                id: step_id.clone(),
                instance: NodeInstance::Step(doc.clone()),
                inputs: doc
                    .get_inputs()
                    .iter()
                    .map(|i| Slot {
                        id: i.id.clone().unwrap(),
                        type_: PortType::Input(i.r#type.clone()),
                    })
                    .collect(),
                outputs: doc
                    .get_output_ids()
                    .iter()
                    .map(|i| Slot {
                        id: i.to_string(),
                        type_: PortType::Output(get_output_type(&doc, i).unwrap()),
                    })
                    .collect(),
                path: Some(step_file),
                position: Point2D::new(rng.random_range(0.0..=1.0), rng.random_range(0.0..=1.0)),
            });
            self.node_map.insert(step.id.clone().unwrap(), node_id);

            for wsip in &step.r#in {
                let source = wsip
                    .source
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("source is not set in step: {:?}", wsip))?;
                let (source, source_port) = source
                    .as_one() //TODO: not capturing everything
                    .split_once('/')
                    .unwrap_or((source.as_one().as_str(), source.as_one().as_str()));
                let type_ = doc
                    .get_inputs()
                    .iter()
                    .find(|i| i.id == wsip.id)
                    .map(|i| i.r#type.clone())
                    .ok_or_else(|| anyhow::anyhow!("Could not find step input: {:?}", wsip.id))?;

                self.connect_edge(
                    source,
                    step.id.as_ref().unwrap(),
                    source_port,
                    wsip.id.as_ref().unwrap(),
                    PortType::Input(type_),
                )?;
            }
        }

        //add output connections
        for output in &workflow.outputs {
            if let Some(output_source) = &output.output_source {
                let (source, source_port) =
                    output_source.as_one().split_once("/").ok_or_else(|| {
                        anyhow::anyhow!(
                            "Output source is not in the correct format: {output_source}"
                        )
                    })?;
                let type_ = output.r#type.clone();
                self.connect_edge(
                    source,
                    output.id.as_ref().unwrap(),
                    source_port,
                    output.id.as_ref().unwrap(),
                    PortType::Output(type_),
                )?
            }
        }

        //layout
        self.auto_layout();

        Ok(())
    }

    fn connect_edge(
        &mut self,
        source: &str,
        target: &str,
        source_port: &str,
        target_port: &str,
        type_: PortType,
    ) -> anyhow::Result<()> {
        let source_idx = self
            .node_map
            .get(source)
            .ok_or_else(|| anyhow::anyhow!("Could not find node in map: {source}"))?;
        let target_idx = self
            .node_map
            .get(target)
            .ok_or_else(|| anyhow::anyhow!("Could not find node in map: {target}"))?;

        self.graph.add_edge(
            *source_idx,
            *target_idx,
            VisualEdge {
                source_port: source_port.to_string(),
                target_port: target_port.to_string(),
                data_type: type_,
            },
        );

        Ok(())
    }

    pub fn auto_layout(&mut self) {
        auto_layout(&mut self.graph);
    }
}

pub fn auto_layout(graph: &mut WorkflowGraph) {
    let node_indices: Vec<_> = graph.node_indices().collect();

    let positions = rust_sugiyama::from_graph(
        graph,
        &(|_, _| (120.0, 190.0)),
        &rust_sugiyama::configure::Config {
            vertex_spacing: 30.0,
            ..Default::default()
        },
    )
    .into_iter()
    .map(|(layout, _, _)| {
        let mut new_layout = HashMap::new();
        for (id, coords) in layout {
            new_layout.insert(id, coords);
        }
        new_layout
    })
    .collect::<Vec<_>>();

    for island in &positions {
        for ix in &node_indices {
            if let Some(pos) = island.get(ix) {
                graph[*ix].position = Point2D::new(pos.1 as f32, pos.0 as f32);
            }
        }
    }
}

/// Sorts `WorkflowStep`s to get the sequence of execution
fn sort_steps(wf: &Workflow) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    for step in &wf.steps {
        in_degree.entry(step.id.clone().unwrap()).or_insert(0);

        for input in &step.r#in {
            let parts: Vec<&str> = if let Some(source) = &input.source {
                source.as_one().split('/').collect()
            } else {
                vec![]
            };

            if parts.len() == 2 {
                let dependency = parts[0];
                graph
                    .entry(dependency.to_string())
                    .or_default()
                    .push(step.id.clone().unwrap());
                *in_degree.entry(step.id.clone().unwrap()).or_insert(0) += 1;
            }
        }
    }
    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|&(_, &degree)| degree == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let mut sorted_steps = Vec::new();
    while let Some(step) = queue.pop_front() {
        sorted_steps.push(step.clone());

        if let Some(dependents) = graph.get(&step) {
            for dependent in dependents {
                if let Some(degree) = in_degree.get_mut(dependent) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }
    }

    if sorted_steps.len() != wf.steps.len() {
        return Err("❗ Cycle detected in the workflow".into());
    }

    Ok(sorted_steps)
}

pub(crate) fn get_output_type(doc: &CWLDocument, id: &str) -> Option<CommandOutputParameterType> {
    match doc {
        CWLDocument::ExpressionTool(et) => et
            .outputs
            .iter()
            .find(|o| o.id.as_ref().unwrap() == id)
            .map(|o| o.r#type.clone())
            .map(Into::into),
        CWLDocument::Operation(op) => op
            .outputs
            .iter()
            .find(|o| o.id.as_ref().unwrap() == id)
            .map(|o| o.r#type.clone())
            .map(Into::into),
        CWLDocument::Workflow(wf) => wf
            .outputs
            .iter()
            .find(|o| o.id.as_ref().unwrap() == id)
            .map(|o| o.r#type.clone()),
        CWLDocument::CommandLineTool(clt) => clt
            .outputs
            .iter()
            .find(|o| o.id.as_ref().unwrap() == id)
            .map(|o| o.r#type.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_load_workflow_graph() {
        let path = "../../testdata/hello_world/workflows/main/main.cwl";
        let CWLDocument::Workflow(workflow) = load_cwl_file(path, true).unwrap() else {
            panic!("Expected a workflow document")
        };
        let graph = load_workflow_graph(&workflow, path).unwrap();

        assert_eq!(graph.node_count(), 5); //2 inputs, 2 steps, 1 output
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    #[serial]
    fn test_load_workflow_graph_02() {
        let path = "../../testdata/mkdir_wf.cwl";
        let CWLDocument::Workflow(workflow) = load_cwl_file(path, true).unwrap() else {
            panic!("Expected a workflow document")
        };
        let graph = load_workflow_graph(&workflow, path).unwrap();

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
    }
}
