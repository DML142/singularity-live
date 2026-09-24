use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Component, Path},
};

use thiserror::Error;

use super::manifest::{ContextManifest, DocumentManifest, VersionProbe};

const SUPPORTED_SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 32 * 1024;
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;
const MAX_PACK_BYTES: usize = 256 * 1024;
const MAX_DOCUMENTS: usize = 16;
const MAX_ID_BYTES: usize = 64;
const MAX_TITLE_BYTES: usize = 128;
const MAX_KEYWORD_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPack {
    id: String,
    name: String,
    pub(super) documents: Vec<LoadedDocument>,
}

impl ContextPack {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn document_count(&self) -> usize {
        self.documents.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadedDocument {
    pub id: String,
    pub title: String,
    pub content: String,
    pub always_include: bool,
    pub keywords: Vec<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextPackLoader;

impl ContextPackLoader {
    /// Loads and strictly validates a context pack directory.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the manifest, referenced paths, or content violate the
    /// versioned schema and safety limits.
    pub fn load(pack_directory: &Path) -> Result<ContextPack, ContextError> {
        Self::load_from(pack_directory, None)
    }

    /// Loads a context pack only when its resolved directory stays inside the supplied root.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the pack resolves outside `allowed_root` or violates the
    /// versioned schema and safety limits.
    pub fn load_beneath(
        allowed_root: &Path,
        pack_directory: &Path,
    ) -> Result<ContextPack, ContextError> {
        Self::load_from(pack_directory, Some(allowed_root))
    }

    fn load_from(
        pack_directory: &Path,
        allowed_root: Option<&Path>,
    ) -> Result<ContextPack, ContextError> {
        let pack_metadata =
            fs::symlink_metadata(pack_directory).map_err(|_| ContextError::MissingPackDirectory)?;
        if pack_metadata.file_type().is_symlink() {
            return Err(ContextError::PackDirectorySymlink);
        }
        let pack_root =
            fs::canonicalize(pack_directory).map_err(|_| ContextError::MissingPackDirectory)?;
        if !pack_root.is_dir() {
            return Err(ContextError::MissingPackDirectory);
        }
        if let Some(allowed_root) = allowed_root {
            let canonical_allowed_root =
                fs::canonicalize(allowed_root).map_err(|_| ContextError::MissingPackDirectory)?;
            if !pack_root.starts_with(canonical_allowed_root) {
                return Err(ContextError::PackOutsideAllowedRoot);
            }
        }
        let manifest = read_manifest(&pack_root)?;

        let mut ids = HashSet::new();
        let mut loaded_documents = Vec::with_capacity(manifest.documents.len());
        let mut total_bytes = 0_usize;

        for document in manifest.documents {
            if !ids.insert(document.id.clone()) {
                return Err(ContextError::DuplicateDocumentId {
                    document_id: document.id,
                });
            }
            loaded_documents.push(load_document(&pack_root, document, &mut total_bytes)?);
        }

        Ok(ContextPack {
            id: manifest.id,
            name: manifest.name,
            documents: loaded_documents,
        })
    }
}

fn read_manifest(pack_root: &Path) -> Result<ContextManifest, ContextError> {
    let manifest_path = pack_root.join("manifest.yaml");
    let canonical_path =
        fs::canonicalize(&manifest_path).map_err(|_| ContextError::MissingManifest)?;
    if !canonical_path.starts_with(pack_root) {
        return Err(ContextError::ManifestOutsidePack);
    }
    let manifest_metadata =
        fs::metadata(&canonical_path).map_err(|_| ContextError::MissingManifest)?;
    if !manifest_metadata.is_file() {
        return Err(ContextError::InvalidManifestFile);
    }
    if manifest_metadata.len() > MAX_MANIFEST_BYTES {
        return Err(ContextError::ManifestTooLarge {
            bytes: manifest_metadata.len(),
        });
    }
    let file = fs::File::open(&canonical_path).map_err(|_| ContextError::MissingManifest)?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ContextError::MalformedManifest {
            reason: "could not read manifest".to_owned(),
        })?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ContextError::ManifestTooLarge {
            bytes: bytes.len() as u64,
        });
    }
    let source = String::from_utf8(bytes).map_err(|_| ContextError::MalformedManifest {
        reason: "manifest is not valid UTF-8".to_owned(),
    })?;
    let version = serde_yaml_ng::from_str::<VersionProbe>(&source)
        .map_err(|error| malformed_manifest(&error))?;
    if version.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(ContextError::UnsupportedSchemaVersion {
            version: version.schema_version,
        });
    }
    let manifest = serde_yaml_ng::from_str::<ContextManifest>(&source)
        .map_err(|error| malformed_manifest(&error))?;
    validate_manifest_metadata(&manifest)?;
    Ok(manifest)
}

fn malformed_manifest(_error: &serde_yaml_ng::Error) -> ContextError {
    ContextError::MalformedManifest {
        reason: "invalid YAML syntax".to_owned(),
    }
}

fn validate_manifest_metadata(manifest: &ContextManifest) -> Result<(), ContextError> {
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(ContextError::UnsupportedSchemaVersion {
            version: manifest.schema_version,
        });
    }
    validate_text(&manifest.id, "pack id", MAX_ID_BYTES)
        .and_then(|()| validate_id(&manifest.id))
        .map_err(|_| ContextError::InvalidPackMetadata)?;
    validate_text(&manifest.name, "pack name", MAX_TITLE_BYTES)
        .map_err(|_| ContextError::InvalidPackMetadata)?;
    if manifest.documents.len() > MAX_DOCUMENTS {
        return Err(ContextError::TooManyDocuments {
            count: manifest.documents.len(),
        });
    }
    Ok(())
}

fn load_document(
    pack_root: &Path,
    document: DocumentManifest,
    total_bytes: &mut usize,
) -> Result<LoadedDocument, ContextError> {
    validate_text(&document.id, "document id", MAX_ID_BYTES)
        .and_then(|()| validate_id(&document.id))
        .map_err(|_| ContextError::InvalidDocumentMetadata {
            document_id: document.id.clone(),
        })?;
    validate_text(&document.title, "document title", MAX_TITLE_BYTES).map_err(|_| {
        ContextError::InvalidDocumentMetadata {
            document_id: document.id.clone(),
        }
    })?;
    validate_relative_markdown_path(&document.path)?;
    let canonical_path = canonical_document_path(pack_root, &document.path)?;
    let metadata = fs::metadata(&canonical_path).map_err(|_| ContextError::MissingDocument {
        path: document.path.clone(),
    })?;
    if !metadata.is_file() {
        return Err(ContextError::MissingDocument {
            path: document.path,
        });
    }
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(ContextError::DocumentTooLarge {
            path: document.path,
            bytes: metadata.len(),
        });
    }
    let bytes = fs::read(&canonical_path).map_err(|_| ContextError::MissingDocument {
        path: document.path.clone(),
    })?;
    *total_bytes = total_bytes
        .checked_add(bytes.len())
        .ok_or(ContextError::PackTooLarge { bytes: usize::MAX })?;
    if *total_bytes > MAX_PACK_BYTES {
        return Err(ContextError::PackTooLarge {
            bytes: *total_bytes,
        });
    }
    let content = String::from_utf8(bytes).map_err(|_| ContextError::InvalidUtf8 {
        path: document.path.clone(),
    })?;
    let keywords = validate_keywords(&document.id, document.keywords)?;
    Ok(LoadedDocument {
        id: document.id,
        title: document.title,
        content,
        always_include: document.always_include,
        keywords,
    })
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextError {
    #[error("Context pack directory does not exist")]
    MissingPackDirectory,
    #[error("Context pack directory must not be a symbolic link")]
    PackDirectorySymlink,
    #[error("Context pack resolves outside the application data directory")]
    PackOutsideAllowedRoot,
    #[error("Context pack manifest.yaml is missing")]
    MissingManifest,
    #[error("Context pack manifest.yaml resolves outside its pack")]
    ManifestOutsidePack,
    #[error("Context pack manifest.yaml must be a regular file")]
    InvalidManifestFile,
    #[error("Context pack manifest exceeds the 32 KiB limit ({bytes} bytes)")]
    ManifestTooLarge { bytes: u64 },
    #[error("Context pack manifest is malformed: {reason}")]
    MalformedManifest { reason: String },
    #[error("Context pack schema version {version} is not supported")]
    UnsupportedSchemaVersion { version: u32 },
    #[error("Context pack id or name is invalid")]
    InvalidPackMetadata,
    #[error("Context pack declares {count} documents; the limit is 16")]
    TooManyDocuments { count: usize },
    #[error("Context document {document_id} has invalid metadata")]
    InvalidDocumentMetadata { document_id: String },
    #[error("Context document id {document_id} is duplicated")]
    DuplicateDocumentId { document_id: String },
    #[error("Context document path {path} is invalid")]
    InvalidDocumentPath { path: String },
    #[error("Context document {path} is missing")]
    MissingDocument { path: String },
    #[error("Context document {path} resolves outside its pack")]
    DocumentOutsidePack { path: String },
    #[error("Context document {path} exceeds the 64 KiB limit ({bytes} bytes)")]
    DocumentTooLarge { path: String, bytes: u64 },
    #[error("Context pack content exceeds the 256 KiB limit ({bytes} bytes)")]
    PackTooLarge { bytes: usize },
    #[error("Context document {path} is not valid UTF-8")]
    InvalidUtf8 { path: String },
    #[error("Context document {document_id} repeats keyword {keyword}")]
    DuplicateKeyword {
        document_id: String,
        keyword: String,
    },
}

fn validate_relative_markdown_path(path_value: &str) -> Result<(), ContextError> {
    let path = Path::new(path_value);
    let normal_components = !path_value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    let markdown_extension = path.extension().and_then(|value| value.to_str()) == Some("md");
    if normal_components && markdown_extension {
        Ok(())
    } else {
        Err(ContextError::InvalidDocumentPath {
            path: path_value.to_owned(),
        })
    }
}

fn canonical_document_path(
    pack_root: &Path,
    path_value: &str,
) -> Result<std::path::PathBuf, ContextError> {
    let candidate = pack_root.join(path_value);
    let canonical = fs::canonicalize(&candidate).map_err(|_| ContextError::MissingDocument {
        path: path_value.to_owned(),
    })?;
    if !canonical.starts_with(pack_root) {
        return Err(ContextError::DocumentOutsidePack {
            path: path_value.to_owned(),
        });
    }
    Ok(canonical)
}

fn validate_keywords(
    document_id: &str,
    keywords: Vec<String>,
) -> Result<Vec<Vec<String>>, ContextError> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(keywords.len());
    for keyword in keywords {
        validate_text(&keyword, "keyword", MAX_KEYWORD_BYTES).map_err(|_| {
            ContextError::InvalidDocumentMetadata {
                document_id: document_id.to_owned(),
            }
        })?;
        let words = normalize_words(&keyword);
        if words.is_empty() {
            return Err(ContextError::InvalidDocumentMetadata {
                document_id: document_id.to_owned(),
            });
        }
        let comparable = words.join(" ");
        if !seen.insert(comparable.clone()) {
            return Err(ContextError::DuplicateKeyword {
                document_id: document_id.to_owned(),
                keyword: comparable,
            });
        }
        normalized.push(words);
    }
    Ok(normalized)
}

pub(super) fn normalize_words(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn validate_text(value: &str, kind: &'static str, max_bytes: usize) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        Err(kind)
    } else {
        Ok(())
    }
}

fn validate_id(value: &str) -> Result<(), &'static str> {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err("id")
    }
}
