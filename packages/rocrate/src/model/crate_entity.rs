#[derive(Debug, Clone)]
pub struct RocrateMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub conforms_to: Vec<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}