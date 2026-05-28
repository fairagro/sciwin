use crate::types::PortType;
use commonwl::{
    OneOrMany,
    inputs::{InputSchema, InputType},
    outputs::{CommandOutputParameterType, CommandOutputSchema, CommandOutputType},
    types::CWLType,
};

fn primary_cwl_type(port_type: &PortType) -> Option<CWLType> {
    match port_type {
        PortType::Input(one_or_many) => {
            let iter: Box<dyn Iterator<Item = &InputType>> = match one_or_many {
                OneOrMany::One(t) => Box::new(std::iter::once(t)),
                OneOrMany::Many(v) => Box::new(v.iter()),
            };
            iter.filter_map(|t| match t {
                InputType::CWLType(cwl) => Some(*cwl),
                InputType::String(_) => Some(CWLType::String),
                InputType::InputSchema(_) => None,
            })
            .find(|t| !matches!(t, CWLType::Null))
            .or(Some(CWLType::Null))
        }

        PortType::Output(copt) => match copt {
            CommandOutputParameterType::Stdout | CommandOutputParameterType::Stderr => None, // handled separately below
            CommandOutputParameterType::CommandOutputType(one_or_many) => {
                let iter: Box<dyn Iterator<Item = &CommandOutputType>> = match one_or_many {
                    OneOrMany::One(t) => Box::new(std::iter::once(t)),
                    OneOrMany::Many(v) => Box::new(v.iter()),
                };
                iter.filter_map(|t| match t {
                    CommandOutputType::CWLType(cwl) => Some(*cwl),
                    CommandOutputType::String(_) => Some(CWLType::String),
                    CommandOutputType::CommandOutputSchema(_) => None,
                })
                .find(|t| !matches!(t, CWLType::Null))
                .or(Some(CWLType::Null))
            }
        },
    }
}
fn is_array(port_type: &PortType) -> bool {
    match port_type {
        PortType::Input(OneOrMany::One(InputType::InputSchema(s))) => {
            matches!(**s, InputSchema::Array(_))
        }
        PortType::Output(CommandOutputParameterType::CommandOutputType(OneOrMany::One(
            CommandOutputType::CommandOutputSchema(s),
        ))) => matches!(**s, CommandOutputSchema::Array(_)),
        _ => false,
    }
}

fn is_optional(port_type: &PortType) -> bool {
    match port_type {
        PortType::Input(OneOrMany::Many(v)) => v
            .iter()
            .any(|t| matches!(t, InputType::CWLType(CWLType::Null))),
        PortType::Output(CommandOutputParameterType::CommandOutputType(OneOrMany::Many(v))) => v
            .iter()
            .any(|t| matches!(t, CommandOutputType::CWLType(CWLType::Null))),
        _ => false,
    }
}

fn cwl_type_to_stroke(t: &CWLType) -> &'static str {
    match t {
        CWLType::String => "stroke-red-400",
        CWLType::File => "stroke-green-400",
        CWLType::Directory => "stroke-blue-400",
        CWLType::Boolean => "stroke-yellow-400",
        CWLType::Double => "stroke-purple-400",
        CWLType::Float => "stroke-pink-400",
        CWLType::Long => "stroke-cyan-400",
        CWLType::Int => "stroke-teal-400",
        CWLType::Null | CWLType::Any => "stroke-slate-400",
    }
}

fn cwl_type_to_bg(t: &CWLType) -> &'static str {
    match t {
        CWLType::File => "bg-green-400",
        CWLType::Directory => "bg-blue-400",
        CWLType::String => "bg-red-400",
        CWLType::Boolean => "bg-yellow-400",
        CWLType::Double => "bg-purple-400",
        CWLType::Float => "bg-pink-400",
        CWLType::Long => "bg-cyan-400",
        CWLType::Int => "bg-teal-400",
        CWLType::Null | CWLType::Any => "bg-slate-400",
    }
}

pub(crate) fn get_stroke_from_port_type(port_type: &PortType) -> &'static str {
    match port_type {
        PortType::Output(CommandOutputParameterType::Stdout) => "stroke-slate-300",
        PortType::Output(CommandOutputParameterType::Stderr) => "stroke-slate-200",
        _ => primary_cwl_type(port_type)
            .as_ref()
            .map(cwl_type_to_stroke)
            .unwrap_or("stroke-slate-400"),
    }
}

pub(crate) fn slot_geometry(port_type: &PortType) -> &'static str {
    let is_file_like = matches!(
        primary_cwl_type(port_type),
        Some(CWLType::File) | Some(CWLType::Directory)
    );
    let is_stream = matches!(
        port_type,
        PortType::Output(CommandOutputParameterType::Stdout | CommandOutputParameterType::Stderr)
    );
    if is_file_like || is_stream {
        "rotate-45"
    } else {
        "rounded-lg"
    }
}

pub(crate) fn slot_bg(port_type: &PortType) -> &'static str {
    match port_type {
        PortType::Output(CommandOutputParameterType::Stdout) => "bg-slate-300",
        PortType::Output(CommandOutputParameterType::Stderr) => "bg-slate-200",
        _ => primary_cwl_type(port_type)
            .as_ref()
            .map(cwl_type_to_bg)
            .unwrap_or("bg-slate-400"),
    }
}

pub(crate) fn slot_border(port_type: &PortType) -> &'static str {
    if is_array(port_type) {
        "border border-green-800"
    } else if is_optional(port_type) {
        "border border-red-800"
    } else {
        "border border-1 border-black"
    }
}
