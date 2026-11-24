# Privacy

MusicCraft has no account of its own, no telemetry and no update check.

- **Your library** stays in the folder you chose. A folder-sync service sees what you put there: the
  music files and the catalogue, which includes titles, play counts, ratings, playlists, stations and
  the names of the computers that share it.
- **On this computer** it keeps its settings (folder, server addresses and user names, the Spotify
  Client ID, volume, looks), a copy of its own catalogue files and the list of files already read.
- **Passwords, tokens and keys** are in the system's credential store.
- **Requests** go only to services you use: your music servers, the radio stations you play, the Radio
  Browser directory when you open *Find Stations*, and Spotify after you sign in. When a personal server or Spotify is the service in use, it is
  asked for the songs you play without a file, and for all of them when you choose *Find All Songs*. A
  service that opens in its own app receives the page or the search that is opened. Requests carry the program's
  name and version, and no personal details beyond what each sign-in needs. Playing a song from a music
  server tells that server it was played.
