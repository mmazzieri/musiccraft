//! The streaming services a library song without a file can play from.

use musiccraft_library::model::{Info, Link};
use musiccraft_library::names::{first_artist, plain_title};

/// A service, by the key the settings and the library's links know it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Service {
    /// A personal server: plays here.
    Server(String),
    /// Plays on a Spotify device.
    Spotify,
    /// Opens in the service's own app or web player. No interface of the service is used, so this
    /// works with any of them, but MusicCraft cannot tell when the song ends.
    Open(Site),
}

/// Where songs can be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    Tidal,
    AppleMusic,
    Deezer,
    YouTubeMusic,
    AmazonMusic,
    Spotify,
}

pub const SITES: [Site; 6] =
    [Site::Tidal, Site::AppleMusic, Site::Deezer, Site::YouTubeMusic, Site::AmazonMusic, Site::Spotify];

impl Site {
    pub fn name(self) -> &'static str {
        match self {
            Site::Tidal => "TIDAL",
            Site::AppleMusic => "Apple Music",
            Site::Deezer => "Deezer",
            Site::YouTubeMusic => "YouTube Music",
            Site::AmazonMusic => "Amazon Music",
            Site::Spotify => "Spotify",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Site::Tidal => "tidal",
            Site::AppleMusic => "apple",
            Site::Deezer => "deezer",
            Site::YouTubeMusic => "youtube",
            Site::AmazonMusic => "amazon",
            Site::Spotify => "spotify",
        }
    }

    /// The public web addresses of the service.
    fn hosts(self) -> &'static [&'static str] {
        match self {
            Site::Tidal => &["tidal.com", "www.tidal.com", "listen.tidal.com"],
            Site::AppleMusic => &["music.apple.com"],
            Site::Deezer => &["www.deezer.com", "deezer.com", "link.deezer.com"],
            Site::YouTubeMusic => &["music.youtube.com", "www.youtube.com", "youtu.be"],
            Site::AmazonMusic => &["music.amazon.com", "music.amazon.it", "music.amazon.co.uk", "music.amazon.de"],
            Site::Spotify => &["open.spotify.com"],
        }
    }

    /// The service's search page for a song.
    fn search(self, text: &str) -> String {
        let q: String = text.bytes().map(encode).collect();
        match self {
            Site::Tidal => format!("https://tidal.com/search?q={q}"),
            Site::AppleMusic => format!("https://music.apple.com/search?term={q}"),
            Site::Deezer => format!("https://www.deezer.com/search/{q}"),
            Site::YouTubeMusic => format!("https://music.youtube.com/search?q={q}"),
            Site::AmazonMusic => format!("https://music.amazon.com/search/{q}"),
            Site::Spotify => format!("https://open.spotify.com/search/{q}"),
        }
    }

    /// Whether an address is a page of this service. Links come from a shared file too, so only
    /// these are ever opened.
    pub fn accepts(self, address: &str) -> bool {
        let Some(rest) = address.strip_prefix("https://") else { return false };
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        address.len() < 400
            && address.bytes().all(|b| b.is_ascii_graphic() && !b"\"'<>`\\^{}|".contains(&b))
            && self.hosts().contains(&host)
    }

    /// The page to open for a song: the one the person gave, or a search.
    pub fn page(self, song: &Info, link: Option<&Link>) -> String {
        match link.filter(|l| self.accepts(&l.id)) {
            Some(l) => l.id.clone(),
            None => {
                let artist = if song.artist.is_empty() { &song.album_artist } else { &song.artist };
                self.search(format!("{} {}", plain_title(&song.title), first_artist(artist)).trim())
            }
        }
    }
}

fn encode(b: u8) -> String {
    if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { char::from(b).to_string() } else { format!("%{b:02X}") }
}

impl Service {
    pub fn from_key(key: &str) -> Option<Service> {
        if let Some(name) = key.strip_prefix("server:") {
            return (!name.is_empty()).then(|| Service::Server(name.to_owned()));
        }
        if let Some(id) = key.strip_prefix("open:") {
            return SITES.into_iter().find(|s| s.id() == id).map(Service::Open);
        }
        (key == "spotify").then_some(Service::Spotify)
    }

    pub fn key(&self) -> String {
        match self {
            Service::Server(name) => format!("server:{name}"),
            Service::Spotify => "spotify".to_owned(),
            Service::Open(site) => format!("open:{}", site.id()),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Service::Server(name) => name.clone(),
            Service::Spotify => "Spotify".to_owned(),
            Service::Open(site) => site.name().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Service, Site};
    use musiccraft_library::model::{Info, Link};

    /// A link in a shared file can only open a page of its own service; anything else falls back to
    /// a search, whose text cannot break out of the address.
    #[test]
    fn only_pages_of_the_service_are_opened() {
        let song = Info { title: "A&B / \"C\" #1?".into(), artist: "È".into(), ..Info::default() };
        let page = |id: &str| Site::Tidal.page(&song, Some(&Link { id: id.into(), ..Link::default() }));
        assert_eq!(page("https://tidal.com/track/123"), "https://tidal.com/track/123");
        for bad in [
            "http://tidal.com/track/1",
            "https://tidal.com.evil.example/x",
            "https://evil.example/tidal.com",
            "https://tidal.com@evil.example/",
            "file:///c:/x.exe",
            "https://tidal.com/\" & calc",
            "",
        ] {
            let p = page(bad);
            assert!(
                p.starts_with("https://tidal.com/search?q=") && !p[27..].contains(['&', '#', '"', ' ', '/']),
                "{p}"
            );
        }
    }

    #[test]
    fn a_service_is_known_by_its_key() {
        for s in [Service::Server("casa: mia".into()), Service::Spotify, Service::Open(Site::Tidal)] {
            assert_eq!(Service::from_key(&s.key()), Some(s));
        }
        assert_eq!(Service::from_key("server:"), None);
    }
}
