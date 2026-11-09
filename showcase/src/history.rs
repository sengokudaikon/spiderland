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
    /// The revision picked, if any.
    pub picked: Option<u32>,
}

impl State {
    /// The history of `id`, before it loads.
    pub fn new(id: DocId) -> State {
        State {
            id,
            revisions: None,
            error: None,
            shown: false,
            picked: None,
        }
    }

    /// The history of `id` where an earlier run left it, before it loads.
    pub fn restored(id: DocId, session: Session) -> State {
        State {
            picked: session.picked,
            ..State::new(id)
        }
    }
}

/// What a history page keeps of itself in the session: the revision picked.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    /// The revision picked, if any.
    pub picked: Option<u32>,
}

/// What `state` keeps in the session.
pub fn session(state: &State) -> Session {
    Session { picked: state.picked }
}

/// What comes back, and what the library tells the page.
#[derive(Debug, strum::IntoStaticStr)]
pub enum Intent {
    /// The saved states, or why they didn't come.
    Loaded(Result<Vec<Revision>, Error>),
    /// Picks a revision.
    Pick(u32),
    /// Where the page stands.
    Life(Lifecycle),
}

impl From<Lifecycle> for Intent {
    fn from(life: Lifecycle) -> Self {
        Intent::Life(life)
    }
}

/// Work for the host.
#[derive(Clone, Debug, PartialEq, Eq, strum::IntoStaticStr)]
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
            Intent::Pick(revision) => state.picked = Some(revision),
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
