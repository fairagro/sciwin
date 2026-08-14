use bon::Builder;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    hash::Hash,
};
use url::Url;
use versions::{Requirement, Versioning};

use crate::authoring::{AuthoringError, AuthoringResult};

const REGISTRY_BASE_URL: &str = "https://fairagro.github.io/sciwin-container-registry/api/";

pub type PackageId = String;
pub type ResolverPackageListResponse = HashMap<PackageId, HashMap<PackageVersion, Vec<String>>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PackageVersion {
    Parsed(Versioning),
    Raw(String),
}

impl Display for PackageVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parsed(v) => write!(f, "{v}"),
            Self::Raw(s) => write!(f, "{s}"),
        }
    }
}

impl Serialize for PackageVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PackageVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match Versioning::new(&s) {
            Some(v) => Self::Parsed(v),
            None => Self::Raw(s),
        })
    }
}

#[derive(Builder, Debug)]
pub struct Package {
    #[builder(into)]
    pub name: String,
    #[builder(into)]
    pub version: Option<Requirement>,
}
pub enum PackageType {
    Python,
    RPackage,
}

impl Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Self::Python => write!(f, "python"),
            Self::RPackage => write!(f, "R-Package"),
        }
    }
}

pub async fn resolve(
    packages: &[&Package],
    package_type: &PackageType,
) -> AuthoringResult<Vec<String>> {
    let base_url =
        Url::parse(REGISTRY_BASE_URL).map_err(|e| AuthoringError::from(anyhow::Error::from(e)))?;
    let endpoint = base_url
        .join("packages/")?
        .join(&format!("{package_type}.json"))?;
    let res = reqwest::get(endpoint).await?;
    let map = res.json::<ResolverPackageListResponse>().await?;

    resolve_packages_from_list(packages, &map)
}

/// Returns the build hashes shared by every requested package's matching
/// versions, i.e. only builds where all packages' requirements are met.
fn resolve_packages_from_list(
    packages: &[&Package],
    list: &ResolverPackageListResponse,
) -> AuthoringResult<Vec<String>> {
    let mut common: Option<HashSet<String>> = None;

    for p in packages {
        let matches: HashSet<String> = list
            .get(&p.name)
            .into_iter()
            .flat_map(|m| {
                m.iter().filter(|(k, _)| match &p.version {
                    Some(req) => match k {
                        PackageVersion::Parsed(versioning) => req.matches(versioning),
                        PackageVersion::Raw(raw) => *raw == req.to_string(),
                    },
                    None => true,
                })
            })
            .flat_map(|(_, value)| value.iter().cloned())
            .collect();

        common = Some(match common {
            Some(prev) => prev.intersection(&matches).cloned().collect(),
            None => matches,
        });

        if common.as_ref().is_some_and(HashSet::is_empty) {
            break;
        }
    }

    let mut matches = common.unwrap_or_default().into_iter().collect::<Vec<_>>();
    matches.sort();
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_simple() {
        let pandas = Package::builder().name("pandas").build();
        let packages = vec![&pandas];
        let matches = resolve(&packages, &PackageType::Python).await.unwrap();
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_resolve_check() {
        let list_raw = include_str!("../../testdata/resolver_response.json");
        let list = serde_json::from_str(list_raw).unwrap();

        let pandas = Package::builder().name("pandas").build();
        let packages = vec![&pandas];
        let matches = resolve_packages_from_list(&packages, &list).unwrap();

        assert_eq!(matches.len(), 20);
    }

    #[test]
    fn test_resolve_check_by_version() {
        let list_raw = include_str!("../../testdata/resolver_response.json");
        let list = serde_json::from_str(list_raw).unwrap();

        let pandas = Package::builder()
            .name("pandas")
            .maybe_version(Requirement::new("=3.0.5"))
            .build();
        let packages = vec![&pandas];
        let matches = resolve_packages_from_list(&packages, &list).unwrap();

        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_resolve_check_greater_than() {
        let list_raw = include_str!("../../testdata/resolver_response.json");
        let list = serde_json::from_str(list_raw).unwrap();

        let pandas = Package::builder()
            .name("pandas")
            .maybe_version(Requirement::new(">=3.0.4"))
            .build();
        let packages = vec![&pandas];
        let matches = resolve_packages_from_list(&packages, &list).unwrap();

        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_resolve_check_by_list() {
        let list_raw = include_str!("../../testdata/resolver_response.json");
        let list = serde_json::from_str(list_raw).unwrap();

        let pandas = Package::builder()
            .name("pandas")
            .maybe_version(Requirement::new(">=3.0.4"))
            .build();
        let matplotlib = Package::builder()
            .name("matplotlib")
            .maybe_version(Requirement::new(">=3.10.6"))
            .build();
        let scienceplots = Package::builder()
            .name("scienceplots")
            .maybe_version(Requirement::new(">=2"))
            .build();
        let packages = vec![&pandas, &matplotlib, &scienceplots];
        let matches = resolve_packages_from_list(&packages, &list).unwrap();
        assert_eq!(
            matches,
            vec!["3cb1d57f1c21df316e42441331e71ea4eb6272c0b4f699e35ede75b4b8dd027d"]
        );
    }

    #[test]
    fn test_deserializes_non_semver_calver_version() {
        let json = r#"{"pandas": {"2020.4.5.2": ["cp310"]}}"#;
        let parsed: ResolverPackageListResponse = serde_json::from_str(json).unwrap();
        assert!(
            parsed["pandas"]
                .keys()
                .any(|v| v.to_string() == "2020.4.5.2")
        );
    }

    #[test]
    fn test_deserializes_unparseable_version_as_raw() {
        let json = r#"{"broken-pkg": {"$VERSION": ["any"]}}"#;
        let parsed: ResolverPackageListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            parsed["broken-pkg"].keys().next(),
            Some(&PackageVersion::Raw("$VERSION".to_string()))
        );
    }
}
