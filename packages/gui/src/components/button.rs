use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct ButtonProps {
    #[props(optional)]
    pub title: String,

    #[props(optional)]
    pub class: String,

    #[props(optional)]
    pub onclick: EventHandler<MouseEvent>,

    pub children: Element,
}

fn base_button(props: ButtonProps, base_class: &str) -> Element {
    let ButtonProps {
        title,
        class,
        onclick,
        children,
    } = props;

    let class = format!("{base_class} {class}");

    rsx! {
        button {
            class: "{class}",
            title: "{title}",
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}

#[component]
pub fn RoundActionButton(props: ButtonProps) -> Element {
    base_button(
        props,
        "cursor-pointer rounded-full justify-center items-center p-3 \
         bg-fairagro-mid-500 select-none hover:bg-fairagro-dark-500 \
         hover:text-white hover:rotate-45 transition-[rotate] duration-500",
    )
}

#[component]
pub fn SmallRoundActionButton(props: ButtonProps) -> Element {
    base_button(
        props,
        "cursor-pointer p-1 rounded-full hover:rotate-20 transition-[rotate] duration-200",
    )
}

#[component]
pub fn NonRotatingActionButton(props: ButtonProps) -> Element {
    base_button(
        props,
        "cursor-pointer flex items-center gap-1 px-2 py-1 rounded-md \
         hover:bg-fairagro-dark-500 transition-colors duration-150",
    )
}