//! Byte-level BPE tokenizer built straight from GGUF metadata
//! (`tokenizer.ggml.tokens` / `tokenizer.ggml.merges`), so the model file is
//! self-contained and no extra tokenizer download or native regex lib is needed.

use std::collections::HashMap;

/// GPT-2 style byte <-> unicode mapping used by Qwen's byte-level BPE.
fn byte_to_unicode() -> [char; 256] {
    let mut table = ['\0'; 256];
    let mut n = 0u32;
    for b in 0..256u32 {
        let printable =
            (33..=126).contains(&b) || (161..=172).contains(&b) || (174..=255).contains(&b);
        table[b as usize] = if printable {
            char::from_u32(b).unwrap_or('?')
        } else {
            let c = char::from_u32(256 + n).unwrap_or('?');
            n += 1;
            c
        };
    }
    table
}

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    id_to_token: Vec<String>,
    ranks: HashMap<(String, String), usize>,
    specials: Vec<(String, u32)>,
    b2u: [char; 256],
    u2b: HashMap<char, u8>,
}

impl Tokenizer {
    pub fn new(tokens: Vec<String>, merges: Vec<String>) -> Self {
        let mut vocab = HashMap::with_capacity(tokens.len());
        for (i, t) in tokens.iter().enumerate() {
            vocab.insert(t.clone(), i as u32);
        }
        let mut ranks = HashMap::with_capacity(merges.len());
        for (i, m) in merges.iter().enumerate() {
            if let Some((a, b)) = m.split_once(' ') {
                ranks.insert((a.to_string(), b.to_string()), i);
            }
        }
        let mut specials: Vec<(String, u32)> = tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                (t.starts_with("<|") && t.ends_with("|>"))
                    || t.as_str() == "<think>"
                    || t.as_str() == "</think>"
                    || t.as_str() == "<tool_call>"
                    || t.as_str() == "</tool_call>"
            })
            .map(|(i, t)| (t.clone(), i as u32))
            .collect();
        // Longest first so overlapping specials resolve greedily.
        specials.sort_by_key(|s| std::cmp::Reverse(s.0.len()));
        let b2u = byte_to_unicode();
        let u2b = b2u.iter().enumerate().map(|(b, c)| (*c, b as u8)).collect();
        Self {
            vocab,
            id_to_token: tokens,
            ranks,
            specials,
            b2u,
            u2b,
        }
    }

    pub fn token_id(&self, token: &str) -> Option<u32> {
        self.vocab.get(token).copied()
    }

    pub fn vocab_size(&self) -> usize {
        self.id_to_token.len()
    }

    /// Encode text, treating `<|...|>`-style special tokens atomically.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let next = self
                .specials
                .iter()
                .filter_map(|(s, id)| rest.find(s.as_str()).map(|pos| (pos, s, *id)))
                .min_by_key(|(pos, s, _)| (*pos, std::cmp::Reverse(s.len())));
            match next {
                Some((pos, s, id)) => {
                    self.encode_plain(&rest[..pos], &mut out);
                    out.push(id);
                    rest = &rest[pos + s.len()..];
                }
                None => {
                    self.encode_plain(rest, &mut out);
                    break;
                }
            }
        }
        out
    }

    fn encode_plain(&self, text: &str, out: &mut Vec<u32>) {
        for piece in pretokenize(text) {
            let mapped: Vec<String> = piece
                .bytes()
                .map(|b| self.b2u[b as usize].to_string())
                .collect();
            for sym in self.bpe(mapped) {
                if let Some(id) = self.vocab.get(&sym) {
                    out.push(*id);
                } else {
                    // Fall back to per-byte tokens; every byte symbol is in the vocab.
                    for ch in sym.chars() {
                        if let Some(id) = self.vocab.get(&ch.to_string()) {
                            out.push(*id);
                        }
                    }
                }
            }
        }
    }

    fn bpe(&self, mut parts: Vec<String>) -> Vec<String> {
        while parts.len() > 1 {
            let mut best: Option<(usize, usize)> = None;
            for i in 0..parts.len() - 1 {
                if let Some(rank) = self.ranks.get(&(parts[i].clone(), parts[i + 1].clone())) {
                    if best.is_none_or(|(r, _)| *rank < r) {
                        best = Some((*rank, i));
                    }
                }
            }
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts[i] = merged;
            parts.remove(i + 1);
        }
        parts
    }

    /// Decode ids to raw bytes (may contain partial UTF-8 at the tail).
    pub fn decode_bytes(&self, ids: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for id in ids {
            let Some(tok) = self.id_to_token.get(*id as usize) else {
                continue;
            };
            if self.specials.iter().any(|(_, s)| s == id) {
                bytes.extend_from_slice(tok.as_bytes());
                continue;
            }
            for ch in tok.chars() {
                match self.u2b.get(&ch) {
                    Some(b) => bytes.push(*b),
                    None => {
                        let mut buf = [0u8; 4];
                        bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                }
            }
        }
        bytes
    }

    pub fn decode(&self, ids: &[u32]) -> String {
        String::from_utf8_lossy(&self.decode_bytes(ids)).into_owned()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Letter,
    Digit,
    Newline,
    Space,
    Other,
}

fn class(c: char) -> Class {
    if c == '\n' || c == '\r' {
        Class::Newline
    } else if c.is_whitespace() {
        Class::Space
    } else if c.is_alphabetic() {
        Class::Letter
    } else if c.is_numeric() {
        Class::Digit
    } else {
        Class::Other
    }
}

/// Hand-written approximation of the Qwen2/3 pre-tokenizer regex:
/// optional single leading space/punct + letters, single digits,
/// optional space + punctuation runs, newline runs, whitespace runs.
pub fn pretokenize(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end = text.len();
    let at = |i: usize| chars.get(i).map(|(p, _)| *p).unwrap_or(end);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let c = chars[i].1;
        let next = chars.get(i + 1).map(|(_, c)| class(*c));
        match class(c) {
            Class::Letter => {
                while i < chars.len() && class(chars[i].1) == Class::Letter {
                    i += 1;
                }
            }
            Class::Digit => i += 1,
            Class::Newline => {
                while i < chars.len() && matches!(class(chars[i].1), Class::Newline | Class::Space)
                {
                    i += 1;
                }
            }
            Class::Space | Class::Other if next == Some(Class::Letter) && c != '\n' => {
                i += 1;
                while i < chars.len() && class(chars[i].1) == Class::Letter {
                    i += 1;
                }
            }
            Class::Space if next == Some(Class::Other) && c == ' ' => {
                i += 1;
                while i < chars.len() && class(chars[i].1) == Class::Other {
                    i += 1;
                }
            }
            Class::Space => {
                while i < chars.len() && class(chars[i].1) == Class::Space {
                    i += 1;
                }
                // Leave the last space to prefix the following word.
                if i < chars.len() && i - start > 1 && class(chars[i].1) != Class::Newline {
                    i -= 1;
                }
            }
            Class::Other => {
                while i < chars.len() && class(chars[i].1) == Class::Other {
                    i += 1;
                }
                while i < chars.len() && class(chars[i].1) == Class::Newline {
                    i += 1;
                }
            }
        }
        out.push(&text[at(start)..at(i)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretokenize_roundtrips_text() {
        let text = "Open my Downloads folder, set volume to 30!\n\n  ok";
        assert_eq!(pretokenize(text).concat(), text);
        assert!(pretokenize(text).contains(&" Downloads"));
    }

    #[test]
    fn bpe_merges_and_decodes() {
        let b2u = byte_to_unicode();
        let mut tokens: Vec<String> = b2u.iter().map(|c| c.to_string()).collect();
        tokens.push("he".into());
        tokens.push("hel".into());
        tokens.push("<|im_end|>".into());
        let tok = Tokenizer::new(tokens, vec!["h e".into(), "he l".into()]);
        let ids = tok.encode("hello<|im_end|>");
        assert_eq!(ids[0], 257);
        assert_eq!(*ids.last().unwrap(), 258);
        assert_eq!(tok.decode(&ids), "hello<|im_end|>");
    }
}
