# spiderland

One-way UI state for Rust desktop apps, and translatable UI text. The core has no toolkit in it. Slint support is a separate crate.

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
| `spiderland-slint` | What a Slint host needs | `spiderland`, `spiderland-phrases`, Slint, `tokio`'s channels |
| `spiderland-phrases` | Words and numbers, ready for translation | Nothing but one Windows call |
| `spiderland-phrases-build` | Build-time catalogs and the gettext template | `rspolib` |

Feature crates take `spiderland` and `spiderland-phrases`, and stay free of Slint. The framework creates no tasks: the app gives
`Background` a way to start a job on its own pool.

## Install

The crates are not on crates.io. Take them from git:

```toml
[dependencies]
spiderland = { git = "https://github.com/sengokudaikon/spiderland" }
spiderland-slint = { git = "https://github.com/sengokudaikon/spiderland" }
```

The crates build on stable. On a nightly compiler, turn on `spiderland`'s `nightly` feature, and a feature that wants no
subscriptions can leave out `type Subscription = Never;`:

```toml
spiderland = { git = "https://github.com/sengokudaikon/spiderland", features = ["nightly"] }
```

`spiderland` pins `tokio = "=1.53.3"`, and `spiderland-slint` also pins `slint = "=1.18.1"`. Your app has to build against the same
versions.

## Docs

- [Features](docs/features.md): everything the crates provide.
- [Writing a page](docs/writing-a-page.md): a feature, its host and its view, step by step.
- [Translation](docs/translation.md): words in views and in Rust, and how they reach translators.
- [Changelog](CHANGELOG.md): what changed in each version, and what a breaking change asks of you.

## License

Either [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

Slint has its own license, which this one does not change. An app using `spiderland-slint` must take Slint under GPL-3.0-only,
its royalty-free license, or its commercial license.

Unless you state otherwise, any contribution you submit for inclusion, as defined in the Apache-2.0 license, is dual licensed as
above, with no additional terms or conditions.
