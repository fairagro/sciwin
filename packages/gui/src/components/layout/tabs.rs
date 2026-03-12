use dioxus::prelude::*;
use dioxus_primitives::tabs::{self, TabContentProps, TabListProps, TabTriggerProps};

#[derive(Props, Clone, PartialEq)]
pub struct TabsProps {
    #[props(default)]
    pub class: String,

    #[props(default)]
    pub default_value: String,

    pub children: Element,
}

#[component]
pub fn Tabs(props: TabsProps) -> Element {
    rsx! {
        tabs::Tabs {
            class: props.class + " select-none grid h-full w-full grid-rows-[auto_1fr]",
            default_value: props.default_value,
            {props.children}
        }
    }
}

#[component]
pub fn TabList(props: TabListProps) -> Element {
    rsx! {
        tabs::TabList { class: "tabs-list select-none", attributes: props.attributes, {props.children} }
    }
}

#[component]
pub fn TabTrigger(props: TabTriggerProps) -> Element {
    rsx! {
        tabs::TabTrigger {
            class: "select-none py-1 px-3 rounded-t-md bg-zinc-300 hover:bg-zinc-200 data-[state=active]:bg-white data-[state=active]:border-b-0 border border-zinc-400",
            id: props.id,
            value: props.value,
            index: props.index,
            disabled: props.disabled,
            attributes: props.attributes,
            {props.children}
        }
    }
}

#[component]
pub fn TabContent(props: TabContentProps) -> Element {
    rsx! {
        tabs::TabContent {
            class: props.class.unwrap_or_default() + " p-1 border-1 border-zinc-400 bg-white",
            value: props.value,
            id: props.id,
            index: props.index,
            attributes: props.attributes,
            {props.children}
        }
    }
}