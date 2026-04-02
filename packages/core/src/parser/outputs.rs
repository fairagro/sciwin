use crate::io::get_filename_without_extension;
use commonwl::{
    OneOrMany,
    outputs::{CommandOutputBinding, CommandOutputParameter},
};
use std::path::Path;

pub(crate) fn get_outputs(files: &[String]) -> Vec<CommandOutputParameter> {
    files
        .iter()
        .map(|f| {
            let filename = get_filename_without_extension(f);
            let output_type = if Path::new(f).extension().is_some() {
                CWLType::File
            } else {
                CWLType::Directory
            };
            CommandOutputParameter::builder()
                .r#type(output_type)
                .id(&filename)
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
                .id("my-file")
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One("my-file.txt".to_string())),
                    ..Default::default()
                })
                .build(),
            CommandOutputParameter::builder()
                .r#type(CWLType::File)
                .id("archive")
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One("archive.tar.gz".to_string())),
                    ..Default::default()
                })
                .build(),
        ];

        let outputs = get_outputs(&files);
        assert_eq!(outputs, expected);
    }
}
