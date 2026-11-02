# Writing a page

A page has five parts. A complete small feature is in the `spiderland` crate's docs.

## 1. Feature

Put it in a crate without Slint.

- State is plain data.
- Anything that touches a disk, a process or a worker is an `Effect`. Its result comes back as an `Intent`.
- Anything from outside, such as a worker's snapshot, arrives as an intent and is kept in the state.
- First loads go in `Feature::start`, not in intents the host sends after building the store.
- To tell another feature something, `emit` an `Output`. Whoever builds the two stores connects them with `Store::forward`, straight
  after building them.

## 2. `present`

Put it next to the feature.

- It takes only what it reads, and returns plain structs.
- Keep what it reads in `Tracked`, so a render can tell when it needs to run again.

## 3. Host

Put it in the crate with the window.

- `callbacks!` maps view callbacks to intents, and does nothing else.
- `render` runs each `Slice`, and syncs `Rows` and `Subscriptions`.
- `perform` matches on the effect.

## 4. View

- Components take properties and callbacks.
- Only the page root reads the page's global.
- What only the view cares about, such as hover, highlights and scroll, stays in Slint.

## 5. Tests

- Test reducer rules with `step`, with no window.
- Test behaviour across the view with `spiderland_slint::testing`: the real window on real background work.

## Watch out for

**Writes**

A write's outcome must reach a store that is still alive. `Background` runs accepted work to its end, but delivers nowhere once
its store is gone. Never tie a write to a scope that can close first.

**Panics**

A panic in background work arrives as `Err(Panicked)`, and each feature decides what it means. The panic then goes on to the app's
pool, which can keep a record of it.

**Slice keys**

A `Slice` key that leaves out an input shows stale values. Keep the key beside the function it guards.

**Slint names**

- Don't name a callback after a value some binding inside the component uses: a callback `start` breaks `alignment: start`.
- A global can't be named `Text`, because that is the element. Name a page's words `HomeText`.
