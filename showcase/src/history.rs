//! One document's history: a page the library shows over its list. Each page is a store of its own, which the library keeps while
//! the page is open. Going back drops it, so its load stops counting with nothing to check.

use crate::service::{DocId, Revision};
use spiderland::{Effects, Error, Feature, Lifecycle, Never};

/// The feature.
#[derive(Debug)]
pub struct History;

/// The page's state.
#[derive(Debug)]
pub struct State {
    /// Whose history.
    pub id: DocId,
    /// Its saved states, newest first, once they have come.
    pub revisions: Option<Vec<Revision>>,
    /// Why they didn't come.
    pub error: Option<String>,
    /// Whether the page is on top. A page under another doesn't show.
    pub shown: bool,
}

impl State {
    /// The history of `id`, before it loads.
    pub fn new(id: DocId) -> State {
        State {
            id,
            revisions: None,
            error: None,
            shown: false,
        }
    }
}

/// What comes back, and what the library tells the page.
#[derive(Debug)]
pub enum Intent {
    /// The saved states, or why they didn't come.
    Loaded(Result<Vec<Revision>, Error>),
    /// Where the page stands.
    Life(Lifecycle),
}

impl From<Lifecycle> for Intent {
    fn from(life: Lifecycle) -> Self {
        Intent::Life(life)
    }
}

/// Work for the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Loads the saved states.
    Load(DocId),
}

impl Feature for History {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = Never;

    fn start(state: &mut State, effects: &mut Effects<Effect>) {
        effects.push(Effect::Load(state.id));
    }

    fn reduce(state: &mut State, intent: Intent, _: &mut Effects<Effect>) {
        match intent {
            Intent::Loaded(Ok(revisions)) => state.revisions = Some(revisions),
            Intent::Loaded(Err(error)) => state.error = Some(format!("The history failed to load: {error}.")),
            Intent::Life(Lifecycle::Shown) => state.shown = true,
            Intent::Life(Lifecycle::Hidden) => state.shown = false,
            Intent::Life(Lifecycle::CloseRequested | Lifecycle::CloseCalledOff) => {}
        }
    }
}

/// What the page's title says.
pub fn title(state: &State) -> String {
    match &state.error {
        Some(error) => error.clone(),
        None if state.revisions.is_none() => format!("Loading the history of document {}", state.id.0),
        None => format!("History of document {}", state.id.0),
    }
}
