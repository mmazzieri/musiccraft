# The library folder

```text
MusicCraft/
  Music/<Album artist>/<Album>/01 Title.ext
  Library/<device>.tracks.json
  Library/<device>.activity.json
  Library/<device>.playlists.json
```

Any file-sync service can share the folder. `<device>` is a random id that each computer makes once.
**A computer writes only the three files with its own id**, so a sync service never has two writers of one
file. Files are replaced whole (`craft-atomic`).

## Files

UTF-8 JSON:

```json
{
  "format": "musiccraft-library", "version": 1, "section": "activity",
  "device": "1f0c…", "device_name": "STUDIO",
  "records": { "track/9be1…": { "rating": { "v": 4, "t": 1795537008982 } } },
  "counters": { "9be1…": { "plays": 3, "skips": 1, "last": 1795537008982 } }
}
```

- A **cell** is a value `v` with the time `t` it was written, in milliseconds since 1970. A file holds the
  cells its own device wrote.
- **Merge**: for each record and field the cell with the greatest `(t, device)` wins. A new cell is stamped
  later than the one it replaces, even when that one came from a clock running ahead.
- **Counters** belong to one device and only grow; what is shown is the sum over devices. Two files of one
  device (a conflict copy made by a sync service) count once, by the greatest value.
- `imported` has the form of a counter and is added to the sum; importing again replaces it. Playlists of
  another program have an id made from theirs, so they are not made twice.
- Reading is idempotent, so conflict copies need no handling. A file that cannot be read is passed over and
  named in Settings; a damaged file of this computer is set aside (`*.damaged-<time>`) and its copy on this
  computer is used.

| Record | Fields | Section |
| --- | --- | --- |
| `track/<id>` | `info` (path, tags, size, duration), `added`, `link:<service>` | tracks |
| `track/<id>` | `loved`, `rating` (0–5), `removed`, `imported` (counts from another program) | activity |
| `playlist/<id>` | `name`, `created`, `items` or `smart`, `removed` | playlists |
| `station/<id>` | `station`, `removed` | playlists |

Fields are never removed or given another meaning within a version. Values a program does not know stay in
the file of the device that wrote them.

## Songs without a file

A song may have an empty `path`. Its id is made from its title, first artist and album without accents,
punctuation, guests and remaster notes, so two computers that add it agree. `link:<service>` says what the
song is on a service: `{ "id": …, "at": …, "chosen": true }`, with an empty `id` when it was looked for and
not found, and `null` when forgotten. Services are `server:<account name>`, `spotify` and `open:<site>`,
where the id is a web page of that site; pages of any other host are never opened. Only a personal server's
artwork id (`art`) is kept with a link. Signing out of Spotify forgets its links.

When a scan finds a file with the same names and a length within five seconds, the song takes it and keeps
its id. Programs that know only songs with files pass these records over.

## Songs

A song's id is made from its path below `Music/`, so two computers that find the same new file give it the
same id. A file that moved is recognised by size, title and album and keeps its id. Paths are stored in
Unicode NFC with `/`; on reading, a path with `..`, a drive, a backslash or an empty part is refused.

A scan reads tags only of new or changed files, and never of files that a sync service has not downloaded
(Windows: `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` or `OFFLINE`; macOS: `SF_DATALESS`). Until another computer
has read such a file, its artist, album and number are guessed from the path.
