use commonwl::{
    Identifiable, OneOrMany, documents::CWLDocument, inputs::{InputType, WorkflowInputParameter}, outputs::{CommandOutputParameterType, WorkflowOutputParameter}
};
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
    Input(WorkflowInputParameter),
    Output(WorkflowOutputParameter),
}

impl NodeInstance {
    pub fn id(&self) -> String {
        match self {
            Self::Step(doc) => doc.get_id().cloned().unwrap_or_default(),
            Self::Input(input) => input.id.clone().unwrap_or_default(),
            Self::Output(output) => output.id.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortType {
    Input(OneOrMany<InputType>),
    Output(CommandOutputParameterType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub id: String,
    pub type_: PortType,
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
    pub data_type: PortType,
}
