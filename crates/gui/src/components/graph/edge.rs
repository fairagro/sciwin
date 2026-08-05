use crate::{
    components::graph::{Line, styling},
    graph::{estimate_node_size},
    types::VisualNode,
    use_app_state, use_slot_positions, SlotType,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use petgraph::graph::EdgeIndex;
use crate::{HEADER_OFFSET, ITEM_HEIGHT};

pub(crate) fn calculate_source_position(source_node: &VisualNode, slot_id: &str) -> (f32, f32) {
    //get positions in array
    let fix = source_node
        .outputs
        .iter()
        .position(|o| o.id == slot_id)
        .unwrap_or_default();
    let (width, _) = estimate_node_size(source_node);
    let y_source = HEADER_OFFSET
        + (fix as f32 * ITEM_HEIGHT)
        + (ITEM_HEIGHT / 2.0 + 5.0)
        + source_node.position.y;
    let x_source = width + source_node.position.x;
    (x_source, y_source)
}

pub fn calculate_target_position(target_node: &VisualNode, slot_id: &str) -> (f32, f32) {
    //get positions in array
    let tix = target_node
        .inputs
        .iter()
        .position(|o| o.id == slot_id)
        .unwrap_or_default();
    let y_target = HEADER_OFFSET
        + (tix as f32 * ITEM_HEIGHT)
        + (ITEM_HEIGHT / 2.0 + 5.0)
        + target_node.position.y
        + (target_node.outputs.len() as f32 * ITEM_HEIGHT);
    let x_target = target_node.position.x;
    (x_target, y_target)
}

#[derive(Props, Clone, Copy, PartialEq)]
pub struct EdgeProps {
    id: EdgeIndex,
}

#[component]
pub fn EdgeElement(props: EdgeProps) -> Element {
    let mut app_state = use_app_state();
    let slot_positions = use_slot_positions();

    let graph = app_state().workflow.graph;
    let (from_node_id, to_node_id) = graph.edge_endpoints(props.id).unwrap();
    let from_node = &graph[from_node_id];
    let to_node = &graph[to_node_id];

    let edge = &graph[props.id];

    let fallback_source = calculate_source_position(from_node, &edge.source_port);
    let fallback_target = calculate_target_position(to_node, &edge.target_port);

    let positions = slot_positions.read();

    let (x_source, y_source) = positions
        .get(&(from_node_id, edge.source_port.clone(), SlotType::Output))
        .copied()
        .unwrap_or(fallback_source);

    let (x_target, y_target) = positions
        .get(&(to_node_id, edge.target_port.clone(), SlotType::Input))
        .copied()
        .unwrap_or(fallback_target);

    let slot_type = to_node
        .inputs
        .iter()
        .find(|i| i.id == edge.target_port)
        .unwrap()
        .type_
        .clone();
    let stroke = styling::get_stroke_from_port_type(&slot_type);

    rsx! {
        Line {
            x_source,
            y_source,
            x_target,
            y_target,
            stroke,
            onclick: move |e: Event<MouseData>| {
                e.stop_propagation();
                if e.trigger_button() == Some(MouseButton::Primary) && e.modifiers().shift() {
                    //disconnect on shift + left click
                    let mut state = app_state.write();
                    state.workflow.remove_connection(props.id)?;
                }
                Ok(())
            },
        }
    }
}
