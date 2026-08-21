use crate::{
    DragState,
    components::graph::styling,
    types::{Slot, SlotType},
    use_app_state, use_drag, use_slot_positions, use_canvas_frame,
};
use dioxus::prelude::*;
use petgraph::graph::NodeIndex;
use std::rc::Rc;

#[derive(Props, Clone, PartialEq)]
pub(crate) struct SlotProps {
    node_id: NodeIndex,
    slot: Slot,
    slot_type: SlotType,
}

#[component]
pub fn SlotElement(props: SlotProps) -> Element {
    let app_state = use_app_state();
    let mut drag_state = use_drag();
    let mut slot_positions = use_slot_positions();
    let canvas_frame = use_canvas_frame();

    let margin = match props.slot_type {
        SlotType::Input => "ml-[-9px]",
        SlotType::Output => "mr-[-9px]",
    };

    let geometry = styling::slot_geometry(&props.slot.type_);
    let bg = styling::slot_bg(&props.slot.type_);
    let border = styling::slot_border(&props.slot.type_);

    let node_id = props.node_id;
    let slot_type = props.slot_type;
    let slot_id_for_measure = props.slot.id.clone();
    let slot_id_for_events = props.slot.id.clone();

    let node_position = use_memo(move || app_state().workflow.graph[node_id].position);

    let mut mounted: Signal<Option<Rc<MountedData>>> = use_signal(|| None);

    use_effect(move || {
        let _ = node_position();

        let frame = canvas_frame();
        let mounted_data = mounted();

        let node_id = node_id;
        let slot_id = slot_id_for_measure.clone();
        let slot_type = slot_type.clone();

        if let Some(m) = mounted_data {
            spawn(async move {
                if let Ok(rect) = m.get_client_rect().await {
                    let center_x = rect.origin.x + rect.size.width / 2.0;
                    let center_y = rect.origin.y + rect.size.height / 2.0;

                    let x = (center_x - frame.origin.0) + frame.scroll.0;
                    let y = (center_y - frame.origin.1) + frame.scroll.1;

                    slot_positions
                        .write()
                        .insert((node_id, slot_id, slot_type), (x as f32, y as f32));
                }
            });
        }
    });

    rsx! {
        div {
            onmounted: move |e| mounted.set(Some(e.data())),
            onmousedown: {
                let slot_id = slot_id_for_events.clone();
                move |_| {
                    drag_state.write().dragging = Some(DragState::Connection {
                        source_node: node_id,
                        source_port: slot_id.clone(),
                    });
                }
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
