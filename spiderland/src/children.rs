//! Child stores a parent keeps, one for each key its state names: a stack of pages, a slot, or a set of documents.

use crate::{Feature, Lifecycle, Store};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// Where a parent's state puts one of its [`Children`]. Each change reaches the child as a [`Lifecycle`] event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// On screen, such as the top page of a stack. Heard as [`Lifecycle::Shown`].
    Shown,
    /// Off screen, such as a page under another. Heard as [`Lifecycle::Hidden`].
    Hidden,
    /// Asked whether it can close. Heard as [`Lifecycle::CloseRequested`], and on leaving it as [`Lifecycle::CloseCalledOff`].
    Closing,
}

impl Standing {
    fn heard(self) -> Lifecycle {
        match self {
            Standing::Shown => Lifecycle::Shown,
            Standing::Hidden => Lifecycle::Hidden,
            Standing::Closing => Lifecycle::CloseRequested,
        }
    }

    /// What a child hears when its standing goes from `was` to `now`.
    fn events(was: Option<Standing>, now: Standing) -> Vec<Lifecycle> {
        match was {
            Some(was) if was == now => Vec::new(),
            Some(Standing::Closing) => vec![Lifecycle::CloseCalledOff, now.heard()],
            _ => vec![now.heard()],
        }
    }
}

/// One child a parent keeps in its [`Children`]: a store, or something of the app's that holds one, such as a page and its view.
pub trait Child: 'static {
    /// Tells the child where its parent's state puts it now. A child hears this as it is built, and each time its
    /// [`Standing`] changes.
    fn tell(&self, life: Lifecycle);
}

/// A store whose feature hears its lifecycle as an intent.
impl<C: Feature> Child for Store<C>
where
    C::Intent: From<Lifecycle>,
{
    fn tell(&self, life: Lifecycle) {
        self.send(life.into());
    }
}

/// The child stores a parent keeps, one for each key its state names. Make one, give a clone to whatever needs to reach a child,
/// such as the parent's host, and wire it with [`Wiring::children`](crate::Wiring::children).
///
/// After each batch, the parent's store asks its state which keys it wants and where each stands. It builds a child for each new
/// key, drops each child no longer named, and tells each one its [`Standing`] as it is built and whenever it changes. So which
/// children exist follows the state alone, as subscriptions do:
/// - A stack is a list of keys in the state: the last is shown and the rest hidden. Going back drops the top child.
/// - A slot is an `Option`. A set, such as the documents open, is a list whose children are all shown.
///
/// A child that is dropped stops what it runs, and what its work sends after that goes nowhere, because [`Intents`](crate::Intents)
/// don't keep a store alive. So an answer for a page no longer shown is let go with nothing to check.
///
/// Children answer through their outputs, which [`build`](crate::Wiring::children) forwards to the parent. A parent asking them
/// to close counts the answers with a [`Closing`](crate::Closing).
///
/// Pages on a stack, the last one shown:
///
/// ```
/// use spiderland::{Children, Effects, Feature, Host, Intents, Lifecycle, Never, Standing, Store};
///
/// /// A page, which hears where it stands.
/// struct Page;
/// impl Feature for Page {
///     type State = Vec<Lifecycle>;
///     type Intent = Lifecycle;
///     type Effect = Never;
///     type Output = Never;
///     type Subscription = Never;
///
///     fn reduce(heard: &mut Vec<Lifecycle>, life: Lifecycle, _: &mut Effects<Never>) {
///         heard.push(life);
///     }
/// }
///
/// /// The pages open, by name, the last one shown.
/// struct Pages;
/// enum Intent {
///     Open(&'static str),
///     Back,
/// }
/// impl Feature for Pages {
///     type State = Vec<&'static str>;
///     type Intent = Intent;
///     type Effect = Never;
///     type Output = Never;
///     type Subscription = Never;
///
///     fn reduce(pages: &mut Vec<&'static str>, intent: Intent, _: &mut Effects<Never>) {
///         match intent {
///             Intent::Open(page) => pages.push(page),
///             Intent::Back => drop(pages.pop()),
///         }
///     }
/// }
///
/// /// Each page's place on the stack.
/// fn stack(pages: &Vec<&'static str>) -> Vec<(&'static str, Standing)> {
///     let top = pages.len().saturating_sub(1);
///     pages.iter().enumerate().map(|(at, page)| (*page, if at == top { Standing::Shown } else { Standing::Hidden })).collect()
/// }
///
/// # struct Quiet;
/// # impl Host<Pages> for Quiet {
/// #     fn perform(&mut self, effect: Never, _: &Intents<Intent>) {
/// #         match effect {}
/// #     }
/// # }
/// # impl Host<Page> for Quiet {
/// #     fn perform(&mut self, effect: Never, _: &Intents<Lifecycle>) {
/// #         match effect {}
/// #     }
/// # }
/// let pages = Children::default();
/// let wiring = Store::<Pages>::build(vec!["home"], Quiet);
/// wiring.children(&pages, stack, |_, _| Store::<Page>::new(Vec::new(), Quiet));
/// let store = wiring.start();
///
/// store.send(Intent::Open("settings"));
/// let home = pages.get(&"home").expect("the first page");
/// assert_eq!(home.try_read(Vec::clone).unwrap(), [Lifecycle::Shown, Lifecycle::Hidden]);
///
/// store.send(Intent::Back);
/// assert_eq!(pages.keys(), ["home"], "going back dropped the page on top");
/// assert_eq!(home.try_read(Vec::clone).unwrap(), [Lifecycle::Shown, Lifecycle::Hidden, Lifecycle::Shown]);
/// ```
pub struct Children<K, T>(Rc<RefCell<Vec<Kept<K, T>>>>);

struct Kept<K, T> {
    key: K,
    standing: Standing,
    /// Its place in the order the state names the keys.
    rank: usize,
    child: Rc<T>,
}

impl<K, T> Default for Children<K, T> {
    fn default() -> Self {
        Children(Rc::default())
    }
}

impl<K, T> Clone for Children<K, T> {
    fn clone(&self) -> Self {
        Children(Rc::clone(&self.0))
    }
}

impl<K: fmt::Debug, T> fmt::Debug for Children<K, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kept = self.0.borrow();
        f.debug_map().entries(kept.iter().map(|kept| (&kept.key, kept.standing))).finish()
    }
}

impl<K: PartialEq, T> Children<K, T> {
    /// The child kept for `key`, if the state names it. Holding it keeps it alive after the state stops naming it, as holding a
    /// [`Store`] does.
    pub fn get(&self, key: &K) -> Option<Rc<T>> {
        self.0.borrow().iter().find(|kept| kept.key == *key).map(|kept| Rc::clone(&kept.child))
    }

    /// The keys of the children kept, in the order the state names them.
    pub fn keys(&self) -> Vec<K>
    where
        K: Clone,
    {
        self.0.borrow().iter().map(|kept| kept.key.clone()).collect()
    }

    /// How many children are kept.
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    /// Whether no child is kept.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl<K: PartialEq, T: Child> Children<K, T> {
    /// Keeps a child for each key in `wanted`, in its order, each key once. A new key's child is built, a key no longer wanted
    /// has its child dropped, and each child is told its standing as it is built and whenever it changes.
    ///
    /// No borrow is held while a child is built, dropped or told, so any of them can reach these children, or send to the
    /// parent, whose store then runs it in turn.
    pub(crate) fn sync(&self, wanted: Vec<(K, Standing)>, build: &mut dyn FnMut(&K) -> T) {
        let mut unique: Vec<(K, Standing)> = Vec::with_capacity(wanted.len());
        for (key, standing) in wanted {
            if !unique.iter().any(|(seen, _)| *seen == key) {
                unique.push((key, standing));
            }
        }
        let gone: Vec<Kept<K, T>> = self.0.borrow_mut().extract_if(.., |kept| !unique.iter().any(|(key, _)| *key == kept.key)).collect();
        drop(gone);

        let mut tell = Vec::new();
        for (rank, (key, standing)) in unique.into_iter().enumerate() {
            let found = self.0.borrow_mut().iter_mut().find(|kept| kept.key == key).map(|kept| {
                let was = kept.standing;
                kept.standing = standing;
                kept.rank = rank;
                (was, Rc::clone(&kept.child))
            });
            let (was, child) = match found {
                Some((was, child)) => (Some(was), child),
                None => {
                    let child = Rc::new(build(&key));
                    self.0.borrow_mut().push(Kept {
                        key,
                        standing,
                        rank,
                        child: Rc::clone(&child),
                    });
                    (None, child)
                }
            };
            tell.push((child, Standing::events(was, standing)));
        }
        self.0.borrow_mut().sort_by_key(|kept| kept.rank);

        for (child, events) in tell {
            for life in events {
                child.tell(life);
            }
        }
    }
}
