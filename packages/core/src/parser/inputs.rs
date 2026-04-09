use std::path::Path;

use super::BAD_WORDS;
use commonwl::{
    IntegerOrExpression, OneOrMany,
    documents::CommandLineTool,
    files::{Directory, Dirent, File, FileOrDirectory},
    inputs::{CommandInputParameter, CommandLineBinding, DefaultValue},
    requirements::{Include, ListingItems, StringOrInclude, ToolRequirements, WorkDirItems},
    types::CWLType,
};
use rand::{Rng, distr::Alphanumeric};
use serde_yaml::Value;
use slugify::slugify;

pub(crate) fn get_inputs(args: &[&str]) -> Vec<CommandInputParameter> {
    let mut inputs = vec![];
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        let input: CommandInputParameter;
        if arg.starts_with('-') {
            if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                //is not a flag, as next one is a value
                input = get_option(arg, args[i + 1]);
                i += 1;
            } else {
                input = get_flag(arg);
            }
        } else {
            input = get_positional(arg, i.try_into().unwrap());
        }
        inputs.push(input);
        i += 1;
    }
    inputs
}

fn get_positional(current: &str, index: isize) -> CommandInputParameter {
    let (current, cwl_type) = parse_input(current);
    let default_value = parse_default_value(current, &cwl_type);

    //check id for bad words
    let mut id = slugify!(&current, separator = "_");
    if BAD_WORDS.iter().any(|&word| current.to_lowercase().contains(word)) {
        let rnd: String = rand::rng().sample_iter(&Alphanumeric).take(2).map(char::from).collect();
        id = format!("secret_{rnd}");
    }

    CommandInputParameter::builder()
        .id(&id)
        .r#type(cwl_type)
        .default(default_value)
        .input_binding(CommandLineBinding::builder().position(IntegerOrExpression::Long(index as i64)).build())
        .build()
}

fn get_flag(current: &str) -> CommandInputParameter {
    let id = current.replace('-', "");
    CommandInputParameter::builder()
        .input_binding(CommandLineBinding::builder().prefix(current).build())
        .id(slugify!(&id, separator = "_").as_str())
        .r#type(CWLType::Boolean)
        .default(DefaultValue::Any(Value::Bool(true)))
        .build()
}

fn get_option(current: &str, next: &str) -> CommandInputParameter {
    let id = current.replace('-', "");

    let (next, cwl_type) = parse_input(next);
    let default_value = parse_default_value(next, &cwl_type);

    CommandInputParameter::builder()
        .input_binding(CommandLineBinding::builder().prefix(current).build())
        .id(slugify!(&id, separator = "_").as_str())
        .r#type(cwl_type)
        .default(default_value)
        .build()
}

fn parse_default_value(value: &str, cwl_type: &CWLType) -> DefaultValue {
    match cwl_type {
        CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(File::builder().location(value).build())),
        CWLType::Directory => DefaultValue::FileOrDirectory(FileOrDirectory::Directory(Directory::builder().location(value).build())),
        CWLType::String => DefaultValue::Any(Value::String(value.to_string())),
        _ => DefaultValue::Any(serde_yaml::from_str(value).unwrap()),
    }
}

fn parse_input(input: &str) -> (&str, CWLType) {
    if let Some((hint, name)) = input.split_once(':') {
        if hint.len() == 1 {
            let type_ = match hint {
                "f" => CWLType::File,
                "d" => CWLType::Directory,
                "s" => CWLType::String,
                "r" => CWLType::Float,
                "i" => CWLType::Int,
                "l" => CWLType::Long,
                "b" => CWLType::Boolean,
                _ => CWLType::Any, //whatever
            };
            (name, type_)
        } else {
            (input, guess_type(input))
        }
    } else {
        (input, guess_type(input))
    }
}

pub(crate) fn add_fixed_inputs(tool: &mut CommandLineTool, inputs: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    for input in inputs {
        let (input, type_) = parse_input(input);

        //todo: add requiement for directory also or add new --mount param and remove block from here
        if matches!(type_, CWLType::File)
            && let Some(requirements) = &mut tool.requirements
        {
            for item in requirements {
                if let ToolRequirements::InitialWorkDirRequirement(req) = item {
                    let dirent = Dirent::builder()
                        .entry(StringOrInclude::Include(Include {
                            include: get_entry_name(input),
                        }))
                        .entryname(input)
                        .build();
                    match &mut req.listing {
                        WorkDirItems::Expression(expr) => {
                            req.listing = WorkDirItems::ListingItems(Box::new(OneOrMany::Many(vec![
                                ListingItems::Dirent(dirent),
                                ListingItems::Expression(expr.to_string()),
                            ])));
                        }
                        WorkDirItems::ListingItems(items) => match &mut **items {
                            OneOrMany::One(item) => **items = OneOrMany::Many(vec![item.clone(), ListingItems::Dirent(dirent)]),
                            OneOrMany::Many(items) => items.push(ListingItems::Dirent(dirent)),
                        },
                    }
                    break;
                }
            }
        }

        let default = match type_ {
            CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(File::builder().location(input).build())),
            CWLType::Directory => DefaultValue::FileOrDirectory(FileOrDirectory::Directory(Directory::builder().location(input).build())),
            _ => DefaultValue::Any(serde_yaml::from_str(input)?),
        };
        let id = slugify!(input, separator = "_");

        tool.inputs
            .push(CommandInputParameter::builder().id(&id).r#type(type_).default(default).build());
    }

    Ok(())
}

fn get_entry_name(input: &str) -> String {
    let i = input
        .trim_start_matches(|c: char| !c.is_alphabetic())
        .to_string()
        .replace(['.', '/'], "_");
    format!("$(inputs.{})", i.to_lowercase()).to_string()
}

/// Tries to guess the CWLType of a given value
pub fn guess_type(value: &str) -> CWLType {
    let path = Path::new(value);
    if path.exists() {
        if path.is_file() {
            return CWLType::File;
        }
        if path.is_dir() {
            return CWLType::Directory;
        }
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return CWLType::File;
    }

    //we do not have to check for files that do not exist yet, as CWLTool would run into a failure
    let yaml_value: Value = serde_yaml::from_str(value).unwrap();
    match yaml_value {
        Value::Null => CWLType::Null,
        Value::Bool(_) => CWLType::Boolean,
        Value::Number(number) => {
            if number.is_f64() {
                CWLType::Float
            } else {
                CWLType::Int
            }
        }
        Value::String(_) => CWLType::String,
        _ => CWLType::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Number;

    #[test]
    pub fn test_get_inputs() {
        let inputs = "--argument1 value1 --flag -a value2 positional1 -v 1";
        let expected = vec![
            CommandInputParameter::builder()
                .id("argument1")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().prefix("--argument1").build())
                .default(DefaultValue::Any(Value::String("value1".to_string())))
                .build(),
            CommandInputParameter::builder()
                .id("flag")
                .r#type(CWLType::Boolean)
                .input_binding(CommandLineBinding::builder().prefix("--flag").build())
                .default(DefaultValue::Any(Value::Bool(true)))
                .build(),
            CommandInputParameter::builder()
                .id("a")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().prefix("-a").build())
                .default(DefaultValue::Any(Value::String("value2".to_string())))
                .build(),
            CommandInputParameter::builder()
                .id("positional1")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().position(5).build())
                .default(DefaultValue::Any(Value::String("positional1".to_string())))
                .build(),
            CommandInputParameter::builder()
                .id("v")
                .r#type(CWLType::Int)
                .input_binding(CommandLineBinding::builder().prefix("-v").build())
                .default(DefaultValue::Any(serde_yaml::from_str("1").unwrap()))
                .build(),
        ];

        let inputs_vec = shlex::split(inputs).unwrap();
        let inputs_slice: Vec<&str> = inputs_vec.iter().map(AsRef::as_ref).collect();

        let result = get_inputs(&inputs_slice);

        assert_eq!(result, expected);
    }

    #[test]
    pub fn test_get_default_value_number() {
        let commandline_args = "-v 42";
        let expected = CommandInputParameter::builder()
            .id("v")
            .r#type(CWLType::Int)
            .input_binding(CommandLineBinding::builder().prefix("-v").build())
            .default(DefaultValue::Any(Value::Number(Number::from(42))))
            .build();

        let args = shlex::split(commandline_args).unwrap();
        let result = get_inputs(&args.iter().map(AsRef::as_ref).collect::<Vec<&str>>());

        assert_eq!(result[0], expected);
    }

    #[test]
    pub fn test_get_default_value_json_str() {
        let arg = "{\"message\": \"Hello World\"}";
        let expected = CommandInputParameter::builder()
            .id("message_hello_world")
            .r#type(CWLType::String)
            .input_binding(CommandLineBinding::builder().position(0).build())
            .default(DefaultValue::Any(Value::String(arg.to_string())))
            .build();
        let result = get_inputs(&[arg]);
        assert_eq!(result[0], expected);
    }

    #[test]
    pub fn test_guess_type() {
        let inputs = &[
            ("../../README.md", CWLType::File),
            ("/some/path/that/does/not/exist.txt", CWLType::String),
            ("src/", CWLType::Directory),
            ("--option", CWLType::String),
            ("2", CWLType::Int),
            ("1.5", CWLType::Float),
            ("https://some_url", CWLType::File), //urls are files!
        ];

        for input in inputs {
            let t = guess_type(input.0);
            assert_eq!(t, input.1);
        }
    }
}
