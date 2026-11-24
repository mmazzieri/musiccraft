# The phone apps

`mobile/android` (Kotlin, Jetpack Compose, Media3) and `mobile/ios` (SwiftUI, AVFoundation) are thin
apps on MusicCraft's own library: the same Rust code as the desktop reads and merges the shared folder's
catalogue, so a phone is one more computer in [the library folder](library-format.md). It writes only its
own three files, with a random device id it makes once.

What they do in this first version: choose the MusicCraft folder, show albums, songs (with a search) and
playlists, play the songs whose file is on the phone in the background, with the system's media controls,
count plays and skips, and set love and ratings. Songs whose file is not on the phone are shown greyed.
Not yet: music servers, radio, Spotify, editing playlists, artwork.

## Where the folder is

- **Android**: the folder is chosen with the system's folder picker and reached through its document
  access, so it can be on the phone, on a memory card, or in a sync app that offers its folders to other
  apps (Syncthing, for one). A file is written by replacing its content: the document access has no atomic
  rename, so the app keeps its own copy of its files and a damaged file is set aside, not written over.
- **iOS**: the folder is chosen in Files; iCloud Drive and the sync apps that offer their folders in Files
  work. Reading and writing go through file coordination, and files are written atomically. Songs that
  iCloud has not downloaded are shown as not on the device.

## The bridge

`crates/ffi` builds `libmusiccraft_ffi`: a shared library for Android (`Core.call` through JNI) and a
static library for iOS (`musiccraft_call` and `musiccraft_free`, `mobile/ios/MusicCraft/MusicCraftCore.h`).
Both take one request in JSON and answer `{"ok": …}` or `{"error": "…"}`. The phone does the file work and
passes text in:

| `op` | Fields | Answers |
| --- | --- | --- |
| `open` | `device`, `device_name` | |
| `load` | `files`: every `Library/*.json` as `{name, text}`; `backups`: the app's own copy | `{songs, warnings, set_aside}`: rename each file in `set_aside` before writing it again |
| `found` | `files`: `{path, size}` below `Music/` | how many songs were added |
| `songs` | `search`, `sort`, `descending` | songs, each with `here` |
| `albums`, `playlists` | | lists |
| `song`, `playlist_songs` | `id` | |
| `set_loved`, `set_rating`, `played`, `skipped` | `id`, `loved` / `stars` | |
| `create_playlist`, `add_to_playlist`, `rename_playlist`, `delete_playlist` | | |
| `unsaved` | | `{name, text}` to write in `Library/` and in the app's own copy |
| `saved` | `names` | |
| `state` | | `{dirty, revision, songs}` |

Only `crates/ffi/src/android.rs` and `crates/ffi/src/c.rs` use `unsafe`: naming the functions for C and Java,
and taking and giving back C strings.

## Building

Android, on a computer with the Android SDK and NDK (Android Studio installs both):

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
cd mobile/android && ./gradlew assembleDebug    # or open the folder in Android Studio
```

iOS, on a Mac with Xcode:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
brew install xcodegen
cd mobile/ios && xcodegen generate && open MusicCraft.xcodeproj
```

Xcode builds the Rust library itself (`mobile/ios/build-rust.sh`). To run on an iPhone, choose a team in
Signing & Capabilities. The `Mobile` workflow builds both apps on every change to them, and keeps the
Android build as an artifact.
