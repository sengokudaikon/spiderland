//! The showcase's scenarios over real windows with no display: the whole app, on its own pool, driven by real input. Slint's
//! platform is set once per process, so the scenarios run in turn on the main thread, each on an app of its own.

#![allow(missing_docs, reason = "the windows' types come from `slint!`, with no docs")]

use showcase::app::{App, Session};
use showcase::pool::Pool;
use showcase::service::{DocId, Service};
use showcase::view::{EditorWindow, LibraryWindow};
use slint::platform::Key;
use slint::{ComponentHandle, Model};
use spiderland_slint::Background;
use spiderland_slint::testing::{Headless, Input, idle, settle, until};

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
        headless.on(app.library().window()).resize(400, 300);
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

    /// Opens a window on `id`, and waits for the document.
    fn open(&self, id: DocId) -> EditorWindow {
        let window = self.app.open(id);
        self.headless.on(window.window()).resize(400, 300);
        idle(&self.background);
        window
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

/// The middle of the row at `index`, and of the part from `x`.
fn row(index: usize) -> f32 {
    40.0 + index as f32 * 20.0
}

const PICK: f32 = 100.0;
const HISTORY: f32 = 280.0;
const OPEN: f32 = 360.0;
const BUTTONS: f32 = 250.0;
const PREVIOUS: f32 = 40.0;
const NEXT: f32 = 120.0;
const REFRESH: f32 = 200.0;
const LARGER: f32 = 300.0;
const WORDS: f32 = 360.0;
const EDITOR_BUTTONS: f32 = 180.0;
const SAVE: f32 = 40.0;
const EXPORT: f32 = 120.0;
const STOP: f32 = 200.0;
const DELETE: f32 = 280.0;
const ANSWERS: f32 = 260.0;
const CONFIRM: f32 = 65.0;
const CANCEL: f32 = 330.0;

/// Scenario 1.
fn search_drops_an_older_answer_and_pages_move(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    assert_eq!(titles(library), ["Alpha notes", "Beta plan", "Gamma list"]);
    assert_eq!(library.get_status(), "Page 1 of 3");
    scene.library().click(NEXT, BUTTONS);
    scene.idle();
    assert_eq!(titles(library), ["Delta notes", "Epsilon", "Zeta notes"]);
    scene.library().click(PREVIOUS, BUTTONS);
    scene.idle();

    let gate = scene.service.gate();
    gate.hold();
    scene.library().click(390.0, 10.0);
    scene.library().type_text("n");
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
    scene.library().click(REFRESH, BUTTONS);
    scene.idle();
    assert_eq!(titles(library), ["Alpha notes", "Delta notes", "Zeta notes"]);
}

/// Scenario 2.
fn the_selection_follows_its_document(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene.library().click(PICK, row(2));
    let selected = || library.get_rows().iter().position(|row| row.selected).map(|at| library.get_rows().row_data(at).unwrap().id);
    assert_eq!(selected(), Some(3));

    scene.service.delete(DocId(1)).unwrap();
    scene.library().click(REFRESH, BUTTONS);
    scene.idle();
    assert_eq!(titles(library), ["Beta plan", "Gamma list", "Delta notes"]);
    assert_eq!(library.get_rows().iter().position(|row| row.selected), Some(1), "the selection followed its document");
    assert_eq!(selected(), Some(3));
}

/// Scenario 3.
fn a_title_too_long_is_refused_in_the_box(headless: &Headless) {
    let scene = Scene::start(headless);
    scene.library().click(OPEN, row(0));
    scene.idle();
    let editor = scene.app.windows(DocId(1)).pop().expect("the editor opened");
    let input = headless.on(editor.window());
    input.resize(400, 300);
    assert_eq!(editor.get_title_shown(), "Alpha notes");

    input.click(390.0, 10.0);
    input.type_text(&"x".repeat(30));
    let kept = format!("Alpha notes{}", "x".repeat(29));
    assert_eq!(editor.get_title_shown(), kept, "the edit past the limit was refused, and the box shows the title kept");

    input.control_key("a");
    input.key(Key::Backspace);
    let doc = scene.app.document(DocId(1)).unwrap();
    assert_eq!(doc.try_read(|state| state.problem()), Some(Some("A title is needed.")));
}

/// Scenarios 8 and 9.
fn settings_and_edits_reach_every_window(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene.library().click(LARGER, BUTTONS);
    scene.idle();
    assert_eq!(library.get_rows().row_count(), 4, "the library lists one more to a page");
    scene.library().click(WORDS, BUTTONS);

    let first = scene.open(DocId(3));
    assert!(first.get_word_count_shown(), "a window opened later starts on the current settings");
    assert_eq!(first.get_words(), 3);

    let second = scene.open(DocId(3));
    let typing = headless.on(first.window());
    typing.click(390.0, 40.0);
    typing.type_text(" four");
    assert_eq!(second.get_body_shown(), "one two three four", "the second window sees the first window's edits");
    assert_eq!(second.get_words(), 4);

    scene.library().click(WORDS, BUTTONS);
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
    input.click(390.0, 40.0);
    input.type_text(" again");

    input.close();
    assert_eq!(editor.get_asking(), "close", "unsaved edits: it asks first");
    input.click(CANCEL, ANSWERS);
    assert!(editor.window().is_visible() && editor.get_asking().is_empty());

    // A save under way when the window closes.
    gate.hold();
    input.click(SAVE, EDITOR_BUTTONS);
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

    headless.on(reopened.window()).close();
    let again = scene.open(DocId(1));
    settle();
    assert_eq!(again.get_toasts_shown(), 0, "the old toast doesn't show again");
    assert_eq!(again.get_status().trim(), "Revision 2");

    // Asked first, saving closes once the save is in.
    gate.open();
    let input = headless.on(again.window());
    input.click(390.0, 40.0);
    input.type_text("!");
    input.close();
    input.click(CONFIRM, ANSWERS);
    scene.idle();
    assert!(!again.window().is_visible());
    assert_eq!(scene.service.get(DocId(1)).unwrap().body, "the first draft again!");
}

impl Scene<'_> {
    /// Opens a window on `id` while calls may wait at the gate, so it doesn't wait for them.
    fn open_held(&self, id: DocId) -> EditorWindow {
        let window = self.app.open(id);
        self.headless.on(window.window()).resize(400, 300);
        window
    }
}

/// Scenario 5.
fn an_export_reports_progress_and_stops(headless: &Headless) {
    let scene = Scene::start(headless);
    let gate = scene.service.gate();
    let editor = scene.open(DocId(2));
    let input = headless.on(editor.window());

    gate.hold();
    input.click(EXPORT, EDITOR_BUTTONS);
    gate.wait_for("export 2 1");
    assert!(editor.get_exporting());
    gate.release("export 2 1");
    until("the first step", || editor.get_export_done() == 1);
    gate.wait_for("export 2 2");
    input.click(STOP, EDITOR_BUTTONS);
    assert!(!editor.get_exporting());
    gate.release("export 2 2");
    scene.idle();
    assert!(!editor.get_exporting(), "a stopped export sends nothing");
    assert_eq!(editor.get_toasts_shown(), 0);

    gate.open();
    input.click(EXPORT, EDITOR_BUTTONS);
    scene.idle();
    assert_eq!((editor.get_exporting(), editor.get_toast_showing().as_str()), (false, "Exported"));
}

/// Scenario 6.
fn a_delete_asks_first(headless: &Headless) {
    let scene = Scene::start(headless);
    let editor = scene.open(DocId(1));
    let input = headless.on(editor.window());
    input.click(DELETE, EDITOR_BUTTONS);
    assert_eq!(editor.get_asking(), "delete");
    input.click(CANCEL, ANSWERS);
    assert!(scene.service.get(DocId(1)).is_some());

    input.click(DELETE, EDITOR_BUTTONS);
    input.click(CONFIRM, ANSWERS);
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
    scene.library().click(HISTORY, row(1));
    scene.idle();
    assert!(library.get_history_shown());
    let revisions: Vec<String> = library.get_revisions().iter().map(|revision| revision.title.to_string()).collect();
    assert_eq!(revisions, ["Beta plan, revised", "Beta plan"]);
    scene.library().click(50.0, 10.0);
    assert!(!library.get_history_shown());

    // Back before the history comes: the page is gone, and so is its answer.
    let gate = scene.service.gate();
    gate.hold();
    scene.library().click(HISTORY, row(2));
    gate.wait_for("history 3");
    assert_eq!(library.get_history_title(), "Loading the history of document 3");
    scene.library().click(50.0, 10.0);
    gate.release("history 3");
    scene.idle();
    assert!(!library.get_history_shown());
    gate.open();
    scene.library().click(HISTORY, row(0));
    scene.idle();
    assert_eq!(library.get_history_title(), "History of document 1", "a page opened later shows only its own");
}

/// Scenario 13.
fn closing_the_library_waits_for_every_document(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    let editor = scene.open(DocId(1));
    let input = headless.on(editor.window());
    input.click(390.0, 40.0);
    input.type_text(" again");

    // Unsaved edits: the document asks first, and staying keeps the library.
    scene.library().close();
    assert_eq!(editor.get_asking(), "close");
    input.click(CANCEL, ANSWERS);
    assert!(library.window().is_visible() && editor.window().is_visible());
    assert_eq!(library.get_status(), "Page 1 of 3", "the close was called off");

    // A save under way: the editor closes at once, and the library waits for the save.
    let gate = scene.service.gate();
    gate.hold();
    input.click(SAVE, EDITOR_BUTTONS);
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
    scene.library().click(REFRESH, BUTTONS);
    settle();
    assert_eq!(library.get_status(), "The search failed: the pool refused background work.");
    scene._pool.refuse(false);
    scene.library().click(REFRESH, BUTTONS);
    scene.idle();
    assert_eq!(library.get_status(), "Page 1 of 3");

    let editor = scene.open(DocId(2));
    let input = headless.on(editor.window());
    scene.service.panic_next_export();
    input.click(EXPORT, EDITOR_BUTTONS);
    scene.idle();
    assert_eq!(editor.get_status().trim(), "The export failed: background work panicked.");
    // Scenario 14: the trail names the steps that led to the failure, and what the editor showed after.
    let trail = scene.app.trail().lines();
    let last = ["document 2 Export", "document 2 Exporting", "document 2: The export failed: background work panicked."];
    assert!(trail.ends_with(&last.map(String::from)), "{trail:#?}");

    scene.service.fail_next_save("the disk is full");
    input.click(390.0, 40.0);
    input.type_text("!");
    input.click(SAVE, EDITOR_BUTTONS);
    scene.idle();
    assert_eq!(editor.get_status().trim(), "the disk is full");
    input.click(SAVE, EDITOR_BUTTONS);
    scene.idle();
    assert_eq!(editor.get_status().trim(), "Revision 2", "the store went on");
}

/// Scenario 15.
fn the_next_start_is_where_the_last_left_off(headless: &Headless) {
    let scene = Scene::start(headless);
    let library = scene.app.library();
    scene.library().click(390.0, 10.0);
    scene.library().type_text("a");
    scene.idle();
    scene.library().click(NEXT, BUTTONS);
    scene.idle();
    assert_eq!(titles(library), ["Delta notes", "Zeta notes", "Eta"]);
    scene.library().click(WORDS, BUTTONS);
    scene.library().click(PICK, row(0));
    scene.library().click(OPEN, row(1));
    scene.library().click(HISTORY, row(2));
    scene.idle();
    assert_eq!(library.get_history_title(), "History of document 7");
    // The history page and the document each keep something of their own: the revision picked, and an export under way.
    scene.library().click(200.0, row(0));
    assert!(library.get_revisions().row_data(0).unwrap().picked);
    let editor = scene.app.windows(DocId(6)).remove(0);
    headless.on(editor.window()).resize(400, 300);
    let gate = scene.service.gate();
    gate.hold();
    headless.on(editor.window()).click(EXPORT, EDITOR_BUTTONS);
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

/// A scenario's name, and the scenario.
type Scenario = (&'static str, fn(&Headless));

fn main() {
    let headless = Headless::install();
    headless.separate_windows();
    let scenarios: [Scenario; 12] = [
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
        // It panics in an export on purpose, and the pool prints that panic.
        ("failures show, and the app goes on", failures_show_and_the_app_goes_on),
    ];
    for (name, scenario) in scenarios {
        scenario(&headless);
        println!("ok: {name}");
    }
}
