//! Interface language.
//!
//! Deliberately lives in the engine rather than the UI. The engine already owns every
//! user-facing string (`view.rs`), which is what keeps the tray tooltip and the popover from
//! ever disagreeing — and keeps the wording unit-tested. Translating in the frontend would
//! break exactly that property, splitting the vocabulary across two languages and two repos
//! of code.
//!
//! The engine does **not** detect the system language itself: the app crate does that with
//! `sys-locale` and passes a `Locale` in. That keeps this crate at three dependencies and
//! keeps locale detection (an OS concern) out of the scheduling logic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    /// Chinese, in the vocabulary DeepSeek's own Chinese pricing page uses — 高峰 / 空闲,
    /// not a literal translation of "peak" and "off-peak". The user met this concept on that
    /// page, so the app should speak its language back.
    Zh,
    #[default]
    En,
}

impl Locale {
    /// Every variant, for iterating in tests and for building the settings list.
    pub const ALL: [Locale; 2] = [Locale::Zh, Locale::En];

    /// The tag written to settings.
    pub fn tag(self) -> &'static str {
        match self {
            Locale::Zh => "zh",
            Locale::En => "en",
        }
    }

    /// The language's own name, for the language picker — shown as "中文" and "English"
    /// regardless of the current locale, because a language picker you cannot read is
    /// useless.
    pub fn native_name(self) -> &'static str {
        match self {
            Locale::Zh => "中文",
            Locale::En => "English",
        }
    }

    /// A BCP-47-ish tag (`zh-CN`, `zh-Hans`, `en-US`, …) to a locale, matched on the language
    /// prefix.
    ///
    /// Anything unrecognised falls back to English. That direction is deliberate: a string
    /// that is still in English is obvious to a non-English speaker, whereas a confidently
    /// wrong language is confusing.
    pub fn from_tag(tag: &str) -> Locale {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Locale::Zh
        } else {
            Locale::En
        }
    }

    /// Pick between two literals. **Argument order is (Chinese, English)** — kept short
    /// because it appears at every call site, with this doc comment and a test pinning the
    /// order so it cannot be silently reversed.
    pub fn pick<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Locale::Zh => zh,
            Locale::En => en,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_takes_chinese_first_then_english() {
        assert_eq!(Locale::Zh.pick("高峰", "Peak"), "高峰");
        assert_eq!(Locale::En.pick("高峰", "Peak"), "Peak");
    }

    #[test]
    fn tags_match_on_the_language_prefix() {
        for tag in ["zh", "zh-CN", "zh-Hans", "zh-Hant-TW", "ZH_cn", " zh "] {
            assert_eq!(Locale::from_tag(tag), Locale::Zh, "{tag:?}");
        }
        for tag in ["en", "en-US", "en-GB", "", "fr", "de-DE", "ja-JP"] {
            assert_eq!(Locale::from_tag(tag), Locale::En, "{tag:?}");
        }
    }

    #[test]
    fn tags_round_trip_through_from_tag() {
        for locale in Locale::ALL {
            assert_eq!(Locale::from_tag(locale.tag()), locale);
        }
    }

    #[test]
    fn every_locale_has_a_distinct_tag_and_native_name() {
        let tags: Vec<&str> = Locale::ALL.iter().map(|l| l.tag()).collect();
        let names: Vec<&str> = Locale::ALL.iter().map(|l| l.native_name()).collect();
        for i in 0..Locale::ALL.len() {
            for j in (i + 1)..Locale::ALL.len() {
                assert_ne!(tags[i], tags[j], "two locales share a tag");
                assert_ne!(names[i], names[j], "two locales share a display name");
            }
        }
    }

    #[test]
    fn serde_writes_the_lowercase_tag() {
        assert_eq!(serde_json::to_string(&Locale::Zh).unwrap(), "\"zh\"");
        assert_eq!(serde_json::to_string(&Locale::En).unwrap(), "\"en\"");
    }
}
