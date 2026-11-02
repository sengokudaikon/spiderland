# Features

## State: `spiderland`

| Item | What it does |
|---|---|
| `Feature` | The pure half: `reduce(state, intent, effects)`. |
| `Host` | The impure half: `render` the state, `perform` the effects. |
| `Store` | Runs the two on the UI thread, and queues intents sent while it works. |
| `Store::ask` | Sends an intent and reads what it did, for a view callback that returns a value. |
| `Feature::Output`, `Effects::emit` | Let a reducer tell whoever listens. `Never` is for a feature that tells no one. |
| `Store::forward` | Turns one store's outputs into another store's intents. |
| `Lifecycle`, `CloseAnswer`, `Closing` | Let a parent show and hide its children, and ask whether they can close. |
| `Feature::start` | Sets up the first state and asks for effects, before the first render. |
| `Intents::map` | Makes a sender of a child's type, so a child's host can run inside its parent's store. |
| `Tracked`, `Slice` | Redo only the parts of a render whose inputs changed. |
| `CheckedSlice` | A `Slice` that fails a test build when its key misses an input. |
| `Subscriptions` | Keep timers and watchers running for as long as the state wants them. |
| `Effects::scoped` | Runs a child scope's reducer inside its parent's. |
| `step` | Runs one reducer step in a test, with no host, and returns its effects and outputs. |
| `start` | Runs `Feature::start` in a test, with no host, and returns its effects. |
| `callbacks!` | Maps a view's callbacks to intents, one per line. |

## Slint host: `spiderland-slint`

| Item | What it does |
|---|---|
| `Background::run` | Runs work on the app's pool and sends the result back as an intent. A panic arrives as `Err(Panicked)`. |
| `Background::wait` | Awaits an answer on the UI thread, with no pool thread held, and sends it back as an intent. |
| `Background::local` | Awaits work on the UI thread that can be dropped at any moment, even from inside its own result. |
| `Background::finish` | Awaits work on the UI thread whose result must always be heard. |
| `on_watch`, `on_wake` | Let a service on another thread tell a feature that something changed. |
| `every` | Sends an intent repeatedly until its timer is dropped. |
| `after` | Sends an intent once after a delay. Keep the timer in one place and it debounces. |
| `Rows` | A list model updated row by row, so a row being dragged or typed in survives its neighbours' changes. |
| `select_language` | Picks the user's language for the views and the phrases alike. |
| `testing` | The real window with no display, driven by real input. Behind the `testing` feature. |

## Words: `spiderland-phrases`

| Item | What it does |
|---|---|
| `phrases!` | Declares words by name, beside the code that uses them. |
| `Phrase` | One piece of text. `Phrase::with` fills in named values. |
| `Plural` | A count's forms. `{n}` picks the form. |
| `list` | Joins items as the language joins them. |
| `Translations` | Checks the bundled catalogs and selects the language shown. |
| `locale` | Number symbols for the language shown. |

## Translation build: `spiderland-phrases-build`

| Item | What it does |
|---|---|
| `bundle_catalogs` | Turns `.po` files into a table of catalogs, in a build script. |
| `Template` | Collects every `phrases!` entry and `@tr` into one `.pot`, and checks that it is current. |
