//! Elements found by accessible label in a window Slint lays out, and driven by real input. One test: Slint's platform is set
//! once per process.

#![allow(missing_docs, reason = "`slint!` makes the view's types public, with no docs")]

use slint::{ComponentHandle, ModelRc, VecModel};
use spiderland_slint::testing::{Headless, element, find};

slint::slint! {
    import { Button, LineEdit, ListView } from "std-widgets.slint";
    export component Page inherits Window {
        in property <[string]> names;
        in property <bool> asking;
        in-out property <string> opened;
        out property <string> typed: search.text;
        preferred-width: 500px;
        preferred-height: 400px;
        VerticalLayout {
            search := LineEdit { accessible-label: "Search"; }
            ListView {
                for name in root.names: HorizontalLayout {
                    Text { text: name; }
                    Button {
                        text: "Open";
                        accessible-label: "Open " + name;
                        clicked => {
                            root.opened = name;
                        }
                    }
                }
            }
            if root.asking: Button { text: "Confirm"; }
        }
    }
}

#[test]
fn elements_found_by_label_take_real_input_where_the_layout_put_them() {
    let headless = Headless::install();
    let page = Page::new().expect("the page");
    page.set_names(ModelRc::new(VecModel::from(vec!["Alpha".into(), "Beta".into(), "Gamma".into()])));
    headless.resize(500, 400);
    page.show().expect("show the page");

    element(&page, "Open Gamma").click();
    assert_eq!(page.get_opened(), "Gamma", "a row's button, found inside the list");
    element(&page, "Search").type_text("notes");
    assert_eq!(page.get_typed(), "notes");

    assert!(find(&page, "Confirm").is_none(), "an element that isn't there isn't found");
    page.set_asking(true);
    let confirm = element(&page, "Confirm");
    let (x, y) = confirm.middle();
    assert!(x > 0.0 && y > 0.0, "it has a place: {confirm:?}");
    let missing = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| element(&page, "Delete").click())).expect_err("no such element");
    let message = missing.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(message.contains("no element labelled \"Delete\""), "{message}");
}
