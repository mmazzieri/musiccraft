use crate::session::{Session, run};
use musiccraft_library::Library;
use musiccraft_library::model::{Info, Presence};
use musiccraft_library::scan::ScanResult;
use serde_json::{Value, json};
use std::path::Path;

fn ask(slot: &mut Option<Session>, request: Value) -> Value {
    run(slot, serde_json::from_value(request).unwrap()).unwrap()
}

/// Every `*.json` in a library folder's `Library/`, as an app passes them.
fn files(root: &Path) -> Value {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root.join("Library")).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        out.push(json!({ "name": name, "text": std::fs::read_to_string(e.path()).unwrap() }));
    }
    Value::from(out)
}

fn write_unsaved(slot: &mut Option<Session>, root: &Path) {
    let unsaved = ask(slot, json!({ "op": "unsaved" }));
    let mut names = Vec::new();
    for f in unsaved.as_array().unwrap() {
        let name = f["name"].as_str().unwrap();
        std::fs::write(root.join("Library").join(name), f["text"].as_str().unwrap()).unwrap();
        names.push(name.to_owned());
    }
    ask(slot, json!({ "op": "saved", "names": names }));
}

/// A phone and a computer that share a folder see each other's songs, plays, love and playlists,
/// through the same files.
#[test]
fn a_phone_and_a_computer_converge() {
    let (shared, local) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut pc = Library::open(shared.path(), local.path(), "pc", "PC").unwrap();
    let mut scan = ScanResult::default();
    scan.presence.insert("A/B/1.mp3".into(), Presence::Here);
    scan.read.push(Info { path: "A/B/1.mp3".into(), title: "One".into(), size: 10, ..Info::default() });
    pc.apply_scan(scan);
    let id = pc.tracks().next().unwrap().id.clone();
    pc.record_play(&id).unwrap();
    pc.save().unwrap();

    let mut phone = None;
    ask(&mut phone, json!({ "op": "open", "device": "phone", "device_name": "Phone" }));
    ask(&mut phone, json!({ "op": "load", "files": files(shared.path()) }));
    ask(&mut phone, json!({ "op": "found", "files": [{ "path": "A/B/1.mp3", "size": 10 }, { "path": "C/2.mp3" }] }));
    let songs = ask(&mut phone, json!({ "op": "songs" }));
    assert_eq!(songs.as_array().unwrap().len(), 2, "the computer's song and the phone's new one");
    let one = ask(&mut phone, json!({ "op": "song", "id": id }));
    assert_eq!((one["title"].as_str(), one["here"].as_bool()), (Some("One"), Some(true)));
    ask(&mut phone, json!({ "op": "played", "id": id }));
    ask(&mut phone, json!({ "op": "set_loved", "id": id, "loved": true }));
    ask(&mut phone, json!({ "op": "create_playlist", "name": "Road", "songs": [id] }));
    write_unsaved(&mut phone, shared.path());
    assert_eq!(ask(&mut phone, json!({ "op": "state" }))["dirty"], json!(false));

    assert!(pc.refresh());
    let t = pc.track(&id).unwrap();
    assert!(t.loved && t.plays == 2, "plays on both are added");
    assert_eq!(pc.playlists().first().map(|p| p.name.as_str()), Some("Road"));
    assert!(pc.tracks().any(|t| t.info.path == "C/2.mp3" && t.info.guessed));
}

/// A damaged file of the phone's own is named for setting aside, and the app's copy is used.
#[test]
fn a_damaged_own_file_is_set_aside() {
    let mut phone = None;
    ask(&mut phone, json!({ "op": "open", "device": "phone", "device_name": "Phone" }));
    ask(&mut phone, json!({ "op": "load", "files": [] }));
    ask(&mut phone, json!({ "op": "found", "files": [{ "path": "1.mp3", "size": 1 }] }));
    let backups = ask(&mut phone, json!({ "op": "unsaved" }));
    let damaged = json!([{ "name": "phone.tracks.json", "text": "{\"format\":" }]);
    let answer = ask(&mut phone, json!({ "op": "load", "files": damaged, "backups": backups }));
    assert_eq!(answer["set_aside"], json!(["phone.tracks.json"]));
    assert_eq!(answer["songs"], json!(1), "the song comes back from the app's copy");
}

/// Junk requests are answered with errors, never a panic, through every way in.
#[test]
fn junk_never_panics() {
    for junk in [
        "",
        "null",
        "{}",
        "{\"op\":\"songs\"}",
        "{\"op\":\"nothing\"}",
        "{\"op\":\"open\",\"device\":\"../x\",\"device_name\":\"\"}",
        "{\"op\":\"open\",\"device\":\"d\",\"device_name\":\"\"}",
        "{\"op\":\"set_rating\",\"id\":\"\",\"stars\":255}",
        "{\"op\":\"load\",\"files\":[{\"name\":\"d.tracks.json\",\"text\":\"[]\"},{\"name\":\"x.json\",\"text\":\"\\u0000\"}]}",
        "{\"op\":\"found\",\"files\":[{\"path\":\"../../etc/passwd.mp3\"},{\"path\":\"\"},{\"path\":\"a//b.mp3\"}]}",
        "{\"op\":\"playlist_songs\",\"id\":\"x\"}",
        "{\"op\":\"saved\",\"names\":[\"../../x\"]}",
    ] {
        let answer: Value = serde_json::from_str(&crate::call(junk)).unwrap();
        assert!(answer.get("ok").is_some() || answer.get("error").is_some(), "{junk:?}");
    }
    let request = std::ffi::CString::new("{\"op\":\"state\"}").unwrap();
    // SAFETY: a valid string in, and the answer freed once.
    #[allow(unsafe_code)]
    unsafe {
        let answer = crate::c::musiccraft_call(request.as_ptr());
        assert!(std::ffi::CStr::from_ptr(answer).to_str().unwrap().contains("\"ok\""));
        crate::c::musiccraft_free(answer);
        assert!(!crate::c::musiccraft_call(std::ptr::null()).is_null());
    }
}
