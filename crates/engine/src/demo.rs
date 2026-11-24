//! A demonstration library, generated here: invented artists, synthesized sound and drawn
//! covers. Used by `--demo`, by the screenshots in the documentation and by the tests.

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag, TagExt};
use std::path::{Path, PathBuf};

const RATE: u32 = 8_000;

struct DemoAlbum {
    artist: &'static str,
    title: &'static str,
    genre: &'static str,
    year: u32,
    songs: &'static [&'static str],
}

/// Invented for this project; any likeness to real artists is chance.
const ALBUMS: &[DemoAlbum] = &[
    DemoAlbum {
        artist: "Lanterna Blu",
        title: "Porti d'inverno",
        genre: "Ambient",
        year: 2021,
        songs: &["Molo Nove", "Sale e vetro", "Faro spento", "Le gru dormono", "Risacca", "Alba di lamiera"],
    },
    DemoAlbum {
        artist: "The Paper Meridians",
        title: "Cartographers",
        genre: "Indie",
        year: 2019,
        songs: &["Fold Lines", "Unmapped", "Compass Rose", "Legend and Scale", "Margins", "Here Be Dragons"],
    },
    DemoAlbum {
        artist: "Ottavia Serpi",
        title: "Quaderno a righe",
        genre: "Songwriter",
        year: 2023,
        songs: &["Prima pagina", "Inchiostro", "La tua grafia", "Cancellature", "Punto e a capo"],
    },
    DemoAlbum {
        artist: "Kestrel Circuit",
        title: "Low Orbit",
        genre: "Electronic",
        year: 2024,
        songs: &["Ignition", "Telemetry", "Apogee", "Quiet Side", "Re-entry", "Splashdown", "Afterburn"],
    },
    DemoAlbum {
        artist: "Quartetto Alabastro",
        title: "Notturni per archi",
        genre: "Classical",
        year: 2016,
        songs: &["Notturno I", "Notturno II", "Notturno III", "Notturno IV"],
    },
    DemoAlbum {
        artist: "Bluma & the Tin Cities",
        title: "Brass Weather",
        genre: "Jazz",
        year: 2018,
        songs: &["Humid Forecast", "Copper Rain", "Barometer Blues", "Clearing Up", "Second Line Storm"],
    },
    DemoAlbum {
        artist: "Nonna Elettrica",
        title: "Tortellini in orbita",
        genre: "Funk",
        year: 2022,
        songs: &["Sfoglia", "Mattarello groove", "Brodo cosmico", "Domenica", "Il bis"],
    },
    DemoAlbum {
        artist: "Field Notes Choir",
        title: "Hedgerow",
        genre: "Folk",
        year: 2015,
        songs: &["Bramble", "Stile", "Lapwing", "The Long Furrow", "Gate at Dusk"],
    },
    DemoAlbum {
        artist: "Kestrel Circuit",
        title: "Ground Station",
        genre: "Electronic",
        year: 2020,
        songs: &["Handshake", "Dish", "Static Garden", "Downlink", "Signal Lost"],
    },
    DemoAlbum {
        artist: "Via Pal",
        title: "Cortile interno",
        genre: "Pop",
        year: 2025,
        songs: &["Panni stesi", "Citofono", "Ringhiera", "Estate al quinto piano", "Ascensore"],
    },
    DemoAlbum {
        artist: "Ivory Plateau",
        title: "Glass Mountain",
        genre: "Post-rock",
        year: 2017,
        songs: &["Basecamp", "Switchbacks", "Thin Air", "Summit in Cloud"],
    },
    DemoAlbum {
        artist: "Lanterna Blu",
        title: "Maree lente",
        genre: "Ambient",
        year: 2024,
        songs: &["Plenilunio", "Bassa marea", "Conchiglie", "Boe", "Orizzonte piatto"],
    },
];

/// Writes the demonstration library's music below `root`, unless it is already there.
pub fn write_library(root: &Path) -> Result<(), String> {
    let done = root.join("demo-complete");
    if done.exists() {
        return Ok(());
    }
    for (i, a) in ALBUMS.iter().enumerate() {
        let dir = root.join("Music").join(safe(a.artist)).join(safe(a.title));
        write_album(&dir, i, a.songs.len(), 20)?;
    }
    std::fs::write(done, b"").map_err(|e| e.to_string())
}

fn safe(name: &str) -> String {
    musiccraft_library::import::safe(name, "Demo")
}

/// Writes the first `count` songs of one album, each some `seconds` long.
pub fn write_album(dir: &Path, album: usize, count: usize, seconds: u32) -> Result<(), String> {
    let a = ALBUMS.get(album).ok_or("there is no such demonstration album")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let cover = cover(album)?;
    for (i, title) in a.songs.iter().take(count).enumerate() {
        let n = u32::try_from(i + 1).unwrap_or(1);
        let seed = u32::try_from(album).unwrap_or(0) * 31 + n;
        let path: PathBuf = dir.join(format!("{n:02} {}.wav", safe(title)));
        let length = seconds + seed * 7 % (seconds + 1);
        std::fs::write(&path, wav(seed, length)).map_err(|e| e.to_string())?;
        tag(&path, a, title, n, &cover)?;
    }
    Ok(())
}

fn tag(path: &Path, a: &DemoAlbum, title: &str, n: u32, cover: &[u8]) -> Result<(), String> {
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let mut t = Tag::new(file.primary_tag_type());
    t.set_title(title.to_owned());
    t.set_artist(a.artist.to_owned());
    t.set_album(a.title.to_owned());
    t.set_genre(a.genre.to_owned());
    t.set_track(n);
    t.insert_text(ItemKey::RecordingDate, a.year.to_string());
    t.push_picture(
        Picture::unchecked(cover.to_vec()).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build(),
    );
    t.save_to_path(path, WriteOptions::default()).map_err(|e| e.to_string())
}

/// A short arpeggio over a soft bass, 8-bit mono.
fn wav(seed: u32, seconds: u32) -> Vec<u8> {
    let scale = [0, 3, 5, 7, 10, 12, 15, 17];
    let root = 196.0 * 2f64.powf(f64::from(seed % 12) / 12.0);
    let samples = RATE * seconds;
    let mut pcm = Vec::with_capacity(samples as usize);
    let step = RATE / 4;
    for i in 0..samples {
        let t = f64::from(i) / f64::from(RATE);
        let beat = i / step;
        let pick = (beat.wrapping_mul(seed | 1) / 3 + beat * 5) % 8;
        let note = scale.get(pick as usize).copied().unwrap_or(0);
        let freq = root * 2f64.powf(f64::from(note) / 12.0);
        let fade = 1.0 - f64::from(i % step) / f64::from(step);
        let wave = |f: f64| (t * f * std::f64::consts::TAU).sin();
        let edge = (t.min(f64::from(seconds) - t) * 2.0).clamp(0.0, 1.0);
        let v = (wave(freq) * 0.5 * fade + wave(root / 2.0) * 0.25) * edge;
        pcm.push((128.0 + v * 110.0).clamp(0.0, 255.0) as u8);
    }
    let len = u32::try_from(pcm.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(pcm.len() + 46);
    out.extend(b"RIFF");
    out.extend((36 + len + len % 2).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    for half in [1u16, 1] {
        out.extend(half.to_le_bytes());
    }
    out.extend(RATE.to_le_bytes());
    out.extend(RATE.to_le_bytes());
    for half in [1u16, 8] {
        out.extend(half.to_le_bytes());
    }
    out.extend(b"data");
    out.extend(len.to_le_bytes());
    out.extend(&pcm);
    if len % 2 == 1 {
        out.push(0);
    }
    out
}

/// An abstract cover, different for each album.
fn cover(album: usize) -> Result<Vec<u8>, String> {
    const N: u32 = 320;
    let a = album as f32;
    let hue = |h: f32, s: f32, v: f32| -> [f32; 3] {
        let f = |n: f32| {
            let k = (n + h * 6.0) % 6.0;
            v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
        };
        [f(5.0), f(3.0), f(1.0)]
    };
    let h = (a * 0.618_034) % 1.0;
    let (dark, light) = (hue(h, 0.75, 0.32), hue((h + 0.12) % 1.0, 0.55, 0.95));
    let accent = hue((h + 0.5) % 1.0, 0.65, 0.98);
    let mut img = image::RgbImage::new(N, N);
    for (px, py, p) in img.enumerate_pixels_mut() {
        let (x, y) = (px as f32 / N as f32 - 0.5, py as f32 / N as f32 - 0.5);
        let shape = match album % 6 {
            // A sun over stripes.
            0 => {
                let sun = (x * x + (y + 0.08) * (y + 0.08)).sqrt() < 0.26;
                let stripe = y > 0.02 && ((y * 26.0).fract() > 0.5);
                if sun && !stripe { 1.0 } else { 0.0 }
            }
            // Rings.
            1 => f32::from(u8::from(((x * x + y * y).sqrt() * 18.0 + a).fract() > 0.62)),
            // Waves.
            2 => f32::from(u8::from((y * 9.0 + (x * 9.0).sin() * 0.9).fract().abs() > 0.68)),
            // Triangles.
            3 => f32::from(u8::from(y > -0.28 + (x * 2.2).abs() && y < 0.3)),
            // Squares turned.
            4 => {
                let (u, v) = ((x + y) * 5.0, (x - y) * 5.0);
                f32::from(u8::from((u.floor() + v.floor()).rem_euclid(2.0) < 1.0 && u.abs() < 2.0 && v.abs() < 2.0))
            }
            // Two discs.
            _ => {
                let d = |cx: f32, cy: f32| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() < 0.22;
                f32::from(u8::from(d(-0.12, 0.05) != d(0.12, -0.05)))
            }
        };
        let t = (y + 0.5 + x * 0.3).clamp(0.0, 1.0);
        for (c, slot) in p.0.iter_mut().enumerate() {
            let at = |v: &[f32; 3]| v.get(c).copied().unwrap_or(0.0);
            let back = at(&dark) * (1.0 - t) + at(&light) * t;
            *slot = ((back * (1.0 - shape) + at(&accent) * shape) * 255.0).clamp(0.0, 255.0) as u8;
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Gives the demonstration library some history. With `hold`, a song is shown paused.
pub fn dress(engine: &mut crate::Engine, hold: bool) {
    use crate::Command;
    use musiccraft_library::Item;
    engine.settle(std::time::Duration::from_secs(20));
    let Some(lib) = engine.library() else { return };
    let mut ids: Vec<String> = lib.tracks().map(|t| t.id.clone()).collect();
    ids.sort_by_key(|id| lib.track(id).map(|t| (t.info.artist.clone(), t.info.album.clone(), t.info.track)));
    let fresh = lib.playlists().is_empty();
    let pick = |from: usize, step: usize, n: usize| -> Vec<String> {
        ids.iter().skip(from).step_by(step).take(n).cloned().collect()
    };
    let items = |ids: Vec<String>| ids.iter().map(|id| Item::local(id)).collect::<Vec<_>>();
    let mut commands = Vec::new();
    if fresh {
        commands.push(Command::Love { ids: pick(1, 5, 12), on: true });
        for (i, stars) in [5u8, 4, 3, 5].into_iter().enumerate() {
            commands.push(Command::Rate { ids: pick(i, 4, 15), stars });
        }
        for (name, from, step) in [("Domenica mattina", 2, 5), ("Night Drive", 0, 7), ("Studio", 3, 4)] {
            commands.push(Command::PlaylistNew { name: name.to_owned(), items: items(pick(from, step, 9)) });
        }
        let rules = musiccraft_library::smart::Rules {
            rules: vec![musiccraft_library::smart::Rule {
                field: musiccraft_library::smart::Field::Rating,
                op: musiccraft_library::smart::Op::Over,
                value: "4".to_owned(),
            }],
            ..Default::default()
        };
        commands.push(Command::SmartNew { name: "Cinque stelle".to_owned(), rules });
    }
    for (title, artist, album) in [
        ("Vento di terra", "Lanterna Blu", "Carta nautica"),
        ("Bussola", "Lanterna Blu", "Carta nautica"),
        ("Relay", "Kestrel Circuit", "Uplink"),
    ] {
        let [title, artist, album] = [title, artist, album].map(str::to_owned);
        commands.push(Command::SongAdd { title, artist, album });
    }
    commands.push(Command::ActiveService { service: "open:tidal".to_owned() });
    let queue = items(pick(13, 1, 20));
    for c in commands {
        // A demonstration without its history is still a demonstration.
        engine.run(c).ok();
    }
    if hold {
        engine.hold(queue, 4);
    }
    engine.save_now();
}
