//! First-run model download to `%LOCALAPPDATA%\Nexus\models`. The model is
//! never committed to git and is only fetched once.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::AssistantError;

pub const MODEL_FILE: &str = "Qwen3-0.6B-Q4_K_M.gguf";
pub const MODEL_URL: &str =
    "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q4_K_M.gguf";
pub const MODEL_BYTES: u64 = 396_705_472;
pub const MODEL_LABEL: &str = "Qwen3-0.6B Q4_K_M (Apache-2.0)";

pub fn models_dir() -> PathBuf {
    crate::audit::data_dir().join("models")
}

pub fn model_path() -> PathBuf {
    if let Some(p) = std::env::var_os("NEXUS_MODEL") {
        return PathBuf::from(p);
    }
    // The Windows installer bundles the model in `<install dir>\models`.
    if let Some(bundled) = bundled_model_path() {
        if bundled.exists() {
            return bundled;
        }
    }
    models_dir().join(MODEL_FILE)
}

/// `models\<MODEL_FILE>` beside the running executable, if it can be resolved.
pub fn bundled_model_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("models").join(MODEL_FILE))
}

pub fn is_installed() -> bool {
    std::fs::metadata(model_path())
        .map(|m| m.len() == MODEL_BYTES || std::env::var_os("NEXUS_MODEL").is_some())
        .unwrap_or(false)
}

/// Download with progress (0..1). Resumes nothing; writes `.part` then renames.
pub fn download(mut progress: impl FnMut(f32)) -> Result<PathBuf, AssistantError> {
    let dest = model_path();
    let dir = dest.parent().map(PathBuf::from).unwrap_or_else(models_dir);
    std::fs::create_dir_all(&dir).map_err(|e| AssistantError::Model(e.to_string()))?;
    let part = dest.with_extension("part");
    let resp = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .build()
        .get(MODEL_URL)
        .call()
        .map_err(|e| AssistantError::Network(e.to_string()))?;
    let mut reader = resp.into_reader();
    let mut file =
        std::fs::File::create(&part).map_err(|e| AssistantError::Model(e.to_string()))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| AssistantError::Network(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| AssistantError::Model(e.to_string()))?;
        done += n as u64;
        progress((done as f32 / MODEL_BYTES as f32).min(1.0));
    }
    drop(file);
    if done != MODEL_BYTES {
        let _ = std::fs::remove_file(&part);
        return Err(AssistantError::Model(format!(
            "download size mismatch ({done} bytes)"
        )));
    }
    std::fs::rename(&part, &dest).map_err(|e| AssistantError::Model(e.to_string()))?;
    Ok(dest)
}
