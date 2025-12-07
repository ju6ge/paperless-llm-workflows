use std::{fs::File, io::Read, path::Path};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) paperless_server: String,
    pub(crate) processing_tag: String,
    pub(crate) processing_color: String,
    pub(crate) correspondent_suggestions: bool,
    pub(crate) finished_tag: String,
    pub(crate) finished_color: String,
    pub(crate) tag_user_name: String,
    pub(crate) model: String,
    pub(crate) num_gpu_layers: usize,
    pub(crate) max_ctx: usize,
}

#[derive(Deserialize, Default)]
pub(crate) struct OverlayConfig {
    pub(crate) host: Option<String>,
    pub(crate) port: Option<u16>,
    pub(crate) paperless_server: Option<String>,
    pub(crate) processing_tag: Option<String>,
    pub(crate) processing_color: Option<String>,
    pub(crate) correspondent_suggestions: Option<bool>,
    pub(crate) finished_tag: Option<String>,
    pub(crate) finished_color: Option<String>,
    pub(crate) tag_user_name: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) num_gpu_layers: Option<usize>,
    pub(crate) max_ctx: Option<usize>,
}

#[derive(Debug, Error)]
enum OverlayConfigError {
    #[error(transparent)]
    ReadError(#[from] std::io::Error),
    #[error(transparent)]
    ParseError(#[from] toml::de::Error),
}

impl Config {
    pub fn new<S: ToString>(processing_tag: S, finished_tag: S, tag_user: S, model: S) -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8123,
            paperless_server: "https://example-paperless.domain".to_string(),
            processing_tag: processing_tag.to_string(),
            processing_color: "#ffe000".to_string(),
            correspondent_suggestions: false,
            finished_tag: finished_tag.to_string(),
            finished_color: "#40aebf".to_string(),
            tag_user_name: tag_user.to_string(),
            model: model.to_string(),
            num_gpu_layers: 1024,
            max_ctx: 0, // 0 will mean that per default max ctx train of the model will be used, this is potentially way to large
        }
    }

    pub fn overlay_config(self, overlay_config: OverlayConfig) -> Self {
        Self {
            host: overlay_config.host.unwrap_or(self.host),
            port: overlay_config.port.unwrap_or(self.port),
            paperless_server: overlay_config
                .paperless_server
                .unwrap_or(self.paperless_server),
            processing_tag: overlay_config.processing_tag.unwrap_or(self.processing_tag),
            processing_color: overlay_config
                .processing_color
                .unwrap_or(self.processing_color),
            correspondent_suggestions: overlay_config
                .correspondent_suggestions
                .unwrap_or(self.correspondent_suggestions),
            finished_tag: overlay_config.finished_tag.unwrap_or(self.finished_tag),
            finished_color: overlay_config.finished_color.unwrap_or(self.finished_color),
            tag_user_name: overlay_config.tag_user_name.unwrap_or(self.tag_user_name),
            model: overlay_config.model.unwrap_or(self.model),
            num_gpu_layers: overlay_config.num_gpu_layers.unwrap_or(self.num_gpu_layers),
            max_ctx: overlay_config.max_ctx.unwrap_or(self.max_ctx),
        }
    }
}

impl OverlayConfig {
    pub(crate) fn read_config_toml(config_file: &Path) -> OverlayConfig {
        match File::open(config_file)
            .map_err(OverlayConfigError::from)
            .and_then(|mut f| {
                let mut config_content = String::new();
                let _ = f.read_to_string(&mut config_content)?;
                Ok(config_content)
            })
            .and_then(|config_content| Ok(toml::from_str(&config_content)?))
        {
            Ok(overlay_config) => overlay_config,
            Err(err) => {
                log::error!("{err} … using default configuration");
                Self::default()
            }
        }
    }

    pub(crate) fn read_from_env() -> OverlayConfig {
        OverlayConfig {
            host: std::env::var("PAPERLESS_WEBHOOK_HOST").ok(),
            port: std::env::var("PAPERLESS_WEBHOOK_PORT")
                .ok()
                .and_then(|num| num.parse().ok()),
            paperless_server: std::env::var("PAPERLESS_SERVER").ok(),
            processing_tag: std::env::var("PROCESSING_TAG_NAME").ok(),
            processing_color: std::env::var("PROCESSING_TAG_COLOR").ok(),
            correspondent_suggestions: std::env::var("CORRESPONDENT_SUGGEST")
                .ok()
                .and_then(|v| v.parse().ok()),
            finished_tag: std::env::var("FINISHED_TAG_NAME").ok(),
            finished_color: std::env::var("FINSHED_TAG_COLOR").ok(),
            tag_user_name: std::env::var("PAPERLESS_USER").ok(),
            model: std::env::var("GGUF_MODEL_PATH").ok(),
            num_gpu_layers: std::env::var("NUM_GPU_LAYERS")
                .ok()
                .and_then(|num| num.parse().ok()),
            max_ctx: std::env::var("PAPERLESS_LLM_MAX_CTX")
                .ok()
                .and_then(|num| num.parse().ok()),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(
            "🧠 processing",
            "🏷️ finished",
            "user",
            "/usr/share/paperless-field-extractor/model.gguf",
        )
    }
}
