//! The assistant's only network use: opt-in web search (DuckDuckGo HTML
//! endpoint, no key, no account) and Open-Meteo weather. Callers must check
//! the Privacy "Assistant network" toggle before calling anything here.

use std::time::Duration;

use crate::AssistantError;

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) NexusOS-Assistant/0.1";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .user_agent(UA)
        .build()
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Remove tags, scripts and styles; collapse whitespace.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 3);
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            for skip in ["script", "style", "noscript", "svg", "head"] {
                if lower[i + 1..].starts_with(skip) {
                    let close = format!("</{skip}");
                    if let Some(end) = lower[i..].find(&close) {
                        i += end;
                    }
                    break;
                }
            }
            match html[i..].find('>') {
                Some(end) => i += end + 1,
                None => break,
            }
            out.push(' ');
        } else {
            let ch_len = html[i..].chars().next().map(char::len_utf8).unwrap_or(1);
            out.push_str(&html[i..i + ch_len]);
            i += ch_len;
        }
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse DuckDuckGo's HTML results page.
pub fn parse_ddg(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for chunk in html.split("class=\"result__a\"").skip(1) {
        let href = chunk
            .split("href=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or_default();
        let url = match href.find("uddg=") {
            Some(p) => url_decode(href[p + 5..].split('&').next().unwrap_or_default()),
            None => decode_entities(href),
        };
        let title = chunk
            .split_once('>')
            .map(|x| x.1)
            .and_then(|s| s.split("</a>").next())
            .map(html_to_text)
            .unwrap_or_default();
        let snippet = chunk
            .split("class=\"result__snippet\"")
            .nth(1)
            .and_then(|s| s.split_once('>'))
            .and_then(|(_, s)| s.split("</a>").next())
            .map(html_to_text)
            .unwrap_or_default();
        if url.starts_with("http") && !url.contains("duckduckgo.com/y.js") && !title.is_empty() {
            hits.push(SearchHit {
                title,
                url,
                snippet,
            });
        }
        if hits.len() >= 5 {
            break;
        }
    }
    hits
}

pub fn search(query: &str) -> Result<Vec<SearchHit>, AssistantError> {
    let body = agent()
        .post("https://html.duckduckgo.com/html/")
        .send_form(&[("q", query), ("kl", "wt-wt")])
        .map_err(|e| AssistantError::Network(e.to_string()))?
        .into_string()
        .map_err(|e| AssistantError::Network(e.to_string()))?;
    let hits = parse_ddg(&body);
    if hits.is_empty() {
        return Err(AssistantError::Network("no results".into()));
    }
    Ok(hits)
}

/// Fetch a page and return up to `max` chars of readable text.
pub fn fetch_text(url: &str, max: usize) -> Result<String, AssistantError> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| AssistantError::Network(e.to_string()))?;
    let mut body = String::new();
    use std::io::Read;
    resp.into_reader()
        .take(600_000)
        .read_to_string(&mut body)
        .map_err(|e| AssistantError::Network(e.to_string()))?;
    let text = html_to_text(&body);
    Ok(text.chars().take(max).collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherNow {
    pub place: String,
    pub temp_c: f32,
    pub wind_kmh: f32,
    pub code: u32,
}

impl WeatherNow {
    pub fn summary(&self) -> &'static str {
        match self.code {
            0 => "CLEAR",
            1..=3 => "CLOUDY",
            45 | 48 => "FOG",
            51..=67 => "RAIN",
            71..=77 => "SNOW",
            80..=82 => "SHOWERS",
            95..=99 => "STORM",
            _ => "—",
        }
    }
}

/// Open-Meteo geocoding + current weather (no key).
pub fn weather(city: &str) -> Result<WeatherNow, AssistantError> {
    let net = |e: ureq::Error| AssistantError::Network(e.to_string());
    let geo: serde_json::Value = agent()
        .get(&format!(
            "https://geocoding-api.open-meteo.com/v1/search?count=1&name={}",
            url_encode(city)
        ))
        .call()
        .map_err(net)?
        .into_string()
        .map_err(|e| AssistantError::Network(e.to_string()))
        .and_then(|t| {
            serde_json::from_str(&t).map_err(|e| AssistantError::Network(e.to_string()))
        })?;
    let first = geo
        .get("results")
        .and_then(|r| r.get(0))
        .ok_or_else(|| AssistantError::Network(format!("unknown place {city}")))?;
    let lat = first
        .get("latitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let lon = first
        .get("longitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let place = first
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(city)
        .to_string();
    let wx: serde_json::Value = agent()
        .get(&format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,wind_speed_10m,weather_code"
        ))
        .call()
        .map_err(net)?
        .into_string()
        .map_err(|e| AssistantError::Network(e.to_string()))
        .and_then(|t| serde_json::from_str(&t).map_err(|e| AssistantError::Network(e.to_string())))?;
    let cur = wx
        .get("current")
        .ok_or_else(|| AssistantError::Network("no current weather".into()))?;
    Ok(WeatherNow {
        place,
        temp_c: cur
            .get("temperature_2m")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        wind_kmh: cur
            .get("wind_speed_10m")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        code: cur
            .get("weather_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ddg_results_and_decodes_redirects() {
        let html = r#"<div><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&amp;rut=abc">Rust <b>Programming</b> Language</a>
        <a class="result__snippet" href="x">A language empowering &amp; everyone</a></div>"#;
        let hits = parse_ddg(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://www.rust-lang.org/");
        assert_eq!(hits[0].title, "Rust Programming Language");
        assert_eq!(hits[0].snippet, "A language empowering & everyone");
    }

    #[test]
    fn strips_scripts_and_tags() {
        let t = html_to_text("<html><head><title>x</title></head><body><script>var a=1;</script><p>Hello&nbsp;<b>world</b></p></body></html>");
        assert_eq!(t, "Hello world");
    }

    #[test]
    fn url_roundtrip() {
        assert_eq!(url_decode(&url_encode("a b&c/é")), "a b&c/é");
    }
}
