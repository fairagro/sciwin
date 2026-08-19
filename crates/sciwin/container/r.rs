use crate::{
    authoring::{AuthoringError, AuthoringResult},
    container::resolver::Package,
};
use r_description::{
    Version, VersionConstraint,
    lossy::{RDescription, Relation},
};
use std::{path::Path, str::FromStr};
use tokio::fs;
use versions::Op;

pub async fn requirements_from_description(path: &Path) -> AuthoringResult<Option<Vec<Package>>> {
    let contents = fs::read_to_string(path).await?;

    requirements_from_r(&contents)
}

pub fn requirements_from_r(contents: &str) -> AuthoringResult<Option<Vec<Package>>> {
    let desc = RDescription::from_str(contents).map_err(AuthoringError::RDescription)?;

    let Some(requirements) = desc.imports else {
        return Ok(None);
    };

    let packages = requirements
        .iter()
        .map(parse_relation)
        .collect::<AuthoringResult<Vec<_>>>()?;

    Ok(Some(packages))
}

/// Converts a single R DESCRIPTION relation into a [`Package`], failing
/// rather than silently dropping a version constraint that `to_versioning`
/// can't represent (e.g. `!=`), mirroring `python.rs`'s `parse_requirement`.
fn parse_relation(relation: &Relation) -> AuthoringResult<Package> {
    let version = relation
        .version
        .as_ref()
        .map(|(constraint, version)| {
            let spec = format!("{constraint}{version}");
            to_versioning(constraint, version).ok_or(AuthoringError::InvalidRequirement { spec })
        })
        .transpose()?;

    Ok(Package::builder()
        .name(relation.name.clone())
        .maybe_version(version)
        .build())
}

/// Maps an R DESCRIPTION version constraint onto `versions::Requirement`.
/// `versions::Op` has no exclusion operator, so `!=` isn't representable
/// and yields `None`, same as the unrepresentable PEP 440 specifiers in
/// `python.rs`'s `parse_version_requirement`.
fn to_versioning(
    constraint: &VersionConstraint,
    version: &Version,
) -> Option<versions::Requirement> {
    let op = match constraint {
        VersionConstraint::LessThan => Op::Less,
        VersionConstraint::LessThanEqual => Op::LessEq,
        VersionConstraint::Equal => Op::Exact,
        VersionConstraint::GreaterThan => Op::Greater,
        VersionConstraint::GreaterThanEqual => Op::GreaterEq,
        VersionConstraint::NotEqual => return None,
    };

    Some(versions::Requirement {
        op,
        version: Some(versions::Versioning::new(version.to_string())?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use versions::Requirement;

    #[tokio::test]
    async fn parse_description() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("../../testdata/test_DESCRIPTION");

        let packages = requirements_from_description(&path).await.unwrap().unwrap();

        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "dplyr");
        assert_eq!(
            packages[0].version,
            Some(Requirement::new(">=1.1.0").unwrap())
        );
        assert_eq!(packages[1].name, "ggplot2");
        assert_eq!(packages[1].version, None);
    }
}
