//! The showcase's scenarios over real windows with no display: the whole app, on its own pool, driven by real input. Each control
//! is found by its accessible label, and clicked or typed into where Slint's layout put it. Slint's platform is set once per
//! process, so the scenarios run in turn on the main thread, each on an app of its own.

#![allow(missing_docs, reason = "the windows' types come from Slint, with no docs")]

use showcase::app::{App, Session};
use showcase::document::{AUTOSAVE_AFTER, RETRY_AFTER};
use showcase::pool::Pool;
use showcase::service::{DocId, Service};
use showcase::view::{EditorWindow, LibraryWindow};
use slint::platform::Key;
use slint::{ComponentHandle, Model};
use spiderland_slint::Background;
use spiderland_slint::testing::{Headless, Input, advance, element, find, idle, settle, until};

/// The sizes the windows are tested at: what each prefers, which shows every row a test reads.
const LIBRARY: (u32, u32) = (640, 480);
const EDITOR: (u32, u32) = (560, 420);

/// One app, on its own service and pool.
struct Scene<'a> {
    headless: &'a Headless,
    service: Service,
    background: Background,
    app: App,
    /// Dropped last, once nothing waits at the gate, as it waits for its threads.
    _pool: Pool,
}

impl<'a> Scene<'a> {
    fn start(headless: &'a Headless) -> Scene<'a> {
        Scene::restore(headless, Session::new())
    }

    /// The app where `session` left off, on a service just started.
    fn restore(headless: &'a Headless, session: Session) -> Scene<'a> {
        let scene = Scene::restore_held(headless, session);
        scene.service.gate().open();
        scene.idle();
        scene
    }

    /// The app where `session` left off, with each call to the service held at its gate from the start.
    fn restore_held(headless: &'a Headless, session: Session) -> Scene<'a> {
        let service = Service::new(showcase::SAMPLE);
        service.gate().hold();
        let pool = Pool::new(4);
        let background = pool.background();
        let app = App::restore(service.clone(), background.clone(), session);
        headless.on(app.library().window()).resize(LIBRARY.0, LIBRARY.1);
        Scene {
            headless,
            service,
            background,
            app,
            _pool: pool,
        }
    }

    fn library(&self) -> Input<'_> {
        self.headless.on(self.app.library().window())
    }

    /// Clicks the library's control labelled `label`.
    fn click(&self, label: &str) {
        element(self.app.library(), label).click();
    }

    /// Opens a window on `id`, and waits for the document.
    fn open(&self, id: DocId) -> EditorWindow {
        let window = self.open_held(id);
        idle(&self.background);
        window
    }

    /// Opens a window on `id` while calls may wait at the gate, so it doesn't wait for them.
    fn open_held(&self, id: DocId) -> EditorWindow {
        let window = self.app.open(id);
        self.headless.on(window.window()).resize(EDITOR.0, EDITOR.1);
        window
    }

    /// The window that opened on `id`, from a click in the library, sized as a test reads it.
    fn opened(&self, id: DocId) -> EditorWindow {
        let window = self.app.windows(id).pop().expect("a window opened on the document");
        self.headless.on(window.window()).resize(EDITOR.0, EDITOR.1);
        window
    }

    /// Types `text` at the end of what the editor's box labelled `label` holds.
    fn type_at_end(&self, editor: &EditorWindow, label: &str, text: &str) {
        element(editor, label).click();
        let input = self.headless.on(editor.window());
        input.key(Key::End);
        input.type_text(text);
    }

    fn idle(&self) {
        idle(&self.background);
    }
}

impl Drop for Scene<'_> {
    fn drop(&mut self) {
        self.service.gate().open();
        self.app.close();
        idle(&self.background);
    }
}

fn titles(library: &LibraryWindow) -> Vec<String> {
    library.get_rows().iter().map(|row| row.title.to_string()).collect()
}

/// Scenario 1.
fn search_drops_an_older_answer_and_pages_move(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    assert_eq!(titles(library), ["Alpha notes", "Beta plan", "Gamma list"]);
    assert_eq!(library.get_status(), "Page 1 of 3");
    scene.click("Next page");
    scene.idle();
    assert_eq!(titles(library), ["Delta notes", "Epsilon", "Zeta notes"]);
    scene.click("Previous page");
    scene.idle();

    let gate = scene.service.gate();
    gate.hold();
    element(library, "Search").type_text("n");
    gate.wait_for("search n");
    scene.library().type_text("o");
    gate.wait_for("search no");
    assert_eq!(library.get_status(), "Searching");
    gate.release("search no");
    until("the answer to the newer search", || titles(library) == ["Alpha notes", "Delta notes", "Zeta notes"]);
    gate.release("search n");
    scene.idle();
    assert_eq!(titles(library), ["Alpha notes", "Delta notes", "Zeta notes"], "the older answer came last, and was dropped");
    assert_eq!(library.get_status(), "Page 1 of 1");

    gate.open();
    scene.click("Refresh");
    scene.idle();
    assert_eq!(titles(library), ["Alpha notes", "Delta notes", "Zeta notes"]);
}

/// Scenario 2.
fn the_selection_follows_its_document(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene.click("Gamma list");
    let selected = || library.get_rows().iter().position(|row| row.selected).map(|at| library.get_rows().row_data(at).unwrap().id);
    assert_eq!(selected(), Some(3));

    scene.service.delete(DocId(1)).unwrap();
    scene.click("Refresh");
    scene.idle();
    assert_eq!(titles(library), ["Beta plan", "Gamma list", "Delta notes"]);
    assert_eq!(library.get_rows().iter().position(|row| row.selected), Some(1), "the selection followed its document");
    assert_eq!(selected(), Some(3));
}

/// Scenario 3.
fn a_title_too_long_is_refused_in_the_box(headless: &Headless) {
    let scene = Scene::start(headless);
    scene.click("Open Alpha notes");
    scene.idle();
    let editor = scene.opened(DocId(1));
    assert_eq!(editor.get_title_shown(), "Alpha notes");

    scene.type_at_end(&editor, "Title", &"x".repeat(30));
    let kept = format!("Alpha notes{}", "x".repeat(29));
    assert_eq!(editor.get_title_shown(), kept, "the edit past the limit was refused, and the box shows the title kept");

    let input = headless.on(editor.window());
    input.control_key("a");
    input.key(Key::Backspace);
    let doc = scene.app.document(DocId(1)).unwrap();
    assert_eq!(doc.try_read(|state| state.problem()), Some(Some("A title is needed.")));
}

/// Scenarios 8 and 9.
fn settings_and_edits_reach_every_window(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene.click("Page size");
    scene.library().key(Key::UpArrow);
    scene.idle();
    assert_eq!(library.get_rows().row_count(), 4, "the library lists one more to a page");
    scene.click("Count words");

    let first = scene.open(DocId(3));
    assert!(first.get_word_count_shown(), "a window opened later starts on the current settings");
    assert_eq!(first.get_words(), 3);

    let second = scene.open(DocId(3));
    scene.type_at_end(&first, "Body", " four");
    assert_eq!(second.get_body_shown(), "one two three four", "the second window sees the first window's edits");
    assert_eq!(second.get_words(), 4);

    scene.click("Count words");
    assert!(!first.get_word_count_shown() && !second.get_word_count_shown(), "a change reaches the windows already open");

    // One of two windows simply goes. The document stays with the other.
    headless.on(second.window()).close();
    assert!(!second.window().is_visible());
    assert!(first.window().is_visible());
}

/// Scenarios 4 and 10.
fn closing_asks_first_and_reopening_shows_the_current_state(headless: &Headless) {
    let scene = Scene::start(headless);
    let gate = scene.service.gate();
    let editor = scene.open(DocId(1));
    let input = headless.on(editor.window());
    scene.type_at_end(&editor, "Body", " again");

    input.close();
    assert_eq!(editor.get_asking(), "close", "unsaved edits: it asks first");
    element(&editor, "Cancel").click();
    assert!(editor.window().is_visible() && editor.get_asking().is_empty());
    assert!(find(&editor, "Cancel").is_none(), "the question has gone");

    // A save under way when the window closes.
    gate.hold();
    element(&editor, "Save").click();
    gate.wait_for("save 1");
    input.close();
    assert!(!editor.window().is_visible(), "nothing is unsaved, so it closes at once");

    let reopened = scene.open_held(DocId(1));
    assert_eq!(reopened.get_body_shown(), "the first draft again", "the reopened window shows the current state");
    assert_eq!(reopened.get_status().trim(), "Saving");
    gate.release("save 1");
    // The library searches again once the save is in.
    gate.open();
    scene.idle();
    assert_eq!(
        scene.service.get(DocId(1)).unwrap().body,
        "the first draft again",
        "the save finished after its window closed"
    );
    assert_eq!(reopened.get_toasts_shown(), 1);
    assert_eq!(reopened.get_toast_showing(), "Saved");
    assert!(find(&reopened, "Saved").is_some(), "the toast shows");

    headless.on(reopened.window()).close();
    let again = scene.open(DocId(1));
    settle();
    assert_eq!(again.get_toasts_shown(), 0, "the old toast doesn't show again");
    assert_eq!(again.get_status().trim(), "Revision 2");

    // Asked first, saving closes once the save is in.
    gate.open();
    scene.type_at_end(&again, "Body", "!");
    headless.on(again.window()).close();
    element(&again, "Save and close").click();
    scene.idle();
    assert!(!again.window().is_visible());
    assert_eq!(scene.service.get(DocId(1)).unwrap().body, "the first draft again!");
}

/// Scenario 5.
fn an_export_reports_progress_and_stops(headless: &Headless) {
    let scene = Scene::start(headless);
    let gate = scene.service.gate();
    let editor = scene.open(DocId(2));

    gate.hold();
    element(&editor, "Export").click();
    gate.wait_for("export 2 1");
    assert!(editor.get_exporting());
    gate.release("export 2 1");
    until("the first step", || editor.get_export_done() == 1);
    assert!(find(&editor, "Exporting 1 of 4").is_some(), "the progress shows");
    gate.wait_for("export 2 2");
    element(&editor, "Stop").click();
    assert!(!editor.get_exporting());
    gate.release("export 2 2");
    scene.idle();
    assert!(!editor.get_exporting(), "a stopped export sends nothing");
    assert_eq!(editor.get_toasts_shown(), 0);

    gate.open();
    element(&editor, "Export").click();
    scene.idle();
    assert_eq!((editor.get_exporting(), editor.get_toast_showing().as_str()), (false, "Exported"));
}

/// Scenario 6.
fn a_delete_asks_first(headless: &Headless) {
    let scene = Scene::start(headless);
    let editor = scene.open(DocId(1));
    element(&editor, "Delete").click();
    assert_eq!(editor.get_asking(), "delete");
    assert!(find(&editor, "Discard").is_none(), "a delete is confirmed or called off");
    element(&editor, "Cancel").click();
    assert!(scene.service.get(DocId(1)).is_some());

    element(&editor, "Delete").click();
    element(&editor, "Yes, delete").click();
    scene.idle();
    assert!(scene.service.get(DocId(1)).is_none());
    assert!(!editor.window().is_visible());
    assert_eq!(
        titles(scene.app.library()),
        ["Beta plan", "Gamma list", "Delta notes"],
        "the library heard and searched again"
    );
}

/// Scenario 7.
fn a_history_shows_and_back_returns(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene.service.save(DocId(2), "Beta plan, revised", "two steps").unwrap();
    scene.click("History of Beta plan");
    scene.idle();
    assert!(library.get_history_shown());
    let revisions: Vec<String> = library.get_revisions().iter().map(|revision| revision.title.to_string()).collect();
    assert_eq!(revisions, ["Beta plan, revised", "Beta plan"]);
    scene.click("Back");
    assert!(!library.get_history_shown());

    // Back before the history comes: the page is gone, and so is its answer.
    let gate = scene.service.gate();
    gate.hold();
    scene.click("History of Gamma list");
    gate.wait_for("history 3");
    assert_eq!(library.get_history_title(), "Loading the history of document 3");
    scene.click("Back");
    gate.release("history 3");
    scene.idle();
    assert!(!library.get_history_shown());
    gate.open();
    scene.click("History of Alpha notes");
    scene.idle();
    assert_eq!(library.get_history_title(), "History of document 1", "a page opened later shows only its own");
}

/// Scenario 13.
fn closing_the_library_waits_for_every_document(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    let editor = scene.open(DocId(1));
    scene.type_at_end(&editor, "Body", " again");

    // Unsaved edits: the document asks first, and staying keeps the library.
    scene.library().close();
    assert_eq!(editor.get_asking(), "close");
    element(&editor, "Cancel").click();
    assert!(library.window().is_visible() && editor.window().is_visible());
    assert_eq!(library.get_status(), "Page 1 of 3", "the close was called off");

    // A save under way: the editor closes at once, and the library waits for the save.
    let gate = scene.service.gate();
    gate.hold();
    element(&editor, "Save").click();
    gate.wait_for("save 1");
    scene.library().close();
    assert!(!editor.window().is_visible(), "nothing is unsaved, so the editor closes");
    assert!(library.window().is_visible(), "the library waits for the save");
    assert_eq!(library.get_status(), "Closing once every document has");
    gate.release("save 1");
    gate.open();
    scene.idle();
    assert!(!library.window().is_visible(), "and closes once it is in");
    assert_eq!(scene.service.get(DocId(1)).unwrap().body, "the first draft again");
    assert!(scene.app.document(DocId(1)).is_none(), "the library let the document go");
    drop(scene);

    // With no document open, it closes at once, from inside its own close.
    let scene = Scene::start(headless);
    scene.library().close();
    assert!(!scene.app.library().window().is_visible());
}

/// Scenario 11.
fn a_change_made_elsewhere_reloads(headless: &Headless) {
    let scene = Scene::start(headless);
    let editor = scene.open(DocId(3));
    scene.service.change_outside(DocId(3), "changed elsewhere");
    until("the reload", || editor.get_body_shown() == "changed elsewhere");
}

/// Scenarios 12 and 14.
fn failures_show_and_the_app_goes_on(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene._pool.refuse(true);
    scene.click("Refresh");
    settle();
    assert_eq!(library.get_status(), "The search failed: the pool refused background work.");
    scene._pool.refuse(false);
    scene.click("Refresh");
    scene.idle();
    assert_eq!(library.get_status(), "Page 1 of 3");

    let editor = scene.open(DocId(2));
    scene.service.panic_next_export();
    element(&editor, "Export").click();
    scene.idle();
    assert_eq!(editor.get_status().trim(), "The export failed: background work panicked.");
    // Scenario 14: the trail names the steps that led to the failure, and what the editor showed after.
    let trail = scene.app.trail().lines();
    let last = ["document 2 Export", "document 2 Exporting", "document 2: The export failed: background work panicked."];
    assert!(trail.ends_with(&last.map(String::from)), "{trail:#?}");

    scene.service.fail_next_save("the disk is full");
    scene.type_at_end(&editor, "Body", "!");
    element(&editor, "Save").click();
    scene.idle();
    assert_eq!(editor.get_status().trim(), "the disk is full");
    element(&editor, "Save").click();
    scene.idle();
    assert_eq!(editor.get_status().trim(), "Revision 2", "the store went on");
}

/// Scenario 15.
fn the_next_start_is_where_the_last_left_off(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    element(library, "Search").type_text("a");
    scene.idle();
    scene.click("Next page");
    scene.idle();
    assert_eq!(titles(library), ["Delta notes", "Zeta notes", "Eta"]);
    scene.click("Count words");
    scene.click("Delta notes");
    scene.click("Open Zeta notes");
    scene.click("History of Eta");
    scene.idle();
    assert_eq!(library.get_history_title(), "History of document 7");
    // The history page and the document each keep something of their own: the revision picked, and an export under way.
    scene.click("Revision 1: Eta");
    assert!(library.get_revisions().row_data(0).unwrap().picked);
    let editor = scene.opened(DocId(6));
    let gate = scene.service.gate();
    gate.hold();
    element(&editor, "Export").click();
    gate.wait_for("export 6 1");
    gate.release("export 6 1");
    until("the first step", || editor.get_export_done() == 1);
    gate.wait_for("export 6 2");
    scene.library().close();
    scene.idle();
    assert!(!library.window().is_visible(), "nothing was unsaved, so the app closed");
    let session = scene.app.session();
    let keys: Vec<&str> = session.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["settings", "library", "document 6", "history 7"], "each child saves under a key of its own");
    drop(scene);

    let scene = Scene::restore_held(headless, session);
    let gate = scene.service.gate();
    gate.wait_for("export 6 2");
    assert!(!gate.waiting().contains(&"export 6 1".to_owned()), "the export goes on from the step it had done");
    gate.open();
    scene.idle();
    let library = scene.app.library();
    assert_eq!(library.get_query_shown(), "a", "the search box shows the search again");
    assert_eq!(library.get_status(), "Page 2 of 2");
    assert_eq!(titles(library), ["Delta notes", "Zeta notes", "Eta"]);
    assert!(library.get_rows().row_data(0).unwrap().selected, "the document picked is picked again");
    assert!(library.get_word_count(), "and the settings are as they were");
    assert!(library.get_history_shown());
    assert_eq!(library.get_history_title(), "History of document 7", "the history page is open again, on top");
    assert!(library.get_revisions().row_data(0).unwrap().picked, "with the revision it had picked");
    let editors = scene.app.windows(DocId(6));
    assert_eq!(editors.len(), 1, "the document open has a window again");
    assert!(editors[0].get_word_count_shown());
}

/// Scenario 16.
fn edits_save_by_themselves(headless: &Headless) {
    let scene = Scene::start(headless);
    let gate = scene.service.gate();
    scene.click("Save by itself");
    assert!(scene.app.library().get_autosave());
    let editor = scene.open(DocId(2));
    let typing = headless.on(editor.window());
    let body = || scene.service.get(DocId(2)).unwrap().body;
    scene.type_at_end(&editor, "Body", "x");
    scene.idle();
    assert_eq!(editor.get_status().as_str(), "Edited", "it waits for the edits to stop");
    advance(AUTOSAVE_AFTER);
    scene.idle();
    assert_eq!((body().as_str(), editor.get_status().as_str()), ("two stepsx", "Revision 2"));

    // An edit during the write waits for it, and goes next.
    gate.hold();
    typing.type_text("y");
    advance(AUTOSAVE_AFTER);
    gate.wait_for("save 2");
    typing.type_text("z");
    advance(AUTOSAVE_AFTER);
    assert_eq!(gate.waiting(), ["save 2"], "one write at a time");
    gate.release("save 2");
    until("the first write", || body() == "two stepsxy");
    gate.wait_for("save 2");
    gate.open();
    scene.idle();
    assert_eq!((body().as_str(), editor.get_status().as_str()), ("two stepsxyz", "Revision 4"));

    // A write that fails tries again, a while later.
    scene.service.fail_next_save("The disk is full.");
    typing.type_text("!");
    advance(AUTOSAVE_AFTER);
    scene.idle();
    assert_eq!(editor.get_status().as_str(), "The disk is full.");
    advance(RETRY_AFTER);
    scene.idle();
    assert_eq!((body().as_str(), editor.get_status().as_str()), ("two stepsxyz!", "Revision 5"));

    // A close with a wait still under way writes what waited first.
    typing.type_text("?");
    typing.close();
    scene.idle();
    assert!(!editor.window().is_visible(), "it didn't ask, and closed once the write was in");
    assert_eq!(body(), "two stepsxyz!?");
}

/// A scenario's name, and the scenario.
type Scenario = (&'static str, fn(&Headless));

fn main() {
    let headless = Headless::install();
    headless.separate_windows();
    let scenarios: [Scenario; 13] = [
        ("search drops an older answer, and pages move", search_drops_an_older_answer_and_pages_move),
        ("the selection follows its document", the_selection_follows_its_document),
        ("a title too long is refused in the box", a_title_too_long_is_refused_in_the_box),
        ("settings and edits reach every window", settings_and_edits_reach_every_window),
        (
            "closing asks first, and reopening shows the current state",
            closing_asks_first_and_reopening_shows_the_current_state,
        ),
        ("an export reports progress and stops", an_export_reports_progress_and_stops),
        ("a delete asks first", a_delete_asks_first),
        ("a history shows, and back returns", a_history_shows_and_back_returns),
        ("a change made elsewhere reloads", a_change_made_elsewhere_reloads),
        ("closing the library waits for every document", closing_the_library_waits_for_every_document),
        ("the next start is where the last left off", the_next_start_is_where_the_last_left_off),
        ("edits save by themselves", edits_save_by_themselves),
        // It panics in an export on purpose, and the pool prints that panic.
        ("failures show, and the app goes on", failures_show_and_the_app_goes_on),
    ];
    for (name, scenario) in scenarios {
        scenario(&headless);
        println!("ok: {name}");
    }
}
