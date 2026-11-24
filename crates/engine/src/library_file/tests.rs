use crate::Engine;
use crate::command::Command;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

/// A library file, in the exporting programs' layout, for the songs in `media` and one song
/// that was only streamed.
fn library_file(media: &Path) -> String {
    let mut files: Vec<_> = std::fs::read_dir(media).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    let url = |p: &Path| format!("file://{}", p.display().to_string().replace(' ', "%20"));
    let mut tracks = String::new();
    for (i, f) in files.iter().enumerate() {
        write!(
            tracks,
            "<key>{n}</key><dict><key>Track ID</key><integer>{n}</integer><key>Name</key><string>x</string>\
             <key>Play Count</key><integer>{plays}</integer><key>Rating</key><integer>100</integer>\
             <key>Date Added</key><date>2010-01-0{d}T00:00:00Z</date>\
             <key>Location</key><string>{loc}</string></dict>",
            n = i + 1,
            plays = 5 + i,
            d = i + 1,
            loc = url(f)
        )
        .unwrap();
    }
    tracks.push_str(
        "<key>9</key><dict><key>Track ID</key><integer>9</integer><key>Name</key><string>Lontano</string>\
         <key>Artist</key><string>Ada Neri</string><key>Loved</key><true/><key>Track Type</key><string>Remote</string></dict>",
    );
    let item = |n: u32| format!("<dict><key>Track ID</key><integer>{n}</integer></dict>");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
         <key>Music Folder</key><string>{folder}/</string><key>Tracks</key><dict>{tracks}</dict>\
         <key>Playlists</key><array>\
         <dict><key>Name</key><string>Library</string><key>Master</key><true/></dict>\
         <dict><key>Name</key><string>In viaggio</string><key>Playlist Items</key><array>{a}{b}{c}</array></dict>\
         <dict><key>Name</key><string>Preferite</string><key>Smart Info</key><data>AAAA</data>\
         <key>Playlist Items</key><array>{c}</array></dict>\
         </array></dict></plist>",
        folder = url(media.parent().unwrap()),
        a = item(9),
        b = item(1),
        c = item(2),
    )
}

/// Songs come with their files, plays, ratings and love; a song without a file stays one; the
/// playlists come along. Importing the same file again doubles nothing.
#[test]
fn a_library_file_comes_in_once() {
    let (outside, root, dir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let media = outside.path().join("Media files/Music");
    crate::demo::write_album(&media, 0, 2, 1).unwrap();
    let xml = outside.path().join("Library.xml");
    std::fs::write(&xml, library_file(&media)).unwrap();
    let mut e = Engine::isolated(dir.path(), Some(root.path().to_owned()));
    for _ in 0..2 {
        e.run(Command::LibraryFileImport { path: xml.clone() }).unwrap();
        e.settle(Duration::from_secs(30));
    }
    let lib = e.library().unwrap();
    assert_eq!(lib.len(), 3);
    let with_file: Vec<_> = lib.tracks().filter(|t| !t.info.path.is_empty()).collect();
    assert_eq!(with_file.len(), 2, "both files were copied into the music folder");
    assert!(with_file.iter().all(|t| t.rating == 5 && (5..=6).contains(&t.plays)), "history taken once");
    assert!(with_file.iter().all(|t| t.info.title != "x"), "the file's own tags win");
    let far = lib.tracks().find(|t| t.info.title == "Lontano").unwrap();
    assert!(far.info.path.is_empty() && far.loved);
    let lists = lib.playlists();
    assert_eq!(lists.len(), 2, "a second import does not double the playlists");
    let trip = lists.iter().find(|p| p.name == "In viaggio").unwrap();
    assert_eq!(trip.items.len(), 3);
    assert_eq!(trip.items[0].id, far.id, "the order of the file is kept");
}

/// A file that is not a library file is an error, and the library stays as it was.
#[test]
fn a_wrong_file_changes_nothing() {
    let (outside, root, dir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let wrong = outside.path().join("Library.xml");
    std::fs::write(&wrong, "<plist><dict><key>Name</key><string>x</string></dict></plist>").unwrap();
    let mut e = Engine::isolated(dir.path(), Some(root.path().to_owned()));
    e.run(Command::LibraryFileImport { path: wrong }).unwrap();
    e.settle(Duration::from_secs(30));
    assert!(e.library().unwrap().is_empty());
    assert!(e.notices.iter().any(|n| n.error));
}
