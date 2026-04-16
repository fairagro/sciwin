use crate::components::graph::SlotElement;
use crate::types::{NodeInstance, SlotType};
use crate::{DragState, use_app_state, use_drag};
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use petgraph::graph::NodeIndex;

#[derive(Props, Clone, Copy, PartialEq)]
pub struct NodeProps {
    id: NodeIndex,
}

#[component]
pub fn NodeElement(props: NodeProps) -> Element {
    let mut app_state = use_app_state();
    let mut drag_state = use_drag();

    let graph = app_state().workflow.graph;
    let node = &graph[props.id];
    let pos_x = node.position.x;
    let pos_y = node.position.y;

    let top_color = match node.instance {
        NodeInstance::Step(_) => "bg-green-900",
        NodeInstance::Input(_) => "bg-blue-900",
        NodeInstance::Output(_) => "bg-red-900",
    };

    let mut drag_offset = drag_state.write().drag_offset;

    rsx! {
        div {
            class: "absolute border bg-zinc-700 rounded-md cursor-pointer w-48 z-2 text-white",
            left: "{pos_x}px",
            top: "{pos_y}px",
            onclick: move |e| {
                e.stop_propagation();
                if e.trigger_button() == Some(MouseButton::Primary)
                    && e.modifiers() == Modifiers::SHIFT
                {
                    app_state.write().workflow.remove_node(props.id)?;
                }
                Ok(())
            },
            div {
                onmousedown: move |e| {
                    drag_offset.write().x = e.data.client_coordinates().x;
                    drag_offset.write().y = e.data.client_coordinates().y;

                    drag_state.write().dragging = Some(DragState::Node(props.id));
                },

                class: "{top_color} rounded-t-md p-1 overflow-hidden",
                "{node.instance.id()}"
            }
            div { class: "p-1",

                div {
                    for slot in node.outputs.iter() {
                        div { class: "flex justify-end items-center",
                            "{slot.id}"
                            SlotElement {
                                slot: slot.clone(),
                                node_id: props.id,
                                slot_type: SlotType::Output,
                            }
                        }
                    }
                }

                div {
                    for slot in node.inputs.iter() {
                        div { class: "flex justify-start items-center",
                            SlotElement {
                                slot: slot.clone(),
                                node_id: props.id,
                                slot_type: SlotType::Input,
                            }
                            "{slot.id}"
                        }
                    }
                }
            }
        }
    }
}
