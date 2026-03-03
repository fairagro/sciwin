use dioxus::prelude::*;
use dioxus_primitives::tabs::{self, TabContentProps, TabListProps, TabTriggerProps};
use crate::use_app_state;

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
    let mut app_state = use_app_state();
    let active_tab = use_signal(|| app_state.read().active_tab.to_string());
    {
        let mut active_tab = active_tab;
        let app_state = app_state;
        use_effect(move || {
            active_tab.set(app_state.read().active_tab.to_string());
        });
    }
    rsx! {
        tabs::Tabs {
            class: format!("{} select-none grid h-full w-full grid-rows-[auto_1fr]", props.class),
            value: active_tab(),
            on_value_change: move |new_value: String| {
                app_state.write().active_tab.set(new_value);
            },
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
            class: format!("{} p-1 border-1 border-zinc-400 bg-white", props.class.clone().unwrap_or_default()),
            value: props.value,
            id: props.id,
            index: props.index,
            attributes: props.attributes,
            {props.children}
        }
    }
}
