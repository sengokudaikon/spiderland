# Features

## State: `spiderland`

| Item | What it does |
|---|---|
| `Feature` | The pure half: `reduce(state, intent, effects)`. |
| `Host` | The impure half: `render` the state, `perform` the effects. A host whose views attach leaves `render` out. |
| `View`, `Store::attach`, `Attached` | A view that attaches to a live store and leaves it while the store goes on. It shows the state at once, so a window opened later needs no catching up. A store has one host and any number of views. |
| `Store::views` | Counts the views attached and not gone, such as to let one of several windows close on its own. |
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
| `Store::observe`, `Seen` | Shows each step as the store runs it: each intent, the state after it, each effect and each output. For a trail of what led to a fault, and what changed on the way. |
| `Lifecycle`, `CloseAnswer`, `Closing` | Let a parent show and hide its children, ask whether they can close, and tell them when the window stays after all. |
| `Children`, `Store::children`, `Wiring::children` | Child stores a parent keeps, one for each key its state names: a stack of pages, a slot, or a set of documents. Each is built as its key appears and dropped once it goes, so an answer for a page closed goes nowhere. |
| `Standing`, `Child` | Where the parent's state puts each child: shown, hidden or asked to close. Each change reaches the child as a `Lifecycle` event, through `Child::tell`, which a store whose intent is `From<Lifecycle>` has. |
| `Store::downgrade`, `WeakStore` | Holds a store without keeping it alive, such as from a window's handler. |
| `Feature::start` | Sets up the first state and asks for effects, before the first render. |
| `Intents::map` | Makes a sender of a child's type, so a child's host can run inside its parent's store. |
| `Pulse` | A one-shot command to a view with properties and no events, such as "focus this". It is plain data for the state. |
| `Fired` | What one view has shown of a `Pulse`: a serial that starts at 0 for that view, so a view that attaches later doesn't act on an old fire. |
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
| `Background::stoppable`, `Stop`, `Progress`, `Stopping` | Runs work on the app's pool that stops when its guard is dropped, such as by a subscription, and reports progress as intents. The work checks `Stop::is_stopped`, or waits on `Stop::stopped` alongside what else it waits on. |
| `Background::pending` | Counts work whose result hasn't come back yet, for a test to wait on. |
| `Latest`, `Asked` | Numbers requests in the state, so that only the answer to the latest one counts. |
| `OneAtATime` | Keeps work one at a time in the state, such as saves: the latest asked for waits while one runs. |
| `on_watch`, `on_wake` | Let a service on another thread tell a feature that something changed. `Listening` is their guard, and `on_wake`'s `Waker` fires it. |
| `EventLoop` | The one thing `Background` and the listeners need from a toolkit: running a future on the UI thread. |
| `Effects::scoped` | Runs a child scope's reducer inside its parent's. |
| `Effects::reduce_child`, `Effects::start_child` | Run a child feature inside its parent's store. Its effects come back wrapped, and what it emits comes back to the parent's `reduce`. |
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
| `WindowView` | A view in one window, attached through `Windowed`. It renders while its window is minimized or hidden, and is gone once the window is dropped or the view says so. |
| `attach_window` | Attaches a `WindowView` for as long as its window is open. When the user closes the window, the store decides: it simply closes, which detaches its view, or stays and hears an intent. The window doesn't keep the store alive. |
| `ask_before_closing` | Lets a store decide whether a window its host shows closes: the window simply closes, or stays and sends an intent, such as to ask about unsaved edits. |
| `Background`, `on_watch`, `on_wake` | `spiderland`'s, on Slint's event loop. |
| `every` | Sends an intent repeatedly until its timer is dropped. |
| `after` | Sends an intent once after a delay. Keep the timer in one place and it debounces. |
| `Rows` | A list model updated row by row, so a row being dragged or typed in survives its neighbours' changes. |
| `Rows::sync_by` | Matches rows by key, so a row's focus follows its item when rows before it come or go. |
| `InnerRows` | Keeps a list inside each row, with one stable model per row, and works around a Slint bug that can lose its changes. |
| `testing` | The real window with no display, driven by real input. Behind the `testing` feature. |
