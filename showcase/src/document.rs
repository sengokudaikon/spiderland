//! One open document: editing, saving, exporting and deleting it, and closing its windows.
//!
//! Its store is one of the library's children, and lives until it says it has closed, not as long as a window shows it. So a save
//! started before its last window closes still finishes, and its outcome is recorded here.

use crate::export::{self, Export};
use crate::service::DocId;
use crate::settings::Preferences;
use spiderland::{CloseAnswer, Effects, Error, Feature, Instance, Lifecycle, OneAtATime, Pulse, Slot};

/// The feature.
#[derive(Debug)]
pub struct Document;

/// The longest title the editor takes, in characters.
pub const TITLE_LIMIT: usize = 40;

/// A title and a body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Content {
    /// The title.
    pub title: String,
    /// The body.
    pub body: String,
}

/// A question the editor asks before it goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Whether to delete the document.
    Delete,
    /// Whether to save the edits before the window closes.
    Close,
}

/// An answer to an [`Ask`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Go ahead: delete, or save and close.
    Confirm,
    /// Close without saving.
    Discard,
    /// Do nothing.
    Cancel,
}

/// The document's state.
#[derive(Debug)]
pub struct State {
    /// Which document.
    pub id: DocId,
    /// Whether it has loaded.
    pub loaded: bool,
    /// What the editor holds.
    pub content: Content,
    /// What the service holds, as last loaded or saved.
    pub saved: Content,
    /// The service's revision of [`State::saved`].
    pub revision: u32,
    /// What the save under way writes.
    pub saving: Option<Content>,
    /// The saves: one at a time, with the latest asked for waiting.
    pub saves: OneAtATime<Content>,
    /// Fires when the reducer refuses an edit, so the text box shows the title it kept.
    pub refused: Pulse,
    /// A short note for the user, such as "Saved".
    pub toast: Pulse<String>,
    /// Fires when the document's windows close: it was closed, or deleted. A window opened after that doesn't act on it.
    pub close: Pulse,
    /// Why the last save, export or delete failed.
    pub error: Option<String>,
    /// The export under way, a feature of its own. Stopped, its work stops, and what it sends after that is let go, even once
    /// another export has started.
    pub export: Slot<export::State>,
    /// The question shown.
    pub asking: Option<Ask>,
    /// Whether a close waits for a save to finish before the windows close.
    pub closing: bool,
    /// Whether its windows have closed. It tells the library it has closed once no save is under way, and a window opened
    /// before then keeps it.
    pub leaving: bool,
    /// A copy of the preferences, which another store owns.
    pub preferences: Preferences,
    /// Whether the document is gone.
    pub deleted: bool,
}

impl State {
    /// The document `id`, before it loads.
    pub fn new(id: DocId, preferences: Preferences) -> State {
        State {
            id,
            loaded: false,
            content: Content::default(),
            saved: Content::default(),
            revision: 0,
            saving: None,
            saves: OneAtATime::default(),
            refused: Pulse::default(),
            toast: Pulse::new(String::new()),
            close: Pulse::default(),
            error: None,
            export: Slot::default(),
            asking: None,
            closing: false,
            leaving: false,
            preferences,
            deleted: false,
        }
    }

    /// Whether the editor holds edits that neither the service nor a save asked for has.
    pub fn dirty(&self) -> bool {
        self.content != self.saved && Some(&self.content) != self.saving.as_ref() && Some(&self.content) != self.saves.waiting()
    }

    /// Why the content can't be saved as it is.
    pub fn problem(&self) -> Option<&'static str> {
        self.content.title.trim().is_empty().then_some("A title is needed.")
    }

    /// How many words the body has.
    pub fn words(&self) -> usize {
        self.content.body.split_whitespace().count()
    }
}

/// What the user asks, and what comes back.
#[derive(Debug, strum::IntoStaticStr)]
pub enum Intent {
    /// The document, or why it didn't load.
    Loaded(Result<Result<crate::service::Doc, String>, Error>),
    /// The title box changed.
    EditTitle(String),
    /// The body box changed.
    EditBody(String),
    /// Saves the edits.
    Save,
    /// A save's new revision, or why it failed.
    Saved(Result<Result<u32, String>, Error>),
    /// Starts an export.
    Export,
    /// What an export hears, for the start it was sent for.
    Exporting(Instance, export::Intent),
    /// Stops the export under way.
    StopExport,
    /// Asks to delete the document.
    Delete,
    /// The answer to the question shown.
    Answer(Choice),
    /// A delete's outcome.
    Deleted(Result<Result<(), String>, Error>),
    /// What the library tells the document, and its last window's close.
    Life(Lifecycle),
    /// A window opened on the document.
    WindowOpened,
    /// Another program changed a document.
    ChangedOutside(Option<DocId>),
    /// The preferences changed.
    Preferences(Preferences),
}

/// Work for the host.
#[derive(Clone, Debug, PartialEq, Eq, strum::IntoStaticStr)]
pub enum Effect {
    /// Loads the document.
    Load(DocId),
    /// Writes the document.
    Save(DocId, Content),
    /// Deletes the document.
    Delete(DocId),
}

/// What the document tells others.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
pub enum Output {
    /// It was saved or deleted, so lists of documents may be stale.
    Changed,
    /// Its answer to [`Lifecycle::CloseRequested`]. `Closed` comes once its windows have closed and no save is under way, so
    /// nothing is left to finish.
    Closed(CloseAnswer),
    /// It has no window and needs the user, as when a save failed after its windows closed.
    Reopen,
}

impl From<Lifecycle> for Intent {
    fn from(life: Lifecycle) -> Self {
        Intent::Life(life)
    }
}

/// What runs while the state wants it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Running {
    /// Hears when another program changes a document.
    Watching,
    /// An export's work, for one start of it.
    Export(Instance, export::Steps),
}

impl Feature for Document {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Output;
    type Subscription = Running;

    fn start(state: &mut State, effects: &mut Effects<Effect>) {
        effects.push(Effect::Load(state.id));
    }

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect, Output>) {
        match intent {
            Intent::Loaded(Ok(Ok(doc))) => {
                state.loaded = true;
                state.saved = Content { title: doc.title, body: doc.body };
                state.content = state.saved.clone();
                state.revision = doc.revision;
            }
            Intent::Loaded(Ok(Err(why))) => state.error = Some(why),
            Intent::Loaded(Err(error)) => state.error = Some(format!("The document failed to load: {error}.")),
            Intent::EditTitle(title) => {
                if title.chars().count() > TITLE_LIMIT {
                    state.refused.fire(());
                } else {
                    state.content.title = title;
                }
            }
            Intent::EditBody(body) => state.content.body = body,
            Intent::Save => save(state, effects),
            Intent::Saved(result) => saved(state, result, effects),
            Intent::Export => {
                if !state.export.is_running() {
                    let (_, export) = state.export.start(export::State::default());
                    effects.start_child::<Export>(export, |never| match never {});
                }
            }
            // An intent for an export that has stopped, or for one before the one under way, is let go.
            Intent::Exporting(instance, intent) => {
                let Some(export) = state.export.get_mut(instance) else {
                    return;
                };
                for ended in effects.reduce_child::<Export>(export, intent, |never| match never {}) {
                    state.export.stop();
                    match ended {
                        export::Output::Finished => state.toast.fire("Exported".to_owned()),
                        export::Output::Failed(why) => state.error = Some(why),
                    }
                }
            }
            Intent::StopExport => {
                state.export.stop();
            }
            Intent::Delete => state.asking = Some(Ask::Delete),
            Intent::Answer(choice) => answer(state, choice, effects),
            Intent::Deleted(Ok(Ok(()))) => {
                state.deleted = true;
                effects.emit(Output::Changed);
                close(state, effects);
            }
            Intent::Deleted(Ok(Err(why))) => state.error = Some(why),
            Intent::Deleted(Err(error)) => state.error = Some(format!("The delete failed: {error}.")),
            Intent::Life(Lifecycle::CloseRequested) => {
                if state.dirty() {
                    state.asking = Some(Ask::Close);
                } else {
                    // A save under way goes on without a window, and its outcome is kept here.
                    close(state, effects);
                }
            }
            Intent::Life(Lifecycle::Shown | Lifecycle::Hidden | Lifecycle::CloseCalledOff) => {}
            Intent::WindowOpened => state.leaving = false,
            Intent::ChangedOutside(changed) => {
                if changed == Some(state.id) && !state.dirty() && !state.saves.is_running() {
                    effects.push(Effect::Load(state.id));
                }
            }
            Intent::Preferences(preferences) => state.preferences = preferences,
        }
    }

    fn subscriptions(state: &State) -> impl IntoIterator<Item = Running> {
        let watching = (!state.deleted).then_some(Running::Watching);
        let exporting = state.export.subscriptions::<Export>().map(|(instance, steps)| Running::Export(instance, steps));
        watching.into_iter().chain(exporting)
    }
}

fn save(state: &mut State, effects: &mut Effects<Effect, Output>) {
    if state.problem().is_some() || !state.dirty() {
        return;
    }
    if let Some(content) = state.saves.ask(state.content.clone()) {
        start_save(state, content, effects);
    }
}

fn start_save(state: &mut State, content: Content, effects: &mut Effects<Effect, Output>) {
    state.saving = Some(content.clone());
    effects.push(Effect::Save(state.id, content));
}

fn saved(state: &mut State, result: Result<Result<u32, String>, Error>, effects: &mut Effects<Effect, Output>) {
    let written = state.saving.take();
    let next = state.saves.done();
    match result {
        Ok(Ok(revision)) => {
            if let Some(written) = written {
                state.saved = written;
            }
            state.revision = revision;
            state.error = None;
            state.toast.fire("Saved".to_owned());
            effects.emit(Output::Changed);
            if let Some(content) = next {
                start_save(state, content, effects);
            }
            if state.closing && !state.saves.is_running() {
                state.closing = false;
                close(state, effects);
            } else {
                settle(state, effects);
            }
        }
        Ok(Err(why)) => failed(state, why, next, effects),
        Err(error) => failed(state, format!("The save failed: {error}."), next, effects),
    }
}

/// A save that failed keeps the edits, and keeps the window open if it was closing. If the windows have closed already, it stays,
/// and asks for a window to show what went wrong. A save that waited still goes.
fn failed(state: &mut State, why: String, next: Option<Content>, effects: &mut Effects<Effect, Output>) {
    state.error = Some(why);
    if let Some(content) = next {
        start_save(state, content, effects);
    }
    if std::mem::take(&mut state.closing) {
        effects.emit(Output::Closed(CloseAnswer::StayingOpen));
    } else if std::mem::take(&mut state.leaving) {
        effects.emit(Output::Closed(CloseAnswer::StayingOpen));
        effects.emit(Output::Reopen);
    }
}

fn answer(state: &mut State, choice: Choice, effects: &mut Effects<Effect, Output>) {
    let Some(asked) = state.asking.take() else {
        return;
    };
    match (asked, choice) {
        (Ask::Delete, Choice::Confirm) => effects.push(Effect::Delete(state.id)),
        (Ask::Delete, _) => {}
        (Ask::Close, Choice::Confirm) => {
            if state.problem().is_some() {
                state.asking = Some(Ask::Close);
                return;
            }
            state.closing = true;
            save(state, effects);
        }
        (Ask::Close, Choice::Discard) => {
            state.content = state.saved.clone();
            close(state, effects);
        }
        (Ask::Close, Choice::Cancel) => effects.emit(Output::Closed(CloseAnswer::StayingOpen)),
    }
}

/// Closes the windows, and stops an export under way. The document says it has closed once no save is under way.
fn close(state: &mut State, effects: &mut Effects<Effect, Output>) {
    state.close.fire(());
    state.export.stop();
    state.leaving = true;
    settle(state, effects);
}

/// Tells the library the document has closed, once its windows have and its saves are done.
fn settle(state: &mut State, effects: &mut Effects<Effect, Output>) {
    if state.leaving && !state.saves.is_running() {
        effects.emit(Output::Closed(CloseAnswer::Closed));
    }
}

/// What the status line says.
pub fn status(state: &State) -> String {
    if let Some(error) = &state.error {
        return error.clone();
    }
    if !state.loaded {
        return "Loading".to_owned();
    }
    if state.saves.is_running() {
        return "Saving".to_owned();
    }
    if state.dirty() { "Edited".to_owned() } else { format!("Revision {}", state.revision) }
}
