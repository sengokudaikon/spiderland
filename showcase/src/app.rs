//! The hosts, and the app that builds the stores and windows and connects them.
//!
//! Every place where the app writes by hand what the framework doesn't yet do is marked `Workaround:`, with what is missing.

use crate::document::{self, Ask, Choice, Document, Running};
use crate::library::{self, Library, Page};
use crate::service::{DocId, EXPORT_STEPS, Service};
use crate::settings::{self, Settings};
use crate::view::{DocRow, EditorWindow, LibraryWindow, RevisionRow};
use slint::{CloseRequestResponse, ComponentHandle, Timer};
use spiderland::{Host, Intents, Lifecycle, Slice, Store, Version, callbacks};
use spiderland_slint::{Background, Rows, WindowHost, Windowed, on_watch};
use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// The app: one settings store, one library with its window, and a store for each document opened, with its windows.
pub struct App(Rc<Inner>);

struct Inner {
    service: Service,
    background: Background,
    settings: Store<Settings>,
    library: Store<Library>,
    library_window: LibraryWindow,
    /// Each document opened, kept after its windows close so that a save under way finishes here.
    documents: RefCell<BTreeMap<DocId, Opened>>,
}

struct Opened {
    store: Store<Document>,
    windows: Windows,
}

/// The windows on one document. The app owns them, and their host paints them all.
type Windows = Rc<RefCell<Vec<EditorWindow>>>;

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("documents", &self.0.documents.borrow().keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl App {
    /// Builds the app on `service`, with work going to `background`, and shows the library.
    pub fn start(service: Service, background: Background) -> App {
        let inner = Rc::new_cyclic(|app: &Weak<Inner>| {
            let settings = Store::<Settings>::new(settings::Preferences::default(), Quiet);
            let library_window = LibraryWindow::new().expect("the library window");
            let host = LibraryHost {
                service: service.clone(),
                background: background.clone(),
                rows: Rows::default(),
                revisions: Rows::default(),
                shown: Slice::default(),
                app: app.clone(),
            };
            library_window.set_rows(host.rows.model());
            library_window.set_revisions(host.revisions.model());
            // The library keeps a copy of the preferences. Shared, it gets them as they are now, and each change after.
            let library = Store::<Library>::build(library::State::new(settings::Preferences::default()), Windowed::new(&library_window, host));
            settings.share(&library.intents(), |preferences| *preferences, |preferences| library::Intent::Preferences(*preferences));
            let library = library.start();
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
            Inner {
                service,
                background,
                settings,
                library,
                library_window,
                documents: RefCell::default(),
            }
        });
        inner.library_window.show().expect("show the library");
        App(inner)
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
        open(&self.0, id)
    }

    /// The store of `id`, once it has been opened.
    pub fn document(&self, id: DocId) -> Option<Store<Document>> {
        self.0.documents.borrow().get(&id).map(|opened| opened.store.clone())
    }

    /// The windows on `id` that are open.
    pub fn windows(&self, id: DocId) -> Vec<EditorWindow> {
        self.0
            .documents
            .borrow()
            .get(&id)
            .map(|opened| opened.windows.borrow().iter().map(ComponentHandle::clone_strong).collect())
            .unwrap_or_default()
    }

    /// Hides every window, so the app can be dropped.
    pub fn close(&self) {
        for opened in self.0.documents.borrow().values() {
            for window in opened.windows.borrow().iter() {
                let _ = window.hide();
            }
        }
        let _ = self.0.library_window.hide();
    }
}

fn open(inner: &Inner, id: DocId) -> EditorWindow {
    let window = EditorWindow::new().expect("an editor window");
    let mut documents = inner.documents.borrow_mut();
    documents.retain(|_, opened| opened.store.try_read(|state| !state.deleted).unwrap_or(true));
    if let Some(opened) = documents.get(&id) {
        // Workaround: no view that attaches to a live store. A window that comes later is painted here from the state, as no
        // render is due, and is handed the toast as seen, so an old one doesn't show again.
        opened.store.try_read(|state| {
            window.set_toast_seen(state.toast.serial());
            paint(&window, state);
        });
        connect(&window, &opened.store.intents(), &opened.windows);
        opened.windows.borrow_mut().push(window.clone_strong());
    } else {
        let windows: Windows = Rc::new(RefCell::new(vec![window.clone_strong()]));
        let host = DocumentHost {
            id,
            windows: windows.clone(),
            service: inner.service.clone(),
            background: inner.background.clone(),
        };
        let wiring = Store::<Document>::build(document::State::new(id, settings::Preferences::default()), host);
        inner
            .settings
            .share(&wiring.intents(), |preferences| *preferences, |preferences| document::Intent::Preferences(*preferences));
        wiring.forward(&inner.library.intents(), |output| {
            (*output == document::Output::Changed).then_some(library::Intent::Changed)
        });
        let store = wiring.start();
        connect(&window, &store.intents(), &windows);
        documents.insert(id, Opened { store, windows });
    }
    window.show().expect("show an editor");
    window
}

/// Sends what the user does in `window` to the document.
fn connect(window: &EditorWindow, intents: &Intents<document::Intent>, windows: &Windows) {
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
    // Workaround: no Slint helper for closing, so the close request is answered here by hand. And with no views that attach and
    // leave, which window may simply go, because another still shows the document, is the app's to know.
    let (this, windows, intents) = (window.as_weak(), Rc::downgrade(windows), intents.clone());
    window.window().on_close_requested(move || {
        let Some(windows) = windows.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };
        let others = windows.borrow().len() > 1;
        if others {
            let mut windows = windows.borrow_mut();
            if let Some(at) = windows
                .iter()
                .position(|window| this.upgrade().is_some_and(|this| std::ptr::eq(window.window(), this.window())))
            {
                let_go(vec![windows.remove(at)]);
            }
            return CloseRequestResponse::HideWindow;
        }
        intents.send(document::Intent::Life(Lifecycle::CloseRequested));
        CloseRequestResponse::KeepWindowShown
    });
}

/// Drops windows on the next turn of the event loop, as one of them may be running the callback that let them go.
fn let_go(windows: Vec<EditorWindow>) {
    Timer::single_shot(Duration::ZERO, move || drop(windows));
}

/// Hosts nothing: for the settings, whose state the others copy.
struct Quiet;

impl Host<Settings> for Quiet {
    fn render(&mut self, _: &settings::Preferences, _: &Intents<settings::Intent>) {}

    fn perform(&mut self, effect: spiderland::Never, _: &Intents<settings::Intent>) {
        match effect {}
    }
}

struct LibraryHost {
    service: Service,
    background: Background,
    rows: Rows<DocRow>,
    revisions: Rows<RevisionRow>,
    shown: Slice<(Version, Option<DocId>)>,
    app: Weak<Inner>,
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
        window.set_status(library::status(state).into());
        window.set_page_size(state.preferences.page_size as i32);
        window.set_word_count(state.preferences.word_count);
        let history = match state.shown() {
            Page::List => None,
            Page::History(id) => Some(id),
        };
        window.set_history_shown(history.is_some());
        let revisions = state
            .history
            .as_ref()
            .filter(|(id, _)| Some(*id) == history)
            .map(|(_, revisions)| revisions.as_slice())
            .unwrap_or_default();
        window.set_history_title(history.map(|id| format!("History of document {}", id.0)).unwrap_or_default().into());
        self.revisions.sync(revisions.iter().map(|revision| RevisionRow {
            revision: revision.revision as i32,
            title: revision.title.as_str().into(),
        }));
    }

    fn perform(&mut self, _: Option<&LibraryWindow>, effect: library::Effect, intents: &Intents<library::Intent>) {
        let service = self.service.clone();
        match effect {
            library::Effect::Search { asked, query, page, size } => {
                self.background
                    .run(intents, move || service.search(&query, page, size), move |result| library::Intent::Found { asked, result });
            }
            library::Effect::LoadHistory(id) => {
                self.background
                    .run(intents, move || service.history(id), move |result| library::Intent::HistoryLoaded { id, result });
            }
            // Workaround: no children a parent owns. Documents aren't the library's children, so its host opens them through the
            // app.
            library::Effect::Open(id) => {
                if let Some(app) = self.app.upgrade() {
                    open(&app, id);
                }
            }
        }
    }
}

/// Workaround: a store has one host, and a host one view. So this host paints every window on its document itself.
struct DocumentHost {
    id: DocId,
    windows: Windows,
    service: Service,
    background: Background,
}

impl Host<Document> for DocumentHost {
    fn render(&mut self, state: &document::State, _: &Intents<document::Intent>) {
        for window in self.windows.borrow().iter() {
            paint(window, state);
        }
    }

    fn perform(&mut self, effect: document::Effect, intents: &Intents<document::Intent>) {
        let service = self.service.clone();
        match effect {
            document::Effect::Load(id) => self.background.run(intents, move || service.load(id), document::Intent::Loaded),
            document::Effect::Save(id, content) => {
                self.background
                    .run(intents, move || service.save(id, &content.title, &content.body), document::Intent::Saved);
            }
            document::Effect::Delete(id) => self.background.run(intents, move || service.delete(id), document::Intent::Deleted),
            document::Effect::Hide => {
                let windows: Vec<EditorWindow> = self.windows.borrow_mut().drain(..).collect();
                for window in &windows {
                    let _ = window.hide();
                }
                let_go(windows);
            }
        }
    }

    fn subscribe(&mut self, key: &Running, intents: &Intents<document::Intent>) -> Box<dyn Any> {
        match *key {
            Running::Watching => {
                let changes = self.service.changes();
                let seen = changes.clone();
                Box::new(on_watch(changes, intents, move || document::Intent::ChangedOutside(*seen.borrow())))
            }
            Running::Exporting => {
                let (service, id) = (self.service.clone(), self.id);
                Box::new(self.background.stoppable(
                    intents,
                    move |stop, progress| {
                        for step in 1..=EXPORT_STEPS {
                            if stop.is_stopped() {
                                return false;
                            }
                            service.export_step(id, step);
                            progress.report(step);
                        }
                        true
                    },
                    document::Intent::Progress,
                    document::Intent::Exported,
                ))
            }
        }
    }
}

/// Shows `state` in one window.
fn paint(window: &EditorWindow, state: &document::State) {
    window.set_doc_title(state.content.title.as_str().into());
    window.set_title_refused(state.refused.serial());
    window.set_body(state.content.body.as_str().into());
    window.set_status(document::status(state).into());
    window.set_problem(state.problem().unwrap_or_default().into());
    window.set_word_count_shown(state.preferences.word_count);
    window.set_words(state.words() as i32);
    window.set_toast_text(state.toast.value().as_str().into());
    window.set_toast(state.toast.serial());
    window.set_asking(
        match state.asking {
            None => "",
            Some(Ask::Delete) => "delete",
            Some(Ask::Close) => "close",
        }
        .into(),
    );
    window.set_exporting(state.export.is_some());
    window.set_export_done(state.export.map_or(0, |export| export.done as i32));
    window.set_export_of(EXPORT_STEPS as i32);
}
