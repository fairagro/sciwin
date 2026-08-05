use crate::components::files::{Node, read_node_type};
use ignore::WalkBuilder;
use repository::Repository;
use std::{collections::HashMap, path::Path};

pub fn get_cwl_files(path: impl AsRef<Path>) -> Vec<Node> {
    let mut result = vec![];

    for entry in WalkBuilder::new(path).standard_filters(true).build().filter_map(Result::ok) {
        if entry.file_type().is_some_and(|t| t.is_file()) && entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("cwl")) {
            let type_ = read_node_type(entry.path());

            result.push(Node {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_path_buf(),
                children: vec![],
                is_dir: false,
                type_,
            });
        }
    }

    result
}

pub fn get_submodules_cwl_files(path: impl AsRef<Path>) -> HashMap<String, Vec<Node>> {
    let Ok(repo) = Repository::open(&path) else { return HashMap::new() };
    let mut map = HashMap::new();
    let Ok(submodules) = repo.submodules() else { return HashMap::new() };

    for module in submodules.iter() {
        let module_name = module.name().unwrap_or("unknown").to_string();
        map.insert(module_name, get_cwl_files(path.as_ref().join(module.path())));
    }

    map
}

#[cfg(test)]
mod tests {
    pub use super::*;

    #[test]
    pub fn test_get_cwl_files() {
        let base = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = format!("{}/../../testdata/hello_world", base);
        let files = get_cwl_files(path);
        assert_eq!(files.len(), 3);
    }
}
