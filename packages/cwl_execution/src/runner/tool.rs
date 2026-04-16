use crate::{
    environment::{collect_environment, RuntimeEnvironment},
    error::ExecutionError,
    expression::{eval, eval_tool, load_lib, parse_expressions, prepare_expression_engine, process_expressions, reset_expression_engine},
    outputs::{evaluate_command_outputs, evaluate_expression_outputs},
    runner::command::run_command,
    staging::stage_required_files,
    validate::set_placeholder_values,
    InputObject,
};
use cwl_core::{prelude::*, requirements::StringOrInclude};
use log::info;
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Instant,
};
use tempfile::tempdir;

pub fn run_tool(
    tool: &mut CWLDocument,
    input_values: &InputObject,
    cwl_path: &PathBuf,
    out_dir: Option<String>,
) -> Result<HashMap<String, DefaultValue>, ExecutionError> {
    //measure performance
    let clock = Instant::now();
    //create staging directory
    let dir = tempdir()?;
    info!("📁 Created staging directory: {:?}", dir.path());
    fn resolve_image_path(image: &str, _cwl_path: &Path) -> String {
        let current_dir = env::current_dir().unwrap();

        if Path::new(image).is_absolute() {
            return image.to_string();
        }

        let joined = current_dir.join(image);

        match std::fs::canonicalize(&joined) {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(e) => {
                log::warn!("⚠️ Could not canonicalize {} ({})", joined.display(), e);
                image.to_string()
            }
        }
    }
    if let CWLDocument::CommandLineTool(clt) = tool {
        for req in &mut clt.requirements {
            if let Requirement::DockerRequirement(docker_req) = req
                && let Some(pull_val) = docker_req.docker_pull.clone()
            {
                let is_dockerfile = pull_val.contains("Dockerfile");

                if Path::new(&pull_val).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("sif")) {
                    let abs = resolve_image_path(&pull_val, cwl_path);
                    docker_req.docker_pull = Some(abs);
                } else if !is_dockerfile {
                    let maybe_sif = format!("{pull_val}.sif");
                    let abs = if Path::new(&maybe_sif).exists() {
                        resolve_image_path(&maybe_sif, cwl_path)
                    } else {
                        pull_val
                    };
                    docker_req.docker_pull = Some(abs);
                }
            }
        }
    }

    //save reference to current working directory
    let current = env::current_dir()?;
    let output_directory = if let Some(out) = out_dir { &PathBuf::from(out) } else { &current };

    //set tool path. all paths are given relative to the tool
    let tool_path = cwl_path.parent().unwrap_or(Path::new("."));

    //create runtime tmpdir
    let tmp_dir = tempdir()?;

    let mut input_values = input_values.handle_requirements(&tool.requirements, &tool.hints);
    input_values.lock();

    //build runtime object
    let mut runtime = RuntimeEnvironment::initialize(tool, &input_values, dir.path(), tool_path, tmp_dir.path())?;

    //replace inputs and runtime placeholders in tool with the actual values
    set_placeholder_values(tool, &runtime, &mut input_values);
    runtime.environment = collect_environment(&input_values);

    // run expression engine
    prepare_expression_engine(&runtime)?;
    if let Some(ijr) = input_values.get_requirement::<InlineJavascriptRequirement>() {
        if let Some(expression_lib) = &ijr.expression_lib {
            for lib in expression_lib {
                if let StringOrInclude::Include(lib_include) = lib {
                    load_lib(tool_path.join(&lib_include.include))?;
                } else if let StringOrInclude::String(lib_string) = lib {
                    eval(lib_string)?;
                }
            }
        }
        process_expressions(tool, &mut input_values)?;
    }
    //stage files listed in input default values, input values or initial work dir requirements
    stage_required_files(tool, &input_values, &mut runtime, tool_path, dir.path(), output_directory)?;

    //change working directory to tmp folder, we will execute tool from root here
    env::set_current_dir(dir.path())?;

    //run the tool
    let mut result_value: Option<serde_yaml::Value> = None;
    if let CWLDocument::CommandLineTool(clt) = tool {
        run_command(clt, &mut runtime)?;
    } else if let CWLDocument::ExpressionTool(et) = tool {
        prepare_expression_engine(&runtime)?;
        let expressions = parse_expressions(&et.expression);
        result_value = Some(eval_tool::<serde_yaml::Value>(&expressions[0].expression())?);
        reset_expression_engine()?;
    }

    //evaluate output files
    prepare_expression_engine(&runtime)?;
    let outputs = if let CWLDocument::CommandLineTool(clt) = &tool {
        evaluate_command_outputs(clt, output_directory)?
    } else if let CWLDocument::ExpressionTool(et) = &tool {
        if let Some(value) = result_value {
            evaluate_expression_outputs(et, &value)?
        } else {
            HashMap::new()
        }
    } else {
        unreachable!()
    };
    reset_expression_engine()?;

    //come back to original directory
    env::set_current_dir(current)?;

    info!("✔️  Tool {:?} executed successfully in {:.0?}!", &cwl_path, clock.elapsed());
    Ok(outputs)
}
