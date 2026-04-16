use crate::{
    InputObject,
    error::ExecutionError,
    execute,
    expression::evaluate_condition,
    inputs::evaluate_input,
    io::{copy_dir, copy_file},
    outputs::{copy_output_dir, get_file_metadata},
    scatter::{self},
};
use cwl_core::{PathItem, ScatterMethod, SingularPlural, StringOrDocument, inputs::LinkMerge, prelude::*};
use log::info;
use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::{self},
    path::{Path, PathBuf},
    time::Instant,
};
use tempfile::tempdir;

pub fn run_workflow(
    workflow: &mut Workflow,
    input_values: &InputObject,
    cwl_path: &PathBuf,
    out_dir: Option<String>,
) -> Result<HashMap<String, DefaultValue>, ExecutionError> {
    let clock = Instant::now();

    let sorted_step_ids = workflow.sort_steps()?;

    let dir = tempdir()?;
    let tmp_path = dir.path().to_string_lossy().into_owned();
    let current = env::current_dir()?;
    let output_directory = if let Some(out) = out_dir {
        out
    } else {
        current.to_string_lossy().into_owned()
    };

    let workflow_folder = if cwl_path.is_file() {
        cwl_path.parent().unwrap_or(Path::new("."))
    } else {
        cwl_path
    };

    let input_values = input_values.handle_requirements(&workflow.requirements, &workflow.hints);

    let mut outputs: HashMap<String, DefaultValue> = HashMap::new();
    for step_id in sorted_step_ids {
        if let Some(step) = workflow.get_step(&step_id) {
            let path = if let StringOrDocument::String(run) = &step.run {
                Some(workflow_folder.join(run))
            } else {
                None
            };

            //map inputs to correct fields
            let mut step_inputs = HashMap::new();
            for parameter in &step.in_ {
                let source = parameter.source.as_deref().unwrap_or_default();
                let source_parts: Vec<&str> = source.split('/').collect();
                //try output
                if source_parts.len() == 2
                    && let Some(out_value) = outputs.get(source)
                {
                    step_inputs.insert(parameter.id.to_string(), out_value.to_default_value());
                    continue;
                }

                //try default
                if let Some(default) = &parameter.default {
                    step_inputs.entry(parameter.id.to_string()).or_insert(default.to_owned());
                }

                //try input
                if let Some(input) = workflow.inputs.iter().find(|i| i.id == *source) {
                    let value = evaluate_input(input, &input_values.inputs)?;
                    match value {
                        DefaultValue::Any(val) if val.is_null() => continue,
                        _ => {
                            step_inputs.insert(parameter.id.to_string(), value.clone());
                        }
                    }
                }
                if source.starts_with("[") {
                    //source can be array of input IDs if this requirement is set!
                    let array: Vec<String> = serde_yaml::from_str(source)?;
                    if workflow.has_requirement(&Requirement::MultipleInputFeatureRequirement) {
                        let mut data = vec![];
                        for item in array {
                            if let Some(input) = workflow.inputs.iter().find(|i| i.id == item) {
                                let value = evaluate_input(input, &input_values.inputs)?;
                                match parameter.link_merge {
                                    None | Some(LinkMerge::MergeNested) => data.push(value),
                                    Some(LinkMerge::MergeFlattened) => {
                                        if let DefaultValue::Array(vec) = value {
                                            data.extend(vec);
                                        } else {
                                            return Err(anyhow::anyhow!("Expected array for MergeFlattened").into());
                                        }
                                    }
                                }
                            } else {
                                return Err(anyhow::anyhow!("Could not find input: {item}").into());
                            }
                        }
                        step_inputs.insert(parameter.id.to_string(), DefaultValue::Array(data));
                    } else if array.len() == 1
                        && let Some(input) = workflow.inputs.iter().find(|i| i.id == array[0])
                    {
                        //if requirement is not set, but array is of length 1 we use first value or wrap into array if linkmerge tells to do
                        let value = evaluate_input(input, &input_values.inputs)?;
                        match parameter.link_merge {
                            Some(LinkMerge::MergeFlattened) | None => step_inputs.insert(parameter.id.to_string(), value),
                            Some(LinkMerge::MergeNested) => step_inputs.insert(parameter.id.to_string(), DefaultValue::Array(vec![value])),
                        };
                    }
                }
            }
            let mut input_values = input_values.handle_requirements(&step.requirements, &step.hints);
            input_values.inputs = step_inputs;

            //check conditional execution
            if let Some(condition) = &step.when {
                if workflow.cwl_version == Some("v1.0".to_string()) || workflow.cwl_version == Some("v1.1".to_string()) {
                    return Err(anyhow::anyhow!("Conditional execution is not supported in CWL {:?}", workflow.cwl_version).into());
                }
                if !evaluate_condition(condition, &input_values.inputs)? {
                    continue;
                }
            }

            //decide if we are going to use scatter or normal execution
            let step_outputs = if let Some(scatter) = &step.scatter
                && workflow.has_requirement(&Requirement::ScatterFeatureRequirement)
            {
                //get input
                let scatter_keys = match scatter {
                    SingularPlural::Singular(item) => vec![item.clone()],
                    SingularPlural::Plural(items) => items.clone(),
                };

                let method = step.scatter_method.as_ref().unwrap_or(&ScatterMethod::DotProduct);

                let scatter_inputs = scatter::gather_inputs(&scatter_keys, &input_values)?;
                let jobs = scatter::gather_jobs(&scatter_inputs, &scatter_keys, method)?;

                let mut step_outputs: HashMap<String, Vec<DefaultValue>> = HashMap::new();
                for job in jobs {
                    let mut sub_inputs = input_values.clone();
                    for (k, v) in job {
                        sub_inputs.inputs.insert(k, v);
                    }

                    let singular_outputs = execute_step(step, &sub_inputs, &path, workflow_folder, &tmp_path)?;

                    for (key, value) in singular_outputs {
                        step_outputs.entry(key).or_default().push(value);
                    }
                }

                //if output arrays are empty we need to add them if scatter is set. see <https://www.commonwl.org/v1.2/Workflow.html#WorkflowStep>
                if step.scatter.is_some() {
                    for out in &step.out {
                        if !step_outputs.contains_key(out) {
                            step_outputs.insert(out.clone(), vec![]);
                        }
                    }
                }

                step_outputs
                    .into_iter()
                    .map(|(k, v)| (k, DefaultValue::Array(v)))
                    .collect::<HashMap<_, _>>()
            } else {
                execute_step(step, &input_values, &path, workflow_folder, &tmp_path)?
            };

            for (key, value) in step_outputs {
                outputs.insert(format!("{}/{}", step.id, key), value);
            }
        } else {
            return Err(anyhow::anyhow!("Could not find step {step_id}").into());
        }
    }

    fn output_file(file: &File, tmp_path: &str, output_directory: &str) -> Result<File, Box<dyn Error>> {
        let path = file.path.as_ref().map_or_else(String::new, |p| p.clone());
        let new_loc = Path::new(&path).to_string_lossy().replace(tmp_path, output_directory);
        copy_file(&path, &new_loc)?;
        let mut file = file.clone();
        file.path = Some(new_loc.to_string());
        file.location = Some(format!("file://{new_loc}"));
        Ok(file)
    }

    fn output_dir(dir: &Directory, tmp_path: &str, output_directory: &str) -> Result<Directory, Box<dyn Error>> {
        let path = dir.path.as_ref().map_or_else(String::new, |p| p.clone());
        let new_loc = Path::new(&path).to_string_lossy().replace(tmp_path, output_directory);
        copy_dir(&path, &new_loc)?;
        let mut dir = dir.clone();
        dir.path = Some(new_loc.to_string());
        dir.location = Some(format!("file://{new_loc}"));
        Ok(dir)
    }

    let mut output_values = HashMap::new();
    for output in &workflow.outputs {
        if let Some(source) = &output.output_source {
            if let Some(value) = &outputs.get(source) {
                let value = match value {
                    DefaultValue::File(file) => DefaultValue::File(output_file(file, &tmp_path, &output_directory)?),
                    DefaultValue::Directory(dir) => DefaultValue::Directory(output_dir(dir, &tmp_path, &output_directory)?),
                    DefaultValue::Any(value) => DefaultValue::Any(value.clone()),
                    DefaultValue::Array(array) => DefaultValue::Array(
                        array
                            .iter()
                            .map(|item| {
                                Ok(match item {
                                    DefaultValue::File(file) => DefaultValue::File(output_file(file, &tmp_path, &output_directory)?),
                                    DefaultValue::Directory(dir) => DefaultValue::Directory(output_dir(dir, &tmp_path, &output_directory)?),
                                    DefaultValue::Any(value) => DefaultValue::Any(value.clone()),
                                    _ => item.clone(),
                                })
                            })
                            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
                    ),
                };
                output_values.insert(&output.id, value.clone());
            } else if let Some(input) = workflow.inputs.iter().find(|i| i.id == *source) {
                let result = evaluate_input(input, &input_values.inputs)?;
                let value = match &result {
                    DefaultValue::File(file) => {
                        let dest = format!("{}/{}", output_directory, file.get_location());
                        fs::copy(workflow_folder.join(file.get_location()), &dest)?;
                        DefaultValue::File(get_file_metadata(Path::new(&dest).to_path_buf(), file.format.clone()))
                    }
                    DefaultValue::Directory(directory) => DefaultValue::Directory(copy_output_dir(
                        workflow_folder.join(directory.get_location()),
                        format!("{}/{}", &output_directory, &directory.get_location()),
                    )?),
                    DefaultValue::Any(inner) => DefaultValue::Any(inner.clone()),
                    DefaultValue::Array(inner) => DefaultValue::Array(inner.clone()),
                };
                output_values.insert(&output.id, value);
            }
        }
    }

    info!("✔️  Workflow {:?} executed successfully in {:.0?}!", &cwl_path, clock.elapsed());
    Ok(output_values.into_iter().map(|(k, v)| (k.clone(), v)).collect())
}

fn execute_step(
    step: &cwl_core::WorkflowStep,
    input_values: &InputObject,
    path: &Option<PathBuf>,
    workflow_folder: &Path,
    tmp_path: &str,
) -> Result<HashMap<String, DefaultValue>, Box<dyn Error>> {
    let step_outputs = if let Some(path) = path {
        info!("🚲 Executing Tool {path:?} ...");
        execute(path, input_values, Some(tmp_path), None)?
    } else if let StringOrDocument::Document(doc) = &step.run {
        info!("🚲 Executing Tool {} ...", step.id);
        execute(workflow_folder, input_values, Some(tmp_path), Some(doc))?
    } else {
        unreachable!()
    };
    Ok(step_outputs)
}
