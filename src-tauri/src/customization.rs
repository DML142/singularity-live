use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::capture::CaptureTargetKind;

const CONFIG_VERSION: u32 = 2;
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
    #[serde(default)]
    close_window_on_screenshot: bool,
    #[serde(default = "default_capture_target_kind")]
    screenshot_target_kind: CaptureTargetKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScreenshotPreferences {
    pub close_window_on_screenshot: bool,
    pub target_kind: CaptureTargetKind,
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
        Ok(self.load_config()?.window_opacity)
    }

    /// Atomically stores a supported app-window opacity value.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is unsupported or the settings file cannot be saved.
    pub fn set_window_opacity(&self, opacity: u8) -> Result<u8, String> {
        validate_window_opacity(opacity)?;
        let mut config = self.load_config()?;
        config.window_opacity = opacity;
        self.save_config(config)?;
        Ok(opacity)
    }

    /// Reads screenshot lifecycle and capture-target preferences.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing settings file is inaccessible or invalid.
    pub fn screenshot_preferences(&self) -> Result<ScreenshotPreferences, String> {
        let config = self.load_config()?;
        Ok(ScreenshotPreferences {
            close_window_on_screenshot: config.close_window_on_screenshot,
            target_kind: config.screenshot_target_kind,
        })
    }

    /// Saves screenshot capture preferences while preserving the other customization values.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings file cannot be read or saved.
    pub fn set_screenshot_preferences(
        &self,
        preferences: ScreenshotPreferences,
    ) -> Result<ScreenshotPreferences, String> {
        let mut config = self.load_config()?;
        config.close_window_on_screenshot = preferences.close_window_on_screenshot;
        config.screenshot_target_kind = preferences.target_kind;
        self.save_config(config)?;
        Ok(preferences)
    }

    fn load_config(&self) -> Result<CustomizationConfig, String> {
        let Some(path) = &self.path else {
            return Ok(default_config());
        };
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(default_config()),
            Err(_) => return Err("Customization settings could not be read".to_owned()),
        };
        let config: CustomizationConfig = serde_json::from_slice(&contents)
            .map_err(|_| "Customization settings are invalid".to_owned())?;
        if !matches!(config.version, 1 | CONFIG_VERSION)
            || validate_window_opacity(config.window_opacity).is_err()
        {
            return Err("Customization settings are invalid".to_owned());
        }
        Ok(config)
    }

    fn save_config(&self, mut config: CustomizationConfig) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Err("The application settings directory is unavailable".to_owned());
        };
        config.version = CONFIG_VERSION;
        let contents = serde_json::to_vec(&config)
            .map_err(|_| "Customization settings could not be saved".to_owned())?;
        replace_atomically(path, &contents)
            .map_err(|_| "Customization settings could not be saved".to_owned())
    }
}

fn default_config() -> CustomizationConfig {
    CustomizationConfig {
        version: CONFIG_VERSION,
        window_opacity: DEFAULT_WINDOW_OPACITY,
        close_window_on_screenshot: false,
        screenshot_target_kind: default_capture_target_kind(),
    }
}

fn default_capture_target_kind() -> CaptureTargetKind {
    CaptureTargetKind::Monitor
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
