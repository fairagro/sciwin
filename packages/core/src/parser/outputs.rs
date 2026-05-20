use crate::io::get_filename_without_extension;
use commonwl::{
    CWLType, SingularPlural,
    outputs::{CommandOutputBinding, CommandOutputParameter},
};
use std::path::Path;
use crate::parser::find_edam_format;

pub(crate) fn get_outputs(files: &[String]) -> Vec<CommandOutputParameter> {
    files
        .iter()
        .map(|f| {
            let is_file = Path::new(f).extension().is_some();
            let output_type = if is_file {
                CWLType::File
            } else {
                CWLType::Directory
            };
            let mut out = CommandOutputParameter::default()
                .with_id(&get_filename_without_extension(f))
                .with_type(output_type)
                .with_binding(CommandOutputBinding {
                    glob: Some(SingularPlural::Singular(f.clone())),
                    ..Default::default()
                });
            if is_file {
                let edam_format = find_edam_format(f);
                out = out.with_format(&edam_format);
            }
            out
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
            CommandOutputParameter::default()
                .with_type(CWLType::File)
                .with_id("my-file")
                .with_format("http://edamontology.org/format_2330")
                .with_binding(CommandOutputBinding {
                    glob: Some(commonwl::SingularPlural::Singular("my-file.txt".to_string())),
                    ..Default::default()
                }),
            CommandOutputParameter::default()
                .with_type(CWLType::File)
                .with_id("archive")
                .with_format("http://edamontology.org/format_3989")
                .with_binding(CommandOutputBinding {
                    glob: Some(commonwl::SingularPlural::Singular("archive.tar.gz".to_string())),
                    ..Default::default()
                }),
        ];

        let outputs = get_outputs(&files);
        assert_eq!(outputs, expected);
    }
}
