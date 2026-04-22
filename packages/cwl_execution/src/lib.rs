pub mod environment;
pub mod error;
pub mod io;
pub mod runner;
pub mod docker;
mod expression; 
mod inputs;
mod outputs;
mod preprocess;
mod scatter;
mod staging;
mod validate;

pub use docker::{ContainerEngine, container_engine, set_container_engine};

use crate::error::{ExecutionError, FileSystemError, YAMLDeserializationError};
use cwl_core::{
    CWLDocument, CWLType, DefaultValue, Directory, File, PathItem, guess_type, load_doc,
    packed::{PackedCWL, unpack_workflow, pack_workflow},
    requirements::{FromRequirement, Requirement},
};
use io::preprocess_path_join;
use preprocess::preprocess_cwl;
use runner::{tool::run_tool, workflow::run_workflow};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{collections::HashMap, fs, path::Path, process::Command, sync::LazyLock};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, System};
use anyhow::anyhow;
use std::fmt::Debug;
use rocrate::{export_rocrate, RocrateArgs};

pub type Result<T> = std::result::Result<T, ExecutionError>;

#[allow(clippy::disallowed_macros)]
pub async fn execute_cwlfile(cwlfile: impl AsRef<Path> + Debug, raw_inputs: &[String], outdir: Option<impl AsRef<Path>>, 
    rocrate_args: &Option<RocrateArgs>, crate_root: Option<&Path>) -> Result<()> {
    //gather inputs
    let mut input_values = if raw_inputs.len() == 1 && !raw_inputs[0].starts_with('-') {
        let yaml = fs::read_to_string(&raw_inputs[0])?;
        serde_yaml::from_str(&yaml).map_err(|e| YAMLDeserializationError::new(Path::new(&raw_inputs[0]), e))?
    } else {
        InputObject {
            inputs: raw_inputs
                .chunks_exact(2)
                .filter_map(|pair| {
                    if let Some(key) = pair[0].strip_prefix("--") {
                        let raw_value = &pair[1];
                        let value = match guess_type(raw_value) {
                            CWLType::File => DefaultValue::File(File::from_location(raw_value)),
                            CWLType::Directory => DefaultValue::Directory(Directory::from_location(raw_value)),
                            CWLType::String => DefaultValue::Any(Value::String(raw_value.to_string())),
                            _ => DefaultValue::Any(serde_yaml::from_str(raw_value).expect("Could not read input")),
                        };
                        Some((key.to_string(), value))
                    } else {
                        None
                    }
                })
                .collect::<HashMap<_, _>>(),
            ..Default::default()
        }
    };

    fn correct_path<T: PathItem>(item: &mut T, path_prefix: &Path) {
        let mut location = item.get_location().clone();
        if location.is_empty() {
            return;
        }
        if location.starts_with("file://") {
            location = location.strip_prefix("file://").unwrap_or(&location).to_string();
        }

        item.set_location(preprocess_path_join(path_prefix, &location));
        if let Some(secondary_files) = item.secondary_files_mut() {
            for sec_file in secondary_files {
                match sec_file {
                    DefaultValue::File(file) => {
                        file.set_location(preprocess_path_join(path_prefix, &file.get_location()));
                    }
                    DefaultValue::Directory(directory) => directory.set_location(preprocess_path_join(path_prefix, &directory.get_location())),
                    _ => (),
                }
            }
        }
    }

    //make paths relative to calling object
    let path_prefix = if let Some(root) = crate_root {
        root.to_path_buf()
    } else if raw_inputs.len() == 1 && !raw_inputs[0].starts_with('-') {
        Path::new(&raw_inputs[0]).parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
    } else {
        Path::new(".").to_path_buf()
    };
    for value in input_values.inputs.values_mut() {
        match value {
            DefaultValue::File(file) => correct_path(file, &path_prefix),
            DefaultValue::Directory(directory) => correct_path(directory, &path_prefix),
            _ => (),
        }
    }
    if let Some(rocrate_args) = rocrate_args && rocrate_args.workflow_name.is_some() {
            let doc = load_doc(&cwlfile)?;
            let CWLDocument::Workflow(workflow) = doc else {
                return Err(ExecutionError::CWLVersionMismatch(format!(
                    "CWL document is not a Workflow: {:?}",
                    cwlfile.as_ref()
            )));
        };
        let packed = pack_workflow(&workflow, &cwlfile, None)?;
        let packed_json = serde_json::to_value(&packed)?;
        std::fs::write("packed.cwl", serde_json::to_string_pretty(&packed)?)?;

        let working_dir = std::env::current_dir()?;
        let graph_json = packed_json.get("$graph").and_then(serde_json::Value::as_array).ok_or_else(|| anyhow!("Missing or invalid '$graph' field"))?;
        execute(&cwlfile, &input_values, outdir, None).await?;
        export_rocrate(
            rocrate_args.output_dir.as_ref(),
            Some(&working_dir.to_string_lossy().to_string()),
            cwlfile.as_ref().to_str().unwrap(),
            rocrate_args.run_type,
            Some("local"),
            graph_json,
            None
        ).await?;
    }
    else {
        let output_values = execute(cwlfile, &input_values, outdir, None).await?;
        let _json = serde_json::to_string_pretty(&output_values)?;
    }

    Ok(())
}

pub async fn execute(
    cwlfile: impl AsRef<Path>,
    input_values: &InputObject,
    outdir: Option<impl AsRef<Path>>,
    cwl_doc: Option<&CWLDocument>,
) -> Result<HashMap<String, DefaultValue>> {
    // Load CWL document
    let mut doc: CWLDocument = if let Some(doc) = cwl_doc {
        doc.clone()
    } else if is_packed(&cwlfile)? {
        let path = cwlfile.as_ref().to_string_lossy();
        let (real_path, id) = path.split_once('#').unwrap_or((path.as_ref(), "main"));
        let contents = fs::read_to_string(real_path)?;
        let packed: PackedCWL = serde_yaml::from_str(&contents).map_err(|e| YAMLDeserializationError::new(Path::new(real_path), e))?;
        if id != "main" {
            packed.graph.into_iter()
                .find(|i| i.id == Some(id.to_string()))
                .ok_or_else(|| ExecutionError::Any(anyhow::anyhow!("Document not found: {id}")))? 
        } else {
            CWLDocument::Workflow(unpack_workflow(&packed)?)
        }
    } else {
        let contents = fs::read_to_string(&cwlfile).map_err(|e| FileSystemError::new(cwlfile.as_ref(), e))?;
        let contents = preprocess_cwl(&contents, &cwlfile)?;
        serde_yaml::from_str(&contents).map_err(|e| YAMLDeserializationError::new(cwlfile.as_ref(), e))?
    };

    match doc {
        CWLDocument::CommandLineTool(_) | CWLDocument::ExpressionTool(_) => run_tool(
            &mut doc,
            input_values,
            &cwlfile.as_ref().to_path_buf(),
            outdir.map(|d| d.as_ref().to_string_lossy().into_owned()),
        ).await,
        CWLDocument::Workflow(mut workflow) => {
            let cwl_path = cwlfile.as_ref().to_path_buf();
            let out_dir = outdir.map(|d| d.as_ref().to_string_lossy().into_owned());
            let future = Box::pin(run_workflow(
                &mut workflow,
                input_values,
                &cwl_path,
                out_dir,
            ));
            future.await
        }
    }
}

fn is_packed(cwlfile: impl AsRef<Path>) -> Result<bool> {
    if cwlfile.as_ref().file_name().unwrap().to_string_lossy().contains("#") {
        return Ok(true);
    }
    let contents = fs::read_to_string(&cwlfile)?;
    Ok(contents.contains("$graph"))
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone)]
pub struct InputObject {
    #[serde(flatten)]
    pub inputs: HashMap<String, DefaultValue>,
    #[serde(default, rename = "cwl:requirements")]
    pub requirements: Vec<Requirement>,
    #[serde(default, rename = "cwl:hints")]
    pub hints: Vec<Requirement>,

    #[serde(skip)]
    cwl_requirements: Vec<Requirement>,
    #[serde(skip)]
    cwl_hints: Vec<Requirement>,
}

impl InputObject {
    pub fn get_requirement<T>(&self) -> Option<&T>
    where
        Requirement: FromRequirement<T>,
    {
        self.requirements.iter().chain(self.hints.iter()).find_map(|req| Requirement::get(req))
    }

    pub fn add_requirement(&mut self, requirement: &Requirement) {
        if let Some(r) = self
            .cwl_requirements
            .iter_mut()
            .find(|r| std::mem::discriminant(*r) == std::mem::discriminant(requirement))
        {
            *r = requirement.clone();
        } else {
            self.cwl_requirements.push(requirement.clone());
        }
    }

    pub fn add_hint(&mut self, hint: &Requirement) {
        if let Some(r) = self
            .cwl_hints
            .iter_mut()
            .find(|r| std::mem::discriminant(*r) == std::mem::discriminant(hint))
        {
            *r = hint.clone();
        } else {
            self.cwl_hints.push(hint.clone());
        }
    }

    pub fn handle_requirements(&self, requirements: &[Requirement], hints: &[Requirement]) -> Self {
        let mut new_obj = self.clone();
        for hint in hints {
            new_obj.add_hint(hint);
        }

        for req in requirements {
            new_obj.add_requirement(req);
        }
        new_obj
    }

    pub fn lock(&mut self) {
        fn merge(dst: &mut Vec<Requirement>, src: &[Requirement]) {
            for req in src {
                if let Some(r) = dst.iter_mut().find(|r| std::mem::discriminant(*r) == std::mem::discriminant(req)) {
                    *r = req.clone();
                } else {
                    dst.push(req.clone());
                }
            }
        }
        merge(&mut self.cwl_requirements, &self.requirements);
        self.requirements = self.cwl_requirements.clone();

        merge(&mut self.cwl_hints, &self.hints);
        self.hints = self.cwl_hints.clone();
    }
}

impl From<HashMap<String, DefaultValue>> for InputObject {
    fn from(inputs: HashMap<String, DefaultValue>) -> Self {
        Self {
            inputs,
            ..Default::default()
        }
    }
}

pub fn format_command(command: &Command) -> String {
    let program = command.get_program().to_string_lossy();

    let args: Vec<String> = command
        .get_args()
        .map(|arg| {
            let arg_str = arg.to_string_lossy();
            arg_str.to_string()
        })
        .collect();

    format!("{} {}", program, args.join(" "))
}

static DISKS: LazyLock<Disks> = LazyLock::new(Disks::new_with_refreshed_list);

static SYSTEM: LazyLock<System> = LazyLock::new(|| {
    let mut system = System::new();
    system.refresh_cpu_list(CpuRefreshKind::nothing());
    system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
    system
});

pub(crate) fn get_processor_count() -> usize {
    SYSTEM.cpus().iter().count()
}

pub(crate) fn get_available_ram() -> u64 {
    SYSTEM.free_memory() / 1024
}

pub(crate) fn get_available_disk_space() -> u64 {
    DISKS[0].available_space() / 1024
}

#[cfg(test)]
mod tests {
    use super::*;
    use cwl_core::{EnviromentDefs, requirements::EnvVarRequirement};

    #[test]
    fn test_add_requirement() {
        let mut input = InputObject::default();
        let base_req = Requirement::EnvVarRequirement(EnvVarRequirement {
            env_def: EnviromentDefs::Map(HashMap::from([("MY_ENV".to_string(), "BASE".to_string())])),
        });
        input.add_requirement(&base_req);
        assert_eq!(input.cwl_requirements.len(), 1);

        let requirement = Requirement::EnvVarRequirement(EnvVarRequirement {
            env_def: EnviromentDefs::Map(HashMap::from([("MY_ENV".to_string(), "OVERWRITE".to_string())])),
        });
        input.add_requirement(&requirement);
        assert_eq!(input.cwl_requirements.len(), 1);
        assert_eq!(input.cwl_requirements[0], requirement);
    }
}
