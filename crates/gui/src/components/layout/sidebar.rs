use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct SidebarProps {
    pub children: Element,
}

#[component]
pub fn Sidebar(props: SidebarProps) -> Element {
    rsx! {
        aside { class: "sticky w-64 shrink-0",
            nav { class: "flex flex-col w-64 fixed left-0 select-none bg-zinc-200 border-r border-zinc-400 h-full",
                div { class: "flex w-full p-2",
                    Logo {}
                    div { class: "mt-4" }
                }
                div { class: "flex flex-col flex-grow overflow-hidden min-h-0 p-2",
                    {props.children}
                }
            }
        }
    }
}

#[component]
pub fn Logo() -> Element {
    rsx! {
        div {
            img { src: asset!("/assets/logo.png"), width: 150 }
        }
    }
}
