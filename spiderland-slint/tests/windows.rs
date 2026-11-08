//! `testing` with a window for each component: input reaches the window it is sent to, each draws at its own size, and hiding them
//! in turn is no fault. One test: Slint's platform is set once per process.

#![allow(missing_docs, reason = "`slint!` makes the view's types public, with no docs")]

use slint::{CloseRequestResponse, ComponentHandle};
use spiderland_slint::testing::{Headless, settle};
use std::cell::Cell;
use std::rc::Rc;

slint::slint! {
    export component Pane inherits Window {
        out property <int> clicks;
        out property <string> typed: field.text;

        field := TextInput {
            y: 0px;
            height: 20px;
        }

        TouchArea {
            y: 40px;
            height: 40px;
            clicked => {
                root.clicks += 1;
            }
        }
    }
}

#[test]
fn each_component_gets_a_window_of_its_own_once_asked() {
    let headless = Headless::install();
    let shared = Pane::new().unwrap();
    assert!(std::ptr::eq(shared.window(), &**headless.window), "until asked, a component shows in the one window");
    drop(shared);

    headless.separate_windows();
    let (left, right) = (Pane::new().unwrap(), Pane::new().unwrap());
    assert!(!std::ptr::eq(left.window(), right.window()));
    headless.on(left.window()).resize(200, 100);
    headless.on(right.window()).resize(300, 150);
    left.show().unwrap();
    right.show().unwrap();

    headless.on(right.window()).click(50.0, 60.0);
    assert_eq!((left.get_clicks(), right.get_clicks()), (0, 1), "a click reaches only its window");
    headless.on(left.window()).click(50.0, 10.0);
    headless.on(left.window()).type_text("ab");
    assert_eq!((left.get_typed().as_str(), right.get_typed().as_str()), ("ab", ""), "typing reaches only its window");

    let (drawn_left, drawn_right) = (left.window().take_snapshot().unwrap(), right.window().take_snapshot().unwrap());
    assert_eq!((drawn_left.width(), drawn_left.height()), (200, 100));
    assert_eq!((drawn_right.width(), drawn_right.height()), (300, 150));

    // A close request goes to the window's own handler, which can keep it.
    let asked = Rc::new(Cell::new(false));
    let asking = asked.clone();
    right.window().on_close_requested(move || {
        asking.set(true);
        CloseRequestResponse::KeepWindowShown
    });
    headless.on(right.window()).close();
    assert!(asked.get() && right.window().is_visible());

    // Slint asks to quit once the last window that shows hides. That is no fault, and hiding the first asks nothing.
    left.hide().unwrap();
    settle();
    right.hide().unwrap();
    settle();
}
