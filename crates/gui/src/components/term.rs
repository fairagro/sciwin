use crate::use_app_state;
use dioxus::prelude::*;
use std::{env, fs, path::Path};

#[component]
pub fn Terminal(value: Signal<String>) -> Element {
    let app_state = use_app_state();
    let working_dir_path = use_memo(move || app_state.read().working_directory.clone().unwrap());
    let working_dir = use_memo(move || working_dir_path().to_string_lossy().to_string());

    let mut candidates: Signal<Vec<String>> = use_signal(Vec::new);
    let mut candidate_selected = use_signal(|| 0);

    let mut insert_candidate = move |item: &str| {
        let tmp = value();
        let last_word = tmp.split_whitespace().last().unwrap_or_default();
        let tmp = tmp.strip_suffix(last_word).unwrap_or(&tmp);
        let tmp = tmp.trim();

        let space = if tmp.is_empty() { "" } else { " " };
        value.set(format!("{tmp}{space}{item}"));
        candidates.set(vec![])
    };

    let mut next_candidate = move || {
        candidate_selected += 1;
        if candidate_selected() >= candidates().len() {
            candidate_selected.set(0);
        }
    };

    let mut prev_candidate = move || {
        if candidate_selected() == 0 {
            candidate_selected.set(candidates().len() - 1);
        } else {
            candidate_selected -= 1;
        }
    };

    let mut insert_selected_candidate = move || {
        let candidate = &candidates()[candidate_selected()];
        insert_candidate(candidate);
    };

    const COMPLETE_ITEMS: usize = 10;

    rsx! {
        div { class: "relative",
            div { class: "bg-zinc-900 text-white text-lg font-mono flex flex-col gap-1 px-2 py-1",
                p { class: "text-xs text-fairagro-mid-500", "{working_dir}" }
                div { class: "flex gap-1",
                    ">"
                    input {
                        class: "appearance-none w-full focus:outline-none",
                        r#type: "text",
                        placeholder: "run command",
                        value: "{value}",
                        oninput: move |e| {
                            value.set(e.value());
                            candidates.clear();
                            Ok(())
                        },
                        onkeydown: move |e| {
                            if e.key() == Key::Tab {
                                //capture tab
                                e.prevent_default();
                                if candidates().is_empty() {
                                    candidates
                                        .set(
                                            handle_completions(&value(), &working_dir_path(), COMPLETE_ITEMS),
                                        );
                                } else {
                                    next_candidate()
                                }
                            }
                            if e.key() == Key::ArrowDown && !candidates().is_empty() {
                                next_candidate();
                            }
                            if e.key() == Key::ArrowUp && !candidates().is_empty() {
                                prev_candidate()
                            }
                            if e.key() == Key::Enter && !candidates().is_empty() {
                                e.prevent_default();
                                e.stop_propagation();
                                insert_selected_candidate()
                            }
                            if e.key() == Key::Escape {
                                candidates.clear();
                            }
                            Ok(())
                        },
                    }
                }
            }
            if !candidates().is_empty() {
                div { class: "flex flex-col absolute bg-zinc-900/70 min-w-40 items-start text-white",
                    for (ix , item) in candidates().into_iter().enumerate() {
                        button {
                            class: if ix == candidate_selected() { "font-bold" },
                            class: "py-0.5 px-2",
                            onclick: move |_| insert_candidate(&item),
                            "{item}"
                        }
                    }
                }
            }
        }
    }
}

fn handle_completions(value: &str, current_dir: &Path, limit: usize) -> Vec<String> {
    let last_word = value.split_whitespace().last().unwrap_or_default();

    let mut candidates = get_path_binaries(last_word);
    candidates.extend(get_files(last_word, current_dir));

    candidates.sort();
    candidates.dedup();
    candidates[..candidates.len().min(limit)].to_vec()
}

fn get_path_binaries(unfinished_word: &str) -> Vec<String> {
    let Ok(path) = env::var("PATH") else { return vec![] };
    env::split_paths(&path)
        .flat_map(|dir| fs::read_dir(dir).unwrap())
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with(unfinished_word) { Some(name) } else { None }
        })
        .collect()
}

fn get_files(unfinished_path: &str, current_dir: &Path) -> Vec<String> {
    let path = if unfinished_path.starts_with("/") {
        Path::new(unfinished_path)
    } else {
        &current_dir.join(unfinished_path)
    };

    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(Path::new(".")) };

    std::fs::read_dir(dir)
        .expect("Directory not found")
        .filter_map(|e| e.ok())
        .map(|e| e.path().display().to_string())
        .filter(|p| p.starts_with(&path.to_string_lossy().to_string()))
        .map(|e| e.strip_prefix(&format!("{}/", current_dir.to_string_lossy())).unwrap_or(&e).to_string())
        .collect()
}
