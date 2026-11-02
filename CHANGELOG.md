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
- `spiderland-slint`: `Background`, `every`, `after`, `on_watch`, `on_wake`, `Rows`, `select_language` and `testing`.
- `spiderland-phrases`: `phrases!`, `Phrase`, `Plural`, `list`, `Translations` and `locale`.
- `spiderland-phrases-build`: `bundle_catalogs` and `Template`.
- `spiderland`: `Feature::Output`, `Effects::emit` and `Store::forward`, so a feature can tell others something. An output goes out
  after its batch's render and effects, and one emitted before there is a listener reaches no one.
- `spiderland`: `Feature::start`, which sets up the first state and asks for effects before the first render, and `start` to test it.
- `spiderland`: `Intents::map`, so a child's host can run inside its parent's store.
- `spiderland`: `Lifecycle`, `CloseAnswer` and `Closing`, so a parent can show and hide its children and ask whether they can close.

### Changed

- **Breaking:** `Feature` requires `type Output`. A feature that tells no one adds `type Output = Never;`. Its `reduce` signature can
  stay as it is.
- **Breaking:** `step` returns `Step { effects, outputs }`. Where a test used the result as a list of effects, read `.effects`.

[Unreleased]: https://github.com/sengokudaikon/spiderland/commits/main
