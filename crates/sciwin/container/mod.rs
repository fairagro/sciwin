use crate::{
    authoring::AuthoringResult,
    container::resolver::{Image, resolve, resolve_images_from_digests},
};
use std::path::Path;

pub mod python;
pub mod r;
pub mod resolver;

pub async fn resolve_python_container(working_dir: &Path) -> AuthoringResult<Option<Image>> {
    let working_dir = dunce::canonicalize(working_dir)?;
    let dependencies = if working_dir.join("requirements.txt").exists() {
        python::requirements_from_requirements_txt(
            &working_dir.join("requirements.txt"),
            &working_dir,
        )
        .await?
    } else if working_dir.join("pyproject.toml").exists() {
        python::requirements_from_pyproject_toml(&working_dir.join("pyproject.toml")).await?
    } else {
        None
    };

    let Some(dependencies) = dependencies else {
        return Ok(None);
    };

    let digests = resolve(&dependencies, &resolver::PackageType::Python).await?;
    let images = resolve_images_from_digests(&digests).await?;

    Ok(images.smallest_without_entrypoint())
}

pub async fn resolve_r_container(working_dir: &Path) -> AuthoringResult<Option<Image>> {
    let working_dir = dunce::canonicalize(working_dir)?;
    let dependencies = if working_dir.join("DESCRIPTION").exists() {
        r::requirements_from_description(&working_dir.join("DESCRIPTION")).await?
    } else {
        None
    };

    let Some(dependencies) = dependencies else {
        return Ok(None);
    };

    let digests = resolve(&dependencies, &resolver::PackageType::RPackage).await?;
    let images = resolve_images_from_digests(&digests).await?;

    Ok(images.smallest_without_entrypoint())
}
