use super::BAD_WORDS;
use commonwl::{
    guess_type,
    inputs::{CommandInputParameter, CommandLineBinding},
    requirements::Requirement,
    CWLType, CommandLineTool, DefaultValue, Directory, File,
};
use rand::{distr::Alphanumeric, Rng};
use serde_yaml::Value;
use slugify::slugify;
use crate::parser::find_edam_format; 

pub(crate) fn get_inputs(args: &[&str]) -> Vec<CommandInputParameter> {
    let mut inputs = vec![];
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        let mut input: CommandInputParameter;
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
        if matches!(input.type_, CWLType::File) && let Some(loc) = get_location(&input) {
            let edam_format = find_edam_format(&loc);
            input = input.with_format(&edam_format);
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

    CommandInputParameter::default()
        .with_id(&id)
        .with_type(cwl_type)
        .with_default_value(default_value)
        .with_binding(CommandLineBinding::default().with_position(index))
}

fn get_flag(current: &str) -> CommandInputParameter {
    let id = current.replace('-', "");
    CommandInputParameter::default()
        .with_binding(CommandLineBinding::default().with_prefix(current))
        .with_id(slugify!(&id, separator = "_").as_str())
        .with_type(CWLType::Boolean)
        .with_default_value(DefaultValue::Any(Value::Bool(true)))
}

fn get_option(current: &str, next: &str) -> CommandInputParameter {
    let id = current.replace('-', "");

    let (next, cwl_type) = parse_input(next);
    let default_value = parse_default_value(next, &cwl_type);

    CommandInputParameter::default()
        .with_binding(CommandLineBinding::default().with_prefix(current))
        .with_id(slugify!(&id, separator = "_").as_str())
        .with_type(cwl_type)
        .with_default_value(default_value)
}

fn parse_default_value(value: &str, cwl_type: &CWLType) -> DefaultValue {
    match cwl_type {
        CWLType::File => DefaultValue::File(File::from_location(value)),
        CWLType::Directory => DefaultValue::Directory(Directory::from_location(value)),
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
            CWLType::File => DefaultValue::File(File::from_location(input)),
            CWLType::Directory => DefaultValue::Directory(Directory::from_location(input)),
            _ => DefaultValue::Any(serde_yaml::from_str(input)?),
        };
        let id = slugify!(input, separator = "_");
        let mut param = CommandInputParameter::default().with_id(&id).with_type(type_.clone()).with_default_value(default);
        if matches!(type_, CWLType::File) {
            let edam_format = find_edam_format(input);
            param = param.with_format(&edam_format);
        }
        tool.inputs.push(param);
    }

    Ok(())
}

fn get_location(param: &CommandInputParameter) -> Option<String> {
    match &param.default {
        Some(DefaultValue::File(file)) => file.location.clone(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_yaml::Number;

    use super::*;

    #[test]
    pub fn test_get_inputs() {
        let inputs = "--argument1 value1 --flag -a value2 positional1 -v 1";
        let expected = vec![
            CommandInputParameter::default()
                .with_id("argument1")
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_prefix("--argument1"))
                .with_default_value(DefaultValue::Any(Value::String("value1".to_string()))),
            CommandInputParameter::default()
                .with_id("flag")
                .with_type(CWLType::Boolean)
                .with_binding(CommandLineBinding::default().with_prefix("--flag"))
                .with_default_value(DefaultValue::Any(Value::Bool(true))),
            CommandInputParameter::default()
                .with_id("a")
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_prefix("-a"))
                .with_default_value(DefaultValue::Any(Value::String("value2".to_string()))),
            CommandInputParameter::default()
                .with_id("positional1")
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_position(5))
                .with_default_value(DefaultValue::Any(Value::String("positional1".to_string()))),
            CommandInputParameter::default()
                .with_id("v")
                .with_type(CWLType::Int)
                .with_binding(CommandLineBinding::default().with_prefix("-v"))
                .with_default_value(DefaultValue::Any(serde_yaml::from_str("1").unwrap())),
        ];

        let inputs_vec = shlex::split(inputs).unwrap();
        let inputs_slice: Vec<&str> = inputs_vec.iter().map(AsRef::as_ref).collect();

        let result = get_inputs(&inputs_slice);

        assert_eq!(result, expected);
    }

    #[test]
    pub fn test_get_default_value_number() {
        let commandline_args = "-v 42";
        let expected = CommandInputParameter::default()
            .with_id("v")
            .with_type(CWLType::Int)
            .with_binding(CommandLineBinding::default().with_prefix("-v"))
            .with_default_value(DefaultValue::Any(Value::Number(Number::from(42))));

        let args = shlex::split(commandline_args).unwrap();
        let result = get_inputs(&args.iter().map(AsRef::as_ref).collect::<Vec<&str>>());

        assert_eq!(result[0], expected);
    }

    #[test]
    pub fn test_get_default_value_json_str() {
        let arg = "{\"message\": \"Hello World\"}";
        let expected = CommandInputParameter::default()
            .with_id("message_hello_world")
            .with_type(CWLType::String)
            .with_binding(CommandLineBinding::default().with_position(0))
            .with_default_value(DefaultValue::Any(Value::String(arg.to_string())));
        let result = get_inputs(&[arg]);
        assert_eq!(result[0], expected);
    }
}
