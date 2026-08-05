use super::{BAD_WORDS, staging};
use crate::{authoring::AuthoringResult, paths::TrustedPathExt};
use commonwl::{
    IntegerOrExpression,
    documents::CommandLineTool,
    files::{Directory, File, FileOrDirectory},
    inputs::{CommandInputParameter, CommandLineBinding, DefaultValue},
    types::CWLType,
};
use rand::{RngExt, distr::Alphanumeric};
use serde_json::Value;
use slugify::slugify;
use std::path::Path;

pub(crate) fn build_inputs(args: &[&str], base: &Path) -> AuthoringResult<Vec<CommandInputParameter>> {
    let mut inputs = vec![];
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        let input: CommandInputParameter = if is_flag_like(arg) {
            if i + 1 < args.len() && !is_flag_like(args[i + 1]) {
                //is not a flag, as next one is a value
                let option = build_option(arg, args[i + 1], base)?;
                i += 1;
                option
            } else {
                build_flag(arg)
            }
        } else {
            build_positional(arg, i.try_into().unwrap(), base)?
        };
        inputs.push(input);
        i += 1;
    }
    Ok(inputs)
}

/// A leading `-` alone doesn't make something a flag — negative numbers
/// (`-5`, `-3.2`) also start with `-` but are values, not option names.
fn is_flag_like(s: &str) -> bool {
    s.starts_with('-') && s.parse::<f64>().is_err()
}

fn build_positional(current: &str, index: isize, base: &Path) -> AuthoringResult<CommandInputParameter> {
    let (current, cwl_type) = parse_input(current, base)?;

    // detected before building id/default -- a secret must never reach the default
    // value either, only its presence (behind a random id) may be recorded
    let is_secret = BAD_WORDS
        .iter()
        .any(|&word| current.to_lowercase().contains(word));

    let id = if is_secret {
        let rnd: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(2)
            .map(char::from)
            .collect();
        format!("secret_{rnd}")
    } else {
        slugify!(&current, separator = "_")
    };

    // secrets are left without a default
    let default_value = (!is_secret).then(|| parse_default_value(current, &cwl_type));

    Ok(CommandInputParameter::builder()
        .id(&id)
        .r#type(cwl_type)
        .maybe_default(default_value)
        .input_binding(
            CommandLineBinding::builder()
                .position(IntegerOrExpression::Int(index as i32))
                .build(),
        )
        .build())
}

fn build_flag(current: &str) -> CommandInputParameter {
    // slugify already strips leading `-`/`--`
    CommandInputParameter::builder()
        .input_binding(CommandLineBinding::builder().prefix(current).build())
        .id(slugify!(current, separator = "_").as_str())
        .r#type(CWLType::Boolean)
        .default(DefaultValue::Any(Value::Bool(true)))
        .build()
}

fn build_option(current: &str, next: &str, base: &Path) -> AuthoringResult<CommandInputParameter> {
    let (next, cwl_type) = parse_input(next, base)?;
    let default_value = parse_default_value(next, &cwl_type);

    Ok(CommandInputParameter::builder()
        .input_binding(CommandLineBinding::builder().prefix(current).build())
        .id(slugify!(current, separator = "_").as_str())
        .r#type(cwl_type)
        .default(default_value)
        .build())
}

fn parse_default_value(value: &str, cwl_type: &CWLType) -> DefaultValue {
    match cwl_type {
        CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(
            File::builder().location(value).build(),
        )),
        CWLType::Directory => DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
            Directory::builder().location(value).build(),
        )),
        CWLType::String => DefaultValue::Any(Value::String(value.to_string())),
        // type hint (e.g. `i:`) promised a scalar, but the value didn't parse as one
        // (bad user input) — fall back to a plain string instead of panicking.
        _ => DefaultValue::Any(
            serde_saphyr::from_str(value).unwrap_or_else(|_| Value::String(value.to_string())),
        ),
    }
}

fn parse_input<'a>(input: &'a str, base: &Path) -> AuthoringResult<(&'a str, CWLType)> {
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
            Ok((name, type_))
        } else {
            Ok((input, guess_type(input, base)?))
        }
    } else {
        Ok((input, guess_type(input, base)?))
    }
}

pub(crate) fn add_fixed_inputs(
    tool: &mut CommandLineTool,
    inputs: &[&str],
    base: &Path,
) -> AuthoringResult<()> {
    for input in inputs {
        let (input, type_) = parse_input(input, base)?;

        //todo: add requiement for directory also or add new --mount param and remove block from here
        if matches!(type_, CWLType::File) {
            staging::stage_fixed_input(tool, input);
        }

        let default = parse_default_value(input, &type_);
        let id = slugify!(input, separator = "_");

        tool.inputs.push(
            CommandInputParameter::builder()
                .id(&id)
                .r#type(type_)
                .default(default)
                .build(),
        );
    }

    Ok(())
}

/// Tries to guess the CWLType of a given value.
///
/// `value` is checked relative to `base` (the project root)
pub fn guess_type(value: &str, base: &Path) -> AuthoringResult<CWLType> {
    let path = base.build_trusted_path(value)?;
    if path.exists() {
        if path.is_file() {
            return Ok(CWLType::File);
        }
        if path.is_dir() {
            return Ok(CWLType::Directory);
        }
    }

    if value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("ftp://")
        || value.starts_with("s3://")
        || value.starts_with("file://")
    {
        return Ok(CWLType::File); //urls are files!
    }

    //we do not have to check for files that do not exist yet, as CWLTool would run into a failure
    let Ok(yaml_value) = serde_saphyr::from_str::<Value>(value) else {
        return Ok(CWLType::String); // not valid YAML scalar syntax -> treat as opaque string
    };
    Ok(match yaml_value {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::requirements::{InitialWorkDirRequirement, ToolRequirements, WorkDirItems};
    use serde_json::Number;

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
                .default(DefaultValue::Any(serde_saphyr::from_str("1").unwrap()))
                .build(),
        ];

        let inputs_vec = shlex::split(inputs).unwrap();
        let inputs_slice: Vec<&str> = inputs_vec.iter().map(AsRef::as_ref).collect();

        let result = build_inputs(&inputs_slice, Path::new(".")).unwrap();

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
        let result = build_inputs(
            &args.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
            Path::new("."),
        ).unwrap();

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
        let result = build_inputs(&[arg], Path::new(".")).unwrap();
        assert_eq!(result[0], expected);
    }

    #[test]
    pub fn test_get_inputs_negative_number_value() {
        // "-5" must be read as the value of --threshold, not misparsed as its own flag
        let inputs = "--threshold -5";
        let args = shlex::split(inputs).unwrap();
        let result = build_inputs(
            &args.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
            Path::new("."),
        ).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id.as_deref(), Some("threshold"));
        assert_eq!(result[0].r#type, CWLType::Int.into());
        assert_eq!(
            result[0].default,
            Some(DefaultValue::Any(Value::Number(Number::from(-5))))
        );
    }

    #[test]
    pub fn test_get_inputs_standalone_negative_number() {
        let result = build_inputs(&["-5"], Path::new(".")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].r#type, CWLType::Int.into());
    }

    #[test]
    pub fn test_parse_default_value_malformed_falls_back_to_string() {
        // an unparsable value under a non-String type hint must degrade to a
        // plain string instead of panicking (was `.unwrap()` before)
        let value = parse_default_value("{unclosed", &CWLType::Int);
        assert_eq!(
            value,
            DefaultValue::Any(Value::String("{unclosed".to_string()))
        );
    }

    #[test]
    pub fn test_guess_type_malformed_yaml_does_not_panic() {
        assert_eq!(
            guess_type("{unclosed", Path::new(".")).unwrap(),
            CWLType::String
        );
    }

    #[test]
    pub fn test_guess_type() {
        // guess_type checks the filesystem relative to a passed-in base, so directory
        // existence is tested against a real tempdir rather than an incidental path relative
        // to the crate.
        let base = &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let dir = tempfile::tempdir().unwrap();
        let dir_path = format!("{}/", dir.path().to_string_lossy());

        let inputs = &[
            ("README.md", CWLType::File),
            ("/some/path/that/does/not/exist.txt", CWLType::String),
            (dir_path.as_str(), CWLType::Directory),
            ("--option", CWLType::String),
            ("2", CWLType::Int),
            ("1.5", CWLType::Float),
            ("https://some_url", CWLType::File), //urls are files!
        ];

        for input in inputs {
            let t = guess_type(input.0, base).unwrap();
            assert_eq!(t, input.1);
        }
    }

    #[test]
    pub fn test_get_flag_multiword_id_keeps_word_boundary() {
        // regression: get_flag/get_option used to `current.replace('-', "")` before
        // slugify, which for a multi-word flag stripped the internal dash *before*
        // slugify could turn it into the separator -- "--dry-run" became id "dryrun"
        // instead of "dry_run". slugify already handles leading/internal dashes itself.
        let result = build_inputs(&["--dry-run"], Path::new(".")).unwrap();
        assert_eq!(result[0].id.as_deref(), Some("dry_run"));
    }

    #[test]
    pub fn test_get_option_multiword_id_keeps_word_boundary() {
        let result = build_inputs(&["--max-retries", "3"], Path::new(".")).unwrap();
        assert_eq!(result[0].id.as_deref(), Some("max_retries"));
    }

    #[test]
    pub fn test_add_fixed_inputs_creates_requirement_when_none_exist() {
        // regression: the File-staging block in add_fixed_inputs used to only mutate an
        // existing InitialWorkDirRequirement and silently do nothing when tool.requirements
        // was None (e.g. base command isn't a script file that pre-created one)
        let mut tool = CommandLineTool::builder().build();
        assert!(tool.requirements.is_none());

        add_fixed_inputs(&mut tool, &["f:foo.txt"], Path::new(".")).unwrap();

        let req = tool
            .get_requirement::<InitialWorkDirRequirement>()
            .expect("InitialWorkDirRequirement must be created");
        match &req.listing {
            WorkDirItems::ListingItems(items) => assert_eq!(items.len(), 1),
            WorkDirItems::Expression(_) => panic!("expected ListingItems"),
        }
    }

    #[test]
    pub fn test_add_fixed_inputs_merges_into_existing_requirement() {
        let mut tool = CommandLineTool::builder()
            .requirements(vec![ToolRequirements::InitialWorkDirRequirement(
                InitialWorkDirRequirement {
                    listing: WorkDirItems::ListingItems(vec![]),
                },
            )])
            .build();

        add_fixed_inputs(&mut tool, &["f:foo.txt", "f:bar.txt"], Path::new(".")).unwrap();

        assert_eq!(tool.requirements.as_ref().unwrap().len(), 1);
        let req = tool.get_requirement::<InitialWorkDirRequirement>().unwrap();
        match &req.listing {
            WorkDirItems::ListingItems(items) => assert_eq!(items.len(), 2),
            WorkDirItems::Expression(_) => panic!("expected ListingItems"),
        }
    }
}
