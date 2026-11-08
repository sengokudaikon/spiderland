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

impl Default for Preferences {
    fn default() -> Self {
        Preferences { page_size: 3, word_count: false }
    }
}

/// The feature that owns the preferences.
#[derive(Debug)]
pub struct Settings;

/// What the user asks of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Lists one more document a page, or one fewer.
    PageSize(usize),
    /// Turns the word count on or off.
    WordCount(bool),
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
