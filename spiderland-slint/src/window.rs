use slint::ComponentHandle;
use spiderland::{Feature, Host, Intents};
use std::any::{Any, type_name};
use std::fmt;

/// A [`Host`] whose view lives in one Slint window. Run it in a [`Store`](spiderland::Store) through [`Windowed`], which holds the
/// window.
///
/// A host can't hold its window strongly. The window keeps the view's callbacks, they keep the store, and the store keeps its
/// host, so none of them would be freed. [`Windowed`] holds a [`slint::Weak`] instead, and upgrades it once per call:
///
/// - `render` gets the window, and isn't called once the window is gone. Slint keeps a window alive while it shows, so it is gone
///   only once nothing holds it, and no later render could reach it.
/// - `perform` always runs, and gets the window if it is still there. A write's outcome must be heard whether or not anything is
///   left to show it.
/// - `subscribe` gets no window. What runs follows the state.
///
/// ```no_run
/// # slint::slint! { export component MainWindow { in property <int> count; } }
/// use spiderland::{Effects, Feature, Intents, Never, Store};
/// use spiderland_slint::{WindowHost, Windowed};
///
/// # struct Counter;
/// # impl Feature for Counter {
/// #     type State = i32;
/// #     type Intent = i32;
/// #     type Effect = Effect;
/// #     type Output = Never;
/// #     type Subscription = Never;
/// #     fn reduce(count: &mut i32, by: i32, effects: &mut Effects<Effect>) {
/// #         *count += by;
/// #         effects.push(Effect::Save(*count));
/// #     }
/// # }
/// enum Effect {
///     Save(i32),
/// }
///
/// struct Page;
///
/// impl WindowHost<Counter> for Page {
///     type Window = MainWindow;
///
///     fn render(&mut self, window: &MainWindow, count: &i32, _: &Intents<i32>) {
///         window.set_count(*count);
///     }
///
///     fn perform(&mut self, _: Option<&MainWindow>, Effect::Save(count): Effect, _: &Intents<i32>) {
///         // Saves whether or not the window is still there.
/// #       let _ = count;
///     }
/// }
///
/// let window = MainWindow::new().unwrap();
/// let store = Store::<Counter>::new(0, Windowed::new(&window, Page));
/// # drop(store);
/// ```
pub trait WindowHost<F: Feature>: 'static {
    /// The window the view lives in.
    type Window: ComponentHandle + 'static;

    /// [`Host::render`], with the window to show the state in.
    fn render(&mut self, window: &Self::Window, state: &F::State, intents: &Intents<F::Intent>);

    /// [`Host::perform`], with the window while there is one.
    ///
    /// A command to the view, such as "focus this", is best a [`Pulse`](spiderland::Pulse) in the state, which `render` shows.
    /// What can only be done here, such as giving a file dialog its parent, does nothing once the window is gone. Work that needs
    /// the window later, such as hiding it on the next turn of the event loop, keeps `window.as_weak()`.
    fn perform(&mut self, window: Option<&Self::Window>, effect: F::Effect, intents: &Intents<F::Intent>);

    /// [`Host::subscribe`]. A host whose feature wants no subscriptions leaves this out, and one whose feature wants some must
    /// write it, as there.
    fn subscribe(&mut self, key: &F::Subscription, intents: &Intents<F::Intent>) -> Box<dyn Any> {
        let _ = (key, intents);
        panic!("{} wants a subscription, and its host doesn't write `WindowHost::subscribe`", type_name::<F>())
    }
}

/// A [`WindowHost`] and its window, held weakly, as a [`Host`] a [`Store`](spiderland::Store) can run.
pub struct Windowed<W: ComponentHandle, H> {
    window: slint::Weak<W>,
    host: H,
}

impl<W: ComponentHandle, H> Windowed<W, H> {
    /// Runs `host` on `window`, holding the window weakly.
    pub fn new(window: &W, host: H) -> Self {
        Windowed { window: window.as_weak(), host }
    }
}

impl<F, W, H> Host<F> for Windowed<W, H>
where
    F: Feature,
    W: ComponentHandle + 'static,
    H: WindowHost<F, Window = W>,
{
    fn render(&mut self, state: &F::State, intents: &Intents<F::Intent>) {
        if let Some(window) = self.window.upgrade() {
            self.host.render(&window, state, intents);
        }
    }

    fn perform(&mut self, effect: F::Effect, intents: &Intents<F::Intent>) {
        let window = self.window.upgrade();
        self.host.perform(window.as_ref(), effect, intents);
    }

    fn subscribe(&mut self, key: &F::Subscription, intents: &Intents<F::Intent>) -> Box<dyn Any> {
        self.host.subscribe(key, intents)
    }
}

impl<W: ComponentHandle, H> fmt::Debug for Windowed<W, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Windowed")
            .field("window", &type_name::<W>())
            .field("gone", &self.window.upgrade().is_none())
            .finish_non_exhaustive()
    }
}
