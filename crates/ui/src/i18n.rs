//! Translations. English in the code is the key; a missing translation shows in English.

use std::collections::HashMap;

/// Languages with a catalogue, with their own names.
pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("it", "Italiano")];

const IT: &str = include_str!("it.tsv");

#[derive(Debug, Default)]
pub struct Lang {
    code: String,
    map: HashMap<&'static str, &'static str>,
}

impl Lang {
    /// `choice` is a language code, or empty to follow the system.
    pub fn new(choice: &str) -> Lang {
        let system = || sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
        let code = if choice.is_empty() { system().get(..2).unwrap_or("en").to_owned() } else { choice.to_owned() };
        let map = if code == "it" { IT.lines().filter_map(|l| l.split_once('\t')).collect() } else { HashMap::new() };
        Lang { code, map }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// The text in the person's language.
    pub fn t<'a>(&self, english: &'a str) -> &'a str
    where
        'static: 'a,
    {
        self.map.get(english).copied().unwrap_or(english)
    }

    /// The text with its `{}` replaced.
    pub fn f(&self, english: &str, value: impl std::fmt::Display) -> String {
        self.t(english).replacen("{}", &value.to_string(), 1)
    }

    /// The text with its two `{}` replaced.
    pub fn f2(&self, english: &str, a: impl std::fmt::Display, b: impl std::fmt::Display) -> String {
        self.t(english).replacen("{}", &a.to_string(), 1).replacen("{}", &b.to_string(), 1)
    }

    /// A count with its noun.
    pub fn n(&self, n: usize, one: &str, many: &str) -> String {
        self.f(if n == 1 { one } else { many }, self.number(n))
    }

    /// A whole number with the language's separator.
    pub fn number(&self, n: usize) -> String {
        let digits = n.to_string();
        let sep = if self.code == "it" { '.' } else { ',' };
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(sep);
            }
            out.push(c);
        }
        out
    }
}

/// Minutes and seconds, with hours when there are any.
pub fn time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::{IT, Lang};
    use std::collections::HashSet;

    /// Every text the window shows has an Italian translation with the same placeholders.
    #[test]
    fn the_italian_catalogue_is_complete() {
        let sources = [
            include_str!("albums.rs"),
            include_str!("app.rs"),
            include_str!("dialogs.rs"),
            include_str!("header.rs"),
            include_str!("menus.rs"),
            include_str!("services.rs"),
            include_str!("sidebar.rs"),
            include_str!("table.rs"),
            include_str!("views.rs"),
            include_str!("../../engine/src/core.rs"),
        ];
        let known: HashSet<&str> = IT.lines().filter_map(|l| l.split_once('\t')).map(|(k, _)| k).collect();
        let mut missing = Vec::new();
        for source in sources {
            for (call, texts) in [(".t(", 1), (".f(", 1), (".f2(", 1), (".n(", 2), (".error(", 1), (".notice(", 1)] {
                for part in source.split(call).skip(1) {
                    let part = part.trim_start();
                    // A count comes first; the others take their text first, when it is a literal.
                    if texts == 2 || part.starts_with('"') {
                        let found = part.split('"').skip(1).step_by(2).take(texts);
                        missing.extend(found.filter(|t| !known.contains(t)).map(str::to_owned));
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "not in it.tsv:\n{}", missing.join("\n"));
        for line in IT.lines() {
            let (en, it) = line.split_once('\t').unwrap();
            assert_eq!(en.matches("{}").count(), it.matches("{}").count(), "{en}");
        }
    }

    #[test]
    fn numbers_are_grouped_as_the_language_does() {
        assert_eq!(Lang::new("it").number(1_234_567), "1.234.567");
        assert_eq!(Lang::new("en").number(999), "999");
    }
}
