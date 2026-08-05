//! Attaching container, network, environment and mount requirements to a tool.

use super::ToolCreationOptions;
use crate::{authoring::AuthoringResult, paths::TrustedPathExt};
use anyhow::Context as _;
use bon::Builder;
use commonwl::{
    documents::CommandLineTool,
    files::{Directory, FileOrDirectory},
    requirements::ToolRequirements,
};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

/// Container to run the tool in: either an image reference to pull, or a path to a
/// `Dockerfile` to build, in which case `tag` names the resulting image.
#[derive(Clone, Debug, PartialEq, Eq, Builder)]
pub struct ContainerInfo {
    #[builder(into)]
    pub image: String,
    #[builder(into)]
    pub tag: Option<String>,
}

pub(super) fn add_tool_requirements(
    cwl: &mut CommandLineTool,
    options: &ToolCreationOptions,
    project_root: &Path,
) -> AuthoringResult<()> {
    // Handle container requirements
    append_container_requirement_mut(cwl, options.container.as_ref());

    if options.enable_network {
        cwl.use_network_mut();
    }

    if let Some(env) = &options.env {
        let data = read_env(env)?;
        cwl.use_env_requirement_mut(&data);
    }

    if !options.mounts.is_empty() {
        let mut entries = Vec::with_capacity(options.mounts.len());
        for m in &options.mounts {
            if project_root.join_trusted_checked(m)?.is_dir() {
                entries.push(FileOrDirectory::Directory(
                    Directory::builder()
                        .location(m.to_string_lossy().into_owned())
                        .build(),
                ));
            } else {
                tracing::warn!("{} is not a directory and has been skipped!", m.display());
            }
        }
        cwl.add_mount_requirement_mut(entries);
    }
    Ok(())
}

pub(super) fn is_sif_image(image: &str) -> bool {
    Path::new(image)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sif"))
}

/// Apptainer/Singularity ran the tool from a `.sif` file, so the requirements are already in
/// place -- but they name the `.sif` path. Swap the Docker requirement for one naming the
/// image without that extension.
pub(super) fn rewrite_sif_container_mut(cwl: &mut CommandLineTool, container: &ContainerInfo) {
    if let Some(reqs) = &mut cwl.requirements {
        reqs.retain(|req| !matches!(req, ToolRequirements::DockerRequirement(_)));
    }

    let mut without_extension = container.clone();
    without_extension.image = container.image.trim_end_matches(".sif").to_string();
    append_container_requirement_mut(cwl, Some(&without_extension));
}

fn append_container_requirement_mut(cwl: &mut CommandLineTool, container: Option<&ContainerInfo>) {
    let Some(container) = container else { return };

    if container.image.contains("Dockerfile") {
        let image_id = container.tag.as_deref().unwrap_or("sciwin-container");
        cwl.use_docker_file_mut(&container.image, image_id);
    } else {
        cwl.use_docker_pull_mut(&container.image);
    }
}

/// Reads a `KEY=value` file. Lines without a `=` are skipped.
fn read_env(path: &Path) -> AuthoringResult<HashMap<String, String>> {
    let f = fs::File::open(path).with_context(|| format!("Failed to open env file {path:?}"))?;
    let reader = BufReader::new(f);

    let mut map = HashMap::new();
    for line in reader.lines() {
        if let Some((key, value)) = line?.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Ok(map)
}
