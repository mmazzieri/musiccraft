//! Personal music servers that speak the Subsonic API, version 1.16, or OpenSubsonic
//! (<https://opensubsonic.netlify.app/>): Navidrome, Gonic, Airsonic, LMS and others.

use crate::http::{self, number, text};
use crate::{Entry, ServiceError};
use md5::{Digest, Md5};
use serde_json::Value;

/// A server and how to sign in to it.
#[derive(Clone, PartialEq, Eq)]
pub struct Server {
    pub url: String,
    pub user: String,
    pub password: String,
}

/// The lists a server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlbumList {
    Newest,
    Recent,
    Frequent,
    ByArtist,
    Random,
}

impl AlbumList {
    fn name(self) -> &'static str {
        match self {
            AlbumList::Newest => "newest",
            AlbumList::Recent => "recent",
            AlbumList::Frequent => "frequent",
            AlbumList::ByArtist => "alphabeticalByArtist",
            AlbumList::Random => "random",
        }
    }
}

impl Server {
    /// Accepts an address as people write it, with or without `https://`.
    pub fn new(url: &str, user: &str, password: &str) -> Result<Server, ServiceError> {
        let mut u = url.trim().trim_end_matches('/').to_owned();
        if !u.contains("://") {
            u = format!("https://{u}");
        }
        let parsed = url::Url::parse(&u).map_err(|e| ServiceError::Other(e.to_string()))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(ServiceError::Other("not a web address".to_owned()));
        }
        Ok(Server { url: u, user: user.trim().to_owned(), password: password.to_owned() })
    }

    /// Whether what is sent can be read on the way.
    pub fn is_plain(&self) -> bool {
        self.url.starts_with("http://")
    }

    fn address(&self, method: &str, params: &[(&str, &str)]) -> String {
        // A new salt for each request, as the API asks; the password itself is never sent.
        let salt = uuid::Uuid::new_v4().simple().to_string();
        let token: String =
            Md5::digest(format!("{}{salt}", self.password).as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let mut q = url::form_urlencoded::Serializer::new(String::new());
        q.extend_pairs([
            ("u", self.user.as_str()),
            ("t", &token),
            ("s", &salt),
            ("v", "1.16.1"),
            ("c", "MusicCraft"),
            ("f", "json"),
        ]);
        q.extend_pairs(params);
        format!("{}/rest/{method}.view?{}", self.url, q.finish())
    }

    fn call(&self, method: &str, params: &[(&str, &str)]) -> Result<Value, ServiceError> {
        let mut v = http::json(http::get(&self.address(method, params))?)?;
        let r = v.get_mut("subsonic-response").map(Value::take).unwrap_or(Value::Null);
        match text(&r, "status").as_str() {
            "ok" => Ok(r),
            "failed" => {
                let e = r.get("error").cloned().unwrap_or(Value::Null);
                Err(match number(&e, "code") {
                    40 | 41 | 44 => ServiceError::SignIn,
                    _ => ServiceError::Other(text(&e, "message")),
                })
            }
            _ => Err(ServiceError::Answer("not a music server of this kind".to_owned())),
        }
    }

    pub fn ping(&self) -> Result<(), ServiceError> {
        self.call("ping", &[]).map(|_| ())
    }

    pub fn albums(&self, list: AlbumList, offset: usize) -> Result<Vec<Entry>, ServiceError> {
        let offset = offset.to_string();
        let v = self.call("getAlbumList2", &[("type", list.name()), ("size", "200"), ("offset", &offset)])?;
        Ok(rows(&v, "/albumList2/album", album))
    }

    pub fn album(&self, id: &str) -> Result<Vec<Entry>, ServiceError> {
        Ok(rows(&self.call("getAlbum", &[("id", id)])?, "/album/song", song))
    }

    pub fn playlists(&self) -> Result<Vec<Entry>, ServiceError> {
        Ok(rows(&self.call("getPlaylists", &[])?, "/playlists/playlist", |v| Entry {
            id: text(v, "id"),
            title: text(v, "name"),
            count: small(v, "songCount"),
            art: text(v, "coverArt"),
            ..Entry::default()
        }))
    }

    pub fn playlist(&self, id: &str) -> Result<Vec<Entry>, ServiceError> {
        Ok(rows(&self.call("getPlaylist", &[("id", id)])?, "/playlist/entry", song))
    }

    /// Songs and albums that match.
    pub fn search(&self, query: &str) -> Result<(Vec<Entry>, Vec<Entry>), ServiceError> {
        let v = self
            .call("search3", &[("query", query), ("songCount", "200"), ("albumCount", "40"), ("artistCount", "0")])?;
        Ok((rows(&v, "/searchResult3/song", song), rows(&v, "/searchResult3/album", album)))
    }

    /// Tells the server a song was played.
    pub fn scrobble(&self, id: &str) -> Result<(), ServiceError> {
        self.call("scrobble", &[("id", id), ("submission", "true")]).map(|_| ())
    }

    pub fn stream_url(&self, id: &str) -> String {
        self.address("stream", &[("id", id)])
    }

    pub fn cover(&self, art: &str) -> Result<Vec<u8>, ServiceError> {
        http::bytes(http::get(&self.address("getCoverArt", &[("id", art), ("size", "400")]))?)
    }
}

fn small(v: &Value, key: &str) -> u32 {
    u32::try_from(number(v, key)).unwrap_or(0)
}

fn rows(v: &Value, pointer: &str, read: impl Fn(&Value) -> Entry) -> Vec<Entry> {
    v.pointer(pointer)
        .and_then(Value::as_array)
        .map(|a| a.iter().map(read).filter(|e| !e.id.is_empty()).collect())
        .unwrap_or_default()
}

fn song(v: &Value) -> Entry {
    Entry {
        id: text(v, "id"),
        title: text(v, "title"),
        artist: text(v, "artist"),
        album: text(v, "album"),
        duration_ms: number(v, "duration").saturating_mul(1000),
        art: text(v, "coverArt"),
        year: small(v, "year"),
        track: small(v, "track"),
        isrc: v.pointer("/isrc/0").and_then(Value::as_str).unwrap_or("").to_owned(),
        count: 0,
    }
}

fn album(v: &Value) -> Entry {
    Entry {
        id: text(v, "id"),
        title: text(v, "name"),
        artist: text(v, "artist"),
        album: text(v, "name"),
        duration_ms: number(v, "duration").saturating_mul(1000),
        art: text(v, "coverArt"),
        year: small(v, "year"),
        track: 0,
        isrc: String::new(),
        count: small(v, "songCount"),
    }
}

#[cfg(test)]
mod tests {
    use super::Server;

    /// The password never appears in an address, and two requests never share a token.
    #[test]
    fn the_password_is_not_sent() {
        let s = Server::new("music.example/", "me", "hunter2 à").unwrap();
        let (a, b) = (s.stream_url("1"), s.stream_url("1"));
        assert!(a.starts_with("https://music.example/rest/stream.view?"));
        assert!(!a.contains("hunter2"));
        assert_ne!(a, b);
    }

    #[test]
    fn only_web_addresses_are_accepted() {
        assert!(Server::new("file:///etc/passwd", "a", "b").is_err());
        assert!(Server::new("http://nas.local:4533", "a", "b").unwrap().is_plain());
    }
}
