//! The showcase's rules, each run through a `TestStore` with no window. The scenario each test covers is in its name's comment.

use showcase::document::{self, Ask, Choice, Content, Document, Running, TITLE_LIMIT};
use showcase::export::{self, Steps};
use showcase::history::{self, History};
use showcase::library::{self, Library, Page};
use showcase::service::{Doc, DocId, Found, Revision, Summary};
use showcase::settings::{self, Preferences, Settings};
use showcase::trail::Trail;
use spiderland::{Asked, CloseAnswer, Error, Lifecycle, Standing, TestStore};

fn summaries(docs: &[(u32, &str)]) -> Vec<Summary> {
    docs.iter()
        .map(|&(id, title)| Summary {
            id: DocId(id),
            title: title.to_owned(),
        })
        .collect()
}

fn found(docs: &[(u32, &str)], total: usize) -> Result<Found, Error> {
    Ok(Found { docs: summaries(docs), total })
}

/// Takes the one search that waits, and returns what it asked for.
fn search(library: &TestStore<Library>) -> (Asked, String, usize) {
    let effects = library.take_effects();
    match &effects[..] {
        [library::Effect::Search { asked, query, page, .. }] => (*asked, query.clone(), *page),
        other => panic!("one search should wait: {other:?}"),
    }
}

fn started_library() -> TestStore<Library> {
    let library = TestStore::<Library>::new(library::State::new(Preferences::default()));
    let (asked, ..) = search(&library);
    library.send(library::Intent::Found {
        asked,
        result: found(&[(1, "Alpha"), (2, "Beta"), (3, "Gamma")], 7),
    });
    library
}

fn titles(library: &TestStore<Library>) -> Vec<String> {
    library.read(|state| state.found.iter().map(|doc| doc.title.clone()).collect())
}

/// Scenario 1.
#[test]
fn an_older_answer_that_comes_last_is_dropped() {
    let library = started_library();
    library.send(library::Intent::Typed("n".into()));
    let (older, ..) = search(&library);
    library.send(library::Intent::Typed("no".into()));
    let (newer, query, page) = search(&library);
    assert_eq!((query.as_str(), page), ("no", 0));
    assert_eq!(library.read(library::status), "Searching");

    library.send(library::Intent::Found {
        asked: newer,
        result: found(&[(1, "Alpha notes")], 1),
    });
    library.send(library::Intent::Found {
        asked: older,
        result: found(&[(2, "Beta plan")], 1),
    });
    assert_eq!(titles(&library), ["Alpha notes"], "the answer to what was typed first came last, and was dropped");
    assert_eq!(library.read(library::status), "Page 1 of 1");
}

/// Scenario 1.
#[test]
fn pages_move_on_and_back_and_a_refresh_asks_again() {
    let library = started_library();
    assert_eq!(library.read(library::status), "Page 1 of 3");
    library.send(library::Intent::PreviousPage);
    assert!(library.take_effects().is_empty(), "there is no page before the first");

    library.send(library::Intent::NextPage);
    let (asked, _, page) = search(&library);
    assert_eq!(page, 1);
    library.send(library::Intent::Found {
        asked,
        result: found(&[(4, "Delta")], 7),
    });
    assert_eq!(library.read(library::status), "Page 2 of 3");

    library.send(library::Intent::Refresh);
    let (_, _, page) = search(&library);
    assert_eq!(page, 1, "a refresh asks for the same page again");
}

/// Scenario 2.
#[test]
fn the_selection_stays_on_its_document_as_rows_before_it_go() {
    let library = started_library();
    library.send(library::Intent::Select(DocId(3)));
    let selected_at = |library: &TestStore<Library>| {
        library.read(|state| {
            library::rows(&state.found, state.selected)
                .iter()
                .position(|row| row.selected)
                .map(|at| (at, state.found[at].id))
        })
    };
    assert_eq!(selected_at(&library), Some((2, DocId(3))));

    // A document before it was deleted.
    library.send(library::Intent::Document(DocId(1), document::Output::Changed));
    let (asked, ..) = search(&library);
    library.send(library::Intent::Found {
        asked,
        result: found(&[(2, "Beta"), (3, "Gamma"), (4, "Delta")], 6),
    });
    assert_eq!(selected_at(&library), Some((1, DocId(3))), "the selection followed its document");
}

/// Scenario 7: the library keeps a history page for as long as it is open.
#[test]
fn a_history_shows_over_the_list_and_back_goes_to_it() {
    let library = started_library();
    library.send(library::Intent::ShowHistory(DocId(2)));
    assert_eq!(library.read(library::State::shown), Page::History(DocId(2)));
    assert_eq!(library.read(library::pages), [(DocId(2), Standing::Shown)], "the page is a child, on top");
    assert!(library.take_effects().is_empty(), "the page loads itself");

    library.send(library::Intent::Back);
    assert_eq!(library.read(library::State::shown), Page::List);
    assert!(library.read(library::pages).is_empty(), "going back lets the page go, and what it was loading");
    library.send(library::Intent::Back);
    assert_eq!(library.read(|state| state.pages.len()), 1, "the list stays");
}

/// Scenario 7: the page's own flow.
#[test]
fn a_history_page_loads_its_revisions_and_shows_while_on_top() {
    let page = TestStore::<History>::new(history::State::new(DocId(2)));
    assert_eq!(page.take_effects(), [history::Effect::Load(DocId(2))]);
    assert_eq!(page.read(history::title), "Loading the history of document 2");
    let revisions = vec![
        Revision {
            revision: 2,
            title: "Beta 2".into(),
        },
        Revision {
            revision: 1,
            title: "Beta".into(),
        },
    ];
    page.send(history::Intent::Loaded(Ok(revisions.clone())));
    assert_eq!(page.read(|state| state.revisions.clone()), Some(revisions));
    assert_eq!(page.read(history::title), "History of document 2");

    page.send(history::Intent::Life(Lifecycle::Shown));
    assert!(page.read(|state| state.shown));
    page.send(history::Intent::Life(Lifecycle::Hidden));
    assert!(!page.read(|state| state.shown), "a page under another doesn't show");

    let failing = TestStore::<History>::new(history::State::new(DocId(3)));
    failing.take_effects();
    failing.send(history::Intent::Loaded(Err(Error::Panicked)));
    assert_eq!(failing.read(history::title), "The history failed to load: background work panicked.");
}

/// Scenarios 4 and 13: the library keeps each document open as a child, until it says it has closed.
#[test]
fn the_library_keeps_documents_open_until_they_close() {
    let library = started_library();
    library.send(library::Intent::Open(DocId(2)));
    library.send(library::Intent::Open(DocId(2)));
    assert_eq!(library.take_effects(), [library::Effect::OpenWindow(DocId(2)), library::Effect::OpenWindow(DocId(2))]);
    assert_eq!(library.read(library::documents), [(DocId(2), Standing::Shown)], "a second window shares the document");

    library.send(library::Intent::Document(DocId(2), document::Output::Closed(CloseAnswer::StayingOpen)));
    assert_eq!(library.read(|state| state.open.clone()), [DocId(2)], "a document that stays is kept");
    library.send(library::Intent::Document(DocId(2), document::Output::Reopen));
    assert_eq!(library.take_effects(), [library::Effect::OpenWindow(DocId(2))], "and it gets a window when it asks");
    library.send(library::Intent::Document(DocId(2), document::Output::Closed(CloseAnswer::Closed)));
    assert!(library.read(|state| state.open.is_empty()), "a document that has closed is let go");
}

/// Scenario 13: closing the library asks every document, and closes once each has.
#[test]
fn closing_the_library_waits_for_every_document() {
    let library = started_library();
    library.send_all([library::Intent::Open(DocId(1)), library::Intent::Open(DocId(2))]);
    library.take_effects();

    library.send(library::Intent::Life(Lifecycle::CloseRequested));
    assert_eq!(library.read(library::documents), [(DocId(1), Standing::Closing), (DocId(2), Standing::Closing)]);
    assert_eq!(library.read(library::status), "Closing once every document has");
    library.send(library::Intent::Open(DocId(3)));
    assert!(library.take_effects().is_empty(), "nothing opens while it closes");

    library.send(library::Intent::Document(DocId(1), document::Output::Closed(CloseAnswer::Closed)));
    assert!(library.take_effects().is_empty(), "one document is still saving");
    library.send(library::Intent::Document(DocId(2), document::Output::Closed(CloseAnswer::Closed)));
    assert_eq!(library.take_effects(), [library::Effect::Close]);

    // One document staying keeps the library, and the others hear it was called off.
    let library = started_library();
    library.send_all([library::Intent::Open(DocId(1)), library::Intent::Open(DocId(2))]);
    library.take_effects();
    library.send(library::Intent::Life(Lifecycle::CloseRequested));
    library.send(library::Intent::Document(DocId(1), document::Output::Closed(CloseAnswer::StayingOpen)));
    library.send(library::Intent::Document(DocId(2), document::Output::Closed(CloseAnswer::Closed)));
    assert!(library.take_effects().is_empty(), "the library stays");
    assert_eq!(library.read(library::documents), [(DocId(1), Standing::Shown)]);

    // With nothing open, it closes at once.
    let library = started_library();
    library.send(library::Intent::Life(Lifecycle::CloseRequested));
    assert_eq!(library.take_effects(), [library::Effect::Close]);
}

/// Scenario 8, the rules: a change goes out once, and only when something changed.
#[test]
fn settings_tell_others_only_what_changed() {
    let settings = TestStore::<Settings>::new(Preferences::default());
    settings.send(settings::Intent::PageSize(4));
    settings.send(settings::Intent::PageSize(4));
    settings.send(settings::Intent::PageSize(99));
    assert_eq!(
        settings.take_outputs(),
        [Preferences { page_size: 4, word_count: false }, Preferences { page_size: 10, word_count: false }]
    );

    let library = started_library();
    library.send(library::Intent::Preferences(Preferences { page_size: 5, word_count: true }));
    let (.., page) = search(&library);
    assert_eq!(page, 0, "a new page size starts from the first page");
    library.send(library::Intent::Preferences(Preferences { page_size: 5, word_count: false }));
    assert!(library.take_effects().is_empty(), "only the page size asks for a search");
}

fn content(title: &str, body: &str) -> Content {
    Content {
        title: title.into(),
        body: body.into(),
    }
}

/// A document that has loaded with `title` and `body`, at revision 1.
fn loaded(title: &str, body: &str) -> TestStore<Document> {
    let doc = TestStore::<Document>::new(document::State::new(DocId(1), Preferences::default()));
    let (title, body) = (title.to_owned(), body.to_owned());
    doc.answer(move |effect| {
        assert_eq!(effect, document::Effect::Load(DocId(1)));
        [document::Intent::Loaded(Ok(Ok(Doc {
            id: DocId(1),
            title: title.clone(),
            body: body.clone(),
            revision: 1,
        })))]
    });
    doc
}

/// Scenario 3.
#[test]
fn a_title_too_long_is_refused_and_one_left_empty_cant_be_saved() {
    let doc = loaded("Alpha", "a");
    let refused = doc.read(|state| state.refused.serial());
    doc.send(document::Intent::EditTitle("x".repeat(TITLE_LIMIT + 1)));
    assert_eq!(doc.read(|state| (state.content.title.clone(), state.refused.serial())), ("Alpha".to_owned(), refused + 1));

    doc.send(document::Intent::EditTitle(" ".into()));
    assert_eq!(doc.read(document::State::problem), Some("A title is needed."));
    doc.send(document::Intent::Save);
    assert!(doc.take_effects().is_empty(), "nothing is saved while the title is missing");
}

/// Scenario 4.
#[test]
fn saves_go_one_at_a_time_with_the_latest_waiting() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::Save);
    assert!(doc.take_effects().is_empty(), "nothing to save");

    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Save);
    assert_eq!(doc.take_effects(), [document::Effect::Save(DocId(1), content("Alpha", "b"))]);
    for body in ["c", "d"] {
        doc.send(document::Intent::EditBody(body.into()));
        doc.send(document::Intent::Save);
    }
    assert!(doc.take_effects().is_empty(), "one save runs, so the next waits");
    assert_eq!(doc.read(document::status), "Saving");

    doc.send(document::Intent::Saved(Ok(Ok(2))));
    assert_eq!(doc.take_effects(), [document::Effect::Save(DocId(1), content("Alpha", "d"))], "only the latest waited");
    doc.send(document::Intent::Saved(Ok(Ok(3))));
    assert_eq!(
        doc.read(|state| (state.revision, state.dirty(), document::status(state))),
        (3, false, "Revision 3".to_owned())
    );
    assert_eq!(doc.take_outputs(), [document::Output::Changed, document::Output::Changed]);
}

/// Scenario 4: a save under way when the window closes is still heard.
#[test]
fn a_save_under_way_finishes_after_its_window_closes() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Save);
    assert_eq!(doc.take_effects().len(), 1);
    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    assert!(doc.take_effects().is_empty());
    assert_eq!(doc.read(|state| state.close.serial()), 1, "nothing is unsaved, so the window goes at once");
    assert!(doc.take_outputs().is_empty(), "the document stays until its save is in");

    doc.send(document::Intent::Saved(Ok(Ok(2))));
    assert_eq!(doc.read(|state| (state.revision, state.saved.clone())), (2, content("Alpha", "b")));
    assert_eq!(doc.take_outputs(), [document::Output::Changed, document::Output::Closed(CloseAnswer::Closed)]);
}

/// Scenario 10: a window opened while the save finishes keeps the document.
#[test]
fn a_window_opened_during_the_save_keeps_the_document() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Save);
    doc.take_effects();
    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    doc.send(document::Intent::WindowOpened);
    doc.send(document::Intent::Saved(Ok(Ok(2))));
    assert_eq!(doc.take_outputs(), [document::Output::Changed], "it was opened again, so it hasn't closed");
}

/// Scenario 10: a save that fails after the windows closed keeps the document, which asks for a window.
#[test]
fn a_save_that_fails_after_the_windows_closed_asks_for_a_window() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Save);
    doc.take_effects();
    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    doc.send(document::Intent::Saved(Ok(Err("the disk is full".into()))));
    assert_eq!(doc.take_outputs(), [document::Output::Closed(CloseAnswer::StayingOpen), document::Output::Reopen]);
    assert!(doc.read(|state| state.dirty()), "the edits are kept");
}

/// Scenario 5.
#[test]
fn an_export_reports_progress_and_stops_when_asked() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::Export);
    assert!(doc.is_running(&Running::Export(Steps)), "the export runs inside the document");
    doc.send(document::Intent::Exporting(export::Intent::Progress(2)));
    assert_eq!(doc.read(|state| state.export.map(|export| export.done)), Some(2));

    doc.send(document::Intent::StopExport);
    assert!(!doc.is_running(&Running::Export(Steps)), "dropping the export stops its work");
    doc.send(document::Intent::Exporting(export::Intent::Progress(3)));
    assert_eq!(doc.read(|state| state.export), None, "and what it sent after that is let go");

    doc.send(document::Intent::Export);
    doc.send(document::Intent::Exporting(export::Intent::Exported(Ok(true))));
    assert_eq!(doc.read(|state| (state.export, state.toast.value().clone())), (None, "Exported".to_owned()));
    assert!(!doc.is_running(&Running::Export(Steps)));
}

/// Scenario 6.
#[test]
fn a_delete_asks_first() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::Delete);
    assert_eq!(doc.read(|state| state.asking), Some(Ask::Delete));
    doc.send(document::Intent::Answer(Choice::Cancel));
    assert_eq!(doc.read(|state| state.asking), None);
    assert!(doc.take_effects().is_empty());

    doc.send(document::Intent::Delete);
    doc.send(document::Intent::Answer(Choice::Confirm));
    assert_eq!(doc.take_effects(), [document::Effect::Delete(DocId(1))]);
    doc.send(document::Intent::Deleted(Ok(Ok(()))));
    assert_eq!(doc.read(|state| state.close.serial()), 1, "its windows close");
    assert_eq!(doc.take_outputs(), [document::Output::Changed, document::Output::Closed(CloseAnswer::Closed)]);
    assert!(!doc.is_running(&Running::Watching), "a deleted document stops watching for changes");
}

/// Scenario 10.
#[test]
fn closing_with_edits_asks_and_goes_once_they_are_saved_or_dropped() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    assert_eq!(doc.read(|state| state.asking), Some(Ask::Close));
    assert!(doc.take_effects().is_empty());
    doc.send(document::Intent::Answer(Choice::Cancel));
    assert_eq!(doc.take_outputs(), [document::Output::Closed(CloseAnswer::StayingOpen)]);

    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    doc.send(document::Intent::Answer(Choice::Confirm));
    assert_eq!(doc.take_effects(), [document::Effect::Save(DocId(1), content("Alpha", "b"))]);
    doc.send(document::Intent::Saved(Ok(Ok(2))));
    assert_eq!(doc.read(|state| state.close.serial()), 1, "it closes once the save is in");
    assert_eq!(doc.take_outputs(), [document::Output::Changed, document::Output::Closed(CloseAnswer::Closed)]);

    doc.send(document::Intent::WindowOpened);
    doc.send(document::Intent::EditBody("c".into()));
    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    doc.send(document::Intent::Answer(Choice::Discard));
    assert_eq!(doc.read(|state| state.close.serial()), 2);
    assert_eq!(doc.read(|state| state.content.clone()), content("Alpha", "b"), "the edits were dropped");
    assert_eq!(doc.take_outputs(), [document::Output::Closed(CloseAnswer::Closed)]);
}

/// Scenario 10: a save that fails while closing keeps the window open.
#[test]
fn a_save_that_fails_while_closing_keeps_the_window() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Life(Lifecycle::CloseRequested));
    doc.send(document::Intent::Answer(Choice::Confirm));
    assert_eq!(doc.take_effects().len(), 1);
    doc.send(document::Intent::Saved(Ok(Err("the disk is full".into()))));
    assert!(doc.take_effects().is_empty(), "nothing hides");
    assert_eq!(doc.take_outputs(), [document::Output::Closed(CloseAnswer::StayingOpen)]);
    assert_eq!(doc.read(document::status), "the disk is full");
}

/// Scenario 11.
#[test]
fn a_change_made_elsewhere_reloads_unless_there_are_edits() {
    let doc = loaded("Alpha", "a");
    assert!(doc.is_running(&Running::Watching));
    doc.send(document::Intent::ChangedOutside(Some(DocId(2))));
    assert!(doc.take_effects().is_empty(), "another document changed");

    doc.send(document::Intent::ChangedOutside(Some(DocId(1))));
    doc.answer(|effect| {
        assert_eq!(effect, document::Effect::Load(DocId(1)));
        [document::Intent::Loaded(Ok(Ok(Doc {
            id: DocId(1),
            title: "Alpha".into(),
            body: "changed elsewhere".into(),
            revision: 2,
        })))]
    });
    assert_eq!(doc.read(|state| state.content.clone()), content("Alpha", "changed elsewhere"));

    doc.send(document::Intent::EditBody("mine".into()));
    doc.send(document::Intent::ChangedOutside(Some(DocId(1))));
    assert!(doc.take_effects().is_empty(), "edits aren't thrown away");
}

/// Scenario 12.
#[test]
fn failures_show_in_the_state_and_the_store_goes_on() {
    let doc = loaded("Alpha", "a");
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Save);
    assert_eq!(doc.take_effects().len(), 1);
    doc.send(document::Intent::Saved(Err(Error::Panicked)));
    assert_eq!(doc.read(document::status), "The save failed: background work panicked.");

    doc.send(document::Intent::Save);
    assert_eq!(doc.take_effects().len(), 1, "the store goes on");
    doc.send(document::Intent::Saved(Ok(Ok(2))));
    assert_eq!(doc.read(document::status), "Revision 2");
    assert_eq!(doc.take_outputs(), [document::Output::Changed]);

    doc.send(document::Intent::Export);
    doc.send(document::Intent::Exporting(export::Intent::Exported(Err(Error::Panicked))));
    assert_eq!(doc.read(document::status), "The export failed: background work panicked.");

    let library = started_library();
    library.send(library::Intent::Refresh);
    let (asked, ..) = search(&library);
    library.send(library::Intent::Found {
        asked,
        result: Err(Error::Refused),
    });
    assert_eq!(library.read(library::status), "The search failed: the pool refused background work.");
}

/// Scenario 14.
#[test]
fn the_trail_names_each_step_and_each_change_to_what_a_store_shows() {
    let trail = Trail::default();
    let library = started_library();
    library.store().observe(trail.follow("library", library::status));
    library.send(library::Intent::Typed("beta".into()));
    let (asked, ..) = search(&library);
    library.send(library::Intent::Select(DocId(2)));
    library.send(library::Intent::Found {
        asked,
        result: Err(Error::Refused),
    });
    assert_eq!(
        trail.lines(),
        [
            "library Typed",
            "library: Searching",
            "library does Search",
            "library Select",
            "library Found",
            "library: The search failed: the pool refused background work.",
        ],
        "a step that changes nothing shown is there by its name alone"
    );

    let trail = Trail::default();
    let doc = loaded("Alpha", "a");
    doc.store().observe(trail.follow("document 1", document::status));
    doc.send(document::Intent::EditBody("b".into()));
    doc.send(document::Intent::Save);
    doc.take_effects();
    doc.send(document::Intent::Saved(Ok(Ok(2))));
    assert_eq!(
        trail.lines(),
        [
            "document 1 EditBody",
            "document 1: Edited",
            "document 1 Save",
            "document 1: Saving",
            "document 1 does Save",
            "document 1 Saved",
            "document 1: Revision 2",
            "document 1 says Changed",
        ],
        "what a store tells others is there too"
    );
}

/// Scenario 15.
#[test]
fn a_library_restored_from_its_session_starts_where_it_left_off() {
    let library = started_library();
    library.send(library::Intent::NextPage);
    let (asked, ..) = search(&library);
    library.send(library::Intent::Found {
        asked,
        result: found(&[(4, "Delta notes"), (5, "Epsilon"), (6, "Zeta notes")], 7),
    });
    library.send(library::Intent::Select(DocId(5)));
    library.send(library::Intent::Open(DocId(6)));
    library.send(library::Intent::ShowHistory(DocId(4)));
    library.take_effects();
    let session = library.read(library::session);
    assert_eq!(
        session,
        library::Session {
            query: String::new(),
            page: 1,
            selected: Some(DocId(5)),
            pages: vec![Page::List, Page::History(DocId(4))],
            open: vec![DocId(6)],
        }
    );

    let restored = TestStore::<Library>::new(library::State::restored(Preferences::default(), session.clone()));
    let effects = restored.take_effects();
    assert!(
        matches!(&effects[..], [library::Effect::Search { page: 1, .. }, library::Effect::OpenWindow(DocId(6))]),
        "{effects:?}"
    );
    restored.send(library::Intent::Preferences(Preferences::default()));
    assert!(
        restored.take_effects().is_empty(),
        "built on the preferences shared, it doesn't search again when they come"
    );
    assert_eq!(restored.read(library::session), session, "it saves the same again");
    assert_eq!(restored.read(library::pages), [(DocId(4), Standing::Shown)], "the history page opens again, shown");
    assert_eq!(restored.read(library::documents), [(DocId(6), Standing::Shown)], "and so does the document");
}
