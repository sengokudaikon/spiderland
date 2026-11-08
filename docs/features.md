# Features

## State: `spiderland`

| Item | What it does |
|---|---|
| `Feature` | The pure half: `reduce(state, intent, effects)`. |
| `Host` | The impure half: `render` the state, `perform` the effects. |
| `Store` | Runs the two on the UI thread, and queues intents sent while it works. |
| `Intents::send_all`, `Store::send_all` | Send several intents as one batch, so the render and the effects come after all of them. |
| `Store::ask` | Sends an intent and reads what it did, for a view callback that returns a value. |
| `Store::try_read` | Answers a view's question from the state, for a pure callback that a binding calls. |
| `Store::with_state` | Changes the state directly, for a test or a capture to set up. Nothing renders until the next intent. Behind the `testing` feature. |
| `Feature::Output`, `Effects::emit` | Let a reducer tell whoever listens. `Never` is for a feature that tells no one. |
| `Store::forward` | Turns one store's outputs into another store's intents. |
| `Store::connect`, `Connection` | Forwards until the returned `Connection` is dropped, for a connection between stores that both go on. |
| `Store::share` | Sends a projection of the state to another store now, and again whenever it changes, so a store built later starts current. |
| `Store::build`, `Wiring` | Builds a store without starting it, so listeners and observers wired on it hear its start too. `Wiring::start` starts it. |
| `Store::observe`, `Seen` | Shows each intent and effect as the store runs it, for a trail of what led to a fault. |
| `Lifecycle`, `CloseAnswer`, `Closing` | Let a parent show and hide its children, ask whether they can close, and tell them when the window stays after all. |
| `Feature::start` | Sets up the first state and asks for effects, before the first render. |
| `Intents::map` | Makes a sender of a child's type, so a child's host can run inside its parent's store. |
| `Pulse` | A one-shot command to a view with properties and no events, such as "focus this". It is plain data for the state. |
| `Tracked`, `Slice` | Redo only the parts of a render whose inputs changed. A `Version` names one state of a `Tracked`. |
| `Slice::show`, `Input`, `Inputs` | Makes a part of a render with a plain `fn` of what it reads, and keys it on the same inputs, so none is left out by mistake. |
| `CheckedSlice` | A `Slice` that fails when its key misses an input. It checks only under the `testing` feature, for tests. |
| `Feature::subscriptions`, `Host::subscribe` | The feature names what its state wants running, the host starts it, and the store stops it once it isn't wanted. |
| `Subscriptions` | Keeps things running by key. The store uses it for a feature's subscriptions. |
| `Background::new`, `Job` | Takes the app's way to start a `Job` on its pool, which says whether the pool took it. |
| `Background::run` | Runs work on the app's pool and sends the result back as an intent. Work that panics, or that the pool refuses or lets go of, arrives as `Err` of an `Error`. |
| `Error` | Why work on the pool has no result of its own: it panicked, or the pool refused it or let go of it. |
| `Background::wait` | Awaits an answer on the UI thread, with no pool thread held, and sends it back as an intent. |
| `Background::local` | Awaits work on the UI thread that can be dropped at any moment, even from inside its own result. `Dropping` is its guard. |
| `Background::finish` | Awaits work on the UI thread that nothing the caller holds can stop, for a result that must be heard. |
| `Background::pending` | Counts work whose result hasn't come back yet, for a test to wait on. |
| `on_watch`, `on_wake` | Let a service on another thread tell a feature that something changed. `Listening` is their guard, and `on_wake`'s `Waker` fires it. |
| `EventLoop` | The one thing `Background` and the listeners need from a toolkit: running a future on the UI thread. |
| `Effects::scoped` | Runs a child scope's reducer inside its parent's. |
| `Effects::detached` | A list no store performs, to run code shared with the reducer outside it. |
| `step`, `Step` | Runs one reducer step in a test, with no host, and returns its effects and outputs. |
| `start` | Runs `Feature::start` in a test, with no host, and returns its effects. |
| `subscriptions` | Lists what a state wants running, in a test, with no host. |
| `TestStore` | Runs the whole loop in a test. Each effect waits for the test to take it or answer it, and one left waiting fails the test. It keeps renders and outputs too. |
| `callbacks!` | Maps a view's callbacks to intents, one per line. |

## Slint host: `spiderland-slint`

| Item | What it does |
|---|---|
| `Slint` | Slint's event loop, as an `EventLoop`. |
| `WindowHost`, `Windowed` | A host handed its window on each call. `render` isn't called once the window is gone, and `perform` always runs, with the window if it is still there. |
| `Background`, `on_watch`, `on_wake` | `spiderland`'s, on Slint's event loop. |
| `every` | Sends an intent repeatedly until its timer is dropped. |
| `after` | Sends an intent once after a delay. Keep the timer in one place and it debounces. |
| `Rows` | A list model updated row by row, so a row being dragged or typed in survives its neighbours' changes. |
| `Rows::sync_by` | Matches rows by key, so a row's focus follows its item when rows before it come or go. |
| `InnerRows` | Keeps a list inside each row, with one stable model per row, and works around a Slint bug that can lose its changes. |
| `testing` | The real window with no display, driven by real input. Behind the `testing` feature. |
