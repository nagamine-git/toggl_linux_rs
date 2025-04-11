use crate::config::GoogleCalendarConfig;
use crate::data_collector::CalendarEvent;
use anyhow::Result;
use chrono::{DateTime, Utc};

pub struct GoogleCalendarClient {
    config: Option<GoogleCalendarConfig>,
}

impl GoogleCalendarClient {
    pub fn new() -> Self {
        Self {
            config: None,
        }
    }

    pub fn with_config(config: GoogleCalendarConfig) -> Self {
        Self {
            config: Some(config),
        }
    }

    pub async fn get_current_events(&self) -> Result<Vec<CalendarEvent>> {
        if let Some(config) = &self.config {
            crate::data_collector::get_calendar_events(config).await
        } else {
            Ok(Vec::new())
        }
    }
} 