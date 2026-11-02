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

[Unreleased]: https://github.com/sengokudaikon/spiderland/commits/main
