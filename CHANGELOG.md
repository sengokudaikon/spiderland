# Changelog

Every notable change goes here, in the same commit as the change. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, a breaking change moves the minor version.

All crates in the workspace share one version.

To release, rename `Unreleased` to the new version and date, set that version in `Cargo.toml`, and push a tag such as `v0.2.0`. The
tag makes a GitHub release from that version's section.

## [Unreleased]

### Added

- `spiderland`: `Feature`, `Host` and `Store` for one-way UI state, with `Tracked`, `Slice`, `CheckedSlice`, `Subscriptions`, `step`
  and `callbacks!`.
- `spiderland-slint`: `Background`, `every`, `after`, `on_watch`, `on_wake`, `Rows` and `testing`.
- `spiderland`: `Feature::Output`, `Effects::emit` and `Store::forward`, so a feature can tell others something. An output goes out
  after its batch's render and effects, and one emitted before there is a listener reaches no one.
- `spiderland`: `Feature::start`, which sets up the first state and asks for effects before the first render, and `start` to test it.
- `spiderland`: `Intents::map`, so a child's host can run inside its parent's store.
- `spiderland`: `Lifecycle`, `CloseAnswer` and `Closing`, so a parent can show and hide its children and ask whether they can close.
- `spiderland`: `Pulse`, a one-shot command to a view with properties and no events.
- `spiderland-slint`: `Rows::sync_by`, which matches rows by key, so a row's focus follows its item when rows before it come or go.
- `spiderland-slint`: `InnerRows`, a list inside each row, with one stable model per row. It works around a Slint 1.18.1 bug that
  keeps an inner change off the screen while its outer row has an update not yet painted.
- `docs/writing-a-page.md`: rules for text boxes, `changed` and lists inside rows, each checked against a real view.
- `spiderland`: `Slice::show`, with `Input` and `Inputs`. It makes a part of a render with a plain `fn` of what it reads, and keys
  it on the same inputs, so none is left out by mistake.
- `spiderland`: `Store::observe` and `Seen`, which show each intent just before it is reduced and each effect just before it is
  performed. The observer gets a reference, so nothing is copied and nothing needs `Debug`.
- `spiderland`: `Intents::send_all` and `Store::send_all`, which send several intents as one batch. All of them are reduced before
  the render and the effects, so a snapshot and the answer that came with it are never shown or acted on apart.
- `spiderland`: `Feature::Subscription`, `Feature::subscriptions` and `Host::subscribe`, so a feature says what runs while its state
  wants it, and `subscriptions` to test that with no host. After each batch's render the store starts new keys and drops the ones
  no longer wanted, whether or not the host rendered anything.
- `spiderland`, `spiderland-slint`: `Dropping`, `Listening`, and the timers `every` and `after` return, are `#[must_use]`. A guard
  written as a plain statement is dropped at once and stops what it guards before it runs, so that now warns.
- All crates: every public type implements `Debug`, and every public item has docs. `missing_docs` and
  `missing_debug_implementations` keep it so.
- `spiderland`: `TestStore`, which runs the whole loop in a test on a host that keeps what the store asks of it. Each effect waits
  for the test to take it or answer it with intents, and one left waiting when the store is dropped fails the test.
  `TestStore::rendering` keeps what a function of the state makes of each render, and `TestStore::take_outputs` gives what the
  store told others.
- `spiderland`: `Store::build`, which builds a store without starting it, and `Wiring`, on which listeners and observers are wired
  before `Wiring::start`. An output from the start, such as one that follows an effect that answers at once, used to reach no one,
  and `observe` missed the start. `Store::new` builds and starts in one step, as before.
- `spiderland`: `Store::connect`, which forwards until the `Connection` it returns is dropped. Dropping it lets go of the mapping at
  once, for a connection between stores that both go on.
- `spiderland`: `Store::share`, which sends a projection of the state to another store now, and again after each batch in which it
  changed, so a store built later starts on the current value.
- `spiderland`: `Background::stoppable`, with `Stop`, `Progress` and `Stopping`. It runs work on the app's pool that stops when its
  guard is dropped, such as when a subscription's key goes, and reports progress as intents. Nothing of it arrives once it is
  stopped.
- `spiderland`: `Latest` and `Asked`, which number requests in the state so only the answer to the latest counts, and `OneAtATime`,
  which keeps work such as saves from overlapping. Both are plain data for a reducer.
- `spiderland`: `View`, `Store::attach` and `Attached`, so a store can have any number of views besides its host, and a view can
  attach later and leave while the store goes on. A view shows the state at once as it attaches. `Wiring::attach` attaches one
  before the start. `Store::views` counts the views attached and not gone. `Host::render` now does nothing unless a host says
  otherwise, so a host whose views attach leaves it out. Existing hosts are unchanged.
- `spiderland`: `Fired`, which gives one view a `Pulse` serial that starts at 0, so a view that attaches after a pulse fired
  doesn't act on that fire.
- `spiderland-slint`: `WindowView`, a view in one window, attached through `Windowed`. It renders while its window is minimized
  or hidden, and is gone once the window is dropped or the view says so. `attach_window` attaches one for as long as its window is
  open, and lets the store decide whether the window closes. The window doesn't keep the store alive. `ask_before_closing` does
  that for a window a host shows.
- `spiderland-slint`: `WindowHost` and `Windowed`, so a host is handed its window rather than upgrading a `Weak` itself. `render`
  gets the window and isn't called once it is gone. `perform` always runs, with the window if it is still there, so a write's
  outcome is still heard.
- `spiderland`: `Effects::reduce_child` and `Effects::start_child`, which run a child feature inside its parent's store. Its
  effects come back wrapped, and what it emits comes back to the parent's `reduce` to act on. With the parent's `subscriptions`
  wrapping the child's and `Intents::map` for its host, a child maps whole. One kept in an `Option` stops what it runs once it
  goes back to `None`.
- `spiderland`: `Children`, child stores a parent keeps, one for each key its state names. `Store::children` and
  `Wiring::children` keep them in step after each batch: a child is built as its key appears, dropped once the key goes, and told
  its `Standing` as a `Lifecycle` event as it is built and whenever it changes. A stack of pages, a slot and a set of documents
  are each a shape of keys in the state. A child dropped stops what it runs, and an answer for it goes nowhere. `Child::tell`
  says how a child hears its lifecycle, and a `Store` whose intent is `From<Lifecycle>` has it.
- `spiderland`: `Store::downgrade` and `WeakStore`, to hold a store without keeping it alive.
- `spiderland-slint`: `InnerRows::fill_by`, which keeps each row's inner model by key, for an outer `Rows::sync_by`. A row that
  stays keeps its model as rows before it come or go, so its list isn't built again and focus inside it stays.
- `spiderland`: `Store::share_scoped` and `Store::observe_scoped`, and the same on `Wiring`. Each returns a `Connection`, and
  dropping it ends the share or the observer at once, while both stores go on. `share` and `observe` stay as they were, for as
  long as the stores live.
- `spiderland`: `Saved`, `Store::save_in` and `Wiring::save_in`, so each store saves its part of the session under a key, in a
  form the app picks, as the app says when. On the next start, each store takes its part back as it is built, and a child its
  own by its key.
- `spiderland`: `Stop::stopped`, a future that completes once the work's `Stopping` is dropped. Work that waits on something
  else, such as a child process, waits on it too, and stops the moment it is asked, with no loop that checks `is_stopped`.
- `spiderland`: `Seen::State`, the state just after each intent is reduced and as a store wired on a `Wiring` starts, and
  `Seen::Output`, each output just before it is heard. So a trail can record what each intent changed, and what a store told
  others. `&Never` converts into `&'static str`, so a trail that names its steps that way covers a feature with no effects or
  no outputs.
- `spiderland-slint`: `testing::Headless::separate_windows`, after which each component made gets a window of its own, and
  `Headless::on`, whose `testing::Input` sends real input to one window. `Input::close` asks a window to close, as its title
  bar's button does. Until `separate_windows`, every component shows in `Headless::window`, as before.
- `showcase`: a small document library built on both crates, as their example and the test of what they can do. Fifteen
  scenarios run through its stores: a search whose late answers are dropped, paging, a selection that follows its document,
  a form that refuses an edit, saves one at a time, an export with progress that stops, a delete that asks first, a history
  page, settings shared by every window, a second window on one document, a close that asks first, failures that show in
  the state, closing the library, which waits for every document's saves, a trail of what each store ran, and a next start
  where the last left off. `tests/flows.rs` checks the rules with no window, and `tests/views.rs` runs the whole app over real windows. What
  the app has to write by hand is marked `Workaround:` in its code. `mise run showcase` runs it on the desktop.

### Changed

- **Breaking:** `Panicked` becomes `Error`, a `#[non_exhaustive]` enum of why work on the pool has no result of its own:
  `Panicked`, `Refused` or `Dropped`. `Background::run` used to send nothing when the pool refused a job or let go of it before it
  ran, which could leave a page loading for good. Now each call sends one result. Its `then` takes `Result<T, Error>`. Where a
  feature named `Panicked`, write `Error`. Match `Err(_)` to handle every reason alike, and build `Err(Error::Panicked)` in tests.
  `Error` implements `Display` and `std::error::Error`, so `?` carries it into an app's own error.
- `tokio` is no longer pinned exactly. The crates take `tokio` 1.53 or any later 1.x, so an app can move its own `tokio` on its own.
- **Breaking:** `Feature` requires `type Output`. A feature that tells no one adds `type Output = Never;`. Its `reduce` signature can
  stay as it is.
- **Breaking:** `step` returns `Step { effects, outputs }`. Where a test used the result as a list of effects, read `.effects`.
- **Breaking:** `Background`, `Job`, `Dropping`, `Listening`, `Waker`, `on_watch` and `on_wake` move to `spiderland`. They need
  one thing from a toolkit, a way to run a future on the UI thread, and the new `EventLoop` trait names it. `spiderland-slint`
  keeps `Background`, `on_watch` and `on_wake` on its `Slint` loop, so those paths stay. Import `Job`, `Dropping`, `Listening`
  and `Waker` from `spiderland`. `spiderland` now depends on `tokio`'s channels.
- **Breaking:** `Feature` requires `type Subscription`. A feature that wants no subscriptions adds `type Subscription = Never;`.
- **Breaking:** `CheckedSlice` checks its key only under `spiderland`'s new `testing` feature, not in every debug build, so a
  normal build no longer makes every value on every render. Nothing fails to build. To keep the check in tests, add
  `spiderland = { ..., features = ["testing"] }` to the `[dev-dependencies]` of each package whose tests show such a slice.
- **Breaking:** `Store::with_state` exists only under the `testing` feature, as a real feature's changes go through its reducer.
  A test that calls it lists `spiderland = { ..., features = ["testing"] }` in its package's `[dev-dependencies]`. Library code
  that called it sends an intent instead, and reads through `Store::try_read`.
- **Breaking:** `Closing` knows which child answered. A child that answers twice is heard once, an answer from a child not
  asked changes nothing, and `hear` returns the result only for the answer that completes the wait. `Closing::asking` takes
  the children's keys, and `Closing::hear` the key of the child answering. Where a parent asked `Closing::asking(n)`, ask
  `Closing::asking(0..n)`, and have each child's answer carry its index to `hear(child, answer)`. `Closing` is no longer `Copy`.
- **Breaking:** `Lifecycle` has `CloseCalledOff`, for a parent to tell its children that the window stays after it asked to
  close, so a child that stopped something to close starts it again. A `match` on `Lifecycle` needs an arm for it.
- **Breaking:** `Seen` is `#[non_exhaustive]`, and has `State` and `Output` as well as `Intent` and `Effect`. A `match` on it
  ends with a `_` arm. An `if let` needs no change. Its `Debug` asks the state and the output to be `Debug` too.

### Removed

- **Breaking:** `spiderland`'s `nightly` feature, which let a feature leave out `type Subscription` on a nightly compiler. It saved
  one line per feature, and cost a second lint and test run. Drop `features = ["nightly"]`, and add `type Subscription = Never;`
  to each feature that left it out.
- **Breaking:** `spiderland-phrases`, `spiderland-phrases-build`, and `spiderland-slint`'s `select_language`. The framework has
  no words of its own, so how an app shows and translates its words is the app's. An app that used them keeps their code as
  its own crate, or takes a translation library. Where it called `select_language`, it picks the language itself: read the
  user's locale, choose the catalog for it, select that for the Rust words, and call `slint::select_bundled_translation` for the
  views. `spiderland-slint` no longer depends on `sys-locale`.

### Fixed

- `spiderland`: `Progress` keeps only the latest report until the UI thread takes it. Every report used to wait in a queue, and
  the reports were merged only as the UI thread took them, so work that reported often while the UI thread was busy kept every
  report it had made.
- `spiderland`: `callbacks!` evaluates its view and store expressions once. It used to evaluate them again for each line, so
  an expression such as `ui.global::<Page>()` ran once per callback.
- `spiderland`: `Slice::update`, `Slice::show` and `CheckedSlice::update` keep the new key only once the part has been shown. A
  panic while showing used to leave the key, so the next call with the same key was skipped and the view kept what it had.
- `spiderland`: the docs of `Background`, `wait`, `finish` and `EventLoop` say what is promised. Nothing the caller holds stops
  `wait` or `finish`, but every result comes back through the event loop, and none arrives once that loop has stopped.
  `Background` says how to stop an app without losing a write's outcome.
- `spiderland`: `Tracked`'s docs say that it sees only changes made through `&mut`, so a `Cell` or a shared `Rc<RefCell<_>>`
  inside one changes with no new version. `Slice::show` no longer claims that an input can't be left out of its key at all.
- `spiderland`: the example of a window that closes once its editor has saved is on `Lifecycle`, where the API docs show it.
- `spiderland-slint`: `testing::idle` and `testing::until` come back only once nothing more is due. `until` used to settle once
  more after `done` held, and that could start more work, so `idle` could return with work still out. `settle` goes in rounds of
  due timers and queued events until a round runs nothing, so a timer an event sets to fire at once fires in the same call.

[Unreleased]: https://github.com/sengokudaikon/spiderland/commits/main
