//! A list model updated in place.

use slint::{Model, ModelRc, VecModel};
use std::any::Any;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

/// Rows are slots: rows both lists have are updated in place, and only if they differ, and the rest come or go at the
/// end. Slint updates a changed row's existing instance, so a row being dragged, typed in or holding focus carries on
/// while others change. Handing the view a new model instead would rebuild every row.
///
/// [`Rows::sync`] holds the slot, and [`Rows::sync_by`] follows the item. Pick by what should stay put when a row before it goes.
///
/// Two `ModelRc`s compare by identity, so a row holding a newly made list always counts as changed. A list kept in a model of its
/// own avoids that, but Slint 1.18.1 can lose a change to it: while its outer row has an update not yet painted, an inner item changed
/// in place stays old on screen. Two renders with no paint between them are enough. [`InnerRows`] avoids both.
pub struct Rows<T: 'static>(Rc<VecModel<T>>);

impl<T: Clone + 'static> fmt::Debug for Rows<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rows").field("len", &self.0.row_count()).finish()
    }
}

impl<T: 'static> Default for Rows<T> {
    fn default() -> Self {
        Rows(Rc::new(VecModel::default()))
    }
}

impl<T: Clone + PartialEq + 'static> Rows<T> {
    /// The model to give the view, once. Later changes go through [`Rows::sync`].
    pub fn model(&self) -> ModelRc<T> {
        ModelRc::from(self.0.clone())
    }

    /// Makes the model hold `rows`, by position: a row at a place both lists have is updated in place if it differs, and the
    /// rest come or go at the end.
    pub fn sync(&self, rows: impl IntoIterator<Item = T>) {
        self.update(rows);
    }

    /// Says whether a row that stayed was changed in place.
    fn update(&self, rows: impl IntoIterator<Item = T>) -> bool {
        let mut changed = false;
        let rows: Vec<T> = rows.into_iter().collect();
        while self.0.row_count() > rows.len() {
            self.0.remove(self.0.row_count() - 1);
        }
        let kept = self.0.row_count();
        for (index, row) in rows.into_iter().enumerate() {
            if index >= kept {
                self.0.push(row);
            } else if self.0.row_data(index).as_ref() != Some(&row) {
                self.0.set_row_data(index, row);
                changed = true;
            }
        }
        changed
    }

    /// Gets a change made in place onto the screen even if Slint would lose it. While an outer row's update waits for a paint, Slint's
    /// repeater only marks a changed inner row as dirty, and nothing refreshes it until the row count next changes. So this changes
    /// the count and puts it back: Slint then refreshes the dirty rows in place, and builds and drops nothing.
    fn refresh(&self) {
        let count = self.0.row_count();
        if let Some(last) = count.checked_sub(1).and_then(|index| self.0.row_data(index)) {
            self.0.push(last);
            self.0.remove(count);
        }
    }

    /// Updates the list to `rows`, matching by `key` rather than by position. A row whose key is kept keeps its instance, and with it
    /// its focus and whatever is being typed there, when rows around it come or go. A row that changes place is rebuilt. Keys should
    /// be unique: of rows sharing a key, the first of them is matched first.
    ///
    /// Rows that keep their order cost one pass. Each row that moves, or comes in before others, costs a pass over the rows after
    /// it, so reordering a long list all at once takes time that grows with the square of its length.
    pub fn sync_by<K: Eq + Hash>(&self, key: impl Fn(&T) -> K, rows: impl IntoIterator<Item = T>) {
        let rows: Vec<T> = rows.into_iter().collect();
        let wanted: HashSet<K> = rows.iter().map(&key).collect();
        let mut current: Vec<K> = self.0.iter().map(|row| key(&row)).collect();
        for index in (0..current.len()).rev() {
            if !wanted.contains(&current[index]) {
                self.0.remove(index);
                current.remove(index);
            }
        }
        let count = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            let row_key = key(&row);
            match current[index..].iter().position(|kept| *kept == row_key) {
                Some(0) => {
                    if self.0.row_data(index).as_ref() != Some(&row) {
                        self.0.set_row_data(index, row);
                    }
                }
                moved => {
                    if let Some(offset) = moved {
                        self.0.remove(index + offset);
                        current.remove(index + offset);
                    }
                    self.0.insert(index, row);
                    current.insert(index, row_key);
                }
            }
        }
        while self.0.row_count() > count {
            self.0.remove(self.0.row_count() - 1);
        }
    }
}

/// One list inside each row of a [`Rows`]. Each row keeps one model, so an inner change reaches the view without the outer row
/// counting as changed, and without losing the change to the Slint bug described on [`Rows`].
///
/// [`InnerRows::fill`] keeps a model per position, for an outer [`Rows::sync`]. [`InnerRows::fill_by`] keeps one per key, for an
/// outer [`Rows::sync_by`]: a row that stays keeps its model as rows before it come or go, so the outer row doesn't count as
/// changed, and focus inside its list stays. Use the same key for both.
pub struct InnerRows<T: 'static>(Vec<Inner<T>>);

/// One row's model, and the key it was kept for, if it was filled by key.
struct Inner<T: 'static> {
    key: Option<Box<dyn Any>>,
    rows: Rows<T>,
}

impl<T: Clone + 'static> fmt::Debug for InnerRows<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter().map(|inner| &inner.rows)).finish()
    }
}

impl<T: 'static> Default for InnerRows<T> {
    fn default() -> Self {
        InnerRows(Vec::new())
    }
}

impl<T: Clone + PartialEq + 'static> InnerRows<T> {
    /// Takes each row with its inner list, updates that list's model in place, and hands the model to `attach` to put in the row. It
    /// returns the rows, for the outer [`Rows::sync`]. Each position keeps its model, and models past the last row are let go of.
    /// Under an outer list that moves rows, a row gets the model of the position it lands on, with the right contents.
    pub fn fill<R, L: IntoIterator<Item = T>>(&mut self, rows: impl IntoIterator<Item = (R, L)>, attach: impl Fn(&mut R, ModelRc<T>)) -> Vec<R> {
        let mut filled = Vec::new();
        for (index, (mut row, list)) in rows.into_iter().enumerate() {
            if index == self.0.len() {
                self.0.push(Inner { key: None, rows: Rows::default() });
            }
            self.0[index].key = None;
            attach(&mut row, self.0[index].update(list));
            filled.push(row);
        }
        self.0.truncate(filled.len());
        filled
    }

    /// [`InnerRows::fill`], keeping a model for each row's `key` rather than its position. It returns the rows, for the outer
    /// [`Rows::sync_by`] with the same key. A row whose key stays keeps its model wherever it moves, and a key that goes takes its
    /// model with it. Keys should be unique: of rows sharing a key, the first gets the model kept.
    pub fn fill_by<K: PartialEq + 'static, R, L: IntoIterator<Item = T>>(
        &mut self,
        key: impl Fn(&R) -> K,
        rows: impl IntoIterator<Item = (R, L)>,
        attach: impl Fn(&mut R, ModelRc<T>),
    ) -> Vec<R> {
        let mut kept = std::mem::take(&mut self.0);
        let mut filled = Vec::new();
        for (mut row, list) in rows {
            let row_key = key(&row);
            let found = kept.iter().position(|inner| inner.key.as_ref().and_then(|key| key.downcast_ref::<K>()) == Some(&row_key));
            let mut inner = match found {
                Some(at) => kept.remove(at),
                None => Inner { key: None, rows: Rows::default() },
            };
            inner.key = Some(Box::new(row_key));
            attach(&mut row, inner.update(list));
            self.0.push(inner);
            filled.push(row);
        }
        filled
    }
}

impl<T: Clone + PartialEq + 'static> Inner<T> {
    /// Updates the model to `list`, gets a change in place onto the screen, and returns the model.
    fn update(&self, list: impl IntoIterator<Item = T>) -> ModelRc<T> {
        if self.rows.update(list) {
            self.rows.refresh();
        }
        self.rows.model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(rows: &Rows<i32>) -> Vec<i32> {
        rows.model().iter().collect()
    }

    #[test]
    fn it_grows_changes_and_shrinks_in_place() {
        let rows = Rows::default();
        let model = rows.model();
        rows.sync([1, 2]);
        assert_eq!(contents(&rows), [1, 2]);
        rows.sync([1, 3, 4]);
        assert_eq!(contents(&rows), [1, 3, 4]);
        rows.sync([5]);
        assert_eq!(contents(&rows), [5]);
        assert_eq!(model.row_count(), 1, "the model given to the view is the one that changes");
    }

    /// Syncs by the tens digit, so the units show which of two rows with one key was kept.
    fn synced(from: &[i32], to: &[i32]) -> Vec<i32> {
        let rows = Rows::default();
        rows.sync(from.iter().copied());
        rows.sync_by(|row| row / 10, to.iter().copied());
        contents(&rows)
    }

    #[test]
    fn syncing_by_key_arrives_at_the_list_asked_for() {
        assert_eq!(synced(&[], &[10, 20]), [10, 20], "from nothing");
        assert_eq!(synced(&[10, 20], &[]), [0; 0], "to nothing");
        assert_eq!(synced(&[10, 20, 30], &[20, 30]), [20, 30], "a removal at the front");
        assert_eq!(synced(&[10, 30], &[10, 20, 30]), [10, 20, 30], "an insertion in the middle");
        assert_eq!(synced(&[10, 20], &[11, 20]), [11, 20], "a changed row");
        assert_eq!(synced(&[10, 20, 30], &[30, 10, 20]), [30, 10, 20], "a move");
        assert_eq!(synced(&[10, 20, 30], &[30, 21, 40, 10]), [30, 21, 40, 10], "all at once");
    }

    #[test]
    fn syncing_by_key_copes_with_keys_that_repeat() {
        assert_eq!(synced(&[10, 11, 20], &[20, 12, 13]), [20, 12, 13]);
        assert_eq!(synced(&[10, 11, 12], &[13]), [13]);
        assert_eq!(synced(&[10], &[11, 12]), [11, 12]);
    }

    #[test]
    fn inner_rows_keep_one_model_per_position_and_let_go_of_the_rest() {
        let mut inner = InnerRows::default();
        let attach = |row: &mut Option<ModelRc<i32>>, model| *row = Some(model);
        let fill = |inner: &mut InnerRows<i32>, lists: Vec<Vec<i32>>| inner.fill(lists.into_iter().map(|list| (None, list)), attach);
        let first = fill(&mut inner, vec![vec![1], vec![2, 3]]);
        let second = fill(&mut inner, vec![vec![4], vec![5]]);
        assert!(first[0] == second[0] && first[1] == second[1], "each position keeps its model");
        assert_eq!(second[1].as_ref().unwrap().iter().collect::<Vec<_>>(), [5], "with the new contents");
        fill(&mut inner, vec![vec![6]]);
        let regrown = fill(&mut inner, vec![vec![6], vec![7]]);
        assert!(regrown[1] != second[1], "a position that went away starts over");
        assert!(regrown[0] == second[0]);
    }

    /// A row with a key and a list of its own, as a view gets it.
    #[derive(Clone, PartialEq)]
    struct Row {
        id: u32,
        items: Option<ModelRc<i32>>,
    }

    /// Fills `inner` by key and syncs `outer` by the same key, from `(id, items)` pairs.
    fn show(outer: &Rows<Row>, inner: &mut InnerRows<i32>, rows: &[(u32, &[i32])]) {
        let rows = rows.iter().map(|&(id, items)| (Row { id, items: None }, items.to_vec()));
        outer.sync_by(|row| row.id, inner.fill_by(|row: &Row| row.id, rows, |row, model| row.items = Some(model)));
    }

    #[test]
    fn inner_rows_by_key_keep_a_rows_model_as_rows_before_it_come_or_go() {
        let (outer, mut inner) = (Rows::default(), InnerRows::default());
        show(&outer, &mut inner, &[(1, &[10]), (2, &[20, 21])]);
        let second = outer.model().row_data(1).unwrap();
        show(&outer, &mut inner, &[(2, &[20, 22])]);
        let kept = outer.model().row_data(0).unwrap();
        assert!(kept.items == second.items, "the row that stayed kept its model, so the outer row is unchanged");
        assert_eq!(kept.items.unwrap().iter().collect::<Vec<_>>(), [20, 22], "with the new contents");

        show(&outer, &mut inner, &[(3, &[30]), (2, &[20])]);
        assert!(outer.model().row_data(1).unwrap().items == second.items, "and as rows come before it");
        show(&outer, &mut inner, &[(3, &[30])]);
        show(&outer, &mut inner, &[(3, &[30]), (2, &[20])]);
        assert!(outer.model().row_data(1).unwrap().items != second.items, "a key that went away starts over");
    }
}
