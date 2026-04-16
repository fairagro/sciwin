use crate::types::{NodeInstance, Slot, VisualEdge, VisualNode};
use commonwl::{StringOrDocument, load_doc, prelude::*};
use dioxus::html::geometry::euclid::Point2D;
use petgraph::{graph::NodeIndex, prelude::*};
use rand::Rng;
use std::{collections::HashMap, path::Path};

pub type WorkflowGraph = StableDiGraph<VisualNode, VisualEdge>;

pub fn load_workflow_graph(workflow: &Workflow, path: impl AsRef<Path>) -> anyhow::Result<WorkflowGraph> {
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
                id: input.id.clone(),
                instance: NodeInstance::Input(input.clone()),
                outputs: vec![Slot {
                    id: input.id.clone(),
                    type_: input.type_.clone(),
                }],
                inputs: vec![],
                path: None,
                position: Point2D::new(0.0, rng.random_range(0.0..=1.0)),
            });
            self.node_map.insert(input.id.clone(), node_id);
        }

        for output in &workflow.outputs {
            let node_id = self.graph.add_node(VisualNode {
                id: output.id.clone(),
                instance: NodeInstance::Output(output.clone()),
                inputs: vec![Slot {
                    id: output.id.clone(),
                    type_: output.type_.clone(),
                }],
                outputs: vec![],
                path: None,
                position: Point2D::new(rng.random_range(0.0..=1.0), 1.0),
            });
            self.node_map.insert(output.id.clone(), node_id);
        }

        // add steps sorted by execution order
        let step_ids = workflow.sort_steps().map_err(|e| anyhow::anyhow!("{e}"))?;
        for step_id in step_ids {
            let step = workflow
                .get_step(&step_id)
                .ok_or_else(|| anyhow::anyhow!("Could not find step: {step_id}"))?;
            let StringOrDocument::String(str) = &step.run else {
                anyhow::bail!("Inline Document not supported")
            };

            let step_file = path.parent().unwrap_or(path).join(str);
            let mut doc = load_doc(&step_file).map_err(|e| anyhow::anyhow!("{e}"))?;
            if doc.id.is_none() {
                doc.id = Some(step_file.file_name().unwrap().to_string_lossy().to_string());
            }

            let node_id = self.graph.add_node(VisualNode {
                id: step_id.clone(),
                instance: NodeInstance::Step(doc.clone()),
                inputs: doc
                    .inputs
                    .iter()
                    .map(|i| Slot {
                        id: i.id.clone(),
                        type_: i.type_.clone(),
                    })
                    .collect(),
                outputs: doc
                    .get_output_ids()
                    .iter()
                    .map(|i| Slot {
                        id: i.to_string(),
                        type_: doc.get_output_type(i).unwrap(),
                    })
                    .collect(),
                path: Some(step_file),
                position: Point2D::new(rng.random_range(0.0..=1.0), rng.random_range(0.0..=1.0)),
            });
            self.node_map.insert(step.id.clone(), node_id);

            for wsip in &step.in_ {
                let source = wsip
                    .source
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("source is not set in step: {:?}", wsip))?;
                let (source, source_port) = source.split_once('/').unwrap_or((source.as_str(), source.as_str()));
                let type_ = doc
                    .inputs
                    .iter()
                    .find(|i| i.id == wsip.id)
                    .map(|i| i.type_.clone())
                    .ok_or_else(|| anyhow::anyhow!("Could not find step input: {}", wsip.id))?;

                self.connect_edge(source, &step.id, source_port, &wsip.id, type_)?;
            }
        }

        //add output connections
        for output in &workflow.outputs {
            if let Some(output_source) = &output.output_source {
                let (source, source_port) = output_source
                    .split_once("/")
                    .ok_or_else(|| anyhow::anyhow!("Output source is not in the correct format: {output_source}"))?;
                let type_ = output.type_.clone();
                self.connect_edge(source, &output.id, source_port, &output.id, type_)?
            }
        }

        //layout
        self.auto_layout();

        Ok(())
    }

    fn connect_edge(&mut self, source: &str, target: &str, source_port: &str, target_port: &str, type_: CWLType) -> anyhow::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::load_workflow;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_load_workflow_graph() {
        let path = "../../testdata/hello_world/workflows/main/main.cwl";
        let workflow = load_workflow(path).unwrap();
        let graph = load_workflow_graph(&workflow, path).unwrap();

        assert_eq!(graph.node_count(), 5); //2 inputs, 2 steps, 1 output
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    #[serial]
    fn test_load_workflow_graph_02() {
        let path = "../../testdata/mkdir_wf.cwl";
        let workflow = load_workflow(path).unwrap();
        let graph = load_workflow_graph(&workflow, path).unwrap();

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
    }
}
