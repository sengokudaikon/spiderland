//! The hosts, and the app that builds the stores and windows and connects them.
//!
//! Every place where the app writes by hand what the framework doesn't yet do is marked `Workaround:`, with what is missing.

use crate::document::{self, Ask, Choice, Document, Running};
use crate::export::{self, Export};
use crate::history::{self, History};
use crate::library::{self, Library, Page};
use crate::service::{DocId, EXPORT_STEPS, Service};
use crate::settings::{self, Settings};
use crate::trail::Trail;
use crate::view::{DocRow, EditorWindow, LibraryWindow, RevisionRow};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use slint::{ComponentHandle, Timer};
use spiderland::{Attached, Child, Children, Fired, Host, Intents, Lifecycle, Never, Saved, Slice, Store, Version, callbacks};
use spiderland_slint::{Background, Rows, WindowHost, WindowView, Windowed, ask_before_closing, attach_window, on_watch};
use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

/// The app: one settings store, and the library with its window. The library keeps the documents open and its history pages as
/// children, each a store of its own. Every store adds what it runs to one trail.
pub struct App(Rc<Inner>);

/// What the app saves: each store's part, under its key, for the next start.
pub type Session = Vec<(String, Value)>;

struct Inner {
    settings: Store<Settings>,
    library: Store<Library>,
    library_window: LibraryWindow,
    documents: Children<DocId, Opened>,
    trail: Trail,
    /// Saved as the library was last asked to close.
    session: Rc<RefCell<Session>>,
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App").field("documents", &self.0.documents.keys()).finish_non_exhaustive()
    }
}

impl App {
    /// Builds the app on `service`, with work going to `background`, and shows the library.
    pub fn start(service: Service, background: Background) -> App {
        App::restore(service, background, Session::new())
    }

    /// Builds the app as [`App::start`] does, where `session` left off: the settings, the search and its page, the document picked,
    /// and the pages and documents open. Each history page and document takes back its own part: the revision picked, and an
    /// export under way, which goes on from the step it had done. A part that doesn't decode, such as one an older build saved,
    /// starts afresh.
    pub fn restore(service: Service, background: Background, session: Session) -> App {
        let saved = Saved::new(session);
        let trail = Trail::default();
        let preferences = saved.take("settings").and_then(decode).unwrap_or_default();
        let wiring = Store::<Settings>::build(preferences, Quiet);
        wiring.observe(trail.follow("settings", settings::summary));
        wiring.save_in(&saved, "settings", encode);
        let settings = wiring.start();
        let library_window = LibraryWindow::new().expect("the library window");
        let documents = Children::default();
        let host = LibraryHost {
            service: service.clone(),
            background: background.clone(),
            rows: Rows::default(),
            shown: Slice::default(),
            documents: documents.clone(),
        };
        library_window.set_rows(host.rows.model());
        // The library keeps a copy of the preferences. Shared, it gets them as they are now, and each change after. It starts on
        // them too, so the first one shared changes nothing, and the page restored stays.
        let state = match saved.take("library").and_then(decode) {
            Some(session) => library::State::restored(preferences, session),
            None => library::State::new(preferences),
        };
        let wiring = Store::<Library>::build(state, Windowed::new(&library_window, host));
        wiring.observe(trail.follow("library", library::status));
        wiring.save_in(&saved, "library", |state| encode(&library::session(state)));
        settings.share(&wiring.intents(), |preferences| *preferences, |preferences| library::Intent::Preferences(*preferences));
        // Each history page is a store of its own, with a view in the library's window. Going back drops it.
        // Each child saves its own part of the session under a key of its own, and takes it back as it is built. The library
        // knows nothing of what they keep.
        wiring.children(&Children::default(), library::pages, {
            let (service, background, window, trail, saved) = (service.clone(), background.clone(), library_window.clone_strong(), trail.clone(), saved.clone());
            move |&id, _| HistoryPage::new(id, &service, &background, &window, &trail, &saved)
        });
        // Each document is a store of its own, which tells the library what it says.
        wiring.children(&documents, library::documents, {
            let (settings, trail, saved) = (settings.clone(), trail.clone(), saved.clone());
            move |&id, library| Opened::new(id, &service, &background, &settings, library, &trail, &saved)
        });
        let library = wiring.start();
        callbacks!(library_window, library.intents(), {
            on_typed: |query| library::Intent::Typed(query.into()),
            on_select: |id| library::Intent::Select(DocId(id as u32)),
            on_show_history: |id| library::Intent::ShowHistory(DocId(id as u32)),
            on_open_document: |id| library::Intent::Open(DocId(id as u32)),
            on_previous_page: || library::Intent::PreviousPage,
            on_next_page: || library::Intent::NextPage,
            on_refresh: || library::Intent::Refresh,
            on_back: || library::Intent::Back,
        });
        callbacks!(library_window, settings.intents(), {
            on_page_size_changed: |size| settings::Intent::PageSize(size.max(0) as usize),
            on_word_count_changed: |on| settings::Intent::WordCount(on),
        });
        // Closing the library closes the app, once every document has closed. The session is saved as the user asks, while the
        // documents it names are still open.
        let session = Rc::new(RefCell::new(Session::new()));
        ask_before_closing(&library_window, &library.intents(), {
            let session = Rc::clone(&session);
            move || {
                session.replace(saved.save());
                Some(library::Intent::Life(Lifecycle::CloseRequested))
            }
        });
        library_window.show().expect("show the library");
        App(Rc::new(Inner {
            settings,
            library,
            library_window,
            documents,
            trail,
            session,
        }))
    }

    /// What the app saved as the library was last asked to close, for [`App::restore`] to start on the next time.
    pub fn session(&self) -> Session {
        self.0.session.borrow().clone()
    }

    /// What the stores ran, and what changed, as the trail keeps it.
    pub fn trail(&self) -> &Trail {
        &self.0.trail
    }

    /// The library's window.
    pub fn library(&self) -> &LibraryWindow {
        &self.0.library_window
    }

    /// The library's store.
    pub fn library_store(&self) -> &Store<Library> {
        &self.0.library
    }

    /// The settings' store.
    pub fn settings(&self) -> &Store<Settings> {
        &self.0.settings
    }

    /// Opens a window on `id`, and returns it. A document already open gets another window on the same store.
    pub fn open(&self, id: DocId) -> EditorWindow {
        self.0.library.send(library::Intent::Open(id));
        let opened = self.0.documents.get(&id).expect("the library opened the document");
        let windows = opened.windows.borrow();
        windows.last().expect("and a window on it").clone_strong()
    }

    /// The store of `id`, while it is open.
    pub fn document(&self, id: DocId) -> Option<Store<Document>> {
        self.0.documents.get(&id).map(|opened| opened.store.clone())
    }

    /// The windows on `id` that are open.
    pub fn windows(&self, id: DocId) -> Vec<EditorWindow> {
        let Some(opened) = self.0.documents.get(&id) else {
            return Vec::new();
        };
        let windows = opened.windows.borrow();
        windows.iter().filter(|window| window.window().is_visible()).map(ComponentHandle::clone_strong).collect()
    }

    /// Hides every window, so the app can be dropped.
    pub fn close(&self) {
        for id in self.0.documents.keys() {
            if let Some(opened) = self.0.documents.get(&id) {
                for window in opened.windows.borrow().iter() {
                    let _ = window.hide();
                }
            }
        }
        let _ = self.0.library_window.hide();
    }
}

/// A store's part of the session, as JSON.
fn encode(part: &impl Serialize) -> Value {
    serde_json::to_value(part).expect("plain data turns into JSON")
}

/// A store's part of the session, if it is the part this build saves.
fn decode<T: DeserializeOwned>(part: Value) -> Option<T> {
    serde_json::from_value(part).ok()
}

/// One document the library keeps: its store, and the windows on it. Each window has a view attached to the store for as long as
/// it is open.
struct Opened {
    store: Store<Document>,
    windows: RefCell<Vec<EditorWindow>>,
}

impl Opened {
    /// Builds the store of `id`, which takes the settings, tells the library what it says, adds what it runs to `trail`, and
    /// keeps its part of the session in `saved`.
    fn new(id: DocId, service: &Service, background: &Background, settings: &Store<Settings>, library: &Intents<library::Intent>, trail: &Trail, saved: &Saved<Value>) -> Opened {
        let host = DocumentHost {
            service: service.clone(),
            background: background.clone(),
            export: ExportHost {
                id,
                service: service.clone(),
                background: background.clone(),
            },
        };
        // It starts on the preferences as they are, so the first one shared changes nothing.
        let preferences = settings.try_read(|preferences| *preferences).unwrap_or_default();
        let key = format!("document {}", id.0);
        let state = match saved.take(&key).and_then(decode) {
            Some(session) => document::State::restored(id, preferences, session),
            None => document::State::new(id, preferences),
        };
        let wiring = Store::<Document>::build(state, host);
        wiring.save_in(saved, key, |state| encode(&document::session(state)));
        wiring.observe(trail.follow(format!("document {}", id.0), document::status));
        settings.share(&wiring.intents(), |preferences| *preferences, |preferences| document::Intent::Preferences(*preferences));
        wiring.forward(library, move |output| library::Intent::Document(id, *output));
        Opened {
            store: wiring.start(),
            windows: RefCell::default(),
        }
    }

    /// Opens another window on the document, which shows it at once, and lets go of the windows closed before.
    fn open_window(&self) {
        let hidden: Vec<_> = self.windows.borrow_mut().extract_if(.., |window| !window.window().is_visible()).collect();
        let_go(hidden);
        let window = EditorWindow::new().expect("an editor window");
        show(&window, &self.store);
        self.windows.borrow_mut().push(window.clone_strong());
        self.store.send(document::Intent::WindowOpened);
        window.show().expect("show an editor");
    }
}

impl Child for Opened {
    fn tell(&self, life: Lifecycle) {
        self.store.tell(life);
    }
}

/// The library lets a document go from inside one of its windows' callbacks, so its windows go on the next turn.
impl Drop for Opened {
    fn drop(&mut self) {
        let_go(std::mem::take(self.windows.get_mut()));
    }
}

/// Attaches a view in `window` to the document, which it shows at once, and sends what the user does there to the document. One
/// of several windows simply closes, and the document stays with the others. The last one asks the document, which may ask about
/// unsaved edits first.
fn show(window: &EditorWindow, store: &Store<Document>) {
    attach_window(store, window, EditorView::default(), |store| {
        (store.views() == 1).then_some(document::Intent::Life(Lifecycle::CloseRequested))
    });
    let intents = store.intents();
    callbacks!(window, intents, {
        on_title_edited: |title| document::Intent::EditTitle(title.into()),
        on_body_edited: |body| document::Intent::EditBody(body.into()),
        on_save: || document::Intent::Save,
        on_export: || document::Intent::Export,
        on_stop_export: || document::Intent::StopExport,
        on_delete: || document::Intent::Delete,
        on_answer: |choice| document::Intent::Answer(match choice {
            0 => Choice::Confirm,
            1 => Choice::Discard,
            _ => Choice::Cancel,
        }),
    });
}

/// Drops windows on the next turn of the event loop, as one of them may be running the callback that let them go.
fn let_go(windows: Vec<EditorWindow>) {
    if !windows.is_empty() {
        Timer::single_shot(Duration::ZERO, move || drop(windows));
    }
}

/// Hosts nothing: for the settings, whose state the others copy.
struct Quiet;

impl Host<Settings> for Quiet {
    fn perform(&mut self, effect: Never, _: &Intents<settings::Intent>) {
        match effect {}
    }
}

struct LibraryHost {
    service: Service,
    background: Background,
    rows: Rows<DocRow>,
    shown: Slice<(Version, Option<DocId>)>,
    documents: Children<DocId, Opened>,
}

impl WindowHost<Library> for LibraryHost {
    type Window = LibraryWindow;

    fn render(&mut self, window: &LibraryWindow, state: &library::State, _: &Intents<library::Intent>) {
        let rows = &self.rows;
        self.shown.show((&state.found, state.selected), library::rows, |shown| {
            rows.sync_by(
                |row| row.id,
                shown.into_iter().map(|row| DocRow {
                    id: row.id.0 as i32,
                    title: row.title.into(),
                    selected: row.selected,
                }),
            );
        });
        window.set_query(state.query.as_str().into());
        window.set_status(library::status(state).into());
        window.set_page_size(state.preferences.page_size as i32);
        window.set_word_count(state.preferences.word_count);
        // The history page shows itself, through its own view.
        window.set_history_shown(matches!(state.shown(), Page::History(_)));
    }

    fn perform(&mut self, window: Option<&LibraryWindow>, effect: library::Effect, intents: &Intents<library::Intent>) {
        match effect {
            library::Effect::Search { asked, query, page, size } => {
                let service = self.service.clone();
                self.background
                    .run(intents, move || service.search(&query, page, size), move |result| library::Intent::Found { asked, result });
            }
            // The document is the library's child, built in this batch if it is new.
            library::Effect::OpenWindow(id) => {
                if let Some(opened) = self.documents.get(&id) {
                    opened.open_window();
                }
            }
            library::Effect::Close => {
                if let Some(window) = window {
                    let _ = window.hide();
                }
            }
        }
    }
}

/// One history page the library keeps: its store, and its view in the library's window.
struct HistoryPage {
    store: Store<History>,
    _view: Attached,
}

impl HistoryPage {
    fn new(id: DocId, service: &Service, background: &Background, window: &LibraryWindow, trail: &Trail, saved: &Saved<Value>) -> HistoryPage {
        let host = HistoryHost {
            service: service.clone(),
            background: background.clone(),
        };
        let key = format!("history {}", id.0);
        let state = match saved.take(&key).and_then(decode) {
            Some(session) => history::State::restored(id, session),
            None => history::State::new(id),
        };
        let wiring = Store::build(state, host);
        wiring.save_in(saved, key, |state| encode(&history::session(state)));
        wiring.observe(trail.follow(format!("history {}", id.0), history::title));
        let store = wiring.start();
        let view = store.attach(Windowed::new(window, HistoryView::default()));
        HistoryPage { store, _view: view }
    }
}

impl Child for HistoryPage {
    fn tell(&self, life: Lifecycle) {
        self.store.tell(life);
    }
}

/// Loads a history page's saved states.
struct HistoryHost {
    service: Service,
    background: Background,
}

impl Host<History> for HistoryHost {
    fn perform(&mut self, history::Effect::Load(id): history::Effect, intents: &Intents<history::Intent>) {
        let service = self.service.clone();
        self.background.run(intents, move || service.history(id), history::Intent::Loaded);
    }
}

/// Shows a history page in the library's window while it is on top. Each page has its rows, and puts them in the window as it
/// comes to the top.
#[derive(Default)]
struct HistoryView {
    revisions: Rows<RevisionRow>,
    showing: bool,
}

impl WindowView<History> for HistoryView {
    type Window = LibraryWindow;

    fn render(&mut self, window: &LibraryWindow, state: &history::State, intents: &Intents<history::Intent>) {
        if !state.shown {
            self.showing = false;
            return;
        }
        if !self.showing {
            // The window's rows and their callback are the page's on top.
            window.set_revisions(self.revisions.model());
            callbacks!(window, intents, {
                on_pick_revision: |revision| history::Intent::Pick(revision as u32),
            });
            self.showing = true;
        }
        window.set_history_title(history::title(state).into());
        self.revisions.sync(state.revisions.iter().flatten().map(|revision| RevisionRow {
            revision: revision.revision as i32,
            title: revision.title.as_str().into(),
            picked: state.picked == Some(revision.revision),
        }));
    }
}

/// Runs a document's effects, and its export's through the export's host. Its windows are views attached to the store, so it
/// shows nothing itself.
struct DocumentHost {
    service: Service,
    background: Background,
    export: ExportHost,
}

impl Host<Document> for DocumentHost {
    fn perform(&mut self, effect: document::Effect, intents: &Intents<document::Intent>) {
        let service = self.service.clone();
        match effect {
            document::Effect::Load(id) => self.background.run(intents, move || service.load(id), document::Intent::Loaded),
            document::Effect::Save(id, content) => {
                self.background
                    .run(intents, move || service.save(id, &content.title, &content.body), document::Intent::Saved);
            }
            document::Effect::Delete(id) => self.background.run(intents, move || service.delete(id), document::Intent::Deleted),
        }
    }

    fn subscribe(&mut self, key: &Running, intents: &Intents<document::Intent>) -> Box<dyn Any> {
        match *key {
            Running::Watching => {
                let changes = self.service.changes();
                let seen = changes.clone();
                Box::new(on_watch(changes, intents, move || document::Intent::ChangedOutside(*seen.borrow())))
            }
            // What the export's host sends comes back wrapped as the document's, with the start it is for.
            Running::Export(instance, steps) => self.export.subscribe(&steps, &intents.map(move |intent| document::Intent::Exporting(instance, intent))),
        }
    }
}

/// Runs an export's work on the pool, stopped once the export goes.
struct ExportHost {
    id: DocId,
    service: Service,
    background: Background,
}

impl Host<Export> for ExportHost {
    fn perform(&mut self, effect: Never, _: &Intents<export::Intent>) {
        match effect {}
    }

    fn subscribe(&mut self, &export::Steps { from }: &export::Steps, intents: &Intents<export::Intent>) -> Box<dyn Any> {
        let (service, id) = (self.service.clone(), self.id);
        Box::new(self.background.stoppable(
            intents,
            move |stop, progress| {
                for step in from + 1..=EXPORT_STEPS {
                    if stop.is_stopped() {
                        return false;
                    }
                    service.export_step(id, step);
                    progress.report(step);
                }
                true
            },
            export::Intent::Progress,
            export::Intent::Exported,
        ))
    }
}

/// One window on a document. Each counts the document's pulses from when it attached, so a window opened later acts only on what
/// fires after that.
#[derive(Default)]
struct EditorView {
    refused: Fired,
    toast: Fired,
    close: Fired,
    /// Set once the document closed and this view hid its window, so the store lets the view go.
    closed: bool,
}

impl WindowView<Document> for EditorView {
    type Window = EditorWindow;

    fn render(&mut self, window: &EditorWindow, state: &document::State, _: &Intents<document::Intent>) {
        if self.close.serial(&state.close) > 0 {
            let _ = window.hide();
            self.closed = true;
            return;
        }
        paint(window, state, self);
    }

    fn is_gone(&self) -> bool {
        self.closed
    }
}

/// Shows `state` in one window.
fn paint(window: &EditorWindow, state: &document::State, view: &mut EditorView) {
    window.set_doc_title(state.content.title.as_str().into());
    window.set_title_refused(view.refused.serial(&state.refused));
    window.set_body(state.content.body.as_str().into());
    window.set_status(document::status(state).into());
    window.set_problem(state.problem().unwrap_or_default().into());
    window.set_word_count_shown(state.preferences.word_count);
    window.set_words(state.words() as i32);
    window.set_toast_text(state.toast.value().as_str().into());
    window.set_toast(view.toast.serial(&state.toast));
    window.set_asking(
        match state.asking {
            None => "",
            Some(Ask::Delete) => "delete",
            Some(Ask::Close) => "close",
        }
        .into(),
    );
    window.set_exporting(state.export.is_running());
    window.set_export_done(state.export.get().map_or(0, |export| export.done as i32));
    window.set_export_of(EXPORT_STEPS as i32);
}
