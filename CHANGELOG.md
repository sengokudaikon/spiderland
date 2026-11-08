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
  it on the same inputs, so none can be left out.
- `spiderland`: `Store::observe` and `Seen`, which show each intent just before it is reduced and each effect just before it is
  performed. The observer gets a reference, so nothing is copied and nothing needs `Debug`.
- `spiderland`: `Intents::send_all` and `Store::send_all`, which send several intents as one batch. All of them are reduced before
  the render and the effects, so a snapshot and the answer that came with it are never shown or acted on apart.
- `spiderland`: `Feature::Subscription`, `Feature::subscriptions` and `Host::subscribe`, so a feature says what runs while its state
  wants it, and `subscriptions` to test that with no host. After each batch's render the store starts new keys and drops the ones
  no longer wanted, whether or not the host rendered anything.
- `spiderland`: a `nightly` feature, off by default, under which `type Subscription` defaults to `Never`.
- `spiderland`: `Panicked` implements `Display` and `std::error::Error`, so `?` carries it into an app's own error.
- `spiderland`, `spiderland-slint`: `Dropping`, `Listening`, and the timers `every` and `after` return, are `#[must_use]`. A guard
  written as a plain statement is dropped at once and stops what it guards before it runs, so that now warns.
- All crates: every public type implements `Debug`, and every public item has docs. `missing_docs` and
  `missing_debug_implementations` keep it so.
- `spiderland`: `TestStore`, which runs the whole loop in a test on a host that keeps what the store asks of it. Each effect waits
  for the test to take it or answer it with intents, and one left waiting when the store is dropped fails the test.
  `TestStore::rendering` keeps what a function of the state makes of each render, and `TestStore::take_outputs` gives what the
  store told others.
- `spiderland-slint`: `WindowHost` and `Windowed`, so a host is handed its window rather than upgrading a `Weak` itself. `render`
  gets the window and isn't called once it is gone. `perform` always runs, with the window if it is still there, so a write's
  outcome is still heard.

### Changed

- **Breaking:** `Feature` requires `type Output`. A feature that tells no one adds `type Output = Never;`. Its `reduce` signature can
  stay as it is.
- **Breaking:** `step` returns `Step { effects, outputs }`. Where a test used the result as a list of effects, read `.effects`.
- **Breaking:** `Background`, `Job`, `Dropping`, `Listening`, `Waker`, `on_watch` and `on_wake` move to `spiderland`. They need
  one thing from a toolkit, a way to run a future on the UI thread, and the new `EventLoop` trait names it. `spiderland-slint`
  keeps `Background`, `on_watch` and `on_wake` on its `Slint` loop, so those paths stay. Import `Job`, `Dropping`, `Listening`
  and `Waker` from `spiderland`. `spiderland` now depends on `tokio`'s channels, at the same pinned version.
- **Breaking:** `Feature` requires `type Subscription`. On a nightly compiler, turn on `spiderland`'s `nightly` feature and change
  nothing else. On stable, a feature that wants no subscriptions adds `type Subscription = Never;`.

### Removed

- **Breaking:** `spiderland-phrases`, `spiderland-phrases-build`, and `spiderland-slint`'s `select_language`. The framework has
  no words of its own, so how an app shows and translates its words is the app's. An app that used them keeps their code as
  its own crate, or takes a translation library. Where it called `select_language`, it picks the language itself: read the
  user's locale, choose the catalog for it, select that for the Rust words, and call `slint::select_bundled_translation` for the
  views. `spiderland-slint` no longer depends on `sys-locale`.

### Fixed

- `spiderland`: `callbacks!` evaluates its view and store expressions once. It used to evaluate them again for each line, so
  an expression such as `ui.global::<Page>()` ran once per callback.

[Unreleased]: https://github.com/sengokudaikon/spiderland/commits/main
