//! Tags and pictures of music files, through `lofty`.

use crate::model::Info;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagItem};
use std::path::Path;

/// The largest picture read from a file or a folder.
const MAX_PICTURE: u64 = 24 * 1024 * 1024;
/// The most lyrics read from a file or written into one, in bytes.
pub const MAX_LYRICS: usize = 64 * 1024;

/// Reads the tags of `file`; `rel` is its path below `Music/`. Fields without a tag are filled
/// from the path.
pub fn read(file: &Path, rel: &str) -> Result<Info, String> {
    let tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let mut info = from_path(rel);
    info.guessed = false;
    let props = tagged.properties();
    info.duration_ms = u64::try_from(props.duration().as_millis()).unwrap_or(0);
    info.bitrate = props.audio_bitrate().or(props.overall_bitrate()).unwrap_or(0);
    info.size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return Ok(info) };
    let set = |slot: &mut String, value: Option<String>| {
        if let Some(v) = value.map(|v| v.trim().trim_matches('\0').to_owned()).filter(|v| !v.is_empty()) {
            *slot = v;
        }
    };
    set(&mut info.title, tag.title().map(|v| v.into_owned()));
    set(&mut info.artist, tag.artist().map(|v| v.into_owned()));
    set(&mut info.album, tag.album().map(|v| v.into_owned()));
    set(&mut info.genre, tag.genre().map(|v| v.into_owned()));
    set(&mut info.album_artist, tag.get_string(ItemKey::AlbumArtist).map(str::to_owned));
    set(&mut info.composer, tag.get_string(ItemKey::Composer).map(str::to_owned));
    set(&mut info.isrc, tag.get_string(ItemKey::Isrc).map(str::to_owned));
    if let Some(n) = tag.track() {
        info.track = n;
    }
    if let Some(n) = tag.disk() {
        info.disc = n;
    }
    for key in [ItemKey::RecordingDate, ItemKey::Year, ItemKey::OriginalReleaseDate] {
        if let Some(y) = tag.get_string(key).and_then(|v| v.get(..4)).and_then(|v| v.parse().ok()) {
            info.year = y;
            break;
        }
    }
    Ok(info)
}

/// What the path says: `Artist/Album/03 Title.mp3`.
pub fn from_path(rel: &str) -> Info {
    let parts: Vec<&str> = rel.split('/').collect();
    let file = parts.last().copied().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let digits: String = stem.chars().take_while(char::is_ascii_digit).collect();
    let rest = stem.get(digits.len()..).unwrap_or("").trim_start_matches([' ', '-', '.', '_']);
    let (track, title) = match digits.parse::<u32>() {
        Ok(n) if digits.len() <= 3 && !rest.is_empty() => (n, rest),
        _ => (0, stem),
    };
    let at = |i: usize| parts.len().checked_sub(i).and_then(|k| parts.get(k)).copied().unwrap_or("").to_owned();
    let (artist, album) = match parts.len() {
        0 | 1 => (String::new(), String::new()),
        2 => (at(2), String::new()),
        _ => (at(3), at(2)),
    };
    Info { path: rel.to_owned(), title: title.to_owned(), artist, album, track, guessed: true, ..Info::default() }
}

/// Writes the person's edits into the file's own tags.
pub fn write(file: &Path, info: &Info) -> Result<(), String> {
    let mut tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let tag = main_tag(&mut tagged)?;
    tag.set_title(info.title.clone());
    tag.set_artist(info.artist.clone());
    tag.set_album(info.album.clone());
    tag.set_genre(info.genre.clone());
    for (key, value) in [(ItemKey::AlbumArtist, &info.album_artist), (ItemKey::Composer, &info.composer)] {
        tag.remove_key(key);
        if !value.is_empty() {
            tag.insert_text(key, value.clone());
        }
    }
    for key in [ItemKey::RecordingDate, ItemKey::Year] {
        tag.remove_key(key);
    }
    if info.year > 0 {
        tag.insert_text(ItemKey::RecordingDate, info.year.to_string());
    }
    if info.track > 0 {
        tag.set_track(info.track);
    } else {
        tag.remove_track();
    }
    if info.disc > 0 {
        tag.set_disk(info.disc);
    } else {
        tag.remove_disk();
    }
    tag.save_to_path(file, WriteOptions::default()).map_err(|e| e.to_string())
}

/// The unsynchronised lyrics inside the file, empty when it has none. Longer lyrics are cut at
/// [`MAX_LYRICS`]. They come from the tag [`read`] takes the other fields from, the one
/// [`write_lyrics`] writes, so lyrics left in another tag are never shown again.
pub fn lyrics(file: &Path) -> Result<String, String> {
    let tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return Ok(String::new()) };
    let found = [ItemKey::UnsyncLyrics, ItemKey::Lyrics]
        .into_iter()
        .flat_map(|k| tag.get_strings(k))
        .map(|v| v.trim_matches(['\0', ' ', '\r', '\n', '\t']))
        .find(|v| !v.is_empty())
        .unwrap_or("");
    let mut end = found.len().min(MAX_LYRICS);
    while !found.is_char_boundary(end) {
        end -= 1;
    }
    Ok(found.get(..end).unwrap_or("").replace("\r\n", "\n").replace('\r', "\n"))
}

/// Writes the lyrics into the file's own tags; empty text takes them away.
pub fn write_lyrics(file: &Path, text: &str) -> Result<(), String> {
    if text.len() > MAX_LYRICS {
        return Err(format!("lyrics are kept up to {} KB", MAX_LYRICS / 1024));
    }
    let text: String = text.chars().filter(|c| *c != '\0').collect();
    let text = text.trim_end();
    let mut tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let keys = [ItemKey::Lyrics, ItemKey::UnsyncLyrics];
    let tag = main_tag(&mut tagged)?;
    for k in keys {
        tag.remove_key(k);
    }
    // Each kind of tag has its own name for them; ID3v2 keeps unsynchronised ones apart.
    if !text.is_empty()
        && !tag.insert_text(ItemKey::Lyrics, text.to_owned())
        && !tag.insert_text(ItemKey::UnsyncLyrics, text.to_owned())
    {
        return Err("this kind of file cannot hold lyrics".to_owned());
    }
    tag.save_to_path(file, WriteOptions::default()).map_err(|e| e.to_string())
}

/// The tag of the file's main kind, made when missing from what the file's other tag says, so
/// that an edit keeps the fields and lyrics written there.
fn main_tag(tagged: &mut lofty::file::TaggedFile) -> Result<&mut Tag, String> {
    if tagged.primary_tag().is_none() {
        let mut fresh = Tag::new(tagged.primary_tag_type());
        if let Some(other) = tagged.first_tag() {
            for item in other.items() {
                if fresh.push(item.clone()) {
                    continue;
                }
                // Lyrics have a different key in each kind of tag (APE `Lyrics`, ID3v2 USLT).
                let other_key = match item.key() {
                    ItemKey::Lyrics => ItemKey::UnsyncLyrics,
                    ItemKey::UnsyncLyrics => ItemKey::Lyrics,
                    _ => continue,
                };
                fresh.push(TagItem::new(other_key, item.value().clone()));
            }
            for picture in other.pictures() {
                fresh.push_picture(picture.clone());
            }
        }
        tagged.insert_tag(fresh);
    }
    tagged.primary_tag_mut().ok_or_else(|| "this kind of file has no tags".to_owned())
}

/// The front cover inside the file, or a picture beside it.
pub fn picture(file: &Path) -> Option<Vec<u8>> {
    if let Ok(tagged) = lofty::read_from_path(file) {
        let pictures = || tagged.tags().iter().flat_map(Tag::pictures);
        let best = pictures().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pictures().next());
        if let Some(p) = best.filter(|p| u64::try_from(p.data().len()).is_ok_and(|n| n <= MAX_PICTURE)) {
            return Some(p.data().to_vec());
        }
    }
    let dir = file.parent()?;
    for name in ["cover.jpg", "cover.png", "folder.jpg", "front.jpg", "cover.jpeg"] {
        let p = dir.join(name);
        if let Ok(m) = std::fs::metadata(&p)
            && m.len() <= MAX_PICTURE
            && !crate::cloud::online_only(&m)
            && let Ok(bytes) = std::fs::read(&p)
        {
            return Some(bytes);
        }
    }
    None
}

/// How much louder or softer to play a song so that songs sound about as loud as each other: a
/// factor for the samples, from the loudness the file's tags give (ReplayGain, or the volume
/// adjustment iTunes writes as `iTunNORM`). `None` when the file says nothing about it.
///
/// The factor stays between −20 and +10 dB and, when the tags give the song's peak, never makes
/// the peak clip.
pub fn leveling(file: &Path) -> Option<f32> {
    let tagged = lofty::read_from_path(file).ok()?;
    let mut found = None;
    for tag in tagged.tags() {
        let gain = tag.get_string(ItemKey::ReplayGainTrackGain).and_then(parse_db);
        if let Some(db) = gain {
            let peak = tag.get_string(ItemKey::ReplayGainTrackPeak).and_then(|v| v.trim().parse::<f32>().ok());
            found = Some((db, peak));
            break;
        }
        if found.is_none() {
            found = tag
                .items()
                .filter(|i| i.key() == ItemKey::Comment && i.description().eq_ignore_ascii_case("iTunNORM"))
                .find_map(|i| i.value().text().and_then(itunnorm));
        }
    }
    if found.is_none() && tagged.file_type() == lofty::file::FileType::Mp4 {
        found = mp4_itunnorm(file);
    }
    let (db, peak) = found?;
    Some(factor(db, peak))
}

/// The factor for a gain in dB and a peak (1.0 is full scale), within the limits above.
fn factor(db: f32, peak: Option<f32>) -> f32 {
    const LEAST: f32 = 0.1; // −20 dB
    let mut f = 10f32.powf(db.clamp(-20.0, 10.0) / 20.0);
    if let Some(peak) = peak.filter(|p| p.is_finite() && *p > 0.0) {
        // A peak far above full scale is a wrong tag, not a reason to play the song in a whisper.
        f = f.min(1.0 / peak).max(LEAST);
    }
    if f.is_finite() && f > 0.0 { f } else { 1.0 }
}

/// "-6.54 dB" as -6.54.
fn parse_db(text: &str) -> Option<f32> {
    let t = text.trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).unwrap_or(t);
    t.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// iTunes' volume adjustment: ten hexadecimal numbers; the first two are the loudness of each
/// channel in thousandths of a reference level, the seventh and eighth their peaks out of 32768.
fn itunnorm(text: &str) -> Option<(f32, Option<f32>)> {
    // A field that is not a number makes the whole tag doubtful: the others would shift places.
    let values =
        text.split_whitespace().take(10).map(|v| u32::from_str_radix(v, 16).ok()).collect::<Option<Vec<u32>>>()?;
    let loudness = (*values.first()?).max(*values.get(1)?);
    if loudness == 0 {
        return None;
    }
    let db = -10.0 * (loudness as f32 / 1000.0).log10();
    let peak = match (values.get(6), values.get(7)) {
        (Some(a), Some(b)) if (*a).max(*b) > 0 => Some((*a).max(*b) as f32 / 32768.0),
        _ => None,
    };
    db.is_finite().then_some((db, peak))
}

/// iTunes keeps its adjustment in an MP4 atom of its own, which the common tag leaves out.
fn mp4_itunnorm(file: &Path) -> Option<(f32, Option<f32>)> {
    use lofty::config::ParseOptions;
    use lofty::mp4::{AtomData, AtomIdent, Mp4File};
    let mut reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let mp4 = Mp4File::read_from(&mut reader, ParseOptions::new().read_properties(false)).ok()?;
    let ident = AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: "iTunNORM".into() };
    let atom = mp4.ilst()?.get(&ident)?;
    atom.data().find_map(|d| match d {
        AtomData::UTF8(t) | AtomData::UTF16(t) => itunnorm(t),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::{factor, from_path, itunnorm, leveling, lyrics, parse_db, read, write_lyrics};
    use lofty::config::WriteOptions;
    use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};

    /// A WAV file of a few silent samples.
    fn wav(path: &std::path::Path) {
        let data = [128u8; 64];
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0]);
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 8, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        std::fs::write(path, b).unwrap();
    }

    /// Lyrics written into a file whose only tag is of another kind keep what that tag says.
    #[test]
    fn lyrics_keep_what_another_tag_says() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("song.wav");
        wav(&file);
        let mut info = Tag::new(TagType::RiffInfo);
        info.set_title("Titolo vero".to_owned());
        info.save_to_path(&file, WriteOptions::default()).unwrap();
        assert_eq!(lyrics(&file).unwrap(), "");
        write_lyrics(&file, "Nuovo testo").unwrap();
        assert_eq!(read(&file, "song.wav").unwrap().title, "Titolo vero");
        assert_eq!(lyrics(&file).unwrap(), "Nuovo testo");
        write_lyrics(&file, "").unwrap();
        assert_eq!(lyrics(&file).unwrap(), "");
        assert_eq!(read(&file, "song.wav").unwrap().title, "Titolo vero");
    }

    /// A song whose file is still in the cloud is listed under the artist, album and number its
    /// path gives, and a title that merely starts with digits keeps them.
    #[test]
    fn a_path_gives_artist_album_number_and_title() {
        let i = from_path("Ada Verdi/Maree/03 - Scirocco.flac");
        assert_eq!(
            (i.artist.as_str(), i.album.as_str(), i.track, i.title.as_str()),
            ("Ada Verdi", "Maree", 3, "Scirocco")
        );
        assert_eq!(from_path("A/B/1979.mp3").title, "1979");
        assert_eq!(from_path("lone.mp3").artist, "");
    }
    /// A loud song is turned down and a soft one up, by either kind of tag, and a file that says
    /// nothing is left alone.
    #[test]
    fn leveling_follows_the_tags() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.wav");
        wav(&plain);
        assert_eq!(leveling(&plain), None);

        let loud = dir.path().join("loud.wav");
        wav(&loud);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert_text(ItemKey::ReplayGainTrackGain, "-6.5 dB".to_owned());
        tag.save_to_path(&loud, WriteOptions::default()).unwrap();
        assert!(leveling(&loud).unwrap() < 1.0);

        // iTunes' adjustment for a soft song: a level of 300 thousandths on both channels.
        let soft = dir.path().join("soft.wav");
        wav(&soft);
        let mut tag = Tag::new(TagType::Id3v2);
        let norm = " 0000012C 0000012C 00000000 00000000 00000000 00000000 00001000 00001000 00000000 00000000";
        let mut item = TagItem::new(ItemKey::Comment, ItemValue::Text(norm.to_owned()));
        item.set_description("iTunNORM".to_owned());
        assert!(tag.insert(item));
        tag.save_to_path(&soft, WriteOptions::default()).unwrap();
        assert!(leveling(&soft).unwrap() > 1.0);
    }

    /// No tag makes a song blast or vanish, and a known peak never clips.
    #[test]
    fn leveling_stays_within_bounds() {
        assert!(factor(1000.0, None) <= 10f32.powf(0.5) + 1e-3);
        assert!(factor(-1000.0, None) >= 0.1 - 1e-3);
        assert!(factor(8.0, Some(0.9)) * 0.9 <= 1.0 + 1e-6);
        assert!((factor(f32::NAN, None) - 1.0).abs() < 1e-6);
        assert_eq!(parse_db("nonsense"), None);
        assert!(parse_db(" +2.25 dB ").is_some_and(|v| v > 2.0));
        assert_eq!(itunnorm("0 0 0"), None);
        assert_eq!(itunnorm("zz"), None);
        assert_eq!(itunnorm("12C 12C zz 0 0 0 8000 1000 0 0"), None);
        assert!(factor(0.0, Some(32768.0)) >= 0.1 - 1e-6);
        let (louder, _) = itunnorm("00000100 00000100").unwrap();
        let (softer, _) = itunnorm("00002000 00002000").unwrap();
        assert!(louder > softer);
    }
}
