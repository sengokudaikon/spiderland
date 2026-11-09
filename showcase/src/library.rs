//! The library: a search over the documents, a page at a time, with a selection and a history page. It keeps the documents open
//! and the history pages as its children, each a store of its own, and closes the app once every document has closed.

use crate::document;
use crate::service::{DocId, Found, Summary};
use crate::settings::Preferences;
use spiderland::{Asked, CloseAnswer, Closing, Effects, Error, Feature, Latest, Lifecycle, Never, Standing, Tracked};

/// The feature.
#[derive(Debug)]
pub struct Library;

/// A page of the library's window. The first is always [`Page::List`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Page {
    /// The search and its results.
    List,
    /// One document's saved states, a [`History`](crate::history::History) of its own.
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
    /// The documents open, each a store the library keeps until it says it has closed.
    pub open: Vec<DocId>,
    /// The library's own close, while it waits for each document open to close.
    pub closing: Option<Closing<DocId>>,
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
            open: Vec::new(),
            closing: None,
        }
    }

    /// Where `session` left off, with `preferences` as they are now. The pages and documents it names open again as the library
    /// starts.
    pub fn restored(preferences: Preferences, session: Session) -> State {
        let mut pages = vec![Page::List];
        pages.extend(session.pages.into_iter().filter(|page| *page != Page::List));
        State {
            query: session.query,
            page: session.page,
            selected: session.selected,
            pages,
            open: session.open,
            ..State::new(preferences)
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

/// Where the user is in the library, which the app saves on closing and starts on the next time: the search, the page of
/// results, the document picked, the pages open and the documents open.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    /// What the search box held.
    pub query: String,
    /// The page of results shown, from 0.
    pub page: usize,
    /// The document picked.
    pub selected: Option<DocId>,
    /// The pages, the one shown last.
    pub pages: Vec<Page>,
    /// The documents open.
    pub open: Vec<DocId>,
}

/// Where the user is, as the app saves it.
pub fn session(state: &State) -> Session {
    Session {
        query: state.query.clone(),
        page: state.page,
        selected: state.selected,
        pages: state.pages.clone(),
        open: state.open.clone(),
    }
}

/// What the user asks, and what comes back.
#[derive(Debug, strum::IntoStaticStr)]
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
    /// Opens a window on a document, the document too if it isn't open.
    Open(DocId),
    /// Shows a document's history.
    ShowHistory(DocId),
    /// Goes back a page.
    Back,
    /// The preferences changed.
    Preferences(Preferences),
    /// What an open document said.
    Document(DocId, document::Output),
    /// What the library's window says: the user asked to close it, which closes the app.
    Life(Lifecycle),
}

/// Work for the host.
#[derive(Clone, Debug, PartialEq, Eq, strum::IntoStaticStr)]
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
    /// Opens a window on an open document.
    OpenWindow(DocId),
    /// Closes the library's window, once every document has closed.
    Close,
}

impl Feature for Library {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = Never;

    fn start(state: &mut State, effects: &mut Effects<Effect>) {
        search(state, effects);
        // A library restored from a session opens a window on each document it had open.
        for id in &state.open {
            effects.push(Effect::OpenWindow(*id));
        }
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
            Intent::Refresh | Intent::Document(_, document::Output::Changed) => search(state, effects),
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
            // Nothing opens while the app closes.
            Intent::Open(id) if state.closing.is_none() => {
                if !state.open.contains(&id) {
                    state.open.push(id);
                }
                effects.push(Effect::OpenWindow(id));
            }
            Intent::Open(_) => {}
            Intent::ShowHistory(id) => state.pages.push(Page::History(id)),
            Intent::Back => {
                if state.pages.len() > 1 {
                    state.pages.pop();
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
            Intent::Document(id, document::Output::Closed(answer)) => {
                // A document that has closed has nothing left to finish, so the library lets it go.
                if answer == CloseAnswer::Closed {
                    state.open.retain(|open| *open != id);
                }
                let heard = state.closing.as_mut().and_then(|closing| closing.hear(id, answer));
                if let Some(answer) = heard {
                    closed(state, answer, effects);
                }
            }
            // A document with no window that needs the user, such as after a save failed, asks for one.
            Intent::Document(id, document::Output::Reopen) => effects.push(Effect::OpenWindow(id)),
            Intent::Life(Lifecycle::CloseRequested) => {
                if state.closing.is_none() {
                    let closing = Closing::asking(state.open.iter().copied());
                    // With no document open, there is no one to wait for.
                    match closing.answer() {
                        Some(answer) => closed(state, answer, effects),
                        None => state.closing = Some(closing),
                    }
                }
            }
            Intent::Life(Lifecycle::Shown | Lifecycle::Hidden | Lifecycle::CloseCalledOff) => {}
        }
    }
}

/// The library's close is decided: every document has closed, or one stays, and so does the app.
fn closed(state: &mut State, answer: CloseAnswer, effects: &mut Effects<Effect>) {
    state.closing = None;
    if answer == CloseAnswer::Closed {
        effects.push(Effect::Close);
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

/// The history pages open, the one on top shown, each a store the library keeps.
pub fn pages(state: &State) -> Vec<(DocId, Standing)> {
    let top = state.pages.len() - 1;
    let histories = state.pages.iter().enumerate().filter_map(|(at, page)| match page {
        Page::List => None,
        Page::History(id) => Some((*id, if at == top { Standing::Shown } else { Standing::Hidden })),
    });
    histories.collect()
}

/// The documents open, each a store the library keeps. While the library closes, each is asked to close.
pub fn documents(state: &State) -> Vec<(DocId, Standing)> {
    let standing = if state.closing.is_some() { Standing::Closing } else { Standing::Shown };
    state.open.iter().map(|id| (*id, standing)).collect()
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
    if state.closing.is_some() {
        return "Closing once every document has".to_owned();
    }
    if let Some(error) = &state.error {
        return error.clone();
    }
    if state.search.is_waiting() {
        return "Searching".to_owned();
    }
    format!("Page {} of {}", state.page + 1, state.page_count())
}
