//! An export of one document on the pool, with its progress. It is a feature of its own, which the document runs inside its store
//! while the export lasts: dropping it stops the work.

use spiderland::{Effects, Error, Feature, Never};

/// The feature.
#[derive(Debug)]
pub struct Export;

/// An export under way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The steps done.
    pub done: u32,
    /// The steps done before this export started, as when it goes on with one an earlier run left under way.
    pub from: u32,
}

impl State {
    /// An export that goes on from `done` steps, as one an earlier run left under way does.
    pub fn resumed(done: u32) -> State {
        State { done, from: done }
    }
}

/// What the work reports.
#[derive(Debug)]
pub enum Intent {
    /// The steps done so far.
    Progress(u32),
    /// The export's end: whether it finished, or why not. An export stopped sends nothing.
    Exported(Result<bool, Error>),
}

/// How the export ended, for the document to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// Every step is done.
    Finished,
    /// Why it failed.
    Failed(String),
}

/// The work, on the pool, which runs while the export lasts, from the step after `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    /// The steps done before it started.
    pub from: u32,
}

impl Feature for Export {
    type State = State;
    type Intent = Intent;
    type Effect = Never;
    type Output = Output;
    type Subscription = Steps;

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Never, Output>) {
        match intent {
            Intent::Progress(done) => state.done = done,
            Intent::Exported(Ok(true)) => effects.emit(Output::Finished),
            Intent::Exported(Ok(false)) => {}
            Intent::Exported(Err(error)) => effects.emit(Output::Failed(format!("The export failed: {error}."))),
        }
    }

    fn subscriptions(state: &State) -> impl IntoIterator<Item = Steps> {
        [Steps { from: state.from }]
    }
}
