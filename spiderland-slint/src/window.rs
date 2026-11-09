use slint::{CloseRequestResponse, ComponentHandle};
use spiderland::{Feature, Host, Intents, Store, View};
use std::any::{Any, type_name};
use std::cell::RefCell;
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

/// A [`View`] that shows a store's state in one Slint window, so that several windows can show one store. Attach it with
/// [`attach_window`], which ties the view to its window and lets the store decide whether the window closes, or with
/// [`Store::attach`](spiderland::Store::attach) through [`Windowed`], which holds the window weakly:
///
/// ```no_run
/// # slint::slint! { export component EditorWindow { in property <int> count; } }
/// # use spiderland::{Effects, Feature, Host, Intents, Never, Store};
/// use spiderland_slint::{WindowView, attach_window};
/// # struct Counter;
/// # impl Feature for Counter {
/// #     type State = i32;
/// #     type Intent = i32;
/// #     type Effect = Never;
/// #     type Output = Never;
/// #     type Subscription = Never;
/// #     fn reduce(count: &mut i32, by: i32, _: &mut Effects<Never>) {
/// #         *count += by;
/// #     }
/// # }
/// # struct Runner;
/// # impl Host<Counter> for Runner {
/// #     fn perform(&mut self, effect: Never, _: &Intents<i32>) {
/// #         match effect {}
/// #     }
/// # }
///
/// struct Editor;
///
/// impl WindowView<Counter> for Editor {
///     type Window = EditorWindow;
///
///     fn render(&mut self, window: &EditorWindow, count: &i32, _: &Intents<i32>) {
///         window.set_count(*count);
///     }
/// }
///
/// let store = Store::new(0, Runner);
/// // Two windows on one store, each closing on its own.
/// for _ in 0..2 {
///     let window = EditorWindow::new().unwrap();
///     attach_window(&store, &window, Editor, |_| None);
///     window.show().unwrap();
/// }
/// ```
///
/// The view renders whether or not its window shows: a minimized window is still open, and shows the current state when it comes
/// back. It is gone once its window is dropped, or once [`WindowView::is_gone`] says so.
pub trait WindowView<F: Feature>: 'static {
    /// The window the view shows the state in.
    type Window: ComponentHandle + 'static;

    /// [`View::render`], with the window. Not called once the window is gone.
    fn render(&mut self, window: &Self::Window, state: &F::State, intents: &Intents<F::Intent>);

    /// True once the view is done with its window for good, such as after it hid it when the store closed. The store then lets
    /// it go. A window that is dropped takes its view with it anyway, so a view whose window goes that way leaves this out.
    fn is_gone(&self) -> bool {
        false
    }
}

/// A [`WindowHost`] and its window, held weakly, as a [`Host`] a [`Store`](spiderland::Store) can run. Or a [`WindowView`] and
/// its window, as a [`View`] that attaches to a store.
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

impl<F, W, V> View<F> for Windowed<W, V>
where
    F: Feature,
    W: ComponentHandle + 'static,
    V: WindowView<F, Window = W>,
{
    fn render(&mut self, state: &F::State, intents: &Intents<F::Intent>) {
        if let Some(window) = self.window.upgrade() {
            self.host.render(&window, state, intents);
        }
    }

    fn is_gone(&self) -> bool {
        self.host.is_gone() || self.window.upgrade().is_none()
    }
}

/// Lets a store decide whether `window` closes when the user asks it to. `ask` says what to do: `None` lets the window close, and
/// `Some(intent)` keeps it shown and sends the intent, so the store can ask about unsaved work first, and close it later. It suits
/// a window whose host shows it. For a view attached to a store, [`attach_window`] does this and detaches the view as it closes.
///
/// ```no_run
/// # slint::slint! { export component MainWindow { } }
/// # use spiderland::{Effects, Feature, Host, Intents, Never, Store};
/// # struct Page;
/// # enum Intent { CloseRequested }
/// # impl Feature for Page {
/// #     type State = ();
/// #     type Intent = Intent;
/// #     type Effect = Never;
/// #     type Output = Never;
/// #     type Subscription = Never;
/// #     fn reduce(_: &mut (), _: Intent, _: &mut Effects<Never>) {}
/// # }
/// # struct Runner;
/// # impl Host<Page> for Runner {
/// #     fn perform(&mut self, effect: Never, _: &Intents<Intent>) {
/// #         match effect {}
/// #     }
/// # }
/// # let store = Store::new((), Runner);
/// # let window = MainWindow::new().unwrap();
/// // The page asks about unsaved work first, and closes the window itself.
/// spiderland_slint::ask_before_closing(&window, &store.intents(), || Some(Intent::CloseRequested));
/// ```
pub fn ask_before_closing<W: ComponentHandle, I: 'static>(window: &W, intents: &Intents<I>, ask: impl Fn() -> Option<I> + 'static) {
    let intents = intents.clone();
    window.window().on_close_requested(move || match ask() {
        Some(intent) => {
            intents.send(intent);
            CloseRequestResponse::KeepWindowShown
        }
        None => CloseRequestResponse::HideWindow,
    });
}

/// Attaches `view`, in `window`, to `store`, for as long as the window is open. It shows the state at once. When the user asks to
/// close the window, `ask` is handed the store and says what to do, as for [`ask_before_closing`]: `None` lets the window close,
/// and its view leaves the store at once. `Some(intent)` keeps it shown and sends the intent. A window that is dropped takes its
/// view with it.
///
/// The window doesn't keep the store alive, so whoever built the store keeps it, such as a parent's
/// [`Children`](spiderland::Children). Once the store is gone, the window simply closes.
///
/// With several windows on one store, the last one can ask first, such as about unsaved edits, while the others simply close:
///
/// ```no_run
/// # slint::slint! { export component EditorWindow { } }
/// # use spiderland::{Effects, Feature, Host, Intents, Never, Store};
/// # use spiderland_slint::WindowView;
/// # struct Document;
/// # enum Intent { CloseRequested }
/// # impl Feature for Document {
/// #     type State = ();
/// #     type Intent = Intent;
/// #     type Effect = Never;
/// #     type Output = Never;
/// #     type Subscription = Never;
/// #     fn reduce(_: &mut (), _: Intent, _: &mut Effects<Never>) {}
/// # }
/// # struct Runner;
/// # impl Host<Document> for Runner {
/// #     fn perform(&mut self, effect: Never, _: &Intents<Intent>) {
/// #         match effect {}
/// #     }
/// # }
/// # struct Editor;
/// # impl WindowView<Document> for Editor {
/// #     type Window = EditorWindow;
/// #     fn render(&mut self, _: &EditorWindow, _: &(), _: &Intents<Intent>) {}
/// # }
/// # let store = Store::new((), Runner);
/// let window = EditorWindow::new().unwrap();
/// spiderland_slint::attach_window(&store, &window, Editor, |store| (store.views() == 1).then_some(Intent::CloseRequested));
/// window.show().unwrap();
/// ```
pub fn attach_window<F, W, V>(store: &Store<F>, window: &W, view: V, ask: impl Fn(&Store<F>) -> Option<F::Intent> + 'static)
where
    F: Feature,
    W: ComponentHandle + 'static,
    V: WindowView<F, Window = W>,
{
    // The close handler keeps the attachment, so it lives as long as the window, and the handler can end it.
    let attached = RefCell::new(Some(store.attach(Windowed::new(window, view))));
    let store = store.downgrade();
    window
        .window()
        .on_close_requested(move || match store.upgrade().and_then(|store| Some((ask(&store)?, store))) {
            Some((intent, store)) => {
                store.send(intent);
                CloseRequestResponse::KeepWindowShown
            }
            None => {
                drop(attached.borrow_mut().take());
                CloseRequestResponse::HideWindow
            }
        });
}

impl<W: ComponentHandle, H> fmt::Debug for Windowed<W, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Windowed")
            .field("window", &type_name::<W>())
            .field("gone", &self.window.upgrade().is_none())
            .finish_non_exhaustive()
    }
}
