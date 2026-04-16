use crate::{
    graph::{WorkflowGraph, load_workflow_graph},
    save_file_canonical,
    types::{NodeInstance, Slot, VisualEdge, VisualNode},
};
use commonwl::{format::format_cwl, load_workflow, prelude::*};
use dioxus::html::geometry::euclid::Point2D;
use petgraph::{
    Direction,
    graph::{EdgeIndex, NodeIndex},
    visit::EdgeRef,
};
use rand::Rng;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Viewmodel implementation for Workflow
#[derive(Default, Debug, Clone)]
pub struct VisualWorkflow {
    pub path: Option<PathBuf>,
    pub workflow: Workflow,
    pub graph: WorkflowGraph,
}

impl VisualWorkflow {
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let workflow = load_workflow(path).map_err(|e| anyhow::anyhow!("{e}"))?;
        let graph = load_workflow_graph(&workflow, path)?;
        Ok(Self {
            path: Some(path.to_path_buf()),
            workflow,
            graph,
        })
    }
}

impl VisualWorkflow {
    pub fn add_new_step_if_not_exists(&mut self, name: &str, path: &str, doc: &mut CWLDocument, working_dir: &Path) -> anyhow::Result<()> {
        //prevent name collisions
        let mut i = 1;
        let mut final_name = name.to_string();
        while self.workflow.has_step(&final_name) {
            final_name = format!("{name}{i}");
            i += 1;
        }
        let name = &final_name.as_str();

        s4n_core::workflow::add_workflow_step(&mut self.workflow, name, path, doc);

        let path = Path::new(path);
        if doc.id.is_none() {
            doc.id = Some(path.file_name().unwrap().to_string_lossy().to_string());
        }

        let mut rng = rand::rng();
        self.graph.add_node(VisualNode {
            id: name.to_string(),
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
            path: Some(working_dir.join(path)),
            position: Point2D::new(rng.random_range(0.0..=100.0), rng.random_range(0.0..=100.0)),
        });

        self.save()
    }

    pub fn add_input(&mut self, id: &str, type_: CWLType) -> anyhow::Result<()> {
        let input = CommandInputParameter::default().with_id(id).with_type(type_);
        self.workflow.inputs.push(input.clone());

        let mut rng = rand::rng();
        self.graph.add_node(VisualNode {
            id: id.to_string(),
            instance: NodeInstance::Input(input.clone()),
            outputs: vec![Slot {
                id: input.id.clone(),
                type_: input.type_.clone(),
            }],
            inputs: vec![],
            path: None,
            position: Point2D::new(0.0, rng.random_range(0.0..=100.0)),
        });

        self.save()
    }

    pub fn add_output(&mut self, id: &str, type_: CWLType) -> anyhow::Result<()> {
        let output = WorkflowOutputParameter {
            type_,
            id: id.to_owned(),
            ..Default::default()
        };

        self.workflow.outputs.push(output.clone());

        let mut rng = rand::rng();
        self.graph.add_node(VisualNode {
            id: id.to_string(),
            instance: NodeInstance::Output(output.clone()),
            outputs: vec![],
            inputs: vec![Slot {
                id: output.id.clone(),
                type_: output.type_.clone(),
            }],
            path: None,
            position: Point2D::new(0.0, rng.random_range(0.0..=100.0)),
        });

        self.save()
    }

    pub fn add_connection(&mut self, from_id: NodeIndex, from_slot_id: &str, to_id: NodeIndex, to_slot_id: &str) -> anyhow::Result<()> {
        let from_node = &self.graph[from_id];
        let to_node = &self.graph[to_id];

        let from_name = &from_node.id;
        let to_name = &to_node.id;

        let from_filename = &from_node.path;
        let to_filename = &to_node.path;

        if self.workflow.has_step(from_name)
            && self.workflow.has_step(to_name)
            && let Some(from_filename) = from_filename
            && let Some(to_filename) = to_filename
        {
            s4n_core::workflow::add_workflow_step_connection(
                &mut self.workflow,
                from_filename,
                from_name,
                from_slot_id,
                to_filename,
                to_name,
                to_slot_id,
            )?;
        } else if !self.workflow.has_step(from_name)
            && let Some(to_filename) = to_filename
        {
            // from name is input
            s4n_core::workflow::add_workflow_input_connection(&mut self.workflow, from_slot_id, to_filename, to_name, to_slot_id)?;
        } else if !self.workflow.has_step(to_name)
            && let Some(from_filename) = from_filename
        {
            // from to name is output
            s4n_core::workflow::add_workflow_output_connection(&mut self.workflow, from_name, from_slot_id, from_filename, to_name)?;
        } else {
            anyhow::bail!("undefined connection command")
        }

        let cwl_type = &from_node.outputs.iter().find(|o| o.id == from_slot_id).unwrap().type_;

        self.graph.add_edge(
            from_id,
            to_id,
            VisualEdge {
                source_port: from_slot_id.to_owned(),
                target_port: to_slot_id.to_owned(),
                data_type: cwl_type.clone(),
            },
        );

        self.save()
    }

    pub fn remove_connection(&mut self, index: EdgeIndex) -> anyhow::Result<()> {
        let edge = &self.graph[index];

        let (from_node_id, to_node_id) = self.graph.edge_endpoints(index).unwrap();
        let from_node = &self.graph[from_node_id];
        let to_node = &self.graph[to_node_id];

        let from_node = &from_node.id;
        let to_node = &to_node.id;

        let to_slot = edge.target_port.clone();
        //todo if input/output in named like a step it is confused!
        if self.workflow.has_step(from_node) && self.workflow.has_step(to_node) {
            s4n_core::workflow::remove_workflow_step_connection(&mut self.workflow, to_node, &to_slot)?
        } else if !self.workflow.has_step(from_node) {
            s4n_core::workflow::remove_workflow_input_connection(&mut self.workflow, from_node, to_node, &to_slot, false)?
        } else if !self.workflow.has_step(to_node) {
            s4n_core::workflow::remove_workflow_output_connection(&mut self.workflow, to_node, false)?
        } else {
            anyhow::bail!("undefined disconnection command")
        }

        self.graph.remove_edge(index);
        self.save()
    }

    pub fn remove_node(&mut self, index: NodeIndex) -> anyhow::Result<()> {
        //disconnect from all nodes (save() is already called in that!)
        while let Some(edge) = self.graph.edges_directed(index, Direction::Incoming).next() {
            self.remove_connection(edge.id())?;
        }
        while let Some(edge) = self.graph.edges_directed(index, Direction::Outgoing).next() {
            self.remove_connection(edge.id())?;
        }

        //remove node
        let node = &self.graph[index];
        match &node.instance {
            NodeInstance::Step(_) => self.workflow.steps.retain(|s| s.id != node.id),
            NodeInstance::Input(_) => self.workflow.inputs.retain(|s| s.id != node.id),
            NodeInstance::Output(_) => self.workflow.outputs.retain(|s| s.id != node.id),
        }

        self.graph.remove_node(index);
        self.save()
    }

    fn save(&mut self) -> anyhow::Result<()> {
        let mut yaml = serde_yaml::to_string(&self.workflow)?;

        yaml = format_cwl(&yaml).map_err(|e| anyhow::anyhow!("Could not format yaml: {e}"))?;
        let path = self.path.clone().unwrap();
        let mut file = fs::File::create(&path)?;
        file.write_all(yaml.as_bytes())?;

        save_file_canonical(&path, &format!("🧩 Saved changes on Workflow {path:?}!"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::execution::io::copy_dir;
    use repository::{Repository, initial_commit};
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;
    use tempfile::tempdir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        copy_dir("../../testdata/hello_world", dir.path()).unwrap();
        initial_commit(&repo).unwrap();
        let current = env::current_dir().unwrap();
        env::set_current_dir(dir.path()).unwrap();
        (current, dir)
    }

    #[test]
    #[serial]
    fn test_load_workflow() {
        let (current, dir) = setup();
        let path = dir.path().join("workflows/main/main.cwl");
        let wf = VisualWorkflow::from_file(path).unwrap();
        assert_eq!(wf.graph.node_count(), 5);
        env::set_current_dir(current).unwrap();
    }

    #[test]
    #[serial]
    fn test_add_input_to_workflow() {
        let (current, dir) = setup();
        let path = dir.path().join("workflows/main/main.cwl");
        let mut wf = VisualWorkflow::from_file(path).unwrap();

        wf.add_input("wurstbrot", CWLType::Any).unwrap();
        assert!(wf.workflow.has_input("wurstbrot"));

        let ix = wf.graph.node_indices().find(|i| wf.graph[*i].id == "wurstbrot").unwrap();
        wf.remove_node(ix).unwrap();
        assert!(!wf.workflow.has_input("wurstbrot"));
        env::set_current_dir(current).unwrap();
    }

    #[test]
    #[serial]
    fn test_add_output_to_workflow() {
        let (current, dir) = setup();
        let path = dir.path().join("workflows/main/main.cwl");
        let mut wf = VisualWorkflow::from_file(path).unwrap();

        wf.add_output("merzlos", CWLType::Any).unwrap();
        assert!(wf.workflow.has_output("merzlos"));

        let ix = wf.graph.node_indices().find(|i| wf.graph[*i].id == "merzlos").unwrap();
        wf.remove_node(ix).unwrap();
        assert!(!wf.workflow.has_output("merzlos"));
        env::set_current_dir(current).unwrap();
    }

    #[test]
    #[serial]
    fn test_add_connection_to_workflow() {
        let (current, dir) = setup();
        let path = dir.path().join("workflows/main/main.cwl");
        let mut wf = VisualWorkflow::from_file(path).unwrap();

        let ix_calc = wf.graph.node_indices().find(|i| wf.graph[*i].id.contains("calculation")).unwrap();
        let ix_plot = wf.graph.node_indices().find(|i| wf.graph[*i].id.contains("plot")).unwrap();

        let edge_ix = wf.graph.find_edge(ix_calc, ix_plot).unwrap();

        wf.remove_connection(edge_ix).unwrap();

        assert_eq!(wf.graph.edge_count(), 3);

        wf.add_connection(ix_calc, "results", ix_plot, "results").unwrap();

        assert_eq!(wf.graph.edge_count(), 4);
        env::set_current_dir(current).unwrap();
    }
}
