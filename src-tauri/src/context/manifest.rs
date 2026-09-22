use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub documents: Vec<DocumentManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DocumentManifest {
    pub id: String,
    pub title: String,
    pub path: String,
    pub always_include: bool,
    pub keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct VersionProbe {
    pub schema_version: u32,
}
