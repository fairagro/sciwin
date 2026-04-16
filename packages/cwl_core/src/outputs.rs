use super::{deserialize::Identifiable, types::CWLType};
use crate::{SingularPlural, types::SecondaryFileSchema};
use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::Value;

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutputParameter {
    #[serde(default)]
    pub id: String,
    pub type_: CWLType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_binding: Option<CommandOutputBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_files: Vec<SecondaryFileSchema>,
}

impl CommandOutputParameter {
    pub fn with_id<T: ToString + ?Sized>(mut self, id: &T) -> Self {
        self.id = id.to_string();
        self
    }
    pub fn with_type(mut self, type_: CWLType) -> Self {
        self.type_ = type_;
        self
    }
    pub fn with_binding(mut self, binding: CommandOutputBinding) -> Self {
        self.output_binding = Some(binding);
        self
    }
}

impl Identifiable for CommandOutputParameter {
    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }
}

pub fn deserialize_outputs<'de, D>(deserializer: D) -> Result<Vec<CommandOutputParameter>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Value = Deserialize::deserialize(deserializer)?;

    let parameters = match value {
        Value::Sequence(seq) => seq
            .into_iter()
            .map(|item| {
                let param: CommandOutputParameter = serde_yaml::from_value(item).map_err(serde::de::Error::custom)?;
                Ok(param)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Value::Mapping(map) => map
            .into_iter()
            .map(|(key, value)| {
                let id = key.as_str().ok_or_else(|| serde::de::Error::custom("Expected string key"))?;
                let param = if let Value::String(type_str) = value {
                    let type_ = serde_yaml::from_value::<CWLType>(Value::String(type_str)).map_err(serde::de::Error::custom)?;
                    CommandOutputParameter::default().with_id(id).with_type(type_)
                } else {
                    let mut param: CommandOutputParameter = serde_yaml::from_value(value).map_err(serde::de::Error::custom)?;
                    param.id = id.to_string();
                    param
                };

                Ok(param)
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(serde::de::Error::custom("Expected sequence or mapping for outputs")),
    };

    Ok(parameters)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutputBinding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glob: Option<SingularPlural<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_eval: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowOutputParameter {
    #[serde(default)]
    pub id: String,
    pub type_: CWLType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_source: Option<String>,
}

impl WorkflowOutputParameter {
    pub fn with_id<T: ToString + ?Sized>(&mut self, id: &T) -> &Self {
        self.id = id.to_string();
        self
    }
}

impl Identifiable for WorkflowOutputParameter {
    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_identifyable_output() {
        let mut output = CommandOutputParameter::default();
        assert_eq!(output.id(), "");
        output.set_id("test".to_string());
        assert_eq!(output.id(), "test");
    }

    #[test]
    pub fn test_identifyable_workflow_output() {
        let mut output = WorkflowOutputParameter::default();
        assert_eq!(output.id(), "");
        output.set_id("test".to_string());
        assert_eq!(output.id(), "test");
    }
}
