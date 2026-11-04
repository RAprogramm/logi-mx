// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Scope-aware ring state for the pie page: which ring the preview paints
//! and which request writes a change back to the daemon.

use std::collections::HashMap;

use logi_mx_daemon::ipc;
use logi_mx_driver::config::{PieConfig, PieProfile, PieSlot};

/// Which ring the page edits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PieScope {
    /// The global ring stored in [`PieConfig::sectors`].
    #[default]
    Global,
    /// A per-application profile keyed by the Hyprland window class.
    App(String)
}

/// Everything the page mirrors from the daemon: tuning, rings and scope.
#[derive(Debug, Default)]
pub struct PieStore {
    /// Global tuning including the global ring slots.
    pub config:   PieConfig,
    /// Per-application rings keyed by window class.
    pub profiles: HashMap<String, PieProfile>,
    /// The ring the editor currently targets.
    pub scope:    PieScope
}

impl PieStore {
    /// The ring visible in the current scope, in wedge order. An app scope
    /// without its own profile shows an empty ring: the daemon falls back to
    /// the global one at press time, and the page says so in the hint.
    #[must_use]
    pub fn visible_ring(&self) -> Vec<PieSlot> {
        match &self.scope {
            PieScope::Global => self.config.sectors.clone(),
            PieScope::App(app) => self
                .profiles
                .get(app)
                .map_or_else(Vec::new, |profile| profile.sectors.clone())
        }
    }

    /// The request that stores `ring` for the current scope.
    #[must_use]
    pub fn set_ring_request(&self, ring: &[PieSlot]) -> ipc::Request {
        match &self.scope {
            PieScope::Global => ipc::Request::SetPieSectors {
                sectors: ring.to_vec()
            },
            PieScope::App(app_id) => ipc::Request::SetAppPieSectors {
                app_id:  app_id.clone(),
                sectors: ring.to_vec()
            }
        }
    }

    /// Mirrors a successful ring write locally so the next repaint and the
    /// next scope switch stay accurate without another round-trip.
    pub fn apply_ring(&mut self, ring: Vec<PieSlot>) {
        match &self.scope {
            PieScope::Global => self.config.sectors = ring,
            PieScope::App(app_id) => {
                if ring.is_empty() {
                    self.profiles.remove(app_id);
                } else {
                    self.profiles.entry(app_id.clone()).or_default().sectors = ring;
                }
            }
        }
    }

    /// The profile app ids in display order.
    #[must_use]
    pub fn apps(&self) -> Vec<String> {
        let mut apps: Vec<String> = self.profiles.keys().cloned().collect();
        apps.sort();
        apps
    }

    /// The scope dropdown entries: every profile plus the app the page
    /// switched to before its first slot exists.
    #[must_use]
    pub fn scope_apps(&self) -> Vec<String> {
        let mut apps = self.apps();
        if let PieScope::App(app) = &self.scope
            && !apps.contains(app)
        {
            apps.push(app.clone());
        }
        apps.sort();
        apps
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
