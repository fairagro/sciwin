use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq, Default)]
pub struct LineProps {
    pub x_source: f32,
    pub y_source: f32,
    pub x_target: f32,
    pub y_target: f32,
    pub stroke: String,
    pub onclick: Option<EventHandler<MouseEvent>>,
}

#[component]
pub fn Line(props: LineProps) -> Element {
    let cx1 = props.x_source + 25.0; // move 50px to the right from source
    let cy1 = props.y_source;

    let cx2 = props.x_target - 25.0; // move 50px to the left from target
    let cy2 = props.y_target;

    let path_data = format!(
        "M {} {} C {} {}, {} {}, {} {}",
        props.x_source, props.y_source, cx1, cy1, cx2, cy2, props.x_target, props.y_target
    );

    let stroke_width = 3;

    rsx! {
        path {
            onclick: move |e| {
                if let Some(handler) = props.onclick {
                    handler.call(e);
                }
            },
            class: "{props.stroke}",
            d: "{path_data}",
            stroke_width: "{stroke_width}",
            fill: "transparent",
            style: "cursor: pointer;",
        }
    }
}
