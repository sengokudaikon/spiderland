//! The library: a search over the documents, a page at a time, with a selection and a history page.

use crate::service::{DocId, Found, Revision, Summary};
use crate::settings::Preferences;
use spiderland::{Asked, Effects, Error, Feature, Latest, Never, Tracked};

/// The feature.
#[derive(Debug)]
pub struct Library;

/// A page of the library's window. The first is always [`Page::List`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The search and its results.
    List,
    /// One document's saved states.
    History(DocId),
}

/// The library's state.
#[derive(Debug)]
pub struct State {
    /// What the search box holds.
    pub query: String,
    /// The page of results shown, from 0.
    pub page: usize,
    /// A copy of the preferences, which another store owns.
    pub preferences: Preferences,
    /// The documents on the page shown.
    pub found: Tracked<Vec<Summary>>,
    /// How many documents match.
    pub total: usize,
    /// The document picked, by name, so it stays picked while rows before it come or go.
    pub selected: Option<DocId>,
    /// The searches asked for. Only the answer to the latest counts.
    pub search: Latest,
    /// Why the last search failed.
    pub error: Option<String>,
    /// The pages, the one shown last.
    pub pages: Vec<Page>,
    /// What the history page shows, once it has come.
    pub history: Option<(DocId, Vec<Revision>)>,
}

impl State {
    /// Before the first search, with `preferences` as they are now.
    pub fn new(preferences: Preferences) -> State {
        State {
            query: String::new(),
            page: 0,
            preferences,
            found: Tracked::default(),
            total: 0,
            selected: None,
            search: Latest::default(),
            error: None,
            pages: vec![Page::List],
            history: None,
        }
    }

    /// The page shown.
    pub fn shown(&self) -> Page {
        *self.pages.last().expect("the list is always there")
    }

    /// How many pages the results fill, at least one.
    pub fn page_count(&self) -> usize {
        self.total.div_ceil(self.preferences.page_size).max(1)
    }
}

/// What the user asks, and what comes back.
#[derive(Debug)]
pub enum Intent {
    /// The search box changed.
    Typed(String),
    /// The next page of results.
    NextPage,
    /// The page before.
    PreviousPage,
    /// Searches again, for what may have changed.
    Refresh,
    /// A search's answer, or why there is none.
    Found {
        /// Which search it answers.
        asked: Asked,
        /// Its answer.
        result: Result<Found, Error>,
    },
    /// Picks a document.
    Select(DocId),
    /// Opens a document in a window of its own.
    Open(DocId),
    /// Shows a document's history.
    ShowHistory(DocId),
    /// Goes back a page.
    Back,
    /// A history's answer.
    HistoryLoaded {
        /// Whose history.
        id: DocId,
        /// Its saved states, newest first.
        result: Result<Vec<Revision>, Error>,
    },
    /// The preferences changed.
    Preferences(Preferences),
    /// A document was saved or deleted, so the results may be stale.
    Changed,
}

/// Work for the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// One page of a search.
    Search {
        /// Which search, for its answer to carry.
        asked: Asked,
        /// What to look for.
        query: String,
        /// Which page.
        page: usize,
        /// How many to a page.
        size: usize,
    },
    /// A document's history.
    LoadHistory(DocId),
    /// A window for a document.
    Open(DocId),
}

impl Feature for Library {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = Never;

    fn start(state: &mut State, effects: &mut Effects<Effect>) {
        search(state, effects);
    }

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
        match intent {
            Intent::Typed(query) => {
                if query != state.query {
                    state.query = query;
                    state.page = 0;
                    search(state, effects);
                }
            }
            Intent::NextPage => {
                if state.page + 1 < state.page_count() {
                    state.page += 1;
                    search(state, effects);
                }
            }
            Intent::PreviousPage => {
                if state.page > 0 {
                    state.page -= 1;
                    search(state, effects);
                }
            }
            Intent::Refresh | Intent::Changed => search(state, effects),
            // An answer to a search typed over comes back late, and is dropped.
            Intent::Found { asked, result } if state.search.answer(asked) => match result {
                Ok(found) => {
                    state.found.set(found.docs);
                    state.total = found.total;
                    state.error = None;
                }
                Err(error) => state.error = Some(format!("The search failed: {error}.")),
            },
            Intent::Found { .. } => {}
            Intent::Select(id) => state.selected = Some(id),
            Intent::Open(id) => effects.push(Effect::Open(id)),
            // Workaround: no navigation stack whose pages are children. The history page has no lifecycle of its own, so the
            // library loads it, and drops an answer for a page no longer shown, by hand.
            Intent::ShowHistory(id) => {
                state.pages.push(Page::History(id));
                state.history = None;
                effects.push(Effect::LoadHistory(id));
            }
            Intent::Back => {
                if state.pages.len() > 1 {
                    state.pages.pop();
                    state.history = None;
                }
            }
            Intent::HistoryLoaded { id, result } => {
                if state.shown() == Page::History(id) {
                    match result {
                        Ok(revisions) => state.history = Some((id, revisions)),
                        Err(error) => state.error = Some(format!("The history failed to load: {error}.")),
                    }
                }
            }
            Intent::Preferences(preferences) => {
                let resized = preferences.page_size != state.preferences.page_size;
                state.preferences = preferences;
                if resized {
                    state.page = 0;
                    search(state, effects);
                }
            }
        }
    }
}

fn search(state: &mut State, effects: &mut Effects<Effect>) {
    effects.push(Effect::Search {
        asked: state.search.ask(),
        query: state.query.clone(),
        page: state.page,
        size: state.preferences.page_size,
    });
}

/// One row of the list, as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The document.
    pub id: DocId,
    /// Its title.
    pub title: String,
    /// Whether it is the one picked.
    pub selected: bool,
}

/// The rows of the page shown.
pub fn rows(found: &Tracked<Vec<Summary>>, selected: Option<DocId>) -> Vec<Row> {
    found
        .iter()
        .map(|doc| Row {
            id: doc.id,
            title: doc.title.clone(),
            selected: selected == Some(doc.id),
        })
        .collect()
}

/// What the status line says.
pub fn status(state: &State) -> String {
    if let Some(error) = &state.error {
        return error.clone();
    }
    if state.search.is_waiting() {
        return "Searching".to_owned();
    }
    format!("Page {} of {}", state.page + 1, state.page_count())
}
