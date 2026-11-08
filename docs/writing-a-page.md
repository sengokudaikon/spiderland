# Writing a page

A page has five parts. A complete small feature is in the `spiderland` crate's docs.

## 1. Feature

Put it in a crate without Slint.

- State is plain data.
- Anything that touches a disk, a process or a worker is an `Effect`. Its result comes back as an `Intent`.
- Anything from outside, such as a worker's snapshot, arrives as an intent and is kept in the state.
- First loads go in `Feature::start`, not in intents the host sends after building the store.
- What runs while the state wants it, such as a timer or a watch on a service, is a `Subscription`. `Feature::subscriptions` lists
  what a state wants, including what always runs. Test it with `subscriptions`.
- To tell another feature something, `emit` an `Output`. Whoever builds the two stores connects them with `Store::forward`, straight
  after building them.

## 2. `present`

Put it next to the feature.

- It takes only what it reads, and returns plain structs.
- Keep what it reads in `Tracked`, so a render can tell when it needs to run again.
- Make it a plain `fn` and run it with `Slice::show`, which keys it on its own inputs.

## 3. Host

Put it in the crate with the window.

- Write it as a `WindowHost`, and run it through `Windowed`, which holds the window weakly. `render` gets the window, and isn't
  called once the window is gone. `perform` always runs, and gets the window only if it is still there.
- `callbacks!` maps view callbacks to intents, and does nothing else.
- `render` runs each `Slice`, and syncs `Rows`. It does nothing else, so skipping it once the window is gone loses nothing.
- `perform` matches on the effect.
- `subscribe` matches on the key, starts what it names, and returns its guard. The store drops the guard when the state stops
  wanting the key, whether or not `render` ran.
- Work that comes back with two things, such as a fresh snapshot and its answer, sends them with `send_all`, so no render or effect
  sees one without the other.

## 4. View

- Components take properties and callbacks.
- Only the page root reads the page's global.
- What only the view cares about, such as hover, highlights and scroll, stays in Slint.
- A one-shot command, such as "focus this", is a `Pulse`: the view acts when its serial changes.

## 5. Tests

- Test reducer rules with `step`, with no window.
- Test a flow across effects with `TestStore`, with no window either. Send intents, then take each effect, or `answer` it with
  the intent its work would send. An effect left waiting fails the test. `TestStore::rendering` keeps what `present` makes of
  each render, and `take_outputs` gives what the feature told others.
- Test behaviour across the view with `spiderland_slint::testing`: the real window on real background work.

## Watch out for

**Writes**

A write's outcome must reach a store that is still alive. `Background` runs accepted work to its end, but delivers nowhere once
its store is gone. Never tie a write to a scope that can close first.

**A load that a newer one replaces, and a debounce**

Both are subscriptions, so `subscriptions` tests them with no host, and nothing in the host keeps a guard by hand.

- A load is keyed on its query. `subscribe` starts it with `Background::local` and returns the `Dropping`. When the query changes,
  the store drops the old load and starts the new one. Once the answer is in the state, the state stops wanting the key.
- A debounce is keyed on an edit count. Each edit moves the key, so the store drops the last `after` timer and starts another,
  and only the last edit's timer fires.

```rust
fn subscriptions(state: &State) -> impl IntoIterator<Item = Running> {
    // `found_for` is the query the last answer was for.
    let loading = (state.found_for != state.query).then(|| Running::Load(state.query.clone()));
    let settling = (state.edits != state.settled).then_some(Running::Settle(state.edits));
    loading.into_iter().chain(settling)
}
```

Work on the pool can't be stopped: `Background::run` runs to its end. For that, the reducer checks that an answer is for the query
the state still holds.

**Panics**

A panic in background work arrives as `Err(Panicked)`, and each feature decides what it means. `Panicked` is a
`std::error::Error`, so `?` carries it into an app's own error. The panic then goes on to the app's pool, which can keep a record
of it.

**Slice keys**

A `Slice` key that leaves out an input shows stale values. `Slice::show` keeps one from being left out by mistake, because its `fn`
can't capture: it reads what the key is made from. It can still read statics, and a `Tracked` sees only changes made through `&mut`,
so keep a `Cell` or a shared `Rc<RefCell<_>>` out of one. A function that reads through a whole editor can't be given its inputs one by one: keep its key beside it, and use
`CheckedSlice`, which fails a test when the key misses an input. It checks only under `spiderland`'s `testing` feature: turn that on
from dev-dependencies, so a normal build doesn't make every value on every render.

**Text boxes**

User input removes a one-way binding on `TextInput.text`, so the box stops following its property. A wrapper has to copy the
property back:

```slint
in property <string> value;
in property <int> refused;
changed value => { input.text = value; }
changed refused => { input.text = value; }
```

The second line is for a reducer that can refuse an edit. A refusal leaves the state's text as it was, so `changed value` never
runs and the box keeps the refused text. Fire a `Pulse` from the reducer and set `refused` to its serial.

**`changed`**

It runs on a later turn of the event loop, and repeats collapse into one. What it sends arrives as a separate intent, after the
render that caused it.

**Lists inside rows**

Use `rows.sync(inner.fill(…))`. A plain `Rows` in a row can lose a change: in Slint 1.18.1, while the outer row has an update not yet
painted, an inner item changed in place stays old on screen. Two renders with no paint between them are enough. `InnerRows` gets
the change onto the screen.

**What Rust can reach**

Rust reaches only exported components and globals. So one feature maps to one global, and one instance per window.

**Slint names**

- Don't name a callback after a value some binding inside the component uses: a callback `start` breaks `alignment: start`.
- A global can't be named `Text`, because that is the element. Name a page's words `HomeText`.
