# Writing a page

A page has five parts. A complete small feature is in the `spiderland` crate's docs. A whole app, with its stores, hosts, views and
tests, is in `showcase/`.

## 1. Feature

Put it in a crate without Slint.

- State is plain data.
- Anything that touches a disk, a process or a worker is an `Effect`. Its result comes back as an `Intent`.
- Anything from outside, such as a worker's snapshot, arrives as an intent and is kept in the state.
- First loads go in `Feature::start`, not in intents the host sends after building the store.
- What runs while the state wants it, such as a timer or a watch on a service, is a `Subscription`. `Feature::subscriptions` lists
  what a state wants, including what always runs. Test it with `subscriptions`.
- To tell another feature something, `emit` an `Output`. Whoever builds the two stores connects them with `Store::forward`. An
  output reaches only those listening when it goes out. So if the store's start has to be heard, build it with `Store::build`,
  forward from the `Wiring`, then `start` it.
- To keep another store current on part of this one's state, such as settings, `share` it. The other store gets the value at once,
  however late it is built, and again whenever it changes. For a store that keeps the copy for a while, such as a panel, use
  `share_scoped`, which ends when its `Connection` is dropped.

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
- A one-shot command, such as "focus this", is a `Pulse`: the view acts when its serial changes. A view that attaches to a store
  later sets that property from a `Fired`, so it doesn't act on a fire from before it attached.

## 5. Tests

- Test reducer rules with `step`, with no window.
- Test a flow across effects with `TestStore`, with no window either. Send intents, then take each effect, or `answer` it with
  the intent its work would send. An effect left waiting fails the test. `TestStore::rendering` keeps what `present` makes of
  each render, and `take_outputs` gives what the feature told others. `subscribed` hands over a running subscription, to answer
  through as its work would: once the store stops it, what it sends goes nowhere.
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

Work that `Background::run` starts on the pool can't be stopped. Start it with `Background::stoppable` from `subscribe` instead,
and the store stops it when the key goes: the work checks its `Stop`, and can report how far it has got through its `Progress`.
Work that waits, such as on a child process, waits on `Stop::stopped` as well, and stops what it waits on as soon as that completes.
Or number the requests with `Latest`, kept in the state, so the reducer counts only the answer to the latest one.

**One at a time**

Saves of one thing shouldn't overlap. Keep a `OneAtATime` in the state: the reducer asks it for each save, starts the one it gives
back, and calls `done` when a save's answer comes. The latest save asked for meanwhile waits, in place of any before it.

**Writes as the user edits**

A document that saves by itself is `OneAtATime` and a debounce together, each in the state:

- Count the edits. A subscription keyed on the count waits a moment, so each edit starts the wait again, and its answer saves
  only if no edit came since.
- The save goes through `OneAtATime`. An edit during a write waits, and goes next.
- Count the saves that failed in a row. A subscription keyed on the count tries again a while later, a few times, then leaves the
  error showing.
- A close with edits not yet written writes them, and the document answers once the write is in. A write that fails keeps the
  window, with its error.

`Latest` is the wrong tool for writes. It drops an old answer, but not the old write, which can land after a newer one.

**Work with no result**

Work on the pool that panics, that the pool refuses, or that the pool lets go of before it runs, arrives as `Err` of an `Error`
that says which. Each feature decides what each means, and none leaves a page waiting for an answer that won't come. `Error` is a
`std::error::Error`, so `?` carries it into an app's own error. A panic then goes on to the app's pool, which can keep a record of
it.

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

**Inputs that come fast**

A drag, a scrub or a resize sends an intent per event, and each is a batch of its own: a reduce, a render, the subscriptions and
the effects. Send such an input through a `Sampled`, which keeps only the latest. `Sampled::new` sends it on the next turn of the
event loop. Slint already merges the pointer's moves within a turn, so for a drag, `Sampled::every` with a frame's interval sends
at most once a frame, and the last move still goes. Send what has to follow it, such as the drop at a drag's end, through the
same sampler's `send`, so it goes after the last move.
A debounce waits for an input to stop, which suits a search, not a drag that shows where it is as it moves.

**Lists inside rows**

Use `rows.sync(inner.fill(…))`, or `rows.sync_by(key, inner.fill_by(key, …))` with the same key. A plain `Rows` in a row can lose a
change: in Slint 1.18.1, while the outer row has an update not yet painted, an inner item changed in place stays old on screen. Two
renders with no paint between them are enough. `InnerRows` gets the change onto the screen.

Under `sync_by`, fill by key too. `fill` keeps a model per position, so when a row before it goes, a row that stays gets another
row's model: its list is built again, and focus inside it is lost. `fill_by` keeps each row's model with its key.

**A child feature inside a page**

A part of a page with rules of its own, such as an export and its progress, can be a `Feature` of its own and still run in the
page's store. The parent keeps the child's state in a `Slot`, which numbers each start with an `Instance`, and maps the rest
with that number:

- Intents: the parent has one that wraps the child's with its instance, as in `Intent::Exporting(instance, intent)`. It finds the
  child with `state.export.get_mut(instance)` and hands the intent on with
  `effects.reduce_child::<Export>(export, intent, |effect| Effect::Export(instance, effect))`. What the child emits comes back to
  the parent's `reduce`, which decides what it means. `effects.start_child` runs the child's `start` on what `Slot::start`
  returns.
- Subscriptions: the parent's `subscriptions` wraps the child's, as in
  `state.export.subscriptions::<Export>().map(|(instance, key)| Running::Export(instance, key))`. Stopped, the child stops what
  it runs, with nothing to cancel.
- Host: the parent's host keeps the child's, and calls it with
  `intents.map(move |intent| Intent::Exporting(instance, intent))`, so what its work sends comes back wrapped, with the instance
  it is for.

`get_mut` finds nothing for an instance that was stopped, so the parent lets its intent go, even once another has started. Work
that a subscription runs stops with its child. An answer from work that can't be stopped, such as `Background::run`, or from a
sender the child's host kept, would otherwise reach the next child. Keying the subscriptions on the instance matters too: a child
stopped and started again in one batch would keep the same key, and with it the old work. `Latest` doesn't serve here: it counts
only the first answer to a request, and a child hears many.

**Children a parent keeps**

A page under another, a dialog, or a document with windows of its own is a store of its own, which a parent keeps. The parent's
state names its children by key, and says where each stands. Wire them on the parent's `Wiring`:

```rust
let documents = Children::default();
wiring.children(&documents, library::documents, move |&id, library| {
    let document = Store::<Document>::build(document::State::new(id), DocumentHost::new(id));
    document.forward(library, move |output| library::Intent::Document(id, *output));
    document.start()
});
```

- A stack is a `Vec` of keys in the state, the last one `Standing::Shown` and the rest `Hidden`. A slot is an `Option`. A set is a
  list whose children are all shown.
- After each batch, a new key's child is built, and a key that went takes its child with it. What the child was loading stops
  counting, with nothing to check, because its `Intents` no longer reach a store.
- Each child hears its standing as a `Lifecycle` event when it is built and whenever it changes. Give the child's intent a
  `From<Lifecycle>`, and `Store` is a `Child`. For a child that is more than a store, such as a page and its view, write
  `Child::tell`.
- To close, the parent sets each child to `Standing::Closing` and keeps a `Closing` in its state. Each child answers with an output
  once it has nothing left to finish, such as a save, and the parent drops it then. So the app's last window can wait for every
  document's saves before it closes, and the event loop runs until it does.
- Children are built before the batch's effects run, so an effect can reach a child its batch named, through `Children::get`.
- `build` is handed the parent's `Intents` to forward to. It lives as long as the parent, so it mustn't hold the parent's store.
- A window doesn't keep its store alive. The parent keeps it, so a window held a moment longer, such as until the next turn of the
  event loop, doesn't keep its work going.

**Saving where the user left off**

Each store that is part of the session saves its own part in one `Saved`, under a key, in a form the app picks, such as JSON. On the
next start, each takes its part back as it is built:

```rust
let saved = Saved::new(read_session());
let state = saved.take("library").and_then(decode).map(library::State::restored).unwrap_or_default();
let wiring = Store::<Library>::build(state, LibraryHost::new());
wiring.save_in(&saved, "library", |state| encode(&library::session(state)));
```

- The app says when to save, by calling `Saved::save`, and keeps the result. Save as the user asks to close, while the children
  that close then are still there to save.
- A child saves under a key of its own, such as `document 3`, and its build takes its part by that key. A parent that rebuilds
  its children from its own state doesn't need to know what each child saved. Hand the `Saved` to the child's build.
- Save the place, not the data a service owns: the search and its page, not its results. A store restored asks for those again
  as it starts.
- A store wired to take a shared value, such as the preferences, starts on the value restored, so the first one shared changes
  nothing.
- A part that doesn't decode, such as one an older build saved, starts afresh.

**More than one window on a store**

A store has one `Host`, which runs its effects, and any number of views. Write each window's view as a `WindowView`, and attach
it with `attach_window(&store, &window, view, ask)`. It shows the state at once, and stays attached for as long as the window is
open. When the user closes the window, `ask` says whether it simply closes, which detaches its view, or stays and sends the store
an intent. With `store.views() == 1`, one of several windows simply closes and the last one asks first.

- A view renders whether or not its window shows, because a minimized window is still open.
- A command for every window, such as "close", is a `Pulse` that each view acts on through its own `Fired`. A window opened after
  the fire doesn't act on it. A view that hides its window for good says so in `WindowView::is_gone`, so the store lets it go.
- The host's `render` can be left out once the views do the showing.
- For a view that isn't a window, `store.attach(view)` returns an `Attached`: dropping it detaches the view.

**What Rust can reach**

Rust reaches only exported components and globals. So one feature maps to one global, and one instance per window.

**Slint names**

- Don't name a callback after a value some binding inside the component uses: a callback `start` breaks `alignment: start`.
- A global can't be named `Text`, because that is the element. Name a page's words `HomeText`.
