use commonwl::prelude::*;
use dioxus::html::geometry::euclid::Point2D;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct VisualNode {
    pub id: String,
    pub instance: NodeInstance,
    pub path: Option<PathBuf>,
    pub position: Point2D<f32, f32>,
    pub inputs: Vec<Slot>,
    pub outputs: Vec<Slot>,
}

#[derive(Debug, Clone)]
pub enum NodeInstance {
    Step(CWLDocument),
    Input(CommandInputParameter), //WorkflowInputParameter
    Output(WorkflowOutputParameter),
}

impl NodeInstance {
    pub fn id(&self) -> String {
        match &self {
            Self::Step(doc) => doc.id.clone().unwrap().clone(),
            Self::Input(input) => input.id.clone(),
            Self::Output(output) => output.id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub id: String,
    pub type_: CWLType,
}

#[derive(Clone, PartialEq)]
pub enum SlotType {
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct VisualEdge {
    pub source_port: String,
    pub target_port: String,
    pub data_type: CWLType,
}
