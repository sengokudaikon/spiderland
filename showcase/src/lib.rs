//! A small document library built on `spiderland` and `spiderland-slint`. It is the framework's own example, and the test of what
//! the framework can do: each scenario below runs through the stores, and a test checks it.
//!
//! The parts:
//! - [`service`]: a fake document service on the showcase's own [`pool`]. A test holds its calls at a gate, and makes them fail.
//! - [`settings`], [`library`] and [`document`]: the features, each pure, each tested with no window.
//! - [`view`]: the windows, in Slint's language.
//! - [`app`]: the hosts, and the app that builds the stores and windows and connects them.
//!
//! The scenarios, and where each is tested. `tests/flows.rs` runs a feature's rules through a `TestStore`, and `tests/views.rs`
//! runs the whole app over real windows with no display.
//!
//! | Scenario | Flows | Views |
//! |---|---|---|
//! | 1. A searchable, paged list that refreshes, which drops an older answer that comes last | yes | yes |
//! | 2. The selection stays on its document when rows before it come or go | yes | yes |
//! | 3. An edit form with validation, and a text box whose edit the reducer refuses | yes | yes |
//! | 4. Saves of one document go one at a time, and one started before its window closes is recorded | yes | yes |
//! | 5. An export on the pool, with progress, that the user stops | yes | yes |
//! | 6. A delete confirmation, a question that returns its answer | yes | yes |
//! | 7. From the library to a document's history, and back | yes | yes |
//! | 8. Settings shared by every page and window, which a window opened later starts on | | yes |
//! | 9. A second window on the same document sees the first window's edits | | yes |
//! | 10. Closing with unsaved edits asks first. Reopening during a save shows the current state, and no old toast | yes | yes |
//! | 11. A document changed outside the app reloads through a watch | yes | yes |
//! | 12. A failed job and a panicking one each show in the state, and the store goes on | yes | yes |
//!
//! Where the app writes by hand what the framework doesn't yet do, the code says `Workaround:` and names what is missing.

pub mod app;
pub mod document;
pub mod library;
pub mod pool;
pub mod service;
pub mod settings;
pub mod view;

/// Documents to start with: a title and a body each.
pub const SAMPLE: [(&str, &str); 7] = [
    ("Alpha notes", "the first draft"),
    ("Beta plan", "two steps"),
    ("Gamma list", "one two three"),
    ("Delta notes", "a delta"),
    ("Epsilon", "small"),
    ("Zeta notes", "the last word"),
    ("Eta", "short"),
];
