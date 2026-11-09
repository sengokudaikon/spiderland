//! What the user chose, shared by every page and window. Each change goes out as an output, and each store that cares keeps a copy.

use spiderland::{Effects, Feature, Never};

/// What the user chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    /// How many documents a page of the library lists.
    pub page_size: usize,
    /// Whether an editor counts the words of its document.
    pub word_count: bool,
}

/// The preferences' name on a trail, where they go out as an output. A struct has one name, where an enum has one a variant.
impl From<&Preferences> for &'static str {
    fn from(_: &Preferences) -> Self {
        "Preferences"
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences { page_size: 3, word_count: false }
    }
}

/// The feature that owns the preferences.
#[derive(Debug)]
pub struct Settings;

/// What the user asks of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
pub enum Intent {
    /// Lists one more document a page, or one fewer.
    PageSize(usize),
    /// Turns the word count on or off.
    WordCount(bool),
}

/// What the preferences say, in a line.
pub fn summary(preferences: &Preferences) -> String {
    let words = if preferences.word_count { "on" } else { "off" };
    format!("{} to a page, the word count {words}", preferences.page_size)
}

/// The fewest and most documents a page lists.
pub const PAGE_SIZES: std::ops::RangeInclusive<usize> = 1..=10;

impl Feature for Settings {
    type State = Preferences;
    type Intent = Intent;
    type Effect = Never;
    type Output = Preferences;
    type Subscription = Never;

    fn reduce(preferences: &mut Preferences, intent: Intent, effects: &mut Effects<Never, Preferences>) {
        let before = *preferences;
        match intent {
            Intent::PageSize(size) => preferences.page_size = size.clamp(*PAGE_SIZES.start(), *PAGE_SIZES.end()),
            Intent::WordCount(on) => preferences.word_count = on,
        }
        if *preferences != before {
            effects.emit(*preferences);
        }
    }
}
