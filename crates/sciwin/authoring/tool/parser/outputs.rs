use crate::authoring::{
    AuthoringResult,
    tool::parser::{edam, sanitize_id},
};
use commonwl::{
    OneOrMany,
    outputs::{CommandOutputBinding, CommandOutputParameter},
    types::CWLType,
};
use futures::future::try_join_all;
use std::path::Path;

pub(crate) async fn get_outputs(files: &[String]) -> AuthoringResult<Vec<CommandOutputParameter>> {
    try_join_all(files.iter().map(async move |f| {
        let file_id = sanitize_id(f);
        let output_type = if Path::new(f).extension().is_some() {
            CWLType::File
        } else {
            CWLType::Directory
        };
        let format = if output_type == CWLType::File {
            Some(edam::find_edam_format(f).await?)
        } else {
            None
        };
        Ok(CommandOutputParameter::builder()
            .r#type(output_type)
            .id(&file_id)
            .output_binding(CommandOutputBinding {
                glob: Some(OneOrMany::One(f.to_string())),
                ..Default::default()
            })
            .maybe_format(format)
            .build())
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    pub async fn test_get_outputs() {
        let files = vec!["my-file.txt".to_string(), "archive.tar.gz".to_string()];
        let expected = vec![
            CommandOutputParameter::builder()
                .r#type(CWLType::File)
                .id("my-file_txt")
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One("my-file.txt".to_string())),
                    ..Default::default()
                })
                .format(OneOrMany::One("http://edamontology.org/format_2330".to_string()))
                .build(),
            CommandOutputParameter::builder()
                .r#type(CWLType::File)
                .id("archive_tar_gz")
                .output_binding(CommandOutputBinding {
                    glob: Some(OneOrMany::One("archive.tar.gz".to_string())),
                    ..Default::default()
                })
                .format(OneOrMany::One("http://edamontology.org/format_3989".to_string()))
                .build(),
        ];

        let outputs = get_outputs(&files).await.unwrap();
        assert_eq!(outputs, expected);
    }

    #[tokio::test]
    pub async fn test_get_outputs_numeric_only_name() {
        // an all-numeric filename must not produce an empty CWL id
        let outputs = get_outputs(&["123".to_string()]).await.unwrap();
        assert_eq!(outputs[0].id.as_deref(), Some("123"));
    }
}
