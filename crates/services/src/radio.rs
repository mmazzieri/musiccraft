//! Internet radio. The directory is Radio Browser (<https://www.radio-browser.info>), a community
//! project whose data is in the public domain and whose interface is open to any program.

use crate::http::{self, number, text};
use crate::{Entry, ServiceError};
use serde_json::Value;

const DIRECTORY: &str = "https://all.api.radio-browser.info";

/// Stations whose name or tags match, the most listened first. An empty query lists the most
/// popular.
pub fn search(query: &str) -> Result<Vec<Entry>, ServiceError> {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.extend_pairs([("limit", "100"), ("hidebroken", "true"), ("order", "clickcount"), ("reverse", "true")]);
    if !query.trim().is_empty() {
        q.append_pair("name", query.trim());
    }
    let v = http::json(http::get(&format!("{DIRECTORY}/json/stations/search?{}", q.finish()))?)?;
    let list = v.as_array().ok_or_else(|| ServiceError::Answer("not a list of stations".to_owned()))?;
    Ok(list.iter().map(station).filter(|e| is_web(&e.id)).collect())
}

fn station(v: &Value) -> Entry {
    let detail: Vec<String> = [
        text(v, "country"),
        text(v, "codec"),
        match number(v, "bitrate") {
            0 => String::new(),
            n => format!("{n} kbps"),
        },
    ]
    .into_iter()
    .filter(|s| !s.is_empty() && s != "UNKNOWN")
    .collect();
    Entry {
        id: text(v, "url_resolved"),
        title: text(v, "name").trim().to_owned(),
        artist: detail.join(" · "),
        album: text(v, "tags").split(',').take(3).collect::<Vec<_>>().join(", "),
        ..Entry::default()
    }
}

pub fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Whether the address names a playlist file that has to be read to find the stream.
pub fn is_playlist(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    path.ends_with(".m3u") || path.ends_with(".pls") || path.ends_with(".m3u8")
}

/// Reads a small text file from the web, such as a station's playlist.
pub fn fetch(url: &str) -> Result<Vec<u8>, ServiceError> {
    http::bytes(http::get(url)?)
}
