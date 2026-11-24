//! The Spotify Web API (<https://developer.spotify.com/documentation/web-api>), in the form its
//! Development Mode has had since February 2026, with the person's own Client ID and the
//! Authorization Code flow with PKCE.
//!
//! Playing happens on a Spotify Connect device: the Spotify app, or Soloist. Nothing is
//! downloaded or kept.

use crate::http::{self, number, text};
use crate::{Entry, ServiceError, secrets};
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

const REFRESH: &str = "spotify-refresh-token";
const SCOPES: &str = "user-library-read playlist-read-private playlist-read-collaborative \
                      user-read-playback-state user-modify-playback-state user-read-currently-playing";

/// Where the service is; tests put a local server here.
#[derive(Debug, Clone)]
pub struct Hosts {
    pub accounts: String,
    pub api: String,
}

impl Default for Hosts {
    fn default() -> Self {
        Hosts { accounts: "https://accounts.spotify.com".to_owned(), api: "https://api.spotify.com/v1".to_owned() }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub active: bool,
}

/// What a Spotify device is playing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playback {
    pub uri: String,
    pub playing: bool,
    pub position_ms: u64,
    pub entry: Entry,
    pub device: String,
}

struct Token {
    access: String,
    until: Instant,
}

/// A signed-in account.
pub struct Spotify {
    hosts: Hosts,
    client_id: String,
    refresh: Mutex<String>,
    token: Mutex<Option<Token>>,
    /// Whether a new refresh token goes to the credential store.
    store: bool,
}

/// A sign-in under way: the page to open in the browser, and the listener for its answer.
pub struct SignIn {
    pub page: String,
    hosts: Hosts,
    client_id: String,
    verifier: String,
    state: String,
    redirect: String,
    listener: TcpListener,
}

fn random() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Starts a sign-in. The redirect address to register for the Client ID is
/// `http://127.0.0.1/callback`; the port is chosen here, as Spotify allows for that address.
pub fn sign_in(client_id: &str, hosts: Hosts) -> Result<SignIn, ServiceError> {
    let client_id = client_id.trim();
    if client_id.is_empty() || !client_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ServiceError::Other("the Client ID has only letters and digits".to_owned()));
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| ServiceError::Other(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| ServiceError::Other(e.to_string()))?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let (verifier, state) = (random(), random());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.extend_pairs([
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", &redirect),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
        ("state", &state),
        ("scope", SCOPES),
    ]);
    Ok(SignIn {
        page: format!("{}/authorize?{}", hosts.accounts, q.finish()),
        hosts,
        client_id: client_id.to_owned(),
        verifier,
        state,
        redirect,
        listener,
    })
}

impl SignIn {
    /// Waits for the browser to come back, then gets the tokens. `store` keeps the account signed
    /// in on this computer.
    pub fn finish(self, wait: Duration, store: bool) -> Result<Spotify, ServiceError> {
        let other = |e: std::io::Error| ServiceError::Other(e.to_string());
        self.listener.set_nonblocking(true).map_err(other)?;
        let until = Instant::now() + wait;
        let code = loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).map_err(other)?;
                    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(other)?;
                    let mut line = String::new();
                    // A browser may open a connection it does not use; keep waiting.
                    if BufReader::new(std::io::Read::take(&stream, 8192)).read_line(&mut line).is_err() {
                        continue;
                    }
                    let answer = self.read_answer(&line);
                    let (status, message) = match &answer {
                        None => ("404 Not Found", "Not here."),
                        Some(Ok(_)) => ("200 OK", "MusicCraft is signed in. You can close this page."),
                        Some(Err(_)) => ("200 OK", "The sign-in was not completed. You can close this page."),
                    };
                    let page = format!("<!doctype html><meta charset=utf-8><title>MusicCraft</title><p>{message}</p>");
                    // The page is a courtesy; a browser that already left does not matter.
                    write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                        page.len()
                    )
                    .ok();
                    if let Some(a) = answer {
                        break a?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > until {
                        return Err(ServiceError::Other("the sign-in was not completed in time".to_owned()));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(other(e)),
            }
        };
        let account = Spotify::new(&self.client_id, "", self.hosts.clone(), store);
        account.tokens(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &self.redirect),
            ("client_id", &self.client_id),
            ("code_verifier", &self.verifier),
        ])?;
        Ok(account)
    }

    /// The code in a request line, if it is the answer to this sign-in.
    fn read_answer(&self, line: &str) -> Option<Result<String, ServiceError>> {
        let target = line.strip_prefix("GET ")?.split(' ').next()?;
        let query = target.strip_prefix("/callback?")?;
        let find =
            |k: &str| url::form_urlencoded::parse(query.as_bytes()).find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
        if find("state").as_deref() != Some(&self.state) {
            return None;
        }
        Some(find("code").ok_or_else(|| ServiceError::Other("the sign-in was refused".to_owned())))
    }
}

impl Spotify {
    pub fn new(client_id: &str, refresh: &str, hosts: Hosts, store: bool) -> Spotify {
        Spotify {
            hosts,
            client_id: client_id.to_owned(),
            refresh: Mutex::new(refresh.to_owned()),
            token: Mutex::new(None),
            store,
        }
    }

    /// The account signed in on this computer, if any.
    pub fn stored(client_id: &str) -> Option<Spotify> {
        if client_id.is_empty() {
            return None;
        }
        Some(Spotify::new(client_id, &secrets::get(REFRESH)?, Hosts::default(), true))
    }

    pub fn sign_out() {
        secrets::remove(REFRESH);
    }

    fn tokens(&self, form: &[(&str, &str)]) -> Result<String, ServiceError> {
        let r = http::agent()
            .post(format!("{}/api/token", self.hosts.accounts))
            .send_form(form.iter().copied())
            .map_err(http::network)?;
        let v = match http::json(r) {
            // A refresh token that is no longer valid.
            Err(ServiceError::Refused { status: 400, .. }) => return Err(ServiceError::SignIn),
            other => other?,
        };
        let access = text(&v, "access_token");
        if access.is_empty() {
            return Err(ServiceError::Answer("no access token".to_owned()));
        }
        let new = text(&v, "refresh_token");
        if !new.is_empty() {
            if self.store {
                secrets::set(REFRESH, &new)?;
            }
            *self.refresh.lock().unwrap_or_else(PoisonError::into_inner) = new;
        }
        let life = Duration::from_secs(number(&v, "expires_in").clamp(60, 86_400) - 30);
        *self.token.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Token { access: access.clone(), until: Instant::now() + life });
        Ok(access)
    }

    fn access(&self) -> Result<String, ServiceError> {
        if let Some(t) = self.token.lock().unwrap_or_else(PoisonError::into_inner).as_ref()
            && t.until > Instant::now()
        {
            return Ok(t.access.clone());
        }
        let refresh = self.refresh.lock().unwrap_or_else(PoisonError::into_inner).clone();
        self.tokens(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &self.client_id)])
    }

    fn get(&self, path: &str) -> Result<Value, ServiceError> {
        let r = http::agent()
            .get(format!("{}{path}", self.hosts.api))
            .header("Authorization", format!("Bearer {}", self.access()?))
            .call()
            .map_err(http::network)?;
        http::json(r)
    }

    fn put(&self, path: &str, body: &Value) -> Result<(), ServiceError> {
        let r = http::agent()
            .put(format!("{}{path}", self.hosts.api))
            .header("Authorization", format!("Bearer {}", self.access()?))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(http::network)?;
        http::bytes(r).map(|_| ())
    }

    /// The name of the account.
    pub fn me(&self) -> Result<String, ServiceError> {
        let v = self.get("/me")?;
        let name = text(&v, "display_name");
        Ok(if name.is_empty() { text(&v, "id") } else { name })
    }

    fn pages(
        &self,
        path: &str,
        max: usize,
        read: impl Fn(&Value) -> Option<Entry>,
    ) -> Result<Vec<Entry>, ServiceError> {
        let mut out = Vec::new();
        let sep = if path.contains('?') { '&' } else { '?' };
        for page in 0..max {
            let v = self.get(&format!("{path}{sep}limit=50&offset={}", page * 50))?;
            let Some(items) = v.get("items").and_then(Value::as_array) else { break };
            out.extend(items.iter().filter_map(&read));
            if items.len() < 50 {
                break;
            }
        }
        Ok(out)
    }

    /// The songs the account has saved, the newest first.
    pub fn saved(&self) -> Result<Vec<Entry>, ServiceError> {
        self.pages("/me/tracks", 40, |v| track(v.get("track")?))
    }

    pub fn playlists(&self) -> Result<Vec<Entry>, ServiceError> {
        self.pages("/me/playlists", 10, |v| {
            let songs = v.get("items").or_else(|| v.get("tracks")).cloned().unwrap_or(Value::Null);
            Some(Entry {
                id: text(v, "id"),
                title: text(v, "name"),
                count: u32::try_from(number(&songs, "total")).unwrap_or(0),
                art: image(v),
                ..Entry::default()
            })
        })
    }

    /// The songs of a playlist the account owns or works on; Spotify does not list others.
    pub fn playlist(&self, id: &str) -> Result<Vec<Entry>, ServiceError> {
        if !is_id(id) {
            return Err(ServiceError::Other("not a playlist".to_owned()));
        }
        self.pages(&format!("/playlists/{id}/items"), 40, |v| track(v.get("item").or_else(|| v.get("track"))?))
    }

    /// Songs that match; Spotify gives ten at a time.
    pub fn search(&self, query: &str) -> Result<Vec<Entry>, ServiceError> {
        let mut out = Vec::new();
        for page in 0..3 {
            let mut q = url::form_urlencoded::Serializer::new(String::new());
            q.extend_pairs([("q", query), ("type", "track"), ("limit", "10"), ("offset", &(page * 10).to_string())]);
            let v = self.get(&format!("/search?{}", q.finish()))?;
            let Some(items) = v.pointer("/tracks/items").and_then(Value::as_array) else { break };
            out.extend(items.iter().filter_map(track));
            if items.len() < 10 {
                break;
            }
        }
        Ok(out)
    }

    pub fn devices(&self) -> Result<Vec<Device>, ServiceError> {
        let v = self.get("/me/player/devices")?;
        Ok(v.get("devices")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|d| Device {
                        id: text(d, "id"),
                        name: text(d, "name"),
                        active: d.get("is_active").and_then(Value::as_bool).unwrap_or(false),
                    })
                    .filter(|d| !d.id.is_empty())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Plays a song on a device: `device` by id, else the active one, else the first.
    pub fn play(&self, uri: &str, device: &str) -> Result<(), ServiceError> {
        let device = if device.is_empty() {
            let all = self.devices()?;
            let d = all.iter().find(|d| d.active).or_else(|| all.first());
            d.map(|d| d.id.clone())
                .ok_or_else(|| ServiceError::Other("open Spotify on this or another device first".to_owned()))?
        } else {
            device.to_owned()
        };
        let mut q = url::form_urlencoded::Serializer::new(String::new());
        q.append_pair("device_id", &device);
        self.put(&format!("/me/player/play?{}", q.finish()), &json!({ "uris": [uri] }))
    }

    pub fn resume(&self) -> Result<(), ServiceError> {
        self.put("/me/player/play", &json!({}))
    }

    pub fn pause(&self) -> Result<(), ServiceError> {
        self.put("/me/player/pause", &json!({}))
    }

    pub fn seek(&self, ms: u64) -> Result<(), ServiceError> {
        self.put(&format!("/me/player/seek?position_ms={ms}"), &json!({}))
    }

    pub fn volume(&self, percent: u32) -> Result<(), ServiceError> {
        self.put(&format!("/me/player/volume?volume_percent={}", percent.min(100)), &json!({}))
    }

    pub fn playback(&self) -> Result<Option<Playback>, ServiceError> {
        let v = self.get("/me/player")?;
        let Some(item) = v.get("item").filter(|i| !i.is_null()) else { return Ok(None) };
        Ok(Some(Playback {
            uri: text(item, "uri"),
            playing: v.get("is_playing").and_then(Value::as_bool).unwrap_or(false),
            position_ms: number(&v, "progress_ms"),
            entry: track(item).unwrap_or_default(),
            device: v.pointer("/device/name").and_then(Value::as_str).unwrap_or("").to_owned(),
        }))
    }
}

fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn image(v: &Value) -> String {
    // Widest first; the second is about 300 pixels.
    let list = v.get("images").and_then(Value::as_array);
    list.and_then(|a| a.get(1).or_else(|| a.first())).map(|i| text(i, "url")).unwrap_or_default()
}

/// A song. Its id is the Spotify address, which is what playing takes.
fn track(v: &Value) -> Option<Entry> {
    let uri = text(v, "uri");
    if !uri.starts_with("spotify:track:") {
        return None;
    }
    let artists: Vec<String> = v
        .get("artists")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|x| text(x, "name")).collect())
        .unwrap_or_default();
    let album = v.get("album").cloned().unwrap_or(Value::Null);
    Some(Entry {
        id: uri,
        title: text(v, "name"),
        artist: artists.join(", "),
        album: text(&album, "name"),
        duration_ms: number(v, "duration_ms"),
        art: image(&album),
        year: text(&album, "release_date").get(..4).and_then(|y| y.parse().ok()).unwrap_or(0),
        track: u32::try_from(number(v, "track_number")).unwrap_or(0),
        count: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::{Hosts, Spotify, sign_in};
    use crate::ServiceError;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    type Log = Arc<Mutex<Vec<String>>>;

    /// A local stand-in for the service: answers each request line with `answer`.
    fn service(answer: impl Fn(&str) -> (u16, String) + Send + 'static) -> (Hosts, Log) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let log: Log = Arc::default();
        let seen = log.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let (mut len, mut head) = (0, String::new());
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap() < 3 {
                        break;
                    }
                    if let Some(n) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = n.trim().parse().unwrap();
                    }
                    head.push_str(&h);
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                seen.lock().unwrap().push(format!("{}{head}{}", line, String::from_utf8_lossy(&body)));
                let (status, text) = answer(&line);
                let mut s = s;
                write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len())
                    .unwrap();
            }
        });
        (Hosts { accounts: base.clone(), api: base }, log)
    }

    fn token(line: &str) -> Option<(u16, String)> {
        line.contains("/api/token")
            .then(|| (200, r#"{"access_token":"A","expires_in":3600,"refresh_token":"R"}"#.to_owned()))
    }

    /// The sign-in takes only the answer that carries its own state, and sends the verifier whose
    /// hash the page was opened with.
    #[test]
    fn sign_in_checks_the_state_and_proves_the_verifier() {
        let (hosts, log) = service(|l| token(l).unwrap_or((404, String::new())));
        let s = sign_in("abc123", hosts).unwrap();
        let url = url::Url::parse(&s.page).unwrap();
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        let addr = q["redirect_uri"].trim_start_matches("http://").trim_end_matches("/callback").to_owned();
        let browser = std::thread::spawn(move || {
            let ask = |target: &str| {
                let mut c = TcpStream::connect(&addr).unwrap();
                write!(c, "GET {target} HTTP/1.1\r\n\r\n").unwrap();
                let mut out = String::new();
                c.read_to_string(&mut out).ok();
                out
            };
            assert!(ask("/callback?code=evil&state=other").starts_with("HTTP/1.1 404"));
            ask(&format!("/callback?code=good&state={}", q["state"]));
            q["code_challenge"].clone()
        });
        s.finish(Duration::from_secs(10), false).unwrap();
        let challenge = browser.join().unwrap();
        let sent = log.lock().unwrap().pop().unwrap();
        let form = sent.lines().last().unwrap();
        assert!(form.contains("code=good"));
        let verifier = form.split("code_verifier=").nth(1).unwrap();
        use base64::Engine;
        use sha2::Digest;
        let hash = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier));
        assert_eq!(hash, challenge);
    }

    /// Both forms of a playlist's songs are read, items that are not songs are passed over, and
    /// playing without a device says so instead of failing silently.
    #[test]
    fn playlists_and_missing_devices() {
        let (hosts, log) = service(|l| {
            token(l).unwrap_or_else(|| {
                (200, if l.contains("/items") {
                    r#"{"items":[{"item":{"uri":"spotify:track:1","name":"Uno","artists":[{"name":"A"},{"name":"B"}],"duration_ms":1000}},{"item":null},{"item":{"uri":"spotify:episode:9"}},{"track":{"uri":"spotify:track:2","name":"Due"}}]}"#.to_owned()
                } else {
                    r#"{"devices":[]}"#.to_owned()
                })
            })
        });
        let s = Spotify::new("abc", "R0", hosts, false);
        let songs = s.playlist("xyz").unwrap();
        assert_eq!(songs.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Uno", "Due"]);
        assert_eq!(songs[0].artist, "A, B");
        assert!(matches!(s.play("spotify:track:1", ""), Err(ServiceError::Other(_))));
        assert!(s.playlist("../me").is_err());
        assert_eq!(log.lock().unwrap().iter().filter(|r| r.contains("/api/token")).count(), 1, "the token is reused");
    }

    /// An expired sign-in and a rate limit are told apart.
    #[test]
    fn refusals_are_named() {
        let (hosts, _) = service(|l| {
            if l.contains("/api/token") {
                (400, r#"{"error":"invalid_grant"}"#.to_owned())
            } else {
                (500, String::new())
            }
        });
        assert!(matches!(Spotify::new("a", "old", hosts, false).me(), Err(ServiceError::SignIn)));
        let (hosts, _) = service(|l| token(l).unwrap_or((429, String::new())));
        assert!(matches!(Spotify::new("a", "R", hosts, false).me(), Err(ServiceError::Wait(_))));
    }
}
