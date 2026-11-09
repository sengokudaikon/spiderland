# spiderland

One-way UI state for Rust desktop apps. The core has no toolkit in it. Slint support is a separate crate.

```text
view callback ─┐
finished work ─┼─ Intent ─► reduce ─► State ─► Host::render ─► view
timer tick ────┘               └────► Effect ─► Host::perform ─► finished work
```

A `Feature` is pure: `reduce` changes the state and lists effects as plain data. A `Host` renders the state and performs the
effects. A `Store` runs the two on the UI thread.

## Example

```rust
use spiderland::{Effects, Feature, Never};

struct Counter;

enum Intent {
    Add(i32),
    Save,
}

enum Effect {
    Write(i32),
}

#[derive(Default)]
struct State {
    count: i32,
}

impl Feature for Counter {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = Never;

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
        match intent {
            Intent::Add(by) => state.count += by,
            Intent::Save => effects.push(Effect::Write(state.count)),
        }
    }
}

// Rules are tested with no window.
let mut state = State::default();
spiderland::step::<Counter>(&mut state, Intent::Add(2));
let step = spiderland::step::<Counter>(&mut state, Intent::Save);
assert!(matches!(step.effects[..], [Effect::Write(2)]));
```

The `spiderland` crate docs add a `Host` and a `Store` to this.

## Crates

| Crate | What it is | Depends on |
|---|---|---|
| `spiderland` | One-way UI state | `tokio`'s channels |
| `spiderland-slint` | What a Slint host needs | `spiderland`, Slint, `tokio`'s channels |

Feature crates take `spiderland` and stay free of Slint. The framework creates no tasks: the app gives `Background` a way to
start a job on its own pool. It has no words of its own either: what a page shows, and how it is translated, is the app's.

## Install

The crates are not on crates.io. Take them from git:

```toml
[dependencies]
spiderland = { git = "https://github.com/sengokudaikon/spiderland" }
spiderland-slint = { git = "https://github.com/sengokudaikon/spiderland" }
```

The crates build on stable.

`spiderland-slint` pins `slint = "=1.18.1"`, and your app has to build against the same version. Both crates take `tokio` 1.53 or
any later 1.x, for its channels.

## Docs

- [Features](docs/features.md): everything the crates provide.
- [Writing a page](docs/writing-a-page.md): a feature, its host and its view, step by step.
- [Showcase](showcase/src/lib.rs): a small document library built with both crates, with several windows, saves, an export and a
  search. Its tests are the framework's acceptance test, and its code marks each workaround for what the framework doesn't do yet.
  `mise run showcase` runs it on the desktop.
- [Changelog](CHANGELOG.md): what changed in each version, and what a breaking change asks of you.

## License

Either [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

Slint has its own license, which this one does not change. An app using `spiderland-slint` must take Slint under GPL-3.0-only,
its royalty-free license, or its commercial license.

Unless you state otherwise, any contribution you submit for inclusion, as defined in the Apache-2.0 license, is dual licensed as
above, with no additional terms or conditions.
