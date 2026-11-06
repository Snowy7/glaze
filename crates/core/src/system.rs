//! Per-user preferences and the opt-in activity publisher, using core's existing
//! config notification and message loop. No consumer is paused in this slice.

use mypaper_config::Config;
use mypaper_tweaks::game_activity::GameWatcher;
use mypaper_tweaks::system::{SystemTweaks, writes};

pub struct SystemSettings {
    preferences: Option<SystemTweaks>,
    game: Option<GameWatcher>,
}

impl SystemSettings {
    pub fn new(config: &Config) -> Self {
        let mut settings = Self {
            preferences: SystemTweaks::new()
                .inspect_err(|error| log::warn!("system preferences unavailable: {error:#}"))
                .ok(),
            game: None,
        };
        settings.apply(config);
        settings
    }

    pub fn follow_config(&mut self) {
        match Config::load() {
            Ok(config) => self.apply(&config),
            Err(error) => log::warn!("system preferences unchanged: {error:#}"),
        }
    }

    fn apply(&mut self, config: &Config) {
        if writes(&config.system).is_empty() {
            // Startup still recovers a crashed run, then releases the unused controller.
            self.preferences = None;
        } else if self.preferences.is_none() {
            self.preferences = SystemTweaks::new()
                .inspect_err(|error| log::warn!("system preferences unavailable: {error:#}"))
                .ok();
        }
        if let Some(preferences) = &mut self.preferences
            && let Err(error) = preferences.apply(&config.system)
        {
            log::warn!("system preferences not applied: {error:#}");
        }
        if config.system.game_activity == Some(true) {
            if self.game.is_none() {
                self.game = GameWatcher::start()
                    .inspect_err(|error| log::warn!("game activity unavailable: {error:#}"))
                    .ok();
            }
        } else {
            self.game = None;
        }
    }

    /// Called on normal quit and panic; drops cover error exits as well.
    pub fn stop(&mut self) {
        self.game = None;
        if let Some(preferences) = &mut self.preferences
            && let Err(error) = preferences.restore()
        {
            log::error!("system preferences await next-start recovery: {error:#}");
        }
        self.preferences = None;
    }
}
