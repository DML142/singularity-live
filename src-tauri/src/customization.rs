use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_WINDOW_OPACITY: u8 = 100;

#[derive(Clone)]
pub struct CustomizationService {
    path: Option<PathBuf>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CustomizationConfig {
    version: u32,
    window_opacity: u8,
}

impl CustomizationService {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
        }
    }

    #[must_use]
    pub fn unavailable() -> Self {
        Self { path: None }
    }

    /// Reads and validates the persisted app-window opacity.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing settings file is inaccessible or invalid.
    pub fn window_opacity(&self) -> Result<u8, String> {
        let Some(path) = &self.path else {
            return Ok(DEFAULT_WINDOW_OPACITY);
        };
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(DEFAULT_WINDOW_OPACITY);
            }
            Err(_) => return Err("Customization settings could not be read".to_owned()),
        };
        let config: CustomizationConfig = serde_json::from_slice(&contents)
            .map_err(|_| "Customization settings are invalid".to_owned())?;
        if config.version != CONFIG_VERSION
            || validate_window_opacity(config.window_opacity).is_err()
        {
            return Err("Customization settings are invalid".to_owned());
        }
        Ok(config.window_opacity)
    }

    /// Atomically stores a supported app-window opacity value.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is unsupported or the settings file cannot be saved.
    pub fn set_window_opacity(&self, opacity: u8) -> Result<u8, String> {
        validate_window_opacity(opacity)?;
        let Some(path) = &self.path else {
            return Err("The application settings directory is unavailable".to_owned());
        };
        let config = CustomizationConfig {
            version: CONFIG_VERSION,
            window_opacity: opacity,
        };
        let contents = serde_json::to_vec(&config)
            .map_err(|_| "Customization settings could not be saved".to_owned())?;
        replace_atomically(path, &contents)
            .map_err(|_| "Customization settings could not be saved".to_owned())?;
        Ok(opacity)
    }
}

fn validate_window_opacity(opacity: u8) -> Result<(), String> {
    if (40..=100).contains(&opacity) && opacity.is_multiple_of(5) {
        Ok(())
    } else {
        Err("Window opacity must be between 40 and 100 percent".to_owned())
    }
}

fn replace_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid settings path"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
}

#[cfg(test)]
mod tests {
    use super::{CustomizationService, DEFAULT_WINDOW_OPACITY};

    #[test]
    fn uses_full_opacity_when_no_customization_file_exists() {
        let directory = tempfile::tempdir().expect("temporary settings directory");
        let settings = CustomizationService::new(directory.path().join("customization.json"));

        assert_eq!(settings.window_opacity(), Ok(DEFAULT_WINDOW_OPACITY));
    }

    #[test]
    fn saves_and_loads_supported_opacity_steps() {
        let directory = tempfile::tempdir().expect("temporary settings directory");
        let settings = CustomizationService::new(directory.path().join("customization.json"));

        assert_eq!(settings.set_window_opacity(65), Ok(65));
        assert_eq!(settings.window_opacity(), Ok(65));
    }

    #[test]
    fn rejects_opacity_values_outside_the_slider_range_or_step() {
        let directory = tempfile::tempdir().expect("temporary settings directory");
        let settings = CustomizationService::new(directory.path().join("customization.json"));

        assert!(settings.set_window_opacity(39).is_err());
        assert!(settings.set_window_opacity(63).is_err());
        assert!(settings.set_window_opacity(101).is_err());
    }
}
