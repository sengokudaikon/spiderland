//! What one-way state does across a real view: the rules in `docs/writing-a-page.md`, `Pulse`, and `Rows` under focus and on screen.
//! One test: Slint's platform is set once per process.

#![allow(missing_docs, reason = "`slint!` makes the view's types public, with no docs")]

use slint::{ComponentHandle, Model, ModelRc, SharedString, Weak};
use spiderland::{Effects, Feature, Host, Intents, Never, Pulse, Store};
use spiderland_slint::testing::{Headless, settle};
use spiderland_slint::{InnerRows, Rows};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    export struct NameRow {
        id: int,
        name: string,
    }

    export struct GridRow {
        lit: bool,
        cells: [bool],
    }

    export component View inherits Window {
        width: 400px;
        height: 400px;

        in property <string> pulse-value;
        in property <int> pulse;
        out property <int> heard;
        out property <string> last-heard;
        changed pulse => {
            heard += 1;
            last-heard = pulse-value;
        }

        in property <string> bound;
        out property <string> bound-shown: bound-input.text;
        bound-input := TextInput {
            x: 0;
            y: 0;
            width: 400px;
            height: 20px;
            text: root.bound;
        }

        in property <string> value;
        in property <int> force;
        callback edited(string);
        out property <string> shown: field.text;
        changed value => {
            field.text = value;
        }
        changed force => {
            field.text = value;
        }
        field := TextInput {
            x: 0;
            y: 20px;
            width: 400px;
            height: 20px;
            edited => {
                root.edited(self.text);
            }
        }

        in property <[NameRow]> names;
        callback name-edited(int, string);
        for row[index] in names: TextInput {
            x: 0;
            y: 40px + index * 20px;
            width: 400px;
            height: 20px;
            text: row.name;
            edited => {
                root.name-edited(row.id, self.text);
            }
        }

        in property <[GridRow]> grid;
        out property <int> cells-built;
        for row[index] in grid: Rectangle {
            x: 0;
            y: 200px + index * 20px;
            width: 400px;
            height: 20px;
            Rectangle {
                x: 0;
                width: 20px;
                background: row.lit ? #ff0000 : #0000ff;
            }
            for cell[position] in row.cells: Rectangle {
                x: 20px + position * 20px;
                width: 20px;
                background: cell ? #ff0000 : #0000ff;
                init => {
                    root.cells-built += 1;
                }
            }
        }
    }
}

/// A field whose reducer refuses digits.
struct Field;

#[derive(Default)]
struct FieldState {
    text: String,
    refused: Pulse,
}

impl Feature for Field {
    type State = FieldState;
    type Intent = String;
    type Effect = ();
    type Output = Never;
    type Subscription = Never;

    fn reduce(state: &mut FieldState, text: String, _: &mut Effects<()>) {
        if text.contains(|c: char| c.is_ascii_digit()) {
            state.refused.fire(());
        } else {
            state.text = text;
        }
    }
}

struct FieldHost {
    view: Weak<View>,
    /// Whether the view hears about a refused edit.
    pulse: bool,
}

impl Host<Field> for FieldHost {
    fn render(&mut self, state: &FieldState, _: &Intents<String>) {
        let view = self.view.upgrade().unwrap();
        view.set_value(state.text.as_str().into());
        if self.pulse {
            view.set_force(state.refused.serial());
        }
    }

    fn perform(&mut self, (): (), _: &Intents<String>) {}
}

fn name(id: i32, name: &str) -> NameRow {
    NameRow { id, name: name.into() }
}

const LIT: [u8; 3] = [0xff, 0, 0];
const DARK: [u8; 3] = [0, 0, 0xff];

/// The colour of column `column` of grid row `row`, as drawn. Column 0 is the row's own field, and the rest are its inner list.
fn drawn(view: &View, row: usize, column: usize) -> [u8; 3] {
    settle();
    let frame = view.window().take_snapshot().unwrap();
    let pixel = frame.as_slice()[(210 + row * 20) * frame.width() as usize + 10 + column * 20];
    [pixel.r, pixel.g, pixel.b]
}

#[test]
fn state_flows_through_a_real_view() {
    let headless = Headless::install();
    let view = View::new().unwrap();
    headless.resize(400, 400);
    view.show().unwrap();

    // `changed` runs on a later turn of the event loop, and collapses repeats.
    let mut pulse = Pulse::new(SharedString::default());
    let show = |pulse: &Pulse<SharedString>| {
        view.set_pulse_value(pulse.value().clone());
        view.set_pulse(pulse.serial());
    };
    pulse.fire("first".into());
    show(&pulse);
    assert_eq!(view.get_heard(), 0, "`changed` hasn't run yet");
    settle();
    assert_eq!(view.get_heard(), 1);
    pulse.fire("skipped".into());
    show(&pulse);
    pulse.fire("last".into());
    show(&pulse);
    settle();
    assert_eq!(view.get_heard(), 2, "two fires with no turn between them are heard as one");
    assert_eq!(view.get_last_heard(), "last");

    // A pulse is heard again for the same value.
    pulse.fire("last".into());
    show(&pulse);
    settle();
    assert_eq!(view.get_heard(), 3);

    // User input removes a one-way binding on `TextInput.text`.
    view.set_bound("one".into());
    settle();
    assert_eq!(view.get_bound_shown(), "one");
    headless.click(5.0, 10.0);
    headless.type_text("x");
    assert!(view.get_bound_shown().contains('x'));
    view.set_bound("two".into());
    settle();
    assert!(view.get_bound_shown().contains('x'), "the binding is gone, so the box no longer follows its property");

    // A wrapper that copies the property back on `changed` keeps following it after input. But a refused edit leaves the state's
    // text as it was, so `changed` can't fire, and the box keeps what was refused.
    let store = Store::new(
        FieldState::default(),
        FieldHost {
            view: view.as_weak(),
            pulse: false,
        },
    );
    let intents = store.intents();
    view.on_edited(move |text| intents.send(text.into()));
    headless.click(5.0, 30.0);
    headless.type_text("ab");
    assert_eq!(view.get_shown(), "ab");
    headless.type_text("7");
    assert_eq!(store.try_read(|state| state.text.clone()).unwrap(), "ab", "the reducer refused it");
    assert_eq!(view.get_shown(), "ab7", "and nothing told the box");
    store.send("clean".into());
    settle();
    assert_eq!(view.get_shown(), "clean", "it follows again once the state's text moves");

    // A pulse forces the text back.
    let store = Store::new(
        FieldState::default(),
        FieldHost {
            view: view.as_weak(),
            pulse: true,
        },
    );
    let intents = store.intents();
    view.on_edited(move |text| intents.send(text.into()));
    settle();
    assert_eq!(view.get_shown(), "");
    headless.type_text("ab");
    headless.type_text("7");
    assert_eq!(store.try_read(|state| state.text.clone()).unwrap(), "ab", "the reducer refused it");
    assert_eq!(view.get_shown(), "ab", "the pulse put the box right");
    headless.type_text("c");
    assert_eq!(view.get_shown(), "abc", "and the box carries on");

    // Rows by key: removing the first row leaves the row being typed in, and its focus, with the item.
    let edits: Rc<RefCell<Vec<(i32, SharedString)>>> = Rc::default();
    let heard = Rc::clone(&edits);
    view.on_name_edited(move |id, text| heard.borrow_mut().push((id, text)));
    let names = Rows::default();
    view.set_names(names.model());
    let key = |row: &NameRow| row.id;
    names.sync_by(key, [name(1, "a"), name(2, "b"), name(3, "c")]);
    settle();
    headless.click(5.0, 70.0);
    headless.type_text("x");
    assert_eq!(edits.borrow().last().unwrap().0, 2, "the click reached the second row");
    names.sync_by(key, [name(2, "b"), name(3, "c")]);
    settle();
    headless.type_text("y");
    let (id, text) = edits.borrow().last().unwrap().clone();
    assert_eq!(id, 2, "the focus went with the item");
    assert!(text.contains('x') && text.contains('y'), "and so did what was typed: {text}");
    assert_eq!(names.model().row_count(), 2);

    // By position, the same removal ends the second slot, and the item's focus with it.
    edits.borrow_mut().clear();
    let slots = Rows::default();
    view.set_names(slots.model());
    slots.sync([name(1, "a"), name(2, "b"), name(3, "c")]);
    settle();
    headless.click(5.0, 70.0);
    headless.type_text("x");
    slots.sync([name(2, "b"), name(3, "c")]);
    settle();
    headless.type_text("y");
    let (id, _) = edits.borrow().last().unwrap().clone();
    assert_eq!(id, 3, "the focus stayed on the slot, which now shows another item");
    view.set_names(ModelRc::default());
    settle();

    // Inner rows: an inner item and its outer row can change together, with no render in between, and the screen shows both.
    let grid = Rows::<GridRow>::default();
    let mut inner = InnerRows::default();
    view.set_grid(grid.model());
    let row = |lit: bool, cells: [bool; 2]| (GridRow { lit, cells: ModelRc::default() }, cells);
    grid.sync(inner.fill([row(false, [false, false]), row(false, [false, true])], |row, cells| row.cells = cells));
    assert_eq!([drawn(&view, 0, 0), drawn(&view, 0, 1), drawn(&view, 1, 2)], [DARK, DARK, LIT]);
    grid.sync(inner.fill([row(true, [true, false]), row(false, [false, false])], |row, cells| row.cells = cells));
    assert_eq!(drawn(&view, 0, 0), LIT, "the outer row changed");
    assert_eq!(drawn(&view, 0, 1), LIT, "and its inner item, in the same render");
    assert_eq!(drawn(&view, 1, 2), DARK, "and an inner item alone, under a row that didn't");
    let model = grid.model().row_data(0).unwrap().cells;
    grid.sync(inner.fill([row(true, [false, true]), row(true, [true, true])], |row, cells| row.cells = cells));
    assert_eq!(
        grid.model().row_data(0).unwrap().cells,
        model,
        "a row keeps its inner model, so that alone doesn't count as a change"
    );
    assert_eq!([drawn(&view, 0, 1), drawn(&view, 0, 2), drawn(&view, 1, 0), drawn(&view, 1, 1)], [DARK, LIT, LIT, LIT]);

    // Slint 1.18.1 loses an inner change made while its outer row has an update not yet painted. Plain `Rows` show it. If this fails
    // after moving the Slint pin, the bug is gone, and `Rows::refresh` and the warning on `Rows` can go too.
    let outer = Rows::<GridRow>::default();
    let cells = Rows::<bool>::default();
    view.set_grid(outer.model());
    cells.sync([false, false]);
    outer.sync([GridRow { lit: false, cells: cells.model() }]);
    assert_eq!(drawn(&view, 0, 1), DARK);
    outer.sync([GridRow { lit: true, cells: cells.model() }]);
    cells.sync([true, false]);
    assert_eq!([drawn(&view, 0, 0), drawn(&view, 0, 1)], [LIT, DARK], "the outer row shows, and the inner change is lost");

    // `InnerRows` gets it onto the screen, through two renders with no paint between them, and builds nothing to do so.
    let grid = Rows::<GridRow>::default();
    let mut inner = InnerRows::default();
    view.set_grid(grid.model());
    grid.sync(inner.fill([row(false, [false, false])], |row, cells| row.cells = cells));
    assert_eq!(drawn(&view, 0, 1), DARK);
    let built = view.get_cells_built();
    assert!(built > 0, "the count of built rows has counted some");
    grid.sync(inner.fill([row(true, [false, false])], |row, cells| row.cells = cells));
    grid.sync(inner.fill([row(true, [true, false])], |row, cells| row.cells = cells));
    assert_eq!([drawn(&view, 0, 0), drawn(&view, 0, 1)], [LIT, LIT], "both show");
    assert_eq!(view.get_cells_built(), built, "the inner rows were refreshed in place");
    assert_eq!(grid.model().row_data(0).unwrap().cells.row_count(), 2, "and the list is as long as it was");
}
