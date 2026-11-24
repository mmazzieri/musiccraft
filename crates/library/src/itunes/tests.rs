use super::{candidates, read};
use std::path::Path;

/// A small library file in the layout the exporting programs write, made up for this test.
const FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Major Version</key><integer>1</integer>
	<key>Music Folder</key><string>file://localhost/C:/Users/ada/Music/iTunes/iTunes%20Media/</string>
	<key>Tracks</key>
	<dict>
		<key>101</key>
		<dict>
			<key>Track ID</key><integer>101</integer>
			<key>Name</key><string>Alba sul mare</string>
			<key>Artist</key><string>Ada Neri</string>
			<key>Album</key><string>Coste</string>
			<key>Track Number</key><integer>3</integer>
			<key>Total Time</key><integer>201000</integer>
			<key>Date Added</key><date>2015-03-01T09:30:00Z</date>
			<key>Play Count</key><integer>12</integer>
			<key>Play Date UTC</key><date>2024-06-02T20:00:00Z</date>
			<key>Skip Count</key><integer>2</integer>
			<key>Rating</key><integer>80</integer>
			<key>Loved</key><true/>
			<key>Track Type</key><string>File</string>
			<key>Location</key><string>file://localhost/C:/Users/ada/Music/iTunes/iTunes%20Media/Music/Ada%20Neri/Coste/03%20Alba.m4a</string>
		</dict>
		<key>102</key>
		<dict>
			<key>Track ID</key><integer>102</integer>
			<key>Name</key><string>Vento</string>
			<key>Artist</key><string>Bea Rossi</string>
			<key>Rating</key><integer>60</integer>
			<key>Rating Computed</key><true/>
			<key>Track Type</key><string>Remote</string>
		</dict>
		<key>103</key>
		<dict>
			<key>Track ID</key><integer>103</integer>
			<key>Name</key><string>A film</string>
			<key>Has Video</key><true/>
			<key>Location</key><string>file:///Users/ada/Movies/film.m4v</string>
		</dict>
		<key>104</key>
		<dict>
			<key>Track ID</key><integer>104</integer>
			<key>Name</key><string>Radio Uno</string>
			<key>Track Type</key><string>URL</string>
			<key>Location</key><string>https://radio.example/stream</string>
		</dict>
	</dict>
	<key>Playlists</key>
	<array>
		<dict>
			<key>Name</key><string>Library</string><key>Master</key><true/>
			<key>Playlist Items</key><array><dict><key>Track ID</key><integer>101</integer></dict></array>
		</dict>
		<dict>
			<key>Name</key><string>Music</string><key>Distinguished Kind</key><integer>4</integer>
			<key>Playlist Items</key><array><dict><key>Track ID</key><integer>101</integer></dict></array>
		</dict>
		<dict><key>Name</key><string>Estate</string><key>Folder</key><true/></dict>
		<dict>
			<key>Name</key><string>Viaggio</string>
			<key>Playlist Items</key><array>
				<dict><key>Track ID</key><integer>102</integer></dict>
				<dict><key>Track ID</key><integer>103</integer></dict>
				<dict><key>Track ID</key><integer>101</integer></dict>
			</array>
		</dict>
		<dict>
			<key>Name</key><string>Top 25</string>
			<key>Smart Info</key><data>AQEAAwAAAAIAAAAZ</data>
			<key>Playlist Items</key><array><dict><key>Track ID</key><integer>101</integer></dict></array>
		</dict>
	</array>
</dict>
</plist>
"#;

/// Songs come with their history, the program's own lists and folders are left out, a smart
/// playlist keeps its songs, videos stay out of the music and radio becomes a station.
#[test]
fn reads_songs_history_and_playlists() {
    let e = read(FILE.as_bytes()).unwrap();
    assert_eq!(e.songs.len(), 2);
    assert_eq!(e.not_music, 1);
    let alba = e.songs.iter().find(|s| s.key == 101).unwrap();
    assert_eq!((alba.info.title.as_str(), alba.info.track), ("Alba sul mare", 3));
    let h = &alba.history;
    assert!(h.loved && h.rating == 4 && h.plays == 12 && h.skips == 2);
    assert!(h.added > 0 && h.last_played > h.added, "dates read and in order");
    let vento = e.songs.iter().find(|s| s.key == 102).unwrap();
    assert_eq!(vento.history.rating, 0, "a rating worked out from the album is not taken");
    assert!(vento.location.is_empty());
    let names: Vec<&str> = e.playlists.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Viaggio", "Top 25"]);
    assert_eq!(e.playlists[0].songs, [102, 101], "the video is not in it, the order is kept");
    assert!(e.playlists[1].smart && !e.playlists[0].smart);
    assert_eq!(e.stations, [("Radio Uno".to_owned(), "https://radio.example/stream".to_owned())]);
}

/// Addresses of both systems become paths, and a file that is not where the address says is
/// looked for below the folder the library file is in.
#[test]
fn locations_become_paths() {
    let e = read(FILE.as_bytes()).unwrap();
    let alba = e.songs.iter().find(|s| s.key == 101).unwrap();
    let found = candidates(&alba.location, &e.music_folder, Path::new("/mnt/copy"));
    assert_eq!(found[0], Path::new("C:/Users/ada/Music/iTunes/iTunes Media/Music/Ada Neri/Coste/03 Alba.m4a"));
    assert_eq!(found[1], Path::new("/mnt/copy/iTunes Media/Music/Ada Neri/Coste/03 Alba.m4a"));
    assert_eq!(candidates("file:///Users/ada/x%C3%A8.mp3", "", Path::new("."))[0], Path::new("/Users/ada/xè.mp3"));
    for bad in ["https://x/y.mp3", "file://", "file:///a%2", "file:///a%00b", "file:///%FF"] {
        assert!(candidates(bad, "", Path::new(".")).is_empty(), "{bad}");
    }
    let up = candidates("file:///M/../../etc/x.mp3", "file:///M/", Path::new("/b"));
    assert_eq!(up.len(), 1, "no way up out of the library file's folder");
}

/// Anything that is not a library file is refused without a panic.
#[test]
fn junk_is_refused() {
    for junk in [&b""[..], b"bplist00", b"<plist><dict><key>Tracks</key><array/></dict></plist>", b"\xff\xfe<", b"<<<<"]
    {
        assert!(read(junk).is_err());
    }
    let odd = FILE.replace("<integer>101</integer>", "<integer>99999999999999999999999999</integer>");
    assert!(read(odd.as_bytes()).is_ok());
}
