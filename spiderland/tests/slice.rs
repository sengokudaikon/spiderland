//! `Slice::show` over a page's shape, from outside the crate as an app sees it: a tracked list, a flag, and a value that counts
//! its own changes.

use spiderland::{Input, Slice, Tracked, Version};

struct Entry {
    name: &'static str,
    undone: bool,
}

/// The worker's latest word. It counts its own changes, so it gives that count as its key.
struct Game {
    generation: u64,
    can_write: bool,
}

impl Input for &Game {
    type Key = u64;

    fn key(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Gates {
    can_mutate: bool,
}

#[derive(Debug, PartialEq)]
struct Row {
    name: &'static str,
    can_undo: bool,
}

fn history_rows(history: &Tracked<Vec<Entry>>, game: &Game, gates: Gates) -> Vec<Row> {
    history
        .iter()
        .map(|entry| Row {
            name: entry.name,
            can_undo: !entry.undone && game.can_write && gates.can_mutate,
        })
        .collect()
}

fn row(name: &'static str, can_undo: bool) -> Row {
    Row { name, can_undo }
}

/// What the render shows this time, or `None` when the region is left as it was.
fn render(slice: &mut Slice<(Version, u64, Gates)>, history: &Tracked<Vec<Entry>>, game: &Game, gates: Gates) -> Option<Vec<Row>> {
    let mut shown = None;
    slice.show((history, game, gates), history_rows, |rows| shown = Some(rows));
    shown
}

#[test]
fn a_region_is_made_again_only_when_what_it_reads_changes() {
    let mut history = Tracked::new(vec![Entry { name: "a", undone: false }]);
    let mut game = Game {
        generation: 1,
        can_write: true,
    };
    let mut gates = Gates { can_mutate: true };
    let mut slice = Slice::default();

    assert_eq!(render(&mut slice, &history, &game, gates), Some(vec![row("a", true)]));
    assert_eq!(render(&mut slice, &history, &game, gates), None, "nothing it reads changed");

    history.push(Entry { name: "b", undone: true });
    assert_eq!(render(&mut slice, &history, &game, gates), Some(vec![row("a", true), row("b", false)]));

    game = Game {
        generation: 2,
        can_write: false,
    };
    assert_eq!(render(&mut slice, &history, &game, gates), Some(vec![row("a", false), row("b", false)]));

    game.can_write = true;
    game.generation += 1;
    gates.can_mutate = false;
    assert_eq!(render(&mut slice, &history, &game, gates), Some(vec![row("a", false), row("b", false)]));
    gates.can_mutate = true;
    assert_eq!(render(&mut slice, &history, &game, gates), Some(vec![row("a", true), row("b", false)]));
}
