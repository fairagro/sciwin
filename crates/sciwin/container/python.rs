use std::path::Path;
use tokio::fs;
use uv_pypi_types::PyProjectToml;
use uv_requirements_txt::{RequirementsTxt, RequirementsTxtRequirement};
use versions::Requirement;

use crate::{
    authoring::{AuthoringError, AuthoringResult},
    container::resolver::Package,
};

pub async fn requirements_from_requirements_txt(
    requirements_txt: &Path,
    working_dir: &Path,
) -> AuthoringResult<Option<Vec<Package>>> {
    let raw = RequirementsTxt::parse(requirements_txt, working_dir).await?;

    let requirements = raw
        .requirements
        .into_iter()
        .map(|entry| parse_requirement(&entry.requirement))
        .collect::<AuthoringResult<Vec<_>>>()?;

    Ok(Some(requirements))
}

pub async fn requirements_from_pyproject_toml(
    pyproject_toml: &Path,
) -> AuthoringResult<Option<Vec<Package>>> {
    let contents = fs::read_to_string(pyproject_toml).await?;
    let raw = PyProjectToml::from_toml(&contents, "pyproject.toml")?;
    let Some(project) = raw.project else {
        return Ok(None);
    };

    let Some(deps) = project.dependencies else {
        return Ok(None);
    };

    let working_dir = pyproject_toml.parent().unwrap_or_else(|| Path::new(""));
    let requirements = deps
        .iter()
        .map(|entry| {
            let requirement = RequirementsTxtRequirement::parse(entry, working_dir, false)
                .map_err(|_| AuthoringError::InvalidRequirement {
                    spec: entry.clone(),
                })?;
            parse_requirement(&requirement)
        })
        .collect::<AuthoringResult<Vec<_>>>()?;

    Ok(Some(requirements))
}

/// Converts a single `requirements.txt` entry into a [`Package`], failing
/// rather than silently dropping entries that have no name (URL-only
/// requirements) or a specifier `versions::Requirement` can't represent
/// (e.g. compound ranges). An entry with no version pin at all is not a
/// failure: it becomes `version: None`, which `resolver::resolve` already
/// treats as an unconstrained match, i.e. the same as a `*` requirement.
fn parse_requirement(requirement: &RequirementsTxtRequirement) -> AuthoringResult<Package> {
    let RequirementsTxtRequirement::Named(named) = requirement else {
        return Err(AuthoringError::InvalidRequirement {
            spec: requirement.to_string(),
        });
    };

    let version = named
        .version_or_url
        .as_ref()
        .map(|version_or_url| {
            let spec = version_or_url.to_string();
            parse_version_requirement(&spec).ok_or(AuthoringError::InvalidRequirement { spec })
        })
        .transpose()?;

    Ok(Package::builder()
        .name(named.name.to_string())
        .maybe_version(version)
        .build())
}

/// Maps a PEP 440 exact-match specifier (`==`) onto `versions::Requirement`'s
/// `=` syntax; every other operator (`~=`, `!=`, compound ranges, wildcards)
/// isn't representable and is left to fail in the caller.
fn parse_version_requirement(spec: &str) -> Option<Requirement> {
    let normalized = spec
        .strip_prefix("==")
        .map_or_else(|| spec.to_string(), |rest| format!("={rest}"));
    Requirement::new(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

    #[tokio::test]
    async fn parse_requirements() {
        let dir = tempdir().unwrap();
        let reqs = r#"
pandas==2.3.2
geopandas==1.1.1
shapely==2.1.1
scikit-learn==1.7.2
joblib==1.5.2
matplotlib==3.10.6
requests==2.32.5
"#;

        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{reqs}").unwrap();

        let requirements = requirements_from_requirements_txt(file.path(), dir.path())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(requirements.len(), 7);
        assert_eq!(requirements[0].name, "pandas");
        assert_eq!(
            requirements[0].version,
            Some(Requirement::new("=2.3.2").unwrap())
        );
    }

    #[tokio::test]
    async fn parse_requirements2() {
        let dir = tempdir().unwrap();
        let reqs = r#"
plotly>=3.0
pandas
kaleido==0.2.1
matplotlib
"#;

        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{reqs}").unwrap();

        let requirements = requirements_from_requirements_txt(file.path(), dir.path())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(requirements.len(), 4);
        assert_eq!(requirements[0].name, "plotly");
        assert_eq!(
            requirements[0].version,
            Some(Requirement::new(">=3.0").unwrap())
        );
        assert_eq!(requirements[1].name, "pandas");
        assert_eq!(requirements[1].version, None);
        assert_eq!(requirements[3].name, "matplotlib");
        assert_eq!(requirements[3].version, None);
    }

    #[tokio::test]
    async fn fails_on_unrepresentable_specifier() {
        let dir = tempdir().unwrap();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "pandas~=2.3.2").unwrap();

        let result = requirements_from_requirements_txt(file.path(), dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pyproject() {
        let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("testdata")
            .join("test_pyproject.toml");
        let requirements = requirements_from_pyproject_toml(&file_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(requirements.len(), 13);
        assert_eq!(requirements[4].name, "llama-index-embeddings-huggingface");
        assert_eq!(requirements[4].version, Requirement::new(">=0.7.0"));
    }
}
