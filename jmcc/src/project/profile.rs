//! Build profiles defined in `[profile.<name>]`.

use serde::{Deserialize, Serialize};

use crate::{CompileOptions, Target};

/// Configuration for a compilation profile (e.g. `[profile.dev]`, `[profile.release]`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProfileConfig {
    /// Optimization level (0..=3).
    #[serde(alias = "opt_level")]
    pub opt_level: Option<u8>,
    /// Passes to explicitly enable.
    pub passes: Option<Vec<String>>,
    /// Passes to explicitly disable.
    #[serde(alias = "disable_passes")]
    pub disable_passes: Option<Vec<String>>,
    /// Target platform name (e.g. "justmc").
    pub target: Option<String>,
    /// Artifact types to emit ("ast", "hir", "mir", "json").
    pub emit: Option<Vec<String>>,
    /// Locale/language for diagnostics ("ru" or "en").
    pub locale: Option<String>,
    /// Disable action line limit.
    #[serde(alias = "disable_action_limit")]
    pub disable_action_limit: Option<bool>,
    /// Target service for module uploading ("official" or "webhook").
    #[serde(
        alias = "upload_target",
        alias = "upload_method",
        alias = "upload-method"
    )]
    pub upload_target: Option<crate::upload::UploadTarget>,
    /// Custom webhook URL for unofficial module uploading.
    #[serde(alias = "webhook_url", alias = "webhook", alias = "upload_webhook")]
    pub webhook_url: Option<String>,
    /// Measure and display handler execution time in chat (`controller::measure_time`).
    #[serde(alias = "debug_time")]
    pub debug_time: Option<bool>,
    /// Minify emitted JSON module (compact representation without whitespace/newlines).
    pub minify: Option<bool>,
    /// Features to enable in this profile.
    pub features: Option<Vec<String>>,
}

impl ProfileConfig {
    /// Returns default profile settings for development (`dev`).
    #[must_use]
    pub fn default_dev() -> Self {
        Self {
            opt_level: Some(1),
            passes: None,
            disable_passes: None,
            target: Some("justmc".to_owned()),
            emit: Some(vec![
                "ast".to_owned(),
                "hir".to_owned(),
                "mir".to_owned(),
                "json".to_owned(),
            ]),
            locale: None,
            disable_action_limit: None,
            upload_target: None,
            webhook_url: None,
            debug_time: Some(true),
            minify: Some(false),
            features: None,
        }
    }

    /// Returns default profile settings for production (`release`).
    #[must_use]
    pub fn default_release() -> Self {
        Self {
            opt_level: Some(3),
            passes: None,
            disable_passes: None,
            target: Some("justmc".to_owned()),
            emit: Some(vec!["json".to_owned()]),
            locale: None,
            disable_action_limit: None,
            upload_target: None,
            webhook_url: None,
            debug_time: Some(false),
            minify: Some(true),
            features: None,
        }
    }

    /// Merges another profile on top of this one (overwriting fields that are `Some`).
    pub fn merge(&mut self, other: &Self) {
        if other.opt_level.is_some() {
            self.opt_level = other.opt_level;
        }
        if other.passes.is_some() {
            self.passes = other.passes.clone();
        }
        if other.disable_passes.is_some() {
            self.disable_passes = other.disable_passes.clone();
        }
        if other.target.is_some() {
            self.target = other.target.clone();
        }
        if other.emit.is_some() {
            self.emit = other.emit.clone();
        }
        if other.locale.is_some() {
            self.locale = other.locale.clone();
        }
        if other.disable_action_limit.is_some() {
            self.disable_action_limit = other.disable_action_limit;
        }
        if other.upload_target.is_some() {
            self.upload_target = other.upload_target;
        }
        if other.webhook_url.is_some() {
            self.webhook_url = other.webhook_url.clone();
        }
        if other.debug_time.is_some() {
            self.debug_time = other.debug_time;
        }
        if other.minify.is_some() {
            self.minify = other.minify;
        }
        if other.features.is_some() {
            self.features = other.features.clone();
        }
    }

    /// Applies profile configuration to standard `CompileOptions`.
    pub fn apply_to(&self, options: &mut CompileOptions) {
        if let Some(opt) = self.opt_level {
            options.opt_level = opt;
        }
        if let Some(passes) = &self.passes {
            options.passes = passes.clone();
        }
        if let Some(dis) = &self.disable_passes {
            options.disable_passes = dis.clone();
        }
        if let Some(target_str) = &self.target
            && target_str.eq_ignore_ascii_case("justmc")
        {
            options.target = Target::Justmc;
        }
        if let Some(emit) = &self.emit {
            options.emit_ast = emit.iter().any(|e| e == "ast");
            options.emit_hir = emit.iter().any(|e| e == "hir");
            options.emit_mir = emit.iter().any(|e| e == "mir");
            options.emit_json = emit.iter().any(|e| e == "json");
        }
        if let Some(loc) = &self.locale {
            options.locale = Some(loc.clone());
        }
        if let Some(dis) = self.disable_action_limit {
            options.disable_action_limit = dis;
        }
        if let Some(target) = self.upload_target {
            options.upload_target = Some(target);
        }
        if let Some(url) = &self.webhook_url {
            options.webhook_url = Some(url.clone());
        }
        if let Some(dt) = self.debug_time {
            options.debug_time = dt;
        }
        if let Some(m) = self.minify {
            options.minify = m;
        }
        if let Some(feats) = &self.features {
            options.features.extend(feats.iter().cloned());
        }
    }
}
