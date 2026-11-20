//! Job file templates: a value for every input of a CWL document, to be filled in by hand.

use crate::execution::RunnerResult;
use commonwl::{
    OneOrMany,
    documents::CWLDocument,
    files::{Directory, File, FileOrDirectory},
    inputs::{DefaultValue, InputSchema, InputType},
    types::CWLType,
};
use serde_json::{Number, Value};
use std::{collections::HashMap, fs, path::Path};

/// Job inputs for the CWL document at `path`: each input's default, else a placeholder of
/// its type.
pub fn input_template(path: impl AsRef<Path>) -> RunnerResult<HashMap<String, DefaultValue>> {
    let contents = fs::read_to_string(path)?;
    let cwl: CWLDocument = serde_saphyr::from_str(&contents).map_err(anyhow::Error::from)?;

    Ok(cwl
        .get_inputs() //we assume there is no stdin
        .into_iter()
        .filter_map(|i| {
            let value = i.default.unwrap_or_else(|| get_default(&i.r#type));
            Some((i.id?, value))
        })
        .collect())
}

fn get_default(r#type: &OneOrMany<InputType>) -> DefaultValue {
    let input_type = match r#type {
        OneOrMany::One(t) => t,
        OneOrMany::Many(ts) => ts.first().unwrap_or(&InputType::CWLType(CWLType::Null)),
    };

    match input_type {
        InputType::CWLType(cwltype) => fs_defaults(cwltype),
        InputType::InputSchema(schema) => match schema.as_ref() {
            InputSchema::Record(_) => DefaultValue::Any(Value::Object(Default::default())),
            InputSchema::Enum(e) => DefaultValue::Any(Value::String(
                e.symbols.first().cloned().unwrap_or_default(),
            )),
            InputSchema::Array(a) => DefaultValue::Any(Value::Array(vec![
                serde_json::to_value(get_default(&a.items)).unwrap_or(Value::Null),
                serde_json::to_value(get_default(&a.items)).unwrap_or(Value::Null),
            ])),
        },
        InputType::String(_) => DefaultValue::Any(Value::Null),
    }
}

fn fs_defaults(cwltype: &CWLType) -> DefaultValue {
    match cwltype {
        CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(
            File::builder().location("./path/to/file.txt").build(),
        )),
        CWLType::Directory => DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
            Directory::builder().location("./path/to/dir").build(),
        )),
        _ => DefaultValue::Any(cwltype_defaults(cwltype)),
    }
}

fn cwltype_defaults(cwltype: &CWLType) -> Value {
    match cwltype {
        CWLType::Boolean => Value::Bool(true),
        CWLType::Int | CWLType::Long => Value::Number(Number::from(42)),
        CWLType::Float | CWLType::Double => Value::Number(Number::from_f64(69.42).unwrap()),
        CWLType::String => Value::String("Hello World".into()),
        CWLType::Any => Value::String("Any Value".into()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_type() {
        assert_eq!(
            cwltype_defaults(&CWLType::Int),
            Value::Number(Number::from(42))
        );
        assert_eq!(cwltype_defaults(&CWLType::Boolean), Value::Bool(true));
        assert_eq!(
            cwltype_defaults(&CWLType::Long),
            Value::Number(Number::from(42))
        );
        assert_eq!(
            cwltype_defaults(&CWLType::Float),
            Value::Number(Number::from_f64(69.42).unwrap())
        );
        assert_eq!(
            cwltype_defaults(&CWLType::String),
            Value::String("Hello World".into())
        );
        assert_eq!(
            cwltype_defaults(&CWLType::Any),
            Value::String("Any Value".into())
        );
    }

    #[test]
    fn test_default_values() {
        assert_eq!(
            fs_defaults(&CWLType::File),
            DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder().location("./path/to/file.txt").build()
            ))
        );
        assert_eq!(
            fs_defaults(&CWLType::Directory),
            DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                Directory::builder().location("./path/to/dir").build()
            ))
        );
        assert_eq!(
            fs_defaults(&CWLType::String),
            DefaultValue::Any(Value::String("Hello World".into()))
        );
    }

    #[test]
    fn test_input_template() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/hello_world/workflows/main/main.cwl");
        let template = input_template(&path).unwrap();
        let expected = HashMap::from([
            (
                "population".to_string(),
                DefaultValue::FileOrDirectory(FileOrDirectory::File(
                    File::builder()
                        .location("../../data/population.csv")
                        .build(),
                )),
            ),
            (
                "speakers".to_string(),
                DefaultValue::FileOrDirectory(FileOrDirectory::File(
                    File::builder()
                        .location("../../data/speakers_revised.csv")
                        .build(),
                )),
            ),
        ]);

        assert_eq!(template, expected);
    }
}
