use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct MainProps {
    pub children: Element,
}

#[component]
pub fn Main(props: MainProps) -> Element {
    rsx! {
        main { class: "relative flex-1 bg-zinc-300 h-full w-full p-2", {props.children} }
    }
}
