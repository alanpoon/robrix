//! Functions for loading and saving Crew settings to persistent storage.

use makepad_widgets::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};
use crate::app_data_dir;

const CREW_STATE_FILE_NAME: &str = "crew_state.json";

/// Returns the static reference to the Crew settings.
pub fn crew_settings() -> &'static Arc<Mutex<CrewSettings>> {
    static CREW_SETTINGS: OnceLock<Arc<Mutex<CrewSettings>>> = OnceLock::new();
    CREW_SETTINGS.get_or_init(|| Arc::new(Mutex::new(CrewSettings::default())))
}

/// The Crew settings that are saved to persistent storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrewSettings {
    /// The base URL for the Crew API endpoint.
    /// Default: "http://localhost:8080"
    pub base_url: String,
    /// The authorization token for the Crew API.
    /// Default: "Bearer my-secret"
    pub authorization: String,
}

impl Default for CrewSettings {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            authorization: "Bearer my-secret".to_string(),
        }
    }
}

impl CrewSettings {
    /// Returns the full API endpoint URL by combining base_url with the /api/chat path.
    pub fn api_endpoint(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
    }
}

/// Loads the Crew settings from persistent storage.
pub async fn load_crew_settings() -> anyhow::Result<CrewSettings> {
    let content = match tokio::fs::read_to_string(
        app_data_dir().join(CREW_STATE_FILE_NAME)
    ).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log!("No saved Crew settings found, using defaults.");
            return Ok(CrewSettings::default());
        }
        Err(e) => return Err(e.into())
    };
    serde_json::from_str(&content)
        .map_err(anyhow::Error::msg)
}

/// Asynchronously save the Crew settings to persistent storage.
pub async fn save_crew_settings_async(settings: CrewSettings) -> anyhow::Result<()> {
    let path = app_data_dir().join(CREW_STATE_FILE_NAME);
    tokio::fs::write(path, serde_json::to_string_pretty(&settings)?).await?;
    log!("Successfully saved Crew settings to persistent storage.");
    Ok(())
}

/// Synchronously save the Crew settings to persistent storage.
pub fn save_crew_settings(settings: CrewSettings) -> anyhow::Result<()> {
    let path = app_data_dir().join(CREW_STATE_FILE_NAME);
    std::fs::write(path, serde_json::to_string_pretty(&settings)?)?;
    log!("Successfully saved Crew settings to persistent storage.");
    Ok(())
}
