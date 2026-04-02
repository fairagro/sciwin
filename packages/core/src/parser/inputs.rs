use super::BAD_WORDS;
use commonwl::{
    IntegerOrExpression,
    files::{Directory, File, FileOrDirectory},
    inputs::{CommandInputParameter, CommandLineBinding, DefaultValue},
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
        if matches!(type_, CWLType::File) {
            for item in &mut tool.requirements {
                if let Requirement::InitialWorkDirRequirement(req) = item {
                    req.add_files(inputs);
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

#[cfg(test)]
mod tests {
    use serde_yaml::Number;

    use super::*;

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
}
