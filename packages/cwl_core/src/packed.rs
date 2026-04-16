use crate::{
    CWLDocument, DefaultValue, Entry, Operation, Requirement, StringOrDocument, Workflow, WorkflowStep,
    inputs::CommandInputParameter,
    io::normalize_path,
    load_doc,
    outputs::{CommandOutputParameter, WorkflowOutputParameter},
    requirements::WorkDirItem,
};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs::{self},
    path::{MAIN_SEPARATOR_STR, Path},
};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PackedCWL {
    #[serde(rename = "$graph")]
    pub graph: Vec<CWLDocument>,
    pub cwl_version: String,
}

fn pack_cwl(doc: &CWLDocument, filename: impl AsRef<Path>, id: Option<&str>) -> Result<Vec<CWLDocument>, Box<dyn Error>> {
    Ok(match doc {
        CWLDocument::CommandLineTool(tool) => {
            let mut tool = tool.clone();
            pack_tool(&mut tool, filename, id)?;
            vec![CWLDocument::CommandLineTool(tool)]
        }
        CWLDocument::Workflow(wf) => {
            let packed_wf = pack_workflow(wf, filename, id)?;
            packed_wf.graph
        }
        CWLDocument::ExpressionTool(tool) => {
            let mut tool = tool.clone();
            pack_tool(&mut tool, filename, id)?;
            vec![CWLDocument::ExpressionTool(tool)]
        }
    })
}

/// Unpacks a packed Workflow into a self containing Workflow
pub fn unpack_workflow(pack: &PackedCWL) -> Result<Workflow, Box<dyn Error>> {
    //get root item; we need to check both # and not # version as json contains # and yaml does not...
    let graph = pack.graph.clone();
    let mut main = graph
        .into_iter()
        .find(|i| i.id == Some("#main".to_string()) || i.id == Some("main".to_string()));
    let Some(CWLDocument::Workflow(main)) = &mut main else {
        return Err("Could not find root entity".into());
    };
    unpack_wf(main, &pack.graph)?;

    Ok(main.to_owned())
}

/// Returns a packed version of a workflow
pub fn pack_workflow(wf: &Workflow, filename: impl AsRef<Path>, id: Option<&str>) -> Result<PackedCWL, Box<dyn Error>> {
    let mut wf = wf.clone(); //make mutable reference
    if let Some(id) = id {
        wf.id = Some(id.to_string());
    } else {
        wf.id = Some("#main".to_string());
    }

    let wf_dir = filename.as_ref().parent().unwrap_or(filename.as_ref());
    let wf_id = wf.id.clone().unwrap();

    let mut graph = vec![];
    for input in &mut wf.inputs {
        pack_input(input, &wf_id, wf_dir)?;
    }

    for output in &mut wf.outputs {
        pack_workflow_output(output, &wf_id);
    }

    for req in &mut wf.requirements {
        pack_requirement(req, wf_dir)?;
    }
    for req in &mut wf.hints {
        pack_requirement(req, wf_dir)?;
    }

    for step in &mut wf.steps {
        graph.extend(pack_step(step, wf_dir, &wf_id)?);
    }
    let cwl_version = wf.cwl_version.as_ref().map_or("v1.2".to_string(), |v| v.clone());
    wf.cwl_version = None;

    graph.push(CWLDocument::Workflow(wf));
    graph.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(PackedCWL { graph, cwl_version })
}

fn unpack_wf(wf: &mut Workflow, graph: &[CWLDocument]) -> Result<(), Box<dyn Error>> {
    let id = wf.id.clone().unwrap();

    for step in &mut wf.steps {
        unpack_step(step, &id, graph)?;
    }

    for input in &mut wf.inputs {
        unpack_input(input, &id);
    }

    for output in &mut wf.outputs {
        unpack_workflow_output(output, &id);
    }

    Ok(())
}

fn pack_tool<T: Operation>(tool: &mut T, filename: impl AsRef<Path>, id: Option<&str>) -> Result<(), Box<dyn Error>> {
    let tool_dir = filename.as_ref().parent().unwrap_or(filename.as_ref());
    let name = filename.as_ref().file_name().unwrap().to_string_lossy();

    if let Some(id) = id {
        tool.id = Some(id.to_string());
    } else if let Some(id) = &mut tool.id {
        *id = format!("#{id}");
    } else {
        tool.id = Some(format!("#{name}"));
    }

    let id = tool.id.clone().unwrap();
    for input in &mut tool.inputs {
        pack_input(input, &id, tool_dir)?;
    }

    for output in tool.outputs_mut() {
        pack_command_output(output, &id);
    }

    for req in &mut tool.requirements {
        pack_requirement(req, tool_dir)?;
    }
    for req in &mut tool.hints {
        pack_requirement(req, tool_dir)?;
    }

    Ok(())
}

fn unpack_tool<T: Operation>(tool: &mut T) {
    //we can savely unwrap here
    let id = tool.id.clone().unwrap();
    for input in &mut tool.inputs {
        unpack_input(input, &id);
    }

    for output in tool.outputs_mut() {
        unpack_command_output(output, &id);
    }
}

fn pack_input(input: &mut CommandInputParameter, root_id: &str, doc_dir: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
    input.id = format!("{root_id}/{}", input.id);

    //generate absolute paths for default values
    if let Some(DefaultValue::File(file)) = &mut input.default
        && let Some(location) = &mut file.location
        && !location.starts_with("file://")
    {
        if Path::new(location).is_absolute() {
            *location = url_from_path(&mut *location);
        } else {
            let path = doc_dir.as_ref().join(&location);
            let path = if path.exists() {
                path.canonicalize().unwrap_or(path).to_string_lossy().into_owned()
            } else {
                normalize_path(&path).unwrap_or(path).to_string_lossy().into_owned()
            };
            *location = url_from_path(&path);
        }
    }

    if let Some(DefaultValue::Directory(dir)) = &mut input.default
        && let Some(location) = &mut dir.location
        && !location.starts_with("file://")
    {
        if Path::new(location).is_absolute() {
            *location = url_from_path(&mut *location);
        } else {
            let path = doc_dir.as_ref().join(&location);
            let path = if path.exists() {
                path.canonicalize().unwrap_or(path).to_string_lossy().into_owned()
            } else {
                normalize_path(&path).unwrap_or(path).to_string_lossy().into_owned()
            };
            *location = url_from_path(&path);
        }
    }

    Ok(())
}

fn unpack_input(input: &mut CommandInputParameter, id: &str) {
    input.id = input.id.strip_prefix(&format!("{id}/")).unwrap_or(&input.id).to_string();
}

fn pack_workflow_output(output: &mut WorkflowOutputParameter, root_id: &str) {
    output.id = format!("{root_id}/{}", output.id);
    if let Some(output_source) = &output.output_source {
        output.output_source = Some(format!("{root_id}/{}", output_source));
    }
}

fn unpack_workflow_output(output: &mut WorkflowOutputParameter, id: &str) {
    output.id = output.id.strip_prefix(&format!("{id}/")).unwrap_or(&output.id).to_string();
    if let Some(output_source) = &output.output_source {
        output.output_source = Some(output_source.strip_prefix(&format!("{id}/")).unwrap_or(output_source).to_string());
    }
}

fn pack_command_output(output: &mut CommandOutputParameter, root_id: &str) {
    output.id = format!("{root_id}/{}", output.id);
}

fn unpack_command_output(output: &mut CommandOutputParameter, id: &str) {
    output.id = output.id.strip_prefix(&format!("{id}/")).unwrap_or(&output.id).to_string();
}

fn pack_requirement(requirement: &mut Requirement, doc_dir: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
    match requirement {
        Requirement::InitialWorkDirRequirement(iwdr) => {
            for item in &mut iwdr.listing {
                if let WorkDirItem::Dirent(dirent) = item {
                    pack_entry(&mut dirent.entry, &doc_dir)?;
                }
            }
        }
        Requirement::DockerRequirement(dr) => {
            if let Some(file) = &mut dr.docker_file {
                pack_entry(file, &doc_dir)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn pack_entry(entry: &mut Entry, doc_dir: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
    if let Entry::Include(include) = &entry {
        let path = &include.include;
        let contents = fs::read_to_string(doc_dir.as_ref().join(path))?;

        *entry = Entry::Source(contents);
    }

    Ok(())
}

fn pack_step(step: &mut WorkflowStep, wf_dir: impl AsRef<Path>, wf_id: &str) -> Result<Vec<CWLDocument>, Box<dyn Error>> {
    let step_id = format!("{wf_id}/{}", step.id);
    step.id = step_id.to_string();

    let mut packed_graph = match &mut step.run {
        StringOrDocument::String(filename) => {
            let path = Path::new(filename);
            let path = if path.is_absolute() { path } else { &wf_dir.as_ref().join(path) };
            let filename = if let Some(filename) = path.file_name() {
                filename.to_string_lossy().into_owned()
            } else {
                format!("{step_id}.cwl")
            };
            let step_hash = format!("#{filename}");
            let cwl = load_doc(path)?;
            let graph = pack_cwl(&cwl, path, Some(&step_hash))?;

            step.run = StringOrDocument::String(step_hash);
            graph
        }
        StringOrDocument::Document(doc) => {
            let step_hash = format!("#{step_id}.cwl");
            let graph = pack_cwl(doc, wf_dir.as_ref().join(&step.id), Some(&step_hash))?;

            step.run = StringOrDocument::String(step_hash);
            graph
        }
    };

    for input in &mut step.in_ {
        input.id = format!("{step_id}/{}", input.id);
        if let Some(src) = &mut input.source {
            *src = format!("{wf_id}/{src}");
        }
    }

    for output in &mut step.out {
        *output = format!("{step_id}/{output}");
    }

    let packed_graph = &mut packed_graph;
    for item in packed_graph.iter_mut() {
        item.cwl_version = None;
    }

    Ok(packed_graph.to_owned())
}

fn unpack_step(step: &mut WorkflowStep, root_id: &str, graph: &[CWLDocument]) -> Result<(), Box<dyn Error>> {
    if let StringOrDocument::String(run) = &step.run {
        //find item with id in list
        let run = run.strip_prefix('#').unwrap_or(run);
        let step_op = graph.iter().find(|i| i.id == Some(format!("#{run}")) || i.id == Some(run.to_string()));
        if let Some(step_op) = step_op {
            let mut step_op = step_op.clone(); //we later need to clone anyways
            match &mut step_op {
                CWLDocument::CommandLineTool(tool) => unpack_tool(tool),
                CWLDocument::ExpressionTool(tool) => unpack_tool(tool),
                CWLDocument::Workflow(wf) => unpack_wf(wf, graph)?,
            }
            step.run = StringOrDocument::Document(Box::new(step_op));
        }
    }

    for input in &mut step.in_ {
        if let Some(stripped) = input.id.strip_prefix(&format!("{}/", step.id)) {
            input.id = stripped.to_string();
        }

        if let Some(source) = &input.source
            && let Some(stripped) = source.strip_prefix(&format!("{root_id}/"))
        {
            input.source = Some(stripped.to_string());
        }
    }

    for output in &mut step.out {
        if let Some(stripped) = output.strip_prefix(&format!("{}/", step.id)) {
            *output = stripped.to_string();
        }
    }

    step.id = step.id.strip_prefix(&format!("{root_id}/")).unwrap_or(&step.id).to_string();

    Ok(())
}

fn url_from_path(path: impl AsRef<Path>) -> String {
    let str = path.as_ref().to_string_lossy().into_owned();
    // windows fixes
    let str = str.replace(MAIN_SEPARATOR_STR, "/");
    //remove windows /? thingy
    let str = str.split_once("/?").unwrap_or(("", &str)).1;

    if !str.starts_with("/") {
        return format!("file:///{str}");
    }

    format!("file://{str}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CWLType, Command, CommandLineTool, Dirent, File, Include, SingularPlural,
        inputs::CommandLineBinding,
        load_workflow,
        outputs::CommandOutputBinding,
        prelude::{DockerRequirement, InitialWorkDirRequirement, Requirement},
    };
    use serde_json::Value;
    use std::path::MAIN_SEPARATOR_STR;
    use test_utils::normalize_json_newlines;

    #[test]
    fn test_pack_input() {
        let mut input = CommandInputParameter::default()
            .with_id("population")
            .with_type(CWLType::File)
            .with_default_value(DefaultValue::File(File::from_location("../../data/population.csv")))
            .with_binding(CommandLineBinding::default().with_prefix("--population"));

        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();

        let file_path = base_dir.join("testdata/hello_world/workflows/calculation");
        pack_input(&mut input, "#calculation.cwl", file_path).unwrap();

        let json = serde_json::json!(&input);
        let reference_json = r##"{
                    "id": "#calculation.cwl/population",
                    "type": "File",
                    "default": {
                        "class": "File",
                        "location": "file://XXX/testdata/hello_world/data/population.csv"
                    },
                    "inputBinding": {
                        "prefix": "--population"
                    }
                }"##
        .replace("XXX", &base_dir.to_string_lossy().replace(MAIN_SEPARATOR_STR, "/"))
        .replace("//?", "");

        let value: Value = serde_json::from_str(&reference_json).unwrap();
        assert_eq!(json, value);
    }

    #[test]
    fn test_pack_workflow_output() {
        let mut output = WorkflowOutputParameter {
            id: "out".to_string(),
            type_: CWLType::File,
            output_source: Some("plot/results".to_string()),
        };

        pack_workflow_output(&mut output, "#main");
        let json = serde_json::json!(&output);

        let reference_json = r##"{
                    "id": "#main/out",
                    "type": "File",
                    "outputSource": "#main/plot/results"
                }"##;

        let value: Value = serde_json::from_str(reference_json).unwrap();
        assert_eq!(json, value);
    }

    #[test]
    fn test_pack_command_output() {
        let mut output = CommandOutputParameter::default()
            .with_id("results")
            .with_type(CWLType::File)
            .with_binding(CommandOutputBinding {
                glob: Some(SingularPlural::Singular("results.csv".to_string())),
                ..Default::default()
            });

        pack_command_output(&mut output, "#calculation.cwl");
        let json = serde_json::json!(&output);

        let reference_json = r##"{
                    "id": "#calculation.cwl/results",
                    "type": "File",
                    "outputBinding": {
                        "glob": "results.csv"
                    }
                }"##;

        let value: Value = serde_json::from_str(reference_json).unwrap();
        assert_eq!(json, value);
    }

    #[test]
    fn test_pack_commandlinetool() {
        let mut tool = CommandLineTool::default()
            .with_base_command(Command::Multiple(vec![
                "python".to_string(),
                "workflows/calculation/calculation.py".to_string(),
            ]))
            .with_inputs(vec![
                CommandInputParameter::default()
                    .with_id("population")
                    .with_type(CWLType::File)
                    .with_default_value(DefaultValue::File(File::from_location("../../data/population.csv")))
                    .with_binding(CommandLineBinding::default().with_prefix("--population")),
                CommandInputParameter::default()
                    .with_id("speakers")
                    .with_type(CWLType::File)
                    .with_default_value(DefaultValue::File(File::from_location("../../data/speakers_revised.csv")))
                    .with_binding(CommandLineBinding::default().with_prefix("--speakers")),
            ])
            .with_outputs(vec![
                CommandOutputParameter::default()
                    .with_id("results")
                    .with_type(CWLType::File)
                    .with_binding(CommandOutputBinding {
                        glob: Some(SingularPlural::Singular("results.csv".to_string())),
                        ..Default::default()
                    }),
            ])
            .with_requirements(vec![
                Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement {
                    listing: vec![WorkDirItem::Dirent(Dirent {
                        entryname: Some("workflows/calculation/calculation.py".to_string()),
                        entry: Entry::Include(Include {
                            include: "calculation.py".to_string(),
                        }),
                        ..Default::default()
                    })],
                }),
                Requirement::DockerRequirement(DockerRequirement {
                    docker_pull: Some("pandas/pandas:pip-all".to_string()),
                    ..Default::default()
                }),
            ]);

        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        let file_path = base_dir.join("testdata/hello_world/workflows/calculation/calculation.cwl");
        pack_tool(&mut tool, file_path, Some("#main")).unwrap();
        let mut json = serde_json::json!(&tool);

        let reference_json = include_str!("../../../testdata/packed/calculation_packed.cwl")
            .replace("/mnt/m4.4_sciwin_client", &base_dir.to_string_lossy().replace(MAIN_SEPARATOR_STR, "/"))
            .replace("//?", "");
        let mut value: Value = serde_json::from_str(&reference_json).unwrap();
        normalize_json_newlines(&mut json);
        normalize_json_newlines(&mut value);
        assert_eq!(json, value);
    }

    #[test]
    fn test_pack_workflow() {
        let file = "../../testdata/hello_world/workflows/main/main.cwl";
        let wf = load_workflow(file).unwrap();

        let packed = pack_workflow(&wf, file, None).unwrap();
        let mut json = serde_json::json!(&packed);

        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../").canonicalize().unwrap();
        let reference_json = include_str!("../../../testdata/packed/main_packed.cwl")
            .replace("/mnt/m4.4_sciwin_client", &base_dir.to_string_lossy().replace(MAIN_SEPARATOR_STR, "/"))
            .replace("//?", "");
        let mut value: Value = serde_json::from_str(&reference_json).unwrap();
        normalize_json_newlines(&mut json);
        normalize_json_newlines(&mut value);
        assert_eq!(json, value);
    }

    #[test]
    fn test_unpack_workflow() {
        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        let pack = include_str!("../../../testdata/packed/main_packed.cwl")
            .replace("/mnt/m4.4_sciwin_client", &base_dir.to_string_lossy().replace(MAIN_SEPARATOR_STR, "/"));

        let pack: PackedCWL = serde_json::from_str(&pack).unwrap();
        let unpacked = unpack_workflow(&pack).unwrap();

        assert!(unpacked.has_step("calculation"));
        let Some(step) = unpacked.get_step("calculation") else { unreachable!() };

        if let StringOrDocument::Document(doc) = &step.run {
            assert_eq!(doc.id, Some("#calculation.cwl".to_string()));
            assert_eq!(doc.class, "CommandLineTool".to_string());
        } else {
            panic!()
        }
    }
}
