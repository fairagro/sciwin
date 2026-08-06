use crate::authoring::{AuthoringError, AuthoringResult};
use file_type::{FileType, format::SourceType};
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use tracing::debug;

// SWAGGER: https://api.terminology.tib.eu/swagger-ui/index.html
const ONTOLOGY_API_BASE: &str = "https://api.terminology.tib.eu/api/v2/ontologies/edam/entities";

#[derive(Deserialize, Debug)]
struct TSResponse {
    elements: Vec<TsElement>,
}

#[derive(Deserialize, Debug)]
struct TsElement {
    iri: String,
    definition: Vec<String>,
}

async fn query_edam_ontology_api(term: &str, strict: bool) -> AuthoringResult<Option<String>> {
    let encoded = urlencoding::encode(term);
    let fields = if strict {
        "&searchFields=label%20synonym"
    } else {
        ""
    };
    let url = format!("{ONTOLOGY_API_BASE}?search={encoded}{fields}");
    let client = Client::new();
    let response = client.get(&url).send().await?;
    let json = response.json::<TSResponse>().await?;

    let item = json.elements.into_iter().next();
    if let Some(item) = &item {
        debug!("Determined {term} to be {:#?}", item.definition.first());
    } else {
        debug!("Terminology Server resolved nothing for {term}");
    }

    Ok(item.map(|e| e.iri))
}

pub async fn find_edam_format(path: impl AsRef<Path>) -> AuthoringResult<String> {
    let file_type = find_file_type(&path)?;

    let ext_opt = path.as_ref().extension().and_then(|e| e.to_str());
    if let Some(ext) = ext_opt {
        let ext_lower = ext.to_lowercase();
        if let Some(iri) = query_edam_ontology_api(&ext_lower, true).await? {
            return Ok(iri);
        }

        //if no iri is found yet, we try the extensions of acquired mime-type
        if let Some(ft) = file_type {
            for alias in ft.extensions().iter().filter(|&&e| e != ext_lower) {
                if let Some(iri) = query_edam_ontology_api(alias, false).await? {
                    return Ok(iri);
                }
            }
        }
    }

    //if not found, fall back to the mimetype of the already-resolved file type
    Ok(extract_mimetype(file_type))
}

fn find_file_type(path: impl AsRef<Path>) -> AuthoringResult<Option<&'static FileType>> {
    if path.as_ref().exists() {
        let file_type = FileType::try_from_file(path.as_ref()).map_err(|e| {
            AuthoringError::Unknown(anyhow::anyhow!(
                "failed to detect file type of {}: {e}",
                path.as_ref().display()
            ))
        })?;
        return Ok(Some(file_type));
    }
    let Some(ext) = path.as_ref().extension().and_then(|e| e.to_str()) else {
        return Ok(None);
    };
    let candidates = FileType::from_extension(ext);
    Ok(candidates
        .iter()
        .find(|i| matches!(i.source_type(), SourceType::Httpd) && !i.media_types().is_empty())
        .or_else(|| candidates.iter().find(|i| !i.media_types().is_empty()))
        .copied())
}

fn extract_mimetype(file_type: Option<&FileType>) -> String {
    file_type
        .and_then(|ft| {
            ft.media_types()
                .iter()
                .find(|t| !t.contains("httpd"))
                .copied()
        }) // cgi mime types are usually not what we want
        .map(str::to_lowercase)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edam_resolution() {
        let probes = [
            "array_test.cwl",
            "create_dir.py",
            "file.txt",
            "archive.tar",
            "archive.tar.gz",
            "meta.json",
            "script.sh",
            "image.svg",
            "piv.jpg",
            "piv.jpeg",
            "noc.png",
            "db.sqlite3",
            "../../testdata/sample.db",
            "test.class.cs",
            "edam.owl",
        ];

        let expected = [
            "http://edamontology.org/format_3857", // CWL File
            "http://edamontology.org/format_3996", // python script
            "http://edamontology.org/format_2330", // textual format
            "http://edamontology.org/format_3981", // TAR format
            "http://edamontology.org/format_3989", // GZIP FORMAT
            "http://edamontology.org/format_3464", // JSON file
            "application/x-sh", // Shell script has no EDAM result, falls back to MIME
            "http://edamontology.org/format_3604", // SVG file
            "http://edamontology.org/format_3579", // JPG
            "http://edamontology.org/format_3579", // JPG
            "http://edamontology.org/format_3603", // PNG
            "http://edamontology.org/format_3621", // sqlite
            "http://edamontology.org/format_3621", // sqlite
            "text/x-csharp",    // C#, not in edam
            "http://edamontology.org/format_3262", // OWL XML
        ];

        for (ix, item) in probes.iter().enumerate() {
            let type_of_file = find_edam_format(item).await.unwrap();

            assert_eq!(type_of_file, expected[ix]);
        }
    }

    #[test]
    fn test_mime_resolution() {
        let probes = [
            "../../array_test.cwl",
            "../../create_dir.py",
            "../../file.txt",
            "archive.tar",
            "archive.tar.gz",
            "meta.json",
            "script.sh",
            "image.svg",
            "piv.jpg",
            "piv.jpeg",
            "noc.png",
            "../../testdata/sample.db",
            "test.class.cs",
            "edam.owl",
        ];

        let expected = [
            "application/cwl",
            "text/x-python",
            "text/plain",
            "application/x-tar",
            "application/gzip",
            "application/json",
            "application/x-sh",
            "image/svg+xml",
            "image/jpeg",
            "image/jpeg",
            "image/png",
            "application/vnd.sqlite3",
            "text/x-csharp",
            "application/owl+xml",
        ];

        for (ix, item) in probes.iter().enumerate() {
            let type_of_file = extract_mimetype(find_file_type(item).unwrap());
            assert_eq!(type_of_file, expected[ix]);
        }
    }
}
