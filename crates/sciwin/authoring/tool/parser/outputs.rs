use commonwl::{
    OneOrMany,
    outputs::{CommandOutputBinding, CommandOutputParameter},
    types::CWLType,
};
use std::path::Path;

use crate::authoring::tool::parser::sanitize_id;

pub(crate) fn get_outputs(files: &[String]) -> Vec<CommandOutputParameter> {
    files
        .iter()
        .map(|f| {
            let file_id = sanitize_id(f);
            let output_type = if Path::new(f).extension().is_some() {
                CWLType::File
            } else {
                CWLType::Directory
            };
            CommandOutputParameter::builder()
                .r#type(output_type)
                .id(&file_id)
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One(f.to_string())),
                    ..Default::default()
                })
                .build()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_get_outputs() {
        let files = vec!["my-file.txt".to_string(), "archive.tar.gz".to_string()];
        let expected = vec![
            CommandOutputParameter::builder()
                .r#type(CWLType::File)
                .id("my-file_txt")
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One("my-file.txt".to_string())),
                    ..Default::default()
                })
                .build(),
            CommandOutputParameter::builder()
                .r#type(CWLType::File)
                .id("archive_tar_gz")
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One("archive.tar.gz".to_string())),
                    ..Default::default()
                })
                .build(),
        ];

        let outputs = get_outputs(&files);
        assert_eq!(outputs, expected);
    }

    #[test]
    pub fn test_get_outputs_numeric_only_name() {
        // an all-numeric filename must not produce an empty CWL id
        let outputs = get_outputs(&["123".to_string()]);
        assert_eq!(outputs[0].id.as_deref(), Some("123"));
    }
}
