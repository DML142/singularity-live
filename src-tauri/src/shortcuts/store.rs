use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

use super::{BindingError, ShortcutBinding, ShortcutBindings, ShortcutPlatform, validate_bindings};

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum BindingStoreError {
    #[error("Shortcut configuration could not be read")]
    Read(#[source] io::Error),
    #[error("Shortcut configuration is invalid")]
    Parse(#[source] serde_json::Error),
    #[error("Shortcut configuration version is unsupported")]
    UnsupportedVersion,
    #[error("Shortcut configuration could not be written")]
    Write(#[source] io::Error),
    #[error(transparent)]
    InvalidBindings(#[from] BindingError),
}

/// Replaces a config file atomically so settings updates cannot truncate the prior file.
pub trait AtomicConfigWriter: Send + Sync {
    /// # Errors
    ///
    /// Returns an I/O error without changing the target when replacement fails.
    fn replace_atomically(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
}

struct TemporaryFileConfigWriter;

impl AtomicConfigWriter for TemporaryFileConfigWriter {
    fn replace_atomically(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "config path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(contents)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VersionedShortcutConfig {
    version: u32,
    bindings: Vec<ShortcutBinding>,
}

pub struct ShortcutConfigStore {
    path: Option<PathBuf>,
    platform: ShortcutPlatform,
    writer: Arc<dyn AtomicConfigWriter>,
}

impl ShortcutConfigStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, platform: ShortcutPlatform) -> Self {
        Self::with_writer(path, platform, Arc::new(TemporaryFileConfigWriter))
    }

    #[must_use]
    pub fn with_writer(
        path: impl Into<PathBuf>,
        platform: ShortcutPlatform,
        writer: Arc<dyn AtomicConfigWriter>,
    ) -> Self {
        Self {
            path: Some(path.into()),
            platform,
            writer,
        }
    }

    #[must_use]
    pub fn unavailable(platform: ShortcutPlatform) -> Self {
        Self {
            path: None,
            platform,
            writer: Arc::new(TemporaryFileConfigWriter),
        }
    }

    /// Loads and validates shortcut settings, using the platform defaults if absent.
    ///
    /// # Errors
    ///
    /// Returns a safe error for malformed, unsupported, invalid, or inaccessible config.
    pub fn load(&self) -> Result<Vec<ShortcutBinding>, BindingStoreError> {
        let Some(path) = &self.path else {
            return Err(BindingStoreError::Read(io::Error::other(
                "application config directory unavailable",
            )));
        };
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ShortcutBindings::defaults(self.platform));
            }
            Err(error) => return Err(BindingStoreError::Read(error)),
        };
        let config: VersionedShortcutConfig =
            serde_json::from_slice(&contents).map_err(BindingStoreError::Parse)?;
        if config.version != CONFIG_VERSION {
            return Err(BindingStoreError::UnsupportedVersion);
        }
        validate_bindings(&config.bindings)?;
        Ok(config.bindings)
    }

    /// Validates and atomically stores shortcut settings.
    ///
    /// # Errors
    ///
    /// Returns validation, serialization, or I/O failure without truncating the old config.
    pub fn save(&self, bindings: &[ShortcutBinding]) -> Result<(), BindingStoreError> {
        validate_bindings(bindings)?;
        let config = VersionedShortcutConfig {
            version: CONFIG_VERSION,
            bindings: bindings.to_vec(),
        };
        let contents = serde_json::to_vec(&config).map_err(BindingStoreError::Parse)?;
        let Some(path) = &self.path else {
            return Err(BindingStoreError::Write(io::Error::other(
                "application config directory unavailable",
            )));
        };
        self.writer
            .replace_atomically(path, &contents)
            .map_err(BindingStoreError::Write)
    }
}
