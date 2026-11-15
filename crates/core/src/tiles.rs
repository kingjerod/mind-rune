//! The row of tiles the window shows sessions in: which sessions, in which order, how wide, and
//! which one is focused.

use crate::sessions::SessionKey;

/// The least a tile is opened or dragged to, in points.
pub const MIN_TILE_WIDTH: f32 = 420.0;

/// One tile of the row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub key: SessionKey,
    /// Its part of the row against the others'; above 0.
    pub share: f32,
}

/// Where a session is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// In the focused tile, in place of its session; in a first tile when there is none.
    Focused,
    /// In a new tile right of the focused one. As `Focused` when there is no room.
    Side,
    /// In a new tile at this position, 0 being leftmost. Nothing when there is no room.
    At(usize),
    /// In place of this tile's session.
    Tile(SessionKey),
}

/// What `Tiles::put` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Put {
    Nothing,
    FocusedOnly,
    Added,
    Replaced(SessionKey),
}

/// The row of tiles, left to right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tiles {
    list: Vec<Tile>,
    /// `None` only while the row is empty.
    focused: Option<SessionKey>,
    /// Goes up with every change: see [`Tiles::version`].
    version: u64,
    /// Goes up with every change but of widths: see [`Tiles::placement`].
    placement: u64,
}

impl Tiles {
    pub fn list(&self) -> &[Tile] {
        &self.list
    }

    pub fn focused(&self) -> Option<SessionKey> {
        self.focused
    }

    pub fn contains(&self, key: SessionKey) -> bool {
        self.index_of(key).is_some()
    }

    pub fn index_of(&self, key: SessionKey) -> Option<usize> {
        self.list.iter().position(|tile| tile.key == key)
    }

    /// Goes up with every change, the focus included. Only tests read it: the sidebar is
    /// rebuilt by [`Tiles::placement`].
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Goes up when the sessions in the row, their order or the focus change: not when only
    /// the widths do ([`Tiles::resize`], [`Tiles::even`]).
    pub fn placement(&self) -> u64 {
        self.placement
    }

    /// Each tile's width in a row `row_width` wide, in order; they add up to `row_width`.
    /// Every width is 0 when `row_width` is 0 or not finite.
    pub fn widths(&self, row_width: f32) -> Vec<f32> {
        if !usable(row_width) {
            return vec![0.0; self.list.len()];
        }
        let total: f32 = self.list.iter().map(|tile| tile.share).sum();
        self.list
            .iter()
            .map(|tile| tile.share / total * row_width)
            .collect()
    }

    /// Every tile and one more could each have `MIN_TILE_WIDTH`. True for an empty row.
    pub fn fits_another(&self, row_width: f32) -> bool {
        self.list.is_empty()
            || usable(row_width) && (self.list.len() + 1) as f32 * MIN_TILE_WIDTH <= row_width
    }

    /// The tile under `x`, measured from the row's left edge.
    pub fn tile_at(&self, row_width: f32, x: f32) -> Option<usize> {
        if !usable(row_width) || !(0.0..row_width).contains(&x) {
            return None;
        }
        let mut right = 0.0;
        let widths = self.widths(row_width);
        let index = widths.iter().position(|width| {
            right += width;
            x < right
        });
        // A sum that rounds a hair short of `row_width` still puts its last points in a tile.
        index.or(self.list.len().checked_sub(1))
    }

    /// The nearest of the edges a new tile can open at: 0 to `len`.
    pub fn edge_at(&self, row_width: f32, x: f32) -> usize {
        let mut edge = 0.0;
        let mut nearest = (0, (x - edge).abs());
        for (index, width) in self.widths(row_width).into_iter().enumerate() {
            edge += width;
            let distance = (x - edge).abs();
            if distance < nearest.1 {
                nearest = (index + 1, distance);
            }
        }
        nearest.0
    }

    /// Where `key`'s tile would sit after a move to `edge`, an edge of the row as it is now as
    /// `edge_at` counts them (0 to `len`; past `len` is `len`). None when `key` has no tile or
    /// the tile would stay where it is: `edge` is its own left or right edge.
    pub fn moved_index(&self, key: SessionKey, edge: usize) -> Option<usize> {
        let index = self.index_of(key)?;
        let edge = edge.min(self.list.len());
        if edge == index || edge == index + 1 {
            return None;
        }
        // Out of its place, the tiles right of it are one place further left.
        Some(if edge < index { edge } else { edge - 1 })
    }

    /// Moves `key`'s tile to `edge` with its share, and focuses it. False when nothing moved.
    pub(crate) fn move_to(&mut self, key: SessionKey, edge: usize) -> bool {
        let (Some(index), Some(to)) = (self.index_of(key), self.moved_index(key, edge)) else {
            return false;
        };
        let tile = self.list.remove(index);
        self.list.insert(to, tile);
        self.focused = Some(key);
        self.placed();
        true
    }

    /// `row_width` is the row's width when the window has told it.
    pub(crate) fn put(&mut self, key: SessionKey, place: Place, row_width: Option<f32>) -> Put {
        if self.contains(key) {
            self.focus(key);
            return Put::FocusedOnly;
        }
        let room = row_width.is_none_or(|width| self.fits_another(width));
        let place = match place {
            Place::Side if !room => Place::Focused,
            place => place,
        };
        match place {
            Place::Focused => match self.focused {
                Some(old) => self.replace(old, key),
                None => self.add(0, key, row_width),
            },
            Place::Side => {
                let index = self.focused.and_then(|old| self.index_of(old));
                self.add(index.map_or(0, |index| index + 1), key, row_width)
            }
            Place::At(_) if !room => Put::Nothing,
            Place::At(index) => self.add(index.min(self.list.len()), key, row_width),
            Place::Tile(old) if self.contains(old) => self.replace(old, key),
            Place::Tile(_) => Put::Nothing,
        }
    }

    /// Shows `key` in `old`'s tile, with its share, and focuses it.
    fn replace(&mut self, old: SessionKey, key: SessionKey) -> Put {
        let Some(index) = self.index_of(old) else {
            return Put::Nothing;
        };
        self.list[index].key = key;
        self.focused = Some(key);
        self.placed();
        Put::Replaced(old)
    }

    /// Opens a tile for `key` at `index` with the mean of the others' shares, then raises any
    /// tile under `MIN_TILE_WIDTH` to it when the row's width is known.
    fn add(&mut self, index: usize, key: SessionKey, row_width: Option<f32>) -> Put {
        let share = if self.list.is_empty() {
            1.0
        } else {
            self.list.iter().map(|tile| tile.share).sum::<f32>() / self.list.len() as f32
        };
        self.list.insert(index, Tile { key, share });
        if let Some(row_width) = row_width {
            self.keep_to_minimum(row_width);
        }
        self.focused = Some(key);
        self.placed();
        Put::Added
    }

    /// Raises each tile under `MIN_TILE_WIDTH` to it and takes the difference from the tiles
    /// above it, in proportion to what each has above it. The total of the shares stays.
    fn keep_to_minimum(&mut self, row_width: f32) {
        if !usable(row_width) || self.list.len() as f32 * MIN_TILE_WIDTH > row_width {
            return;
        }
        let mut widths = self.widths(row_width);
        let short: f32 = widths.iter().map(|w| (MIN_TILE_WIDTH - w).max(0.0)).sum();
        let spare: f32 = widths.iter().map(|w| (w - MIN_TILE_WIDTH).max(0.0)).sum();
        if short <= 0.0 || spare <= 0.0 {
            return;
        }
        for width in &mut widths {
            if *width < MIN_TILE_WIDTH {
                *width = MIN_TILE_WIDTH;
            } else {
                *width -= (*width - MIN_TILE_WIDTH) * short / spare;
            }
        }
        let total: f32 = self.list.iter().map(|tile| tile.share).sum();
        for (tile, width) in self.list.iter_mut().zip(widths) {
            tile.share = width / row_width * total;
        }
    }

    pub(crate) fn focus(&mut self, key: SessionKey) -> bool {
        if self.focused == Some(key) || !self.contains(key) {
            return false;
        }
        self.focused = Some(key);
        self.placed();
        true
    }

    /// Takes `key`'s tile out; the focus goes to the tile on its right, else on its left.
    pub(crate) fn remove(&mut self, key: SessionKey) -> bool {
        let Some(index) = self.index_of(key) else {
            return false;
        };
        self.list.remove(index);
        if self.focused == Some(key) {
            let next = self
                .list
                .get(index)
                .or(index.checked_sub(1).map(|i| &self.list[i]));
            self.focused = next.map(|tile| tile.key);
        }
        self.placed();
        true
    }

    /// Makes `left`'s tile `width` wide and gives or takes the difference from the tile on
    /// its right. Neither goes below `MIN_TILE_WIDTH`; nothing happens when the two have less
    /// than twice that between them, or `left` is the last tile.
    pub(crate) fn resize(&mut self, left: SessionKey, width: f32, row_width: f32) -> bool {
        let Some(index) = self.index_of(left) else {
            return false;
        };
        if index + 1 >= self.list.len() || !usable(row_width) || width.is_nan() {
            return false;
        }
        let widths = self.widths(row_width);
        let pair = widths[index] + widths[index + 1];
        if pair < 2.0 * MIN_TILE_WIDTH {
            return false;
        }
        let width = width.clamp(MIN_TILE_WIDTH, pair - MIN_TILE_WIDTH);
        let shares = self.list[index].share + self.list[index + 1].share;
        let share = shares * width / pair;
        if share == self.list[index].share {
            return false;
        }
        self.list[index].share = share;
        self.list[index + 1].share = shares - share;
        self.version += 1;
        true
    }

    /// Gives `left`'s tile and the one on its right the shares `shares`, as they were before a
    /// drag of the divider between them. A change of widths only: the placement stays.
    pub(crate) fn set_pair(&mut self, left: SessionKey, shares: [f32; 2]) -> bool {
        let Some(index) = self.index_of(left).filter(|i| i + 1 < self.list.len()) else {
            return false;
        };
        if [self.list[index].share, self.list[index + 1].share] == shares {
            return false;
        }
        [self.list[index].share, self.list[index + 1].share] = shares;
        self.version += 1;
        true
    }

    pub(crate) fn even(&mut self) -> bool {
        if self.list.iter().all(|tile| tile.share == 1.0) {
            return false;
        }
        for tile in &mut self.list {
            tile.share = 1.0;
        }
        self.version += 1;
        true
    }

    /// Replaces the row with `saved`, put right as [`Tiles::restored`] does, its shares then
    /// scaled so that their mean is 1: shares too large to add up still leave a row to share.
    /// An empty `saved` empties the row.
    pub(crate) fn restore(&mut self, saved: Vec<Tile>, focused: Option<SessionKey>) {
        let Tiles {
            mut list, focused, ..
        } = Tiles::restored(saved, focused);
        self.placed();
        if list.is_empty() {
            self.list.clear();
            self.focused = None;
            return;
        }
        let mean = list.iter().map(|tile| f64::from(tile.share)).sum::<f64>() / list.len() as f64;
        for tile in &mut list {
            // A share far below the others' would round to 0, which is no share.
            tile.share = ((f64::from(tile.share) / mean) as f32).max(f32::MIN_POSITIVE);
        }
        self.list = list;
        self.focused = focused;
    }

    /// From what was saved: shares that are not finite or not above 0 become 1, a key named
    /// twice is kept once, and the focus is `focused` when it is among them, else the first.
    pub(crate) fn restored(saved: Vec<Tile>, focused: Option<SessionKey>) -> Tiles {
        let mut list: Vec<Tile> = Vec::with_capacity(saved.len());
        for mut tile in saved {
            if list.iter().any(|kept| kept.key == tile.key) {
                continue;
            }
            if !(tile.share.is_finite() && tile.share > 0.0) {
                tile.share = 1.0;
            }
            list.push(tile);
        }
        let focused = focused
            .filter(|key| list.iter().any(|tile| tile.key == *key))
            .or(list.first().map(|tile| tile.key));
        Tiles {
            list,
            focused,
            version: 0,
            placement: 0,
        }
    }

    /// Counts a change of which sessions are in the row, their order or the focus.
    fn placed(&mut self) {
        self.version += 1;
        self.placement += 1;
    }
}

/// A row width the arithmetic can divide: finite and above 0.
fn usable(row_width: f32) -> bool {
    row_width.is_finite() && row_width > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: SessionKey = SessionKey(1);
    const B: SessionKey = SessionKey(2);
    const C: SessionKey = SessionKey(3);
    const D: SessionKey = SessionKey(4);

    /// A row of `keys` with even shares, `focused` focused.
    fn row(keys: &[SessionKey], focused: SessionKey) -> Tiles {
        let tiles = keys.iter().map(|&key| Tile { key, share: 1.0 }).collect();
        Tiles::restored(tiles, Some(focused))
    }

    fn keys(tiles: &Tiles) -> Vec<SessionKey> {
        tiles.list().iter().map(|tile| tile.key).collect()
    }

    fn shares(tiles: &Tiles) -> Vec<f32> {
        tiles.list().iter().map(|tile| tile.share).collect()
    }

    fn assert_widths(tiles: &Tiles, row_width: f32, expected: &[f32]) {
        let widths = tiles.widths(row_width);
        assert_eq!(widths.len(), expected.len(), "{widths:?}");
        for (width, expected) in widths.iter().zip(expected) {
            assert!(
                (width - expected).abs() < 0.5,
                "{widths:?}, not {expected:?}"
            );
        }
    }

    #[test]
    fn a_first_session_opens_a_first_tile() {
        for place in [Place::Focused, Place::Side, Place::At(0), Place::At(5)] {
            let mut tiles = Tiles::default();
            assert_eq!(tiles.put(A, place, None), Put::Added, "{place:?}");
            assert_eq!(keys(&tiles), vec![A], "{place:?}");
            assert_eq!(tiles.focused(), Some(A), "{place:?}");
        }
    }

    #[test]
    fn the_focused_tile_swaps_its_session() {
        let mut tiles = Tiles::restored(
            vec![Tile { key: A, share: 1.0 }, Tile { key: B, share: 2.0 }],
            Some(B),
        );
        assert_eq!(tiles.put(C, Place::Focused, None), Put::Replaced(B));
        assert_eq!(keys(&tiles), vec![A, C]);
        assert_eq!(shares(&tiles), vec![1.0, 2.0]);
        assert_eq!(tiles.focused(), Some(C));
    }

    #[test]
    fn a_session_is_never_in_two_tiles() {
        for place in [Place::Side, Place::At(0), Place::Tile(A)] {
            let mut tiles = row(&[A, B], A);
            assert_eq!(tiles.put(B, place, None), Put::FocusedOnly, "{place:?}");
            assert_eq!(keys(&tiles), vec![A, B], "{place:?}");
            assert_eq!(tiles.focused(), Some(B), "{place:?}");
        }
    }

    #[test]
    fn to_the_side_opens_right_of_the_focused_tile() {
        let mut tiles = row(&[A, B], A);
        assert_eq!(tiles.put(C, Place::Side, None), Put::Added);
        assert_eq!(keys(&tiles), vec![A, C, B]);
        assert_eq!(tiles.focused(), Some(C));
        assert_eq!(tiles.put(D, Place::At(0), None), Put::Added);
        assert_eq!(keys(&tiles), vec![D, A, C, B]);
        assert_eq!(tiles.focused(), Some(D));
    }

    #[test]
    fn a_new_tile_takes_an_equal_part() {
        let uneven = Tiles::restored(
            vec![Tile { key: A, share: 3.0 }, Tile { key: B, share: 1.0 }],
            Some(A),
        );

        let mut tiles = uneven.clone();
        assert_eq!(tiles.put(C, Place::Side, None), Put::Added);
        assert_eq!(keys(&tiles), vec![A, C, B]);
        assert_widths(&tiles, 1600.0, &[800.0, 533.3, 266.7]);

        let mut tiles = uneven;
        assert_eq!(tiles.put(C, Place::Side, Some(1600.0)), Put::Added);
        assert_eq!(keys(&tiles), vec![A, C, B]);
        let widths = tiles.widths(1600.0);
        assert!((widths[2] - MIN_TILE_WIDTH).abs() < 0.5, "{widths:?}");
        assert!(
            widths[0] < 800.0 && widths[0] > MIN_TILE_WIDTH,
            "{widths:?}"
        );
        assert!(
            widths[1] < 533.3 && widths[1] > MIN_TILE_WIDTH,
            "{widths:?}"
        );
        assert!(
            (widths.iter().sum::<f32>() - 1600.0).abs() < 0.5,
            "{widths:?}"
        );
    }

    #[test]
    fn without_room_side_swaps_and_at_does_nothing() {
        let mut tiles = row(&[A, B], A);
        assert_eq!(tiles.put(C, Place::Side, Some(1000.0)), Put::Replaced(A));
        assert_eq!(keys(&tiles), vec![C, B]);
        assert_eq!(tiles.put(D, Place::At(0), Some(1000.0)), Put::Nothing);
        assert_eq!(keys(&tiles), vec![C, B]);
        assert_eq!(tiles.focused(), Some(C));
    }

    #[test]
    fn room_is_420_points_a_tile() {
        let one = row(&[A], A);
        assert!(!one.fits_another(839.0));
        assert!(one.fits_another(840.0));
        assert!(Tiles::default().fits_another(100.0));
    }

    #[test]
    fn closing_moves_the_focus_right_then_left() {
        let mut tiles = row(&[A, B, C], B);
        assert!(tiles.remove(B));
        assert_eq!(keys(&tiles), vec![A, C]);
        assert_eq!(tiles.focused(), Some(C));

        let mut tiles = row(&[A, B, C], C);
        assert!(tiles.remove(C));
        assert_eq!(tiles.focused(), Some(B));

        let mut tiles = row(&[A, B, C], C);
        assert!(tiles.remove(A));
        assert_eq!(tiles.focused(), Some(C));

        let mut tiles = row(&[A], A);
        assert!(tiles.remove(A));
        assert!(tiles.list().is_empty());
        assert_eq!(tiles.focused(), None);
        assert!(!tiles.remove(A));
    }

    #[test]
    fn closing_leaves_the_others_shares() {
        let mut tiles = Tiles::restored(
            vec![
                Tile { key: A, share: 3.0 },
                Tile { key: B, share: 1.0 },
                Tile { key: C, share: 1.0 },
            ],
            Some(A),
        );
        assert!(tiles.remove(B));
        assert_eq!(shares(&tiles), vec![3.0, 1.0]);
        assert_widths(&tiles, 1600.0, &[1200.0, 400.0]);
    }

    #[test]
    fn a_divider_moves_width_between_its_two_tiles() {
        let mut tiles = row(&[A, B, C], A);
        assert_widths(&tiles, 1500.0, &[500.0, 500.0, 500.0]);
        assert!(tiles.resize(A, 560.0, 1500.0));
        assert_widths(&tiles, 1500.0, &[560.0, 440.0, 500.0]);
        assert!(tiles.resize(A, 600.0, 1500.0));
        assert_widths(&tiles, 1500.0, &[580.0, 420.0, 500.0]);
        assert!(tiles.resize(A, 100.0, 1500.0));
        assert_widths(&tiles, 1500.0, &[420.0, 580.0, 500.0]);
        assert!(!tiles.resize(C, 600.0, 1500.0));
        assert_widths(&tiles, 1500.0, &[420.0, 580.0, 500.0]);
    }

    #[test]
    fn a_divider_with_nothing_to_give_does_nothing() {
        let mut tiles = row(&[A, B, C], A);
        assert!(!tiles.resize(A, 300.0, 700.0));
        assert!(!tiles.resize(B, 100.0, 700.0));
        assert_eq!(shares(&tiles), vec![1.0, 1.0, 1.0]);
        assert_widths(&tiles, 700.0, &[233.3, 233.3, 233.3]);
    }

    #[test]
    fn evening_out_gives_every_tile_the_same() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.resize(A, 600.0, 1500.0));
        assert!(tiles.even());
        assert_widths(&tiles, 1500.0, &[500.0, 500.0, 500.0]);
        assert!(!tiles.even());
    }

    #[test]
    fn the_tile_and_the_edge_under_a_point() {
        let tiles = Tiles::restored(
            vec![
                Tile { key: A, share: 5.0 },
                Tile { key: B, share: 5.0 },
                Tile { key: C, share: 6.0 },
            ],
            Some(A),
        );
        assert_widths(&tiles, 1600.0, &[500.0, 500.0, 600.0]);
        assert_eq!(tiles.tile_at(1600.0, 499.0), Some(0));
        assert_eq!(tiles.tile_at(1600.0, 500.0), Some(1));
        assert_eq!(tiles.tile_at(1600.0, 1599.0), Some(2));
        assert_eq!(tiles.tile_at(1600.0, 1700.0), None);
        assert_eq!(tiles.edge_at(1600.0, 10.0), 0);
        assert_eq!(tiles.edge_at(1600.0, 260.0), 1);
        assert_eq!(tiles.edge_at(1600.0, 1580.0), 3);
    }

    #[test]
    fn a_saved_row_is_put_right() {
        let tiles = Tiles::restored(
            vec![
                Tile { key: A, share: 0.0 },
                Tile {
                    key: B,
                    share: f32::NAN,
                },
                Tile { key: A, share: 4.0 },
                Tile {
                    key: C,
                    share: -2.0,
                },
                Tile { key: D, share: 2.5 },
            ],
            Some(SessionKey(99)),
        );
        assert_eq!(keys(&tiles), vec![A, B, C, D]);
        assert_eq!(shares(&tiles), vec![1.0, 1.0, 1.0, 2.5]);
        assert_eq!(tiles.focused(), Some(A));
        assert_eq!(
            Tiles::restored(tiles.list().to_vec(), Some(C)).focused(),
            Some(C)
        );
        assert_eq!(Tiles::restored(Vec::new(), Some(A)).focused(), None);
    }

    #[test]
    fn every_change_counts() {
        let mut tiles = Tiles::default();
        let mut last = tiles.version();
        let mut changed = |tiles: &Tiles, expected: bool| {
            let now = tiles.version();
            assert_eq!(now > last, expected, "{now} after {last}");
            assert!(now >= last);
            last = now;
        };

        tiles.put(A, Place::Focused, None);
        changed(&tiles, true);
        tiles.put(B, Place::Side, None);
        changed(&tiles, true);
        tiles.put(C, Place::Focused, None);
        changed(&tiles, true);
        tiles.put(A, Place::Side, None);
        changed(&tiles, true);
        tiles.put(A, Place::Side, None);
        changed(&tiles, false);
        tiles.put(D, Place::Tile(SessionKey(99)), None);
        changed(&tiles, false);
        tiles.put(D, Place::At(0), Some(500.0));
        changed(&tiles, false);

        assert!(tiles.focus(C));
        changed(&tiles, true);
        assert!(!tiles.focus(C));
        changed(&tiles, false);
        assert!(!tiles.focus(SessionKey(99)));
        changed(&tiles, false);

        assert!(tiles.resize(A, 600.0, 1500.0));
        changed(&tiles, true);
        assert!(!tiles.resize(C, 600.0, 1500.0));
        changed(&tiles, false);

        assert!(tiles.even());
        changed(&tiles, true);
        assert!(!tiles.even());
        changed(&tiles, false);

        assert!(tiles.set_pair(A, [0.5, 1.5]));
        changed(&tiles, true);
        assert!(!tiles.set_pair(A, [0.5, 1.5]));
        changed(&tiles, false);
        assert!(!tiles.set_pair(C, [0.5, 1.5]), "the last tile");
        changed(&tiles, false);

        assert!(tiles.remove(A));
        changed(&tiles, true);
        assert!(!tiles.remove(A));
        changed(&tiles, false);

        tiles.restore(vec![Tile { key: A, share: 1.0 }], Some(A));
        changed(&tiles, true);
    }

    #[test]
    fn a_change_of_widths_alone_leaves_the_placement() {
        let mut tiles = row(&[A, B, C], A);
        let placement = tiles.placement();
        assert!(tiles.resize(A, 600.0, 1500.0));
        assert!(tiles.even());
        assert!(tiles.set_pair(A, [0.25, 1.75]));
        assert_eq!(shares(&tiles), vec![0.25, 1.75, 1.0]);
        assert_eq!(tiles.placement(), placement);
        assert!(tiles.focus(B));
        assert_eq!(tiles.placement(), placement + 1);
        assert!(tiles.remove(C));
        assert_eq!(tiles.placement(), placement + 2);
        tiles.put(C, Place::Side, None);
        assert_eq!(tiles.placement(), placement + 3);
        tiles.put(D, Place::Focused, None);
        assert_eq!(tiles.placement(), placement + 4);
        tiles.restore(vec![Tile { key: A, share: 2.0 }], None);
        assert_eq!(tiles.placement(), placement + 5);
    }

    #[test]
    fn a_restored_row_has_shares_of_mean_1() {
        let mut tiles = Tiles::default();
        tiles.restore(
            vec![
                Tile { key: A, share: 3.0 },
                Tile { key: B, share: 1.0 },
                Tile {
                    key: C,
                    share: f32::NAN,
                },
            ],
            Some(B),
        );
        assert_eq!(keys(&tiles), vec![A, B, C]);
        assert_eq!(shares(&tiles), vec![1.8, 0.6, 0.6]);
        assert_eq!(tiles.focused(), Some(B));

        tiles.restore(
            vec![
                Tile {
                    key: A,
                    share: f32::MIN_POSITIVE,
                },
                Tile {
                    key: B,
                    share: f32::MAX,
                },
            ],
            None,
        );
        assert!(
            shares(&tiles).iter().all(|share| *share > 0.0),
            "{:?}",
            shares(&tiles)
        );
        assert_widths(&tiles, 1000.0, &[0.0, 1000.0]);
    }

    #[test]
    fn a_tile_moves_to_an_edge_with_its_share() {
        let mut tiles = Tiles::restored(
            vec![
                Tile { key: A, share: 1.0 },
                Tile { key: B, share: 2.0 },
                Tile { key: C, share: 3.0 },
            ],
            Some(B),
        );
        let placement = tiles.placement();
        assert!(tiles.move_to(A, 3));
        assert_eq!(keys(&tiles), vec![B, C, A]);
        assert_eq!(shares(&tiles), vec![2.0, 3.0, 1.0]);
        assert_eq!(tiles.focused(), Some(A));
        assert_eq!(tiles.placement(), placement + 1);
    }

    #[test]
    fn a_tile_moves_left() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(C, 0));
        assert_eq!(keys(&tiles), vec![C, A, B]);
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(C, 1));
        assert_eq!(keys(&tiles), vec![A, C, B]);
    }

    #[test]
    fn a_tile_moves_one_place_right() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(A, 2));
        assert_eq!(keys(&tiles), vec![B, A, C]);
    }

    #[test]
    fn its_own_edges_move_nothing() {
        let mut tiles = row(&[A, B, C], A);
        let (version, placement) = (tiles.version(), tiles.placement());
        for edge in [1, 2] {
            assert!(!tiles.move_to(B, edge), "edge {edge}");
            assert_eq!(keys(&tiles), vec![A, B, C], "edge {edge}");
            assert_eq!(tiles.focused(), Some(A), "edge {edge}");
            assert_eq!((tiles.version(), tiles.placement()), (version, placement));
        }
    }

    #[test]
    fn an_edge_past_the_end_is_the_end() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(A, 9));
        assert_eq!(keys(&tiles), vec![B, C, A]);
        let mut tiles = row(&[A, B, C], A);
        assert!(!tiles.move_to(C, 9));
        assert_eq!(keys(&tiles), vec![A, B, C]);
    }

    #[test]
    fn a_session_with_no_tile_moves_nothing() {
        let mut tiles = row(&[A, B, C], A);
        let version = tiles.version();
        assert!(!tiles.move_to(D, 0));
        assert_eq!(tiles.moved_index(D, 0), None);
        assert_eq!(keys(&tiles), vec![A, B, C]);
        assert_eq!((tiles.focused(), tiles.version()), (Some(A), version));
    }

    #[test]
    fn moved_index_says_where_a_tile_would_sit() {
        let tiles = row(&[A, B, C], B);
        let at = |key: SessionKey| (0..=3).map(|edge| tiles.moved_index(key, edge)).collect();
        let a: Vec<Option<usize>> = at(A);
        assert_eq!(a, vec![None, None, Some(1), Some(2)]);
        let c: Vec<Option<usize>> = at(C);
        assert_eq!(c, vec![Some(0), Some(1), None, None]);
    }

    #[test]
    fn a_row_without_width_does_not_panic() {
        let mut tiles = row(&[A, B], A);
        for width in [0.0, -10.0, f32::NAN, f32::INFINITY] {
            assert_eq!(tiles.widths(width), vec![0.0, 0.0], "{width}");
            assert!(!tiles.fits_another(width), "{width}");
            assert_eq!(tiles.tile_at(width, 10.0), None, "{width}");
            assert_eq!(tiles.edge_at(width, 10.0), 0, "{width}");
            assert!(!tiles.resize(A, 300.0, width), "{width}");
            assert_eq!(
                tiles.put(C, Place::At(0), Some(width)),
                Put::Nothing,
                "{width}"
            );
        }
        assert_eq!(shares(&tiles), vec![1.0, 1.0]);
        assert!(!tiles.resize(A, f32::NAN, 1500.0));
        assert_eq!(tiles.tile_at(1500.0, f32::NAN), None);
        assert_eq!(tiles.edge_at(1500.0, f32::NAN), 0);
        assert!(Tiles::default().fits_another(f32::NAN));
        assert_eq!(Tiles::default().edge_at(1500.0, 10.0), 0);
    }
}
