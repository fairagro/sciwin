use anyhow::Context;
use reqwest::{
    blocking::{Client, Response},
    header::{CONTENT_TYPE, HeaderMap},
};
use serde_json::Value;
use std::{collections::HashMap, fmt::Display};

pub struct Reana {
    server: String,
    token: String,
}

impl Reana {
    pub fn new(server: String, token: String) -> Self {
        Self { server, token }
    }

    fn url(&self, endpoint: &WorkflowEndpoint, params: Option<HashMap<String, String>>) -> String {
        let mut url = format!("{}/api/{endpoint}?access_token={}", self.server, self.token);

        if let Some(params) = params {
            for (key, value) in params {
                url.push_str(&format!("&{key}={value}"));
            }
        }
        url
    }

    pub fn post(&self, endpoint: &WorkflowEndpoint, body: Content, params: Option<HashMap<String, String>>) -> anyhow::Result<Response> {
        let mut headers = HeaderMap::new();

        headers.insert(
            CONTENT_TYPE,
            match body {
                Content::Json(_) => "application/json",
                Content::OctetStream(_) => "application/octet-stream",
            }
            .parse()?,
        );

        let client = Client::builder().default_headers(headers).build()?;
        let url = self.url(endpoint, params);
        match body {
            Content::Json(json) => client.post(&url).json(&json).send()?.error_for_status(),
            Content::OctetStream(file) => client.post(&url).body(file).send()?.error_for_status(),
        }
        .with_context(|| format!("❌ Failed to send POST request to URL: {url}"))
    }

    pub fn get(&self, endpoint: &WorkflowEndpoint) -> anyhow::Result<Response> {
        let client = Client::builder().build()?;
        let url = self.url(endpoint, None);
        client
            .get(&url)
            .send()
            .with_context(|| format!("❌ Failed to send GET request to URL: {url}"))
    }

    pub fn ping(&self) -> anyhow::Result<Response> {
        let ping_url = format!("{}/api/ping", self.server);
        let client = Client::builder()
            .build()
            .context("Failed to build HTTP client")?;

        client
            .get(&ping_url)
            .send()
            .with_context(|| format!("❌ Failed to send GET request to URL: {ping_url}"))
    }
}

pub enum WorkflowEndpoint<'a> {
    Root,
    Start(&'a str),
    Logs(&'a str),
    Status(&'a str),
    Specification(&'a str),
    Workspace(&'a str, Option<String>),
}

impl<'a> Display for WorkflowEndpoint<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root => write!(f, "workflows"),
            Self::Start(workflow) => write!(f, "workflows/{workflow}/start"),
            Self::Logs(workflow) => write!(f, "workflows/{workflow}/logs"),
            Self::Status(workflow) => write!(f, "workflows/{workflow}/status"),
            Self::Specification(workflow) => write!(f, "workflows/{workflow}/specification"),
            Self::Workspace(workflow, file) => {
                if let Some(file) = file {
                    write!(f, "workflows/{workflow}/workspace/{file}")
                } else {
                    write!(f, "workflows/{workflow}/workspace")
                }
            }
        }
    }
}

pub enum Content {
    Json(Value),
    OctetStream(Vec<u8>),
}
