use crate::{
    DragState,
    components::graph::styling,
    types::{Slot, SlotType},
    use_app_state, use_drag,
};
use dioxus::prelude::*;
use petgraph::graph::NodeIndex;

#[derive(Props, Clone, PartialEq)]
pub(crate) struct SlotProps {
    node_id: NodeIndex,
    slot: Slot,
    slot_type: SlotType,
}

#[component]
pub fn SlotElement(props: SlotProps) -> Element {
    let mut drag_state = use_drag();

    let margin = match props.slot_type {
        SlotType::Input => "ml-[-9px]",
        SlotType::Output => "mr-[-9px]",
    };

    let geometry = styling::slot_geometry(&props.slot.type_);
    let bg = styling::slot_bg(&props.slot.type_);
    let border = styling::slot_border(&props.slot.type_);

    let node_id = props.node_id;
    let slot_id = props.slot.id.clone();

    rsx! {
        div {
            onmousedown: move |_| {
                drag_state.write().dragging = Some(DragState::Connection {
                    source_node: node_id,
                    source_port: slot_id.clone(),
                });
            },
            onmouseup: move |_| {
                //check whether we are in connection mode and node/port has changed
                let graph = &use_app_state()().workflow.graph;
                if let Some(DragState::Connection { source_node, source_port }) = drag_state()
                    //get source and target nodes
                    //check whether this edge already exists
                    //do not create edge twice
                    //check valid connection type
                    .dragging && (source_node, &source_port) != (node_id, &props.slot.id)
                {
                    let source = &graph[source_node];
                    let target = &graph[node_id];
                    if graph.contains_edge(source_node, node_id) {
                        let edges = graph.edges_connecting(source_node, node_id);
                        for edge in edges {
                            if edge.weight().source_port == source_port
                                && edge.weight().target_port == props.slot.id
                            {
                                return Ok(());
                            }
                        }
                    }
                    let (check, reversed) = if let Some(output) = source
                        .outputs
                        .iter()
                        .find(|i| i.id == source_port)

                        && let Some(input) = target.inputs.iter().find(|i| i.id == props.slot.id)
                    {
                        (input.type_.accepts(&output.type_), false)
                    } else if let Some(output) = source
                        .inputs
                        .iter()
                        .find(|i| i.id == source_port)
                        && let Some(input) = target
                            .outputs
                            .iter()
                            .find(|i| i.id == props.slot.id)
                    {
                        (input.type_.accepts(&output.type_), true)
                    } else {
                        (false, false)
                    };
                    if check {
                        if !reversed {
                            use_app_state()
                                .write()
                                .workflow
                                .add_connection(
                                    source_node,
                                    &source_port,
                                    node_id,
                                    &props.slot.id,
                                )?;
                        } else {
                            use_app_state()
                                .write()
                                .workflow
                                .add_connection(
                                    node_id,
                                    &props.slot.id,
                                    source_node,
                                    &source_port,
                                )?;
                        }
                    }
                }
                Ok(())
            },
            class: "{bg} w-3 h-3 m-2 {geometry} {margin} {border} z-2",
        }
    }
}
