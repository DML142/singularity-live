use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::domain::ContextDocument;

const SETTINGS_VERSION: u32 = 1;
const MAX_USER_FILES: usize = 8;
const MAX_USER_CONTEXT_BYTES: usize = 12 * 1024;
const MAX_USER_FILE_BYTES: usize = 12 * 1024;
const MAX_USER_FILE_BYTES_U64: u64 = 12 * 1024;
const MAX_FILE_NAME_BYTES: usize = 128;
const MAX_SETTINGS_BYTES: usize = 128 * 1024;
const MAX_SETTINGS_BYTES_U64: u64 = 128 * 1024;

pub struct UserContextService {
    path: Option<PathBuf>,
    lock: Mutex<()>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserContextFileInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredUserContextFile {
    id: String,
    name: String,
    content: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UserContextSettings {
    version: u32,
    files: Vec<StoredUserContextFile>,
}

impl UserContextService {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            lock: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            path: None,
            lock: Mutex::new(()),
        }
    }

    /// Returns user-added document names without exposing their contents to the webview.
    ///
    /// # Errors
    ///
    /// Returns an error when saved context settings cannot be read or validated.
    pub fn list(&self) -> Result<Vec<UserContextFileInfo>, String> {
        let _lock = self.lock();
        let settings = read_settings(self.path()?)?;
        Ok(settings
            .files
            .iter()
            .map(UserContextFileInfo::from)
            .collect())
    }

    /// Imports selected Markdown or text files and persists their contents in app data.
    ///
    /// # Errors
    ///
    /// Returns an error when files are unsupported, invalid, too large, or cannot be saved.
    pub fn add_files(&self, paths: &[PathBuf]) -> Result<Vec<UserContextFileInfo>, String> {
        if paths.is_empty() {
            return self.list();
        }
        let _lock = self.lock();
        let path = self.path()?;
        let mut settings = read_settings(path)?;
        if settings.files.len().saturating_add(paths.len()) > MAX_USER_FILES {
            return Err(format!("You can add up to {MAX_USER_FILES} context files"));
        }

        let mut added_bytes = settings
            .files
            .iter()
            .map(|file| file.content.len())
            .sum::<usize>();
        for path in paths {
            let file = read_imported_file(path)?;
            added_bytes = added_bytes
                .checked_add(file.content.len())
                .ok_or_else(|| "Combined context files exceed 12 KiB".to_owned())?;
            if added_bytes > MAX_USER_CONTEXT_BYTES {
                return Err("Combined context files exceed 12 KiB".to_owned());
            }
            settings.files.push(file);
        }

        save_settings(path, &settings)?;
        Ok(settings
            .files
            .iter()
            .map(UserContextFileInfo::from)
            .collect())
    }

    /// Removes one user-added context document by its opaque ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the file does not exist or settings cannot be saved.
    pub fn remove_file(&self, id: &str) -> Result<Vec<UserContextFileInfo>, String> {
        let _lock = self.lock();
        let path = self.path()?;
        let mut settings = read_settings(path)?;
        let previous_len = settings.files.len();
        settings.files.retain(|file| file.id != id);
        if settings.files.len() == previous_len {
            return Err("The context file was not found".to_owned());
        }
        save_settings(path, &settings)?;
        Ok(settings
            .files
            .iter()
            .map(UserContextFileInfo::from)
            .collect())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn path(&self) -> Result<&Path, String> {
        self.path
            .as_deref()
            .ok_or_else(|| "The application data directory is unavailable".to_owned())
    }
}

impl From<&StoredUserContextFile> for UserContextFileInfo {
    fn from(value: &StoredUserContextFile) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
        }
    }
}

pub(crate) fn load_context_documents(path: &Path) -> Result<Vec<ContextDocument>, String> {
    let settings = read_settings(path)?;
    Ok(settings
        .files
        .into_iter()
        .map(|file| ContextDocument {
            id: format!("user-{}", file.id),
            title: file.name,
            content: file.content,
        })
        .collect())
}

fn read_imported_file(path: &Path) -> Result<StoredUserContextFile, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("md" | "txt")) {
        return Err("Only Markdown (.md) and text (.txt) files are supported".to_owned());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "The selected context file could not be read".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("The selected context file must be a regular file".to_owned());
    }
    if metadata.len() > MAX_USER_FILE_BYTES_U64 {
        return Err("Each context file must be 12 KiB or smaller".to_owned());
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && value.len() <= MAX_FILE_NAME_BYTES)
        .filter(|value| !value.chars().any(char::is_control))
        .ok_or_else(|| "The selected context file has an invalid name".to_owned())?
        .to_owned();
    let mut content = Vec::new();
    fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_USER_FILE_BYTES_U64 + 1)
                .read_to_end(&mut content)
        })
        .map_err(|_| "The selected context file could not be read".to_owned())?;
    if content.len() > MAX_USER_FILE_BYTES {
        return Err("Each context file must be 12 KiB or smaller".to_owned());
    }
    let content = String::from_utf8(content)
        .map_err(|_| "Context files must contain valid UTF-8 text".to_owned())?;
    if content.trim().is_empty() {
        return Err("Context files cannot be empty".to_owned());
    }
    Ok(StoredUserContextFile {
        id: Uuid::new_v4().to_string(),
        name,
        content,
    })
}

fn read_settings(path: &Path) -> Result<UserContextSettings, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(default_settings());
        }
        Err(_) => return Err("Context file settings could not be read".to_owned()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_SETTINGS_BYTES_U64
    {
        return Err("Context file settings are invalid".to_owned());
    }
    let mut contents = Vec::new();
    fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_SETTINGS_BYTES_U64 + 1)
                .read_to_end(&mut contents)
        })
        .map_err(|_| "Context file settings could not be read".to_owned())?;
    if contents.len() > MAX_SETTINGS_BYTES {
        return Err("Context file settings are invalid".to_owned());
    }
    let settings: UserContextSettings = serde_json::from_slice(&contents)
        .map_err(|_| "Context file settings are invalid".to_owned())?;
    validate_settings(&settings)?;
    Ok(settings)
}

fn validate_settings(settings: &UserContextSettings) -> Result<(), String> {
    if settings.version != SETTINGS_VERSION || settings.files.len() > MAX_USER_FILES {
        return Err("Context file settings are invalid".to_owned());
    }
    let mut total_bytes = 0_usize;
    for file in &settings.files {
        if Uuid::parse_str(&file.id).is_err()
            || file.name.is_empty()
            || file.name.len() > MAX_FILE_NAME_BYTES
            || file.name.chars().any(char::is_control)
            || !matches!(
                Path::new(&file.name)
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("md" | "txt")
            )
            || file.content.trim().is_empty()
            || file.content.len() > MAX_USER_FILE_BYTES
        {
            return Err("Context file settings are invalid".to_owned());
        }
        total_bytes = total_bytes
            .checked_add(file.content.len())
            .ok_or_else(|| "Context file settings exceed 12 KiB".to_owned())?;
        if total_bytes > MAX_USER_CONTEXT_BYTES {
            return Err("Context file settings exceed 12 KiB".to_owned());
        }
    }
    Ok(())
}

fn save_settings(path: &Path, settings: &UserContextSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The application data directory is unavailable".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|_| "Context file settings could not be saved".to_owned())?;
    let contents = serde_json::to_vec(settings)
        .map_err(|_| "Context file settings could not be saved".to_owned())?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|_| "Context file settings could not be saved".to_owned())?;
    temporary
        .write_all(&contents)
        .map_err(|_| "Context file settings could not be saved".to_owned())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Context file settings could not be saved".to_owned())?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|_| "Context file settings could not be saved".to_owned())
}

fn default_settings() -> UserContextSettings {
    UserContextSettings {
        version: SETTINGS_VERSION,
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{UserContextService, load_context_documents};

    #[test]
    fn imports_lists_and_removes_markdown_and_text_files() {
        let directory = tempfile::tempdir().expect("temporary context directory");
        let source_markdown = directory.path().join("notes.md");
        let source_text = directory.path().join("facts.txt");
        fs::write(&source_markdown, "## Project\nUse Rust.").expect("write Markdown");
        fs::write(&source_text, "Keep the API typed.").expect("write text file");
        let service = UserContextService::new(directory.path().join("user-context.json"));

        let files = service
            .add_files(&[source_markdown, source_text])
            .expect("context files import");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "notes.md");
        assert_eq!(files[1].name, "facts.txt");
        let documents = load_context_documents(&directory.path().join("user-context.json"))
            .expect("context files load");
        assert_eq!(documents[0].content, "## Project\nUse Rust.");

        let remaining = service
            .remove_file(&files[0].id)
            .expect("context file removal");
        assert_eq!(remaining, vec![files[1].clone()]);
    }

    #[test]
    fn rejects_unsupported_large_and_invalid_utf8_files() {
        let directory = tempfile::tempdir().expect("temporary context directory");
        let unsupported = directory.path().join("notes.rtf");
        let oversized = directory.path().join("large.txt");
        let invalid_utf8 = directory.path().join("binary.md");
        fs::write(&unsupported, "not supported").expect("write unsupported file");
        fs::write(&oversized, "x".repeat(12 * 1024 + 1)).expect("write large file");
        fs::write(&invalid_utf8, [0xff, 0xfe]).expect("write invalid UTF-8 file");
        let service = UserContextService::new(directory.path().join("user-context.json"));

        assert!(service.add_files(&[unsupported]).is_err());
        assert!(service.add_files(&[oversized]).is_err());
        assert!(service.add_files(&[invalid_utf8]).is_err());
        assert!(
            service
                .list()
                .expect("context list remains readable")
                .is_empty()
        );
    }
}
