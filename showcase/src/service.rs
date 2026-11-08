//! A fake document service. It runs on the showcase's pool, so each call blocks a pool thread, as a disk or a network would.
//!
//! A test controls it: each call can wait at a [`Gate`] until the test lets it through, a save can be made to fail, and an export
//! step can be made to panic.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Names one document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocId(pub u32);

/// A document as the service keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    /// Its name.
    pub id: DocId,
    /// Its title, never empty.
    pub title: String,
    /// What it says.
    pub body: String,
    /// Moves on with each save.
    pub revision: u32,
}

/// One line of a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The document.
    pub id: DocId,
    /// Its title.
    pub title: String,
}

/// One page of a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The documents on this page.
    pub docs: Vec<Summary>,
    /// How many documents match, on every page.
    pub total: usize,
}

/// One saved state of a document, for its history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    /// Which save it was.
    pub revision: u32,
    /// The title it had.
    pub title: String,
}

/// How many steps an export takes. Each passes the gate, so a test can move it on one step at a time.
pub const EXPORT_STEPS: u32 = 4;

/// Where each call can wait for the test. Open by default.
#[derive(Debug, Default)]
pub struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct GateState {
    holding: bool,
    /// The calls waiting here, by name.
    waiting: Vec<String>,
    /// Calls let through one at a time, by name, not yet gone.
    released: Vec<String>,
}

impl Gate {
    /// From now on, each call waits here until [`Gate::release`] or [`Gate::open`] lets it through.
    pub fn hold(&self) {
        self.state.lock().unwrap().holding = true;
    }

    /// Lets every call through, those waiting and those to come.
    pub fn open(&self) {
        self.state.lock().unwrap().holding = false;
        self.changed.notify_all();
    }

    /// The calls waiting here, by name, in the order they came.
    pub fn waiting(&self) -> Vec<String> {
        self.state.lock().unwrap().waiting.clone()
    }

    /// Waits until a call named `name` waits here. Panics after five seconds.
    pub fn wait_for(&self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = self.state.lock().unwrap();
        while !state.waiting.iter().any(|waiting| waiting == name) {
            let left = deadline.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("no call named {name} came"));
            state = self.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    /// Lets one waiting call named `name` through. Panics if none waits.
    pub fn release(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        assert!(
            state.waiting.iter().any(|waiting| waiting == name),
            "no call named {name} waits; waiting: {:?}",
            state.waiting
        );
        state.released.push(name.to_owned());
        self.changed.notify_all();
    }

    fn pass(&self, name: String) {
        let mut state = self.state.lock().unwrap();
        if !state.holding {
            return;
        }
        state.waiting.push(name.clone());
        self.changed.notify_all();
        loop {
            if let Some(at) = state.released.iter().position(|released| *released == name) {
                state.released.remove(at);
                break;
            }
            if !state.holding {
                break;
            }
            state = self.changed.wait(state).unwrap();
        }
        let at = state.waiting.iter().position(|waiting| *waiting == name).expect("a call that waited is listed");
        state.waiting.remove(at);
        self.changed.notify_all();
    }
}

#[derive(Debug, Default)]
struct Kept {
    docs: BTreeMap<DocId, Doc>,
    history: BTreeMap<DocId, Vec<Revision>>,
    /// Why the next save fails.
    failing: Option<String>,
    /// Whether the next export step panics.
    panicking: bool,
}

/// The documents, behind a gate. Clones share them.
#[derive(Clone, Debug)]
pub struct Service(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    kept: Mutex<Kept>,
    gate: Gate,
    /// The document another program changed last.
    outside: watch::Sender<Option<DocId>>,
}

impl Service {
    /// A service holding a document for each title and body, numbered from 1.
    pub fn new<'a>(docs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Service {
        let mut kept = Kept::default();
        for (n, (title, body)) in docs.into_iter().enumerate() {
            let id = DocId(n as u32 + 1);
            let doc = Doc {
                id,
                title: title.to_owned(),
                body: body.to_owned(),
                revision: 1,
            };
            kept.history.insert(
                id,
                vec![Revision {
                    revision: 1,
                    title: doc.title.clone(),
                }],
            );
            kept.docs.insert(id, doc);
        }
        Service(Arc::new(Inner {
            kept: Mutex::new(kept),
            gate: Gate::default(),
            outside: watch::channel(None).0,
        }))
    }

    /// Where its calls wait for the test.
    pub fn gate(&self) -> &Gate {
        &self.0.gate
    }

    /// The documents whose titles hold `query`, ignoring case, `size` to a page. Waits at the gate as `search {query}`.
    pub fn search(&self, query: &str, page: usize, size: usize) -> Found {
        self.0.gate.pass(format!("search {query}"));
        let query = query.to_lowercase();
        let kept = self.0.kept.lock().unwrap();
        let matching: Vec<Summary> = kept
            .docs
            .values()
            .filter(|doc| doc.title.to_lowercase().contains(&query))
            .map(|doc| Summary {
                id: doc.id,
                title: doc.title.clone(),
            })
            .collect();
        let total = matching.len();
        Found {
            docs: matching.into_iter().skip(page * size).take(size).collect(),
            total,
        }
    }

    /// One document. Waits at the gate as `load {id}`.
    pub fn load(&self, id: DocId) -> Result<Doc, String> {
        self.0.gate.pass(format!("load {}", id.0));
        self.get(id).ok_or_else(|| format!("document {} is gone", id.0))
    }

    /// Writes a document and returns its new revision. Waits at the gate as `save {id}`.
    pub fn save(&self, id: DocId, title: &str, body: &str) -> Result<u32, String> {
        self.0.gate.pass(format!("save {}", id.0));
        let mut kept = self.0.kept.lock().unwrap();
        if let Some(why) = kept.failing.take() {
            return Err(why);
        }
        let doc = kept.docs.get_mut(&id).ok_or_else(|| format!("document {} is gone", id.0))?;
        doc.title = title.to_owned();
        doc.body = body.to_owned();
        doc.revision += 1;
        let revision = Revision {
            revision: doc.revision,
            title: doc.title.clone(),
        };
        kept.history.entry(id).or_default().push(revision.clone());
        Ok(revision.revision)
    }

    /// Deletes a document. Waits at the gate as `delete {id}`.
    pub fn delete(&self, id: DocId) -> Result<(), String> {
        self.0.gate.pass(format!("delete {}", id.0));
        let mut kept = self.0.kept.lock().unwrap();
        kept.history.remove(&id);
        kept.docs.remove(&id).map(drop).ok_or_else(|| format!("document {} is gone", id.0))
    }

    /// A document's saved states, newest first. Waits at the gate as `history {id}`.
    pub fn history(&self, id: DocId) -> Vec<Revision> {
        self.0.gate.pass(format!("history {}", id.0));
        let kept = self.0.kept.lock().unwrap();
        kept.history.get(&id).map(|revisions| revisions.iter().rev().cloned().collect()).unwrap_or_default()
    }

    /// One step of an export. Waits at the gate as `export {id} {step}`.
    pub fn export_step(&self, id: DocId, step: u32) {
        self.0.gate.pass(format!("export {} {step}", id.0));
        let panicking = std::mem::take(&mut self.0.kept.lock().unwrap().panicking);
        if panicking {
            panic!("an export step panicked, as this test asked");
        }
    }

    /// Says when another program changed a document, with the one it changed.
    pub fn changes(&self) -> watch::Receiver<Option<DocId>> {
        self.0.outside.subscribe()
    }

    /// Changes a document's body as another program would, and says so on [`Service::changes`].
    pub fn change_outside(&self, id: DocId, body: &str) {
        {
            let mut kept = self.0.kept.lock().unwrap();
            let doc = kept.docs.get_mut(&id).expect("a document to change");
            doc.body = body.to_owned();
            doc.revision += 1;
        }
        self.0.outside.send_replace(Some(id));
    }

    /// Makes the next save fail, saying `why`.
    pub fn fail_next_save(&self, why: &str) {
        self.0.kept.lock().unwrap().failing = Some(why.to_owned());
    }

    /// Makes the next export step panic.
    pub fn panic_next_export(&self) {
        self.0.kept.lock().unwrap().panicking = true;
    }

    /// A document as it is now, with no wait, for a test to check.
    pub fn get(&self, id: DocId) -> Option<Doc> {
        self.0.kept.lock().unwrap().docs.get(&id).cloned()
    }
}
