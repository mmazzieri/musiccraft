//! Names compared as people mean them, for songs that come from different places.

use crate::model::{Info, fold};

/// Words of a name, without case, accents and punctuation.
pub fn words(s: &str) -> Vec<String> {
    fold(s)
        .replace('&', " and ")
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
        .map(|w| w.replace(['\'', '\u{2019}'], ""))
        .filter(|w| !w.is_empty())
        .collect()
}

/// A title without what stores add to it: guests and the year of a remaster. "Live", "Remix" and
/// the like stay, because they name another recording.
pub fn plain_title(title: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    // Bracketed parts are kept or dropped whole.
    while let Some(open) = rest.find(['(', '[']) {
        let (before, tail) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = tail.find([')', ']']) else {
            rest = tail;
            break;
        };
        let (part, after) = tail.split_at(close + 1);
        if !is_extra(part) {
            out.push_str(part);
        }
        rest = after;
    }
    out.push_str(rest);
    match out.rsplit_once(" - ") {
        Some((head, tail)) if !head.trim().is_empty() && is_extra(tail) => head.trim().to_owned(),
        _ => out.trim().to_owned(),
    }
}

fn is_extra(part: &str) -> bool {
    let w = words(part);
    let Some(first) = w.first() else { return true };
    let guest = matches!(first.as_str(), "feat" | "ft" | "featuring" | "with");
    let remaster = w.iter().any(|x| x.starts_with("remaster"));
    let other_take = ["live", "remix", "mix", "demo", "acoustic", "instrumental", "karaoke", "edit"];
    (guest || remaster) && !w.iter().any(|x| other_take.contains(&x.as_str()))
}

/// The same song on the same album has the same key, whoever lists it. Guests are often written
/// after the artist, so only the first artist counts.
pub fn song_key(i: &Info) -> String {
    let artist = first_artist(if i.artist.is_empty() { &i.album_artist } else { &i.artist });
    format!(
        "{}\u{1f}{}\u{1f}{}",
        words(&plain_title(&i.title)).join(" "),
        words(artist).join(" "),
        words(&plain_title(&i.album)).join(" ")
    )
}

/// The artist before guests and partners.
pub fn first_artist(artist: &str) -> &str {
    let lower = artist.to_ascii_lowercase();
    let cut = [", ", "; ", " feat. ", " feat ", " ft. ", " featuring "].iter().filter_map(|s| lower.find(s)).min();
    cut.and_then(|n| artist.get(..n)).unwrap_or(artist)
}

#[cfg(test)]
mod tests {
    use super::{plain_title, words};

    /// What a store adds to a title does not make it another song; another take does.
    #[test]
    fn store_additions_are_dropped_and_other_takes_kept() {
        let same = |a: &str, b: &str| words(&plain_title(a)) == words(&plain_title(b));
        assert!(same("L\u{2019}alba (feat. Ada Verdi)", "L'Alba"));
        assert!(same("Faro spento - 2011 Remaster", "Faro spento"));
        assert!(same("Sale & vetro", "Sale and Vetro"));
        assert!(!same("Faro spento (Live)", "Faro spento"));
        assert!(!same("Faro spento - Live / Remastered", "Faro spento"));
        assert!(!same("(I Can't) Wait", "Wait"));
        assert!(!words("\u{6771}\u{4eac}").is_empty());
        assert_eq!(plain_title("Broken (bracket"), "Broken (bracket");
    }
}
