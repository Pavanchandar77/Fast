//! In-process local inference: Qwen3 GGUF (Q4_K_M) on CPU via candle.
//! Nothing here touches the network.

use std::path::Path;
use std::time::Instant;

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen3::ModelWeights;

use crate::tokenizer::Tokenizer;
use crate::AssistantError;

/// Result of one generation call, with real timing numbers.
#[derive(Clone, Debug, Default)]
pub struct Generation {
    pub text: String,
    pub prompt_tokens: usize,
    pub new_tokens: usize,
    pub prefill_secs: f32,
    pub decode_secs: f32,
}

impl Generation {
    pub fn tokens_per_sec(&self) -> f32 {
        if self.decode_secs <= 0.0 {
            0.0
        } else {
            self.new_tokens as f32 / self.decode_secs
        }
    }
}

pub struct LocalModel {
    base: ModelWeights,
    /// Cached system-prompt prefix: (prefix text, weights with KV filled, token count).
    prefix: Option<(String, ModelWeights, usize)>,
    tok: Tokenizer,
    stop_ids: Vec<u32>,
    device: Device,
}

fn str_array(ct: &gguf_file::Content, key: &str) -> Result<Vec<String>, AssistantError> {
    let v = ct
        .metadata
        .get(key)
        .ok_or_else(|| AssistantError::Model(format!("gguf missing {key}")))?;
    let arr = v
        .to_vec()
        .map_err(|e| AssistantError::Model(e.to_string()))?;
    Ok(arr
        .iter()
        .map(|x| x.to_string().cloned().unwrap_or_default())
        .collect())
}

impl LocalModel {
    pub fn load(path: &Path) -> Result<Self, AssistantError> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| AssistantError::Model(format!("open {}: {e}", path.display())))?;
        let ct = gguf_file::Content::read(&mut file)
            .map_err(|e| AssistantError::Model(e.to_string()))?;
        let tokens = str_array(&ct, "tokenizer.ggml.tokens")?;
        let merges = str_array(&ct, "tokenizer.ggml.merges")?;
        let tok = Tokenizer::new(tokens, merges);
        let device = Device::Cpu;
        let base = ModelWeights::from_gguf(ct, &mut file, &device)
            .map_err(|e| AssistantError::Model(e.to_string()))?;
        let stop_ids = ["<|im_end|>", "<|endoftext|>"]
            .iter()
            .filter_map(|t| tok.token_id(t))
            .collect();
        Ok(Self {
            base,
            prefix: None,
            tok,
            stop_ids,
            device,
        })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tok
    }

    /// Chat-style generation with Qwen3's no-think template.
    ///
    /// `prefill` is placed at the start of the assistant turn (used to force a
    /// `<tool_call>` JSON shape). `on_text` gets streamed text deltas and can
    /// return false to stop early. Generation also stops at any `stop` string.
    pub fn chat(
        &mut self,
        system: &str,
        user: &str,
        prefill: &str,
        max_new: usize,
        stop: &[&str],
        mut on_text: impl FnMut(&str) -> bool,
    ) -> Result<Generation, AssistantError> {
        let err = |e: candle_core::Error| AssistantError::Model(e.to_string());
        let prefix = format!("<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n");
        let rest =
            format!("{user}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n{prefill}");
        let started = Instant::now();

        let cached = matches!(&self.prefix, Some((p, _, _)) if *p == prefix);
        if !cached {
            let mut w = self.base.clone();
            w.clear_kv_cache();
            let ids = self.tok.encode(&prefix);
            let input = Tensor::new(ids.as_slice(), &self.device)
                .map_err(err)?
                .unsqueeze(0)
                .map_err(err)?;
            w.forward(&input, 0).map_err(err)?;
            self.prefix = Some((prefix.clone(), w, ids.len()));
        }
        let (_, prefix_w, prefix_len) = self.prefix.as_ref().expect("prefix cached");
        let mut model = prefix_w.clone();
        let mut pos = *prefix_len;

        let rest_ids = self.tok.encode(&rest);
        let input = Tensor::new(rest_ids.as_slice(), &self.device)
            .map_err(err)?
            .unsqueeze(0)
            .map_err(err)?;
        let mut logits = model.forward(&input, pos).map_err(err)?;
        pos += rest_ids.len();
        let prefill_secs = started.elapsed().as_secs_f32();

        let mut sampler = LogitsProcessor::from_sampling(7, Sampling::ArgMax);
        let mut out_ids: Vec<u32> = Vec::new();
        let mut emitted = 0usize;
        let decode_start = Instant::now();
        let mut text = String::new();
        for _ in 0..max_new {
            let l = logits.squeeze(0).map_err(err)?;
            let l = if out_ids.is_empty() {
                l
            } else {
                let start = out_ids.len().saturating_sub(64);
                candle_transformers::utils::apply_repeat_penalty(&l, 1.1, &out_ids[start..])
                    .map_err(err)?
            };
            let next = sampler.sample(&l).map_err(err)?;
            if self.stop_ids.contains(&next) {
                break;
            }
            out_ids.push(next);
            let bytes = self.tok.decode_bytes(&out_ids);
            let valid = match std::str::from_utf8(&bytes) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            text = String::from_utf8_lossy(&bytes[..valid]).into_owned();
            if valid > emitted {
                let delta = &text[emitted..];
                if !on_text(delta) {
                    break;
                }
                emitted = valid;
            }
            if stop.iter().any(|s| text.contains(s)) {
                break;
            }
            let input = Tensor::new(&[next], &self.device)
                .map_err(err)?
                .unsqueeze(0)
                .map_err(err)?;
            logits = model.forward(&input, pos).map_err(err)?;
            pos += 1;
        }
        let decode_secs = decode_start.elapsed().as_secs_f32();
        tracing::info!(
            "llm: prompt {} tok, prefill {:.2}s, {} new tok in {:.2}s ({:.1} tok/s), threads={}, avx2={}",
            rest_ids.len(),
            prefill_secs,
            out_ids.len(),
            decode_secs,
            out_ids.len() as f32 / decode_secs.max(0.001),
            candle_core::utils::get_num_threads(),
            candle_core::utils::with_avx(),
        );
        Ok(Generation {
            text,
            prompt_tokens: pos - out_ids.len(),
            new_tokens: out_ids.len(),
            prefill_secs,
            decode_secs: decode_start.elapsed().as_secs_f32(),
        })
    }
}
