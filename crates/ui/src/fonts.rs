//! Scripts the built-in fonts do not cover are shown with a font of the system, read when a
//! title first needs it. Nothing is embedded or copied.

use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

/// Fonts systems come with, for writing systems beyond Latin, Greek and Cyrillic.
const CANDIDATES: &[&str] = &[
    "C:/Windows/Fonts/YuGothM.ttc",
    "C:/Windows/Fonts/msgothic.ttc",
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/malgun.ttf",
    "C:/Windows/Fonts/segoeui.ttf",
    "C:/Windows/Fonts/Nirmala.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
];

#[derive(Default)]
pub struct Fonts {
    loaded: bool,
    seen: u64,
}

impl Fonts {
    /// Looks at the texts once per library `revision`.
    pub fn check<'a>(&mut self, ctx: &egui::Context, revision: u64, mut texts: impl Iterator<Item = &'a str>) {
        if self.loaded || self.seen == revision {
            return;
        }
        self.seen = revision;
        // Armenian is the first block after Cyrillic; general punctuation and symbols start at
        // 0x2000 and the built-in fonts have them.
        let uncovered =
            |c: char| matches!(u32::from(c), 0x530..0x1E00 | 0x2E80..0xE000 | 0xF900..0xFB00 | 0xFB13..0xFE00);
        if !texts.any(|t| t.chars().any(uncovered)) {
            return;
        }
        self.loaded = true;
        let mut defs = FontDefinitions::default();
        for path in CANDIDATES {
            if let Ok(bytes) = std::fs::read(path) {
                defs.font_data.insert((*path).to_owned(), Arc::new(FontData::from_owned(bytes)));
                for family in [FontFamily::Proportional, FontFamily::Monospace] {
                    defs.families.entry(family).or_default().push((*path).to_owned());
                }
            }
        }
        ctx.set_fonts(defs);
    }
}
