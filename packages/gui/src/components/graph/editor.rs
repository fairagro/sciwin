use crate::{
    DragContext, DragState,
    components::{
        ICON_SIZE, SmallRoundActionButton,
        files::Node,
        graph::{
            EdgeElement, Line, LineProps, NodeAddForm, NodeElement, calculate_source_position, calculate_target_position, get_stroke_from_cwl_type,
        },
    },
    graph::auto_layout,
    use_app_state,
    workflow::VisualWorkflow,
};
use commonwl::load_doc;
use dioxus::html::geometry::{
    ClientPoint, Pixels, PixelsSize, PixelsVector2D,
    euclid::{Point2D, Rect},
};
use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::md_maps_icons::MdCleaningServices};
use petgraph::visit::IntoNodeIdentifiers;
use serde_json::Value;
use std::{path::PathBuf, rc::Rc};

#[component]
pub fn GraphEditor(path: String) -> Element {
    let mut app_state = use_app_state();
    let mut path = use_reactive(&path, PathBuf::from);

    let dragging = None::<DragState>;
    let drag_offset = use_signal(ClientPoint::zero);
    let mut drag_state = use_signal(|| DragContext { drag_offset, dragging });
    use_context_provider(|| drag_state);

    let mut mouse_pos = use_signal(ClientPoint::zero);
    let mut open_add_menu = use_signal(|| false);

    {
        use_effect(move || {
            let path = path();
            let data = load_doc(&path).unwrap();
            if let commonwl::CWLDocument::Workflow(_) = data {
                let workflow = VisualWorkflow::from_file(path).unwrap();
                app_state.write().workflow = workflow;
            }
        });
    }

    let graph = app_state().workflow.graph;

    let mut new_line = use_signal(|| None::<LineProps>);
    let mut div_ref: Signal<Option<Rc<MountedData>>> = use_signal(|| None);

    struct DivDims {
        rect: Rect<f64, Pixels>,
        scroll_offset: PixelsVector2D,
        scroll_size: PixelsSize,
    }
    let read_dims = move || async move {
        let div = div_ref()?;
        Some(DivDims {
            rect: div.get_client_rect().await.ok()?,
            scroll_offset: div.get_scroll_offset().await.ok()?,
            scroll_size: div.get_scroll_size().await.ok()?,
        })
    };

    let mut dim_w = use_signal(|| 0.0);
    let mut dim_h = use_signal(|| 0.0);

    let update_dims = move || {
        spawn(async move {
            if let Some(dims) = read_dims().await {
                dim_w.set(dims.scroll_size.width);
                dim_h.set(dims.scroll_size.height);
            }
        });
    };

    let get_position_relative = move |current_pos: Point2D<f64, _>| async move {
        let dims = read_dims().await.unwrap();
        let rect = dims.rect;
        let scroll = dims.scroll_offset;
        let base_pos = (current_pos.x - rect.origin.x, current_pos.y - rect.origin.y);
        let x_target = base_pos.0 + scroll.x;
        let y_target = base_pos.1 + scroll.y;
        (x_target, y_target)
    };

    rsx! {
        div {
            class: "relative select-none overflow-scroll w-full h-full focus:outline-none ",
            style: "background: url({asset!(\"/assets/graph-paper.svg\")});",
            tabindex: "0",
            onresize: move |_| update_dims(),
            onscroll: move |_| update_dims(),
            onmounted: move |e| div_ref.set(Some(e.data())),
            ondragover: move |e| {
                e.prevent_default();
                e.stop_propagation();
            },
            ondrop: move |e| {
                e.data_transfer().set_drop_effect("copy");
                e.prevent_default();
                e.stop_propagation();
                let item = app_state().get_data_transfer::<Node>()?;

                let mut cwl = load_doc(&item.path).map_err(|e| anyhow::anyhow!("{e}"))?;
                let working_dir = app_state().working_directory.unwrap();
                if let Some(path_relative_to_root) = pathdiff::diff_paths(
                    &item.path,
                    &working_dir,
                ) {
                    let name = item.name.strip_suffix(".cwl").unwrap_or(&item.name);
                    app_state
                        .write()
                        .workflow
                        .add_new_step_if_not_exists(
                            name,
                            path_relative_to_root.to_string_lossy().as_ref(),
                            &mut cwl,
                            &working_dir,
                        )?;
                }

                //we accepted the data transfer so we clear it
                app_state.write().set_data_transfer(&Value::Null)?;
                Ok(())
            },
            onmousemove: move |e| async move {
                e.stop_propagation();
                if !open_add_menu() {
                    //store mouse pos if menu is closed
                    let current_pos = e.client_coordinates();
                    let pos = get_position_relative(current_pos).await;
                    mouse_pos.set(Point2D::new(pos.0, pos.1));

                }
                if let Some(dragstate) = drag_state().dragging {
                    //we are dragging
                    let current_pos = e.client_coordinates();

                    match dragstate {
                        DragState::None => todo!(),
                        DragState::Node(node_index) => {
                            //we are dragging a node
                            let last_pos = (drag_state().drag_offset)();

                            let deltaX = current_pos.x - last_pos.x;
                            let deltaY = current_pos.y - last_pos.y;

                            let pos = app_state.read().workflow.graph[node_index].position;
                            app_state.write().workflow.graph[node_index].position = Point2D::new(
                                //we are dragging from a connection

                                pos.x + deltaX as f32,
                                pos.y + deltaY as f32,
                            );
                            drag_state.write().drag_offset.set(current_pos);
                        }
                        DragState::Connection { source_node, source_port } => {
                            let base_pos = get_position_relative(current_pos).await;
                            let x_target = (base_pos.0) as f32;
                            let y_target = (base_pos.1) as f32;

                            let source_node = &app_state.read().workflow.graph[source_node];
                            let (x_source, y_source) = calculate_source_position(
                                source_node,
                                &source_port,
                            );
                            if let Some(cwl_type) = source_node
                                .outputs
                                .iter()
                                .find(|i| i.id == source_port)
                                .map(|i| i.type_.clone())
                            {
                                let stroke = get_stroke_from_cwl_type(cwl_type);
                                new_line
                                    .set(
                                        Some(LineProps {
                                            x_source,
                                            y_source,
                                            x_target,
                                            y_target,
                                            stroke: stroke.to_string(),
                                            onclick: None,
                                        }),
                                    );
                            } else {
                                let cwl_type = source_node
                                    .inputs
                                    .iter()
                                    .find(|i| i.id == source_port)
                                    .map(|i| i.type_.clone())
                                    .unwrap();
                                let stroke = get_stroke_from_cwl_type(cwl_type);
                                let (x_source, y_source) = calculate_target_position(
                                    source_node,
                                    &source_port,
                                );
                                new_line
                                    .set(
                                        Some(LineProps {
                                            x_source,
                                            y_source,
                                            x_target,
                                            y_target,
                                            stroke: stroke.to_string(),
                                            onclick: None,
                                        }),
                                    );
                            }
                        }
                    }
                }
            },
            onmouseup: move |_| {
                //reset state
                drag_state.write().dragging = None;
                new_line.set(None);
            },
            onkeydown: move |e| {
                //listen for shift+a
                if e.key() == Key::Character("A".to_string())
                    || e.key() == Key::Character("a".to_string())
                        && e.modifiers() == Modifiers::SHIFT
                {
                    open_add_menu.toggle();
                    e.stop_propagation();
                }
                if e.key() == Key::Escape {
                    open_add_menu.set(false);
                    e.stop_propagation();
                }
            },
            SmallRoundActionButton {
                class: "hover:bg-fairagro-mid-200 fixed top-2 right-2 z-10",
                title: "Auto Align Nodes",
                onclick: move |_| {
                    let mut graph = app_state().workflow.graph;
                    auto_layout(&mut graph);
                    app_state.write().workflow.graph = graph;
                },
                Icon {
                    width: ICON_SIZE,
                    height: ICON_SIZE,
                    icon: MdCleaningServices,
                }
            }
            NodeAddForm {
                open: open_add_menu,
                pos: mouse_pos,
                project_path: app_state().working_directory.unwrap_or_default(),
            }
            for id in graph.node_identifiers() {
                NodeElement { id }
            }
            svg {
                width: "{dim_w}",
                height: "{dim_h}",
                view_box: "0 0 {dim_w} {dim_h}",
                class: "absolute inset-0  pointer-events-auto",
                for id in graph.edge_indices() {
                    g {
                        EdgeElement { id }
                    }
                }
                if let Some(line) = &*new_line.read() {
                    g {
                        Line {
                            x_source: line.x_source,
                            y_source: line.y_source,
                            x_target: line.x_target,
                            y_target: line.y_target,
                            stroke: line.stroke.clone(),
                            onclick: line.onclick,
                        }
                    }
                }
            }
        }
    }
}
