//! What the services list, fetched in the background and kept for a while.

use crate::lock;
use musiccraft_services::spotify::{Device, Spotify};
use musiccraft_services::subsonic::{AlbumList, Server};
use musiccraft_services::{Entry, ServiceError, radio};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_secs(600);

/// A list a service can give.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Page {
    ServerAlbums(String, AlbumList),
    ServerAlbum(String, String),
    ServerPlaylists(String),
    ServerPlaylist(String, String),
    ServerSearch(String, String),
    SpotifySaved,
    SpotifyPlaylists,
    SpotifyPlaylist(String),
    SpotifySearch(String),
    SpotifyDevices,
    Radio(String),
}

#[derive(Debug, Clone, Default)]
pub struct Content {
    pub songs: Vec<Entry>,
    pub albums: Vec<Entry>,
    pub devices: Vec<Device>,
}

#[derive(Clone)]
pub enum PageState {
    Loading,
    Ready(Arc<Content>),
    Failed(String),
}

/// What a page is fetched with.
pub enum Via {
    Server(Server),
    Spotify(Arc<Spotify>),
    Nothing,
}

#[derive(Clone, Default)]
pub struct Pages(Arc<Mutex<HashMap<Page, (PageState, Instant)>>>);

impl Pages {
    /// The page, fetching it when it is not there or old.
    pub fn get(&self, page: &Page, via: impl FnOnce() -> Option<Via>) -> PageState {
        let mut map = lock(&self.0);
        if let Some((state, at)) = map.get(page)
            && (at.elapsed() < FRESH || matches!(state, PageState::Loading))
        {
            return state.clone();
        }
        let Some(via) = via() else {
            return PageState::Failed("not signed in".to_owned());
        };
        map.insert(page.clone(), (PageState::Loading, Instant::now()));
        map.retain(|_, (s, at)| at.elapsed() < FRESH * 2 || matches!(s, PageState::Loading));
        drop(map);
        let (all, key) = (self.clone(), page.clone());
        let started = crate::spawn("page", move || {
            let state = match fetch(&key, &via) {
                Ok(c) => PageState::Ready(Arc::new(c)),
                Err(e) => PageState::Failed(e.to_string()),
            };
            lock(&all.0).insert(key, (state, Instant::now()));
        });
        if let Err(e) = started {
            let failed = PageState::Failed(e);
            lock(&self.0).insert(page.clone(), (failed.clone(), Instant::now()));
            return failed;
        }
        PageState::Loading
    }

    /// Drops everything, to fetch it again.
    pub fn clear(&self) {
        lock(&self.0).retain(|_, (s, _)| matches!(s, PageState::Loading));
    }

    pub fn is_busy(&self) -> bool {
        lock(&self.0).values().any(|(s, _)| matches!(s, PageState::Loading))
    }
}

fn fetch(page: &Page, via: &Via) -> Result<Content, ServiceError> {
    let mut c = Content::default();
    match (page, via) {
        (Page::ServerAlbums(_, list), Via::Server(s)) => {
            // A large server is listed up to 2,000 albums.
            for i in 0..10 {
                let more = s.albums(*list, i * 200)?;
                let n = more.len();
                c.albums.extend(more);
                if n < 200 || *list == AlbumList::Random {
                    break;
                }
            }
        }
        (Page::ServerAlbum(_, id), Via::Server(s)) => c.songs = s.album(id)?,
        (Page::ServerPlaylists(_), Via::Server(s)) => c.albums = s.playlists()?,
        (Page::ServerPlaylist(_, id), Via::Server(s)) => c.songs = s.playlist(id)?,
        (Page::ServerSearch(_, q), Via::Server(s)) => (c.songs, c.albums) = s.search(q)?,
        (Page::SpotifySaved, Via::Spotify(s)) => c.songs = s.saved()?,
        (Page::SpotifyPlaylists, Via::Spotify(s)) => c.albums = s.playlists()?,
        (Page::SpotifyPlaylist(id), Via::Spotify(s)) => c.songs = s.playlist(id)?,
        (Page::SpotifySearch(q), Via::Spotify(s)) => c.songs = s.search(q)?,
        (Page::SpotifyDevices, Via::Spotify(s)) => c.devices = s.devices()?,
        (Page::Radio(q), _) => c.songs = radio::search(q)?,
        _ => return Err(ServiceError::SignIn),
    }
    Ok(c)
}
