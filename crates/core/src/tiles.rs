//! The row of tiles the window shows sessions in: its columns of one tile or two, which
//! sessions, in which order, how wide and high, and which one is focused.

use crate::sessions::SessionKey;

/// The least a tile is opened or dragged to, in points.
pub const MIN_TILE_WIDTH: f32 = 420.0;

/// The least a tile of a column of two is opened or dragged to, in points.
pub const MIN_TILE_HEIGHT: f32 = 280.0;

/// One column of the row: one tile, or two, one above the other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// Its part of the row's width against the others'; above 0.
    pub share: f32,
    /// Its tile, or the upper of its two.
    pub top: SessionKey,
    pub under: Option<Under>,
}

/// The lower tile of a column of two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Under {
    pub key: SessionKey,
    /// The upper tile's part of the column's height as it was set; above 0 and below 1. What
    /// is drawn keeps both tiles usable ([`Tiles::rects`]).
    pub split: f32,
}

/// Where a tile is in its column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Alone in its column.
    Whole,
    /// The upper of two.
    Top,
    /// The lower of two.
    Bottom,
}

/// One tile as laid out: `rect` is `[x, y, w, h]` from the row's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Laid {
    pub key: SessionKey,
    pub column: usize,
    pub level: Level,
    pub rect: [f32; 4],
}

/// A place in the row a tile can go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// A column of its own at this edge between columns, as `edge_at` counts them.
    Edge(usize),
    /// Over this tile, in its column.
    Above(SessionKey),
    /// Under this tile, in its column.
    Below(SessionKey),
}

/// What is known of the row's size, in points. What is not known does not hold anything back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowSize {
    pub width: Option<f32>,
    pub height: Option<f32>,
}

/// Which part of a tile a point is in, for a drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Left,
    Right,
    Upper,
    Lower,
}

/// The outer quarters of `rect`'s width are `Left` and `Right`; between them its upper half
/// is `Upper` and its lower half `Lower`. `at` is in the coordinates of `rect`.
pub fn part_at(rect: [f32; 4], at: [f32; 2]) -> Part {
    let [x, y, w, h] = rect;
    if at[0] < x + w / 4.0 {
        Part::Left
    } else if at[0] >= x + w - w / 4.0 {
        Part::Right
    } else if at[1] < y + h / 2.0 {
        Part::Upper
    } else {
        Part::Lower
    }
}

/// Where a session is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// In the focused tile, in place of its session; in a first tile when there is none.
    Focused,
    /// In a new column right of the focused tile's. As `Focused` when there is no room.
    Side,
    /// In a new column at this position, 0 being leftmost. Nothing when there is no room.
    At(usize),
    /// In place of this tile's session.
    Tile(SessionKey),
    /// In a new tile over this tile, in its column. Nothing when the column could not take it
    /// (see [`Tiles::takes`]).
    Above(SessionKey),
    /// In a new tile under this tile, in its column. Nothing when the column could not take
    /// it (see [`Tiles::takes`]).
    Below(SessionKey),
}

/// What `Tiles::put` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Put {
    Nothing,
    FocusedOnly,
    Added,
    Replaced(SessionKey),
}

/// The row of tiles, its columns left to right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tiles {
    columns: Vec<Column>,
    /// `None` only while the row is empty.
    focused: Option<SessionKey>,
    /// Goes up with every change: see [`Tiles::version`].
    version: u64,
    /// Goes up with every change but of widths and splits: see [`Tiles::placement`].
    placement: u64,
}

impl Tiles {
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Every tile's session, column by column from the left, the upper before the lower.
    pub fn keys(&self) -> Vec<SessionKey> {
        self.columns
            .iter()
            .flat_map(|column| std::iter::once(column.top).chain(column.under.map(|u| u.key)))
            .collect()
    }

    /// How many tiles there are.
    pub fn len(&self) -> usize {
        self.columns.len() + self.columns.iter().filter(|c| c.under.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn focused(&self) -> Option<SessionKey> {
        self.focused
    }

    pub fn contains(&self, key: SessionKey) -> bool {
        self.find(key).is_some()
    }

    /// The column `key`'s tile is in, and where in it.
    pub fn find(&self, key: SessionKey) -> Option<(usize, Level)> {
        self.columns.iter().enumerate().find_map(|(index, column)| {
            let level = match column.under {
                None if column.top == key => Level::Whole,
                Some(_) if column.top == key => Level::Top,
                Some(under) if under.key == key => Level::Bottom,
                _ => return None,
            };
            Some((index, level))
        })
    }

    /// The other tile of `key`'s column, when it has two.
    pub fn mate(&self, key: SessionKey) -> Option<SessionKey> {
        let (index, level) = self.find(key)?;
        let column = &self.columns[index];
        match level {
            Level::Whole => None,
            Level::Top => column.under.map(|under| under.key),
            Level::Bottom => Some(column.top),
        }
    }

    /// The upper tile's part of its column's height, when `top` is the upper of two.
    pub fn split(&self, top: SessionKey) -> Option<f32> {
        match self.find(top)? {
            (index, Level::Top) => self.columns[index].under.map(|under| under.split),
            _ => None,
        }
    }

    /// Goes up with every change, the focus included. Only tests read it: the sidebar is
    /// rebuilt by [`Tiles::placement`].
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Goes up when the sessions in the row, their places or the focus change: not when only
    /// the widths or the splits do ([`Tiles::resize`], [`Tiles::even`],
    /// [`Tiles::resize_split`]).
    pub fn placement(&self) -> u64 {
        self.placement
    }

    /// Each column's width in a row `row_width` wide, in order; they add up to `row_width`.
    /// Every width is 0 when `row_width` is 0 or not finite.
    pub fn widths(&self, row_width: f32) -> Vec<f32> {
        if !usable(row_width) {
            return vec![0.0; self.columns.len()];
        }
        let total: f32 = self.columns.iter().map(|column| column.share).sum();
        self.columns
            .iter()
            .map(|column| column.share / total * row_width)
            .collect()
    }

    /// Every tile's rectangle in a row of `size`, in the order of `keys`. The rectangles meet
    /// with no gap and end at the row's right and bottom edges. Along an axis whose length is 0
    /// or not finite, every length is 0. The two of a column are drawn as `upper_height`
    /// says, which leaves the split as it was set.
    pub fn rects(&self, size: [f32; 2]) -> Vec<Laid> {
        let [width, height] = size.map(|length| if usable(length) { length } else { 0.0 });
        let widths = self.widths(width);
        let last = self.columns.len().saturating_sub(1);
        let (mut x, mut sum) = (0.0, 0.0);
        let mut laid = Vec::with_capacity(self.len());
        for (index, (column, &w)) in self.columns.iter().zip(&widths).enumerate() {
            sum += w;
            // On the row's grid, `x + w` is exactly where the next column starts, and the last
            // column ends exactly at the row's edge.
            let end = if index == last {
                width
            } else {
                on_grid(sum, width)
            };
            let w = end - x;
            let tile = |key, level, y: f32, h: f32| Laid {
                key,
                column: index,
                level,
                rect: [x, y, w, h],
            };
            match column.under {
                None => laid.push(tile(column.top, Level::Whole, 0.0, height)),
                Some(under) => {
                    let upper = on_grid(upper_height(under.split, height), height);
                    laid.push(tile(column.top, Level::Top, 0.0, upper));
                    laid.push(tile(under.key, Level::Bottom, upper, height - upper));
                }
            }
            x = end;
        }
        laid
    }

    /// The tile under `at`, measured from the row's top left. None outside the row.
    pub fn tile_at(&self, size: [f32; 2], at: [f32; 2]) -> Option<Laid> {
        let [width, height] = size;
        if !usable(width) || !usable(height) {
            return None;
        }
        if !(0.0..width).contains(&at[0]) || !(0.0..height).contains(&at[1]) {
            return None;
        }
        // The last tile that starts left of and above `at`: the tiles are in order, column by
        // column and down each, so no point falls between two.
        self.rects(size)
            .into_iter()
            .rev()
            .find(|laid| laid.rect[0] <= at[0] && laid.rect[1] <= at[1])
    }

    /// Every column and one more could each have `MIN_TILE_WIDTH`. True for an empty row.
    pub fn fits_another(&self, row_width: f32) -> bool {
        self.columns.is_empty()
            || usable(row_width) && (self.columns.len() + 1) as f32 * MIN_TILE_WIDTH <= row_width
    }

    /// A column of a row this high can hold two tiles of `MIN_TILE_HEIGHT`.
    pub fn fits_two(row_height: f32) -> bool {
        usable(row_height) && 2.0 * MIN_TILE_HEIGHT <= row_height
    }

    /// The nearest of the edges between columns a new column can open at: 0 to the number of
    /// columns.
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

    /// Whether a session that is in no tile, shown at `spot`, would open a tile there: at an
    /// edge when another column fits; over or under a tile that has its column to itself when
    /// the column fits two.
    pub fn takes(&self, spot: Spot, row: RowSize) -> bool {
        match spot {
            Spot::Edge(_) => row.width.is_none_or(|width| self.fits_another(width)),
            Spot::Above(key) | Spot::Below(key) => self.takes_a_mate(key, row),
        }
    }

    /// `key`'s tile has its column to itself and the column fits two.
    fn takes_a_mate(&self, key: SessionKey, row: RowSize) -> bool {
        matches!(self.find(key), Some((_, Level::Whole))) && row.height.is_none_or(Tiles::fits_two)
    }

    /// Whether moving `key`'s tile to `spot` is allowed and would change the row. An edge past
    /// the number of columns is that number.
    pub fn would_move(&self, key: SessionKey, spot: Spot, row: RowSize) -> bool {
        let Some((index, level)) = self.find(key) else {
            return false;
        };
        match spot {
            Spot::Edge(edge) if level == Level::Whole => {
                let edge = edge.min(self.columns.len());
                edge != index && edge != index + 1
            }
            // One of two gets a column of its own at any edge, the two beside its own included.
            Spot::Edge(_) => row.width.is_none_or(|width| self.fits_another(width)),
            Spot::Above(other) | Spot::Below(other) if other == key => false,
            Spot::Above(other) if self.mate(key) == Some(other) => level == Level::Bottom,
            Spot::Below(other) if self.mate(key) == Some(other) => level == Level::Top,
            Spot::Above(other) | Spot::Below(other) => self.takes_a_mate(other, row),
        }
    }

    /// Moves `key`'s tile to `spot`, and focuses it. False, and nothing changed, when
    /// [`Tiles::would_move`] is false.
    ///
    /// Out of its place, a column it had to itself goes, and a mate it leaves has the column
    /// with its share. At an edge, a tile that had its column keeps its share; one out of a
    /// column of two gets the mean of the columns' shares, and the columns are kept to
    /// `MIN_TILE_WIDTH` as when a tile is added. Over or under a tile, it shares that tile's
    /// column half and half; two mates swap and keep their heights.
    pub(crate) fn move_to(&mut self, key: SessionKey, spot: Spot, row: RowSize) -> bool {
        if !self.would_move(key, spot, row) {
            return false;
        }
        let Some((index, level)) = self.find(key) else {
            return false;
        };
        match spot {
            Spot::Above(other) | Spot::Below(other) if self.mate(key) == Some(other) => {
                let column = &mut self.columns[index];
                if let Some(under) = column.under.as_mut() {
                    std::mem::swap(&mut column.top, &mut under.key);
                    under.split = 1.0 - under.split;
                }
            }
            Spot::Edge(edge) => {
                let edge = edge.min(self.columns.len());
                match self.take_out(index, level) {
                    // Out of its place, the columns right of it are one place further left.
                    Some(share) => {
                        let at = if edge <= index { edge } else { edge - 1 };
                        self.columns.insert(at, whole(key, share));
                    }
                    // No column went: nothing shifts.
                    None => {
                        let share = self.mean_share();
                        self.columns.insert(edge, whole(key, share));
                        if let Some(width) = row.width {
                            self.keep_to_minimum(width);
                        }
                    }
                }
            }
            Spot::Above(other) | Spot::Below(other) => {
                self.take_out(index, level);
                self.stack(other, key, matches!(spot, Spot::Above(_)));
            }
        }
        self.focused = Some(key);
        self.placed();
        true
    }

    /// Takes the tile at `index`, `level` out of its column: the share of a column it had to
    /// itself, which goes, or none when its mate keeps the column. The focus is left as it is.
    fn take_out(&mut self, index: usize, level: Level) -> Option<f32> {
        let column = &mut self.columns[index];
        match (level, column.under.take()) {
            (Level::Top, Some(under)) => column.top = under.key,
            (Level::Bottom, _) => {}
            _ => return Some(self.columns.remove(index).share),
        }
        None
    }

    /// Puts `key` over or under `other`, alone in its column, half and half.
    fn stack(&mut self, other: SessionKey, key: SessionKey, above: bool) {
        let Some((index, Level::Whole)) = self.find(other) else {
            return;
        };
        let column = &mut self.columns[index];
        let (top, lower) = if above { (key, other) } else { (other, key) };
        column.top = top;
        column.under = Some(Under {
            key: lower,
            split: 0.5,
        });
    }

    /// The mean of the columns' shares; 1 for an empty row.
    fn mean_share(&self) -> f32 {
        if self.columns.is_empty() {
            1.0
        } else {
            self.columns.iter().map(|column| column.share).sum::<f32>() / self.columns.len() as f32
        }
    }

    /// Shows `key` at `place`. A session already in a tile only has it focused.
    pub(crate) fn put(&mut self, key: SessionKey, place: Place, row: RowSize) -> Put {
        if self.contains(key) {
            self.focus(key);
            return Put::FocusedOnly;
        }
        let room = row.width.is_none_or(|width| self.fits_another(width));
        let place = match place {
            Place::Side if !room => Place::Focused,
            place => place,
        };
        match place {
            Place::Focused => match self.focused {
                Some(old) => self.replace(old, key),
                None => self.add(0, key, row.width),
            },
            Place::Side => {
                let index = self.focused.and_then(|old| self.find(old));
                self.add(index.map_or(0, |(index, _)| index + 1), key, row.width)
            }
            Place::At(_) if !room => Put::Nothing,
            Place::At(index) => self.add(index.min(self.columns.len()), key, row.width),
            Place::Tile(old) if self.contains(old) => self.replace(old, key),
            Place::Tile(_) => Put::Nothing,
            Place::Above(other) | Place::Below(other) if self.takes_a_mate(other, row) => {
                self.stack(other, key, matches!(place, Place::Above(_)));
                self.focused = Some(key);
                self.placed();
                Put::Added
            }
            Place::Above(_) | Place::Below(_) => Put::Nothing,
        }
    }

    /// Shows `key` in `old`'s tile, upper or lower, with every size as it was, and focuses it.
    fn replace(&mut self, old: SessionKey, key: SessionKey) -> Put {
        let Some((index, level)) = self.find(old) else {
            return Put::Nothing;
        };
        let column = &mut self.columns[index];
        match (level, column.under.as_mut()) {
            (Level::Bottom, Some(under)) => under.key = key,
            _ => column.top = key,
        }
        self.focused = Some(key);
        self.placed();
        Put::Replaced(old)
    }

    /// Opens a column for `key` at `index` with the mean of the others' shares, then raises any
    /// column under `MIN_TILE_WIDTH` to it when the row's width is known.
    fn add(&mut self, index: usize, key: SessionKey, row_width: Option<f32>) -> Put {
        let share = self.mean_share();
        self.columns.insert(index, whole(key, share));
        if let Some(row_width) = row_width {
            self.keep_to_minimum(row_width);
        }
        self.focused = Some(key);
        self.placed();
        Put::Added
    }

    /// Raises each column under `MIN_TILE_WIDTH` to it and takes the difference from the
    /// columns above it, in proportion to what each has above it. The total of the shares stays.
    fn keep_to_minimum(&mut self, row_width: f32) {
        if !usable(row_width) || self.columns.len() as f32 * MIN_TILE_WIDTH > row_width {
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
        let total: f32 = self.columns.iter().map(|column| column.share).sum();
        for (column, width) in self.columns.iter_mut().zip(widths) {
            column.share = width / row_width * total;
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

    /// Takes `key`'s tile out. Out of a column of two, the other has the column, and the focus
    /// if `key` had it. A column it had to itself goes, and the focus goes to the upper tile of
    /// the column now at its place, else of the one on its left.
    pub(crate) fn remove(&mut self, key: SessionKey) -> bool {
        let Some((index, level)) = self.find(key) else {
            return false;
        };
        let gone = self.take_out(index, level).is_some();
        if self.focused == Some(key) {
            self.focused = if gone {
                let next = self
                    .columns
                    .get(index)
                    .or(index.checked_sub(1).map(|i| &self.columns[i]));
                next.map(|column| column.top)
            } else {
                Some(self.columns[index].top)
            };
        }
        self.placed();
        true
    }

    /// Makes the column of `left`, any tile of it, `width` wide and gives or takes the
    /// difference from the column on its right. Neither goes below `MIN_TILE_WIDTH`; nothing
    /// happens when the two have less than twice that between them, or `left`'s is the last
    /// column.
    pub(crate) fn resize(&mut self, left: SessionKey, width: f32, row_width: f32) -> bool {
        let Some((index, _)) = self.find(left) else {
            return false;
        };
        if index + 1 >= self.columns.len() || !usable(row_width) || width.is_nan() {
            return false;
        }
        let widths = self.widths(row_width);
        let pair = widths[index] + widths[index + 1];
        if pair < 2.0 * MIN_TILE_WIDTH {
            return false;
        }
        let width = width.clamp(MIN_TILE_WIDTH, pair - MIN_TILE_WIDTH);
        let shares = self.columns[index].share + self.columns[index + 1].share;
        let share = shares * width / pair;
        if share == self.columns[index].share {
            return false;
        }
        self.columns[index].share = share;
        self.columns[index + 1].share = shares - share;
        self.version += 1;
        true
    }

    /// Gives the column of `left`, any tile of it, and the one on its right the shares
    /// `shares`, as they were before a drag of the divider between them. A change of widths
    /// only: the placement stays.
    pub(crate) fn set_pair(&mut self, left: SessionKey, shares: [f32; 2]) -> bool {
        let Some(index) = self
            .find(left)
            .map(|(index, _)| index)
            .filter(|i| i + 1 < self.columns.len())
        else {
            return false;
        };
        if [self.columns[index].share, self.columns[index + 1].share] == shares {
            return false;
        }
        [self.columns[index].share, self.columns[index + 1].share] = shares;
        self.version += 1;
        true
    }

    /// Every column gets the same width; the splits stay as they are.
    pub(crate) fn even(&mut self) -> bool {
        if self.columns.iter().all(|column| column.share == 1.0) {
            return false;
        }
        for column in &mut self.columns {
            column.share = 1.0;
        }
        self.version += 1;
        true
    }

    /// Makes `top`, the upper of two, `height` high in a row `row_height` high, neither of the
    /// two below `MIN_TILE_HEIGHT`. False, and nothing changed, when `top` is not the upper of
    /// two, `height` is not a number or the row does not fit two. A change of heights only:
    /// the placement stays.
    pub(crate) fn resize_split(&mut self, top: SessionKey, height: f32, row_height: f32) -> bool {
        if height.is_nan() || !Tiles::fits_two(row_height) {
            return false;
        }
        let height = height.clamp(MIN_TILE_HEIGHT, row_height - MIN_TILE_HEIGHT);
        self.set_split(top, height / row_height)
    }

    /// Gives `top`, the upper of two, the part `split` of its column's height exactly, as it
    /// was before a drag of the line between the two. False for a split not above 0 and below
    /// 1. A change of heights only: the placement stays.
    pub(crate) fn set_split(&mut self, top: SessionKey, split: f32) -> bool {
        let Some((index, Level::Top)) = self.find(top) else {
            return false;
        };
        let Some(under) = self.columns[index].under.as_mut() else {
            return false;
        };
        if !(split > 0.0 && split < 1.0) || under.split == split {
            return false;
        }
        under.split = split;
        self.version += 1;
        true
    }

    /// Replaces the row with `saved`, put right as [`Tiles::restored`] does, its shares then
    /// scaled so that their mean is 1: shares too large to add up still leave a row to share.
    /// An empty `saved` empties the row.
    pub(crate) fn restore(&mut self, saved: Vec<Column>, focused: Option<SessionKey>) {
        let Tiles {
            mut columns,
            focused,
            ..
        } = Tiles::restored(saved, focused);
        self.placed();
        if columns.is_empty() {
            self.columns.clear();
            self.focused = None;
            return;
        }
        let mean = columns
            .iter()
            .map(|column| f64::from(column.share))
            .sum::<f64>()
            / columns.len() as f64;
        for column in &mut columns {
            // A share far below the others' would round to 0, which is no share.
            column.share = ((f64::from(column.share) / mean) as f32).max(f32::MIN_POSITIVE);
        }
        self.columns = columns;
        self.focused = focused;
    }

    /// From what was saved: shares that are not finite or not above 0 become 1, and splits not
    /// above 0 and below 1 become 0.5. A key met before is left out: the lower tile of a column
    /// whose upper one is left out has the column; a column with neither goes. The focus is
    /// `focused` when it is among them, else the first column's upper tile.
    pub(crate) fn restored(saved: Vec<Column>, focused: Option<SessionKey>) -> Tiles {
        let mut tiles = Tiles {
            columns: Vec::with_capacity(saved.len()),
            focused: None,
            version: 0,
            placement: 0,
        };
        for column in saved {
            let under = column
                .under
                .filter(|under| under.key != column.top && !tiles.contains(under.key));
            let (top, under) = match (tiles.contains(column.top), under) {
                (false, under) => (column.top, under),
                (true, Some(under)) => (under.key, None),
                (true, None) => continue,
            };
            let share = if column.share.is_finite() && column.share > 0.0 {
                column.share
            } else {
                1.0
            };
            let under = under.map(|under| Under {
                split: if under.split > 0.0 && under.split < 1.0 {
                    under.split
                } else {
                    0.5
                },
                ..under
            });
            tiles.columns.push(Column { share, top, under });
        }
        tiles.focused = focused
            .filter(|key| tiles.contains(*key))
            .or(tiles.columns.first().map(|column| column.top));
        tiles
    }

    /// Counts a change of which sessions are in the row, their places or the focus.
    fn placed(&mut self) {
        self.version += 1;
        self.placement += 1;
    }
}

/// A column holding `key` alone.
fn whole(key: SessionKey, share: f32) -> Column {
    Column {
        share,
        top: key,
        under: None,
    }
}

/// How high the upper tile of a column split at `split` is drawn in a row `height` high: its
/// part of the height, kept so that neither of the two is lower than `MIN_TILE_HEIGHT` while
/// the row fits two; in a lower row, half.
fn upper_height(split: f32, height: f32) -> f32 {
    if Tiles::fits_two(height) {
        (split * height).clamp(MIN_TILE_HEIGHT, height - MIN_TILE_HEIGHT)
    } else {
        height / 2.0
    }
}

/// `length` at the nearest multiple of the step f32 has just below `whole`, and no more than
/// `whole`. Lengths on that grid up to `whole` add and subtract with no rounding, so tiles
/// placed on it meet and end at the row's edges exactly.
fn on_grid(length: f32, whole: f32) -> f32 {
    let step = whole - whole.next_down();
    ((length / step).round() * step).min(whole)
}

/// A length the arithmetic can divide: finite and above 0.
fn usable(length: f32) -> bool {
    length.is_finite() && length > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: SessionKey = SessionKey(1);
    const B: SessionKey = SessionKey(2);
    const C: SessionKey = SessionKey(3);
    const D: SessionKey = SessionKey(4);
    const E: SessionKey = SessionKey(5);

    /// A row 1,600 by 900.
    const SIZE: RowSize = RowSize {
        width: Some(1600.0),
        height: Some(900.0),
    };

    fn sized(width: f32, height: f32) -> RowSize {
        RowSize {
            width: Some(width),
            height: Some(height),
        }
    }

    /// A row whose width alone is known.
    fn wide(width: f32) -> RowSize {
        RowSize {
            width: Some(width),
            height: None,
        }
    }

    fn two(top: SessionKey, under: SessionKey, split: f32) -> Column {
        Column {
            share: 1.0,
            top,
            under: Some(Under { key: under, split }),
        }
    }

    /// A row of `keys`, a column each, with even shares, `focused` focused.
    fn row(keys: &[SessionKey], focused: SessionKey) -> Tiles {
        let columns = keys.iter().map(|&key| whole(key, 1.0)).collect();
        Tiles::restored(columns, Some(focused))
    }

    /// Puts `under`, in no tile yet, under `top`, alone in its column, with `split`.
    fn stack(tiles: &mut Tiles, top: SessionKey, under: SessionKey, split: f32) {
        let (column, _) = tiles.find(top).expect("top is in a tile");
        tiles.columns[column].under = Some(Under { key: under, split });
    }

    /// What `put` is asked to do for a session in no tile to end at `spot`.
    fn place_of(spot: Spot) -> Place {
        match spot {
            Spot::Edge(edge) => Place::At(edge),
            Spot::Above(key) => Place::Above(key),
            Spot::Below(key) => Place::Below(key),
        }
    }

    fn keys(tiles: &Tiles) -> Vec<SessionKey> {
        tiles.keys()
    }

    /// Each column's tiles, the upper first.
    fn shape(tiles: &Tiles) -> Vec<Vec<SessionKey>> {
        tiles
            .columns()
            .iter()
            .map(|column| {
                std::iter::once(column.top)
                    .chain(column.under.map(|under| under.key))
                    .collect()
            })
            .collect()
    }

    fn shares(tiles: &Tiles) -> Vec<f32> {
        tiles.columns().iter().map(|column| column.share).collect()
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
            assert_eq!(
                tiles.put(A, place, RowSize::default()),
                Put::Added,
                "{place:?}"
            );
            assert_eq!(keys(&tiles), vec![A], "{place:?}");
            assert_eq!(tiles.focused(), Some(A), "{place:?}");
        }
    }

    #[test]
    fn the_focused_tile_swaps_its_session() {
        let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 2.0)], Some(B));
        assert_eq!(
            tiles.put(C, Place::Focused, RowSize::default()),
            Put::Replaced(B)
        );
        assert_eq!(keys(&tiles), vec![A, C]);
        assert_eq!(shares(&tiles), vec![1.0, 2.0]);
        assert_eq!(tiles.focused(), Some(C));
    }

    #[test]
    fn a_tile_of_two_swaps_its_session() {
        let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 3.0)], Some(A));
        stack(&mut tiles, B, C, 0.25);
        assert_eq!(tiles.put(D, Place::Tile(C), SIZE), Put::Replaced(C));
        assert_eq!(shape(&tiles), vec![vec![A], vec![B, D]]);
        assert_eq!(tiles.focused(), Some(D));
        assert_eq!(tiles.put(E, Place::Focused, SIZE), Put::Replaced(D));
        assert_eq!(tiles.put(C, Place::Tile(B), SIZE), Put::Replaced(B));
        assert_eq!(shape(&tiles), vec![vec![A], vec![C, E]]);
        assert_eq!(tiles.focused(), Some(C));
        assert_eq!(shares(&tiles), vec![1.0, 3.0]);
        assert_eq!(tiles.split(C), Some(0.25));
    }

    #[test]
    fn a_session_is_never_in_two_tiles() {
        for place in [
            Place::Side,
            Place::At(0),
            Place::Tile(A),
            Place::Above(A),
            Place::Below(A),
        ] {
            let mut tiles = row(&[A, B], A);
            assert_eq!(
                tiles.put(B, place, RowSize::default()),
                Put::FocusedOnly,
                "{place:?}"
            );
            assert_eq!(shape(&tiles), vec![vec![A], vec![B]], "{place:?}");
            assert_eq!(tiles.focused(), Some(B), "{place:?}");
        }
    }

    #[test]
    fn to_the_side_opens_right_of_the_focused_tile() {
        let mut tiles = row(&[A, B], A);
        assert_eq!(tiles.put(C, Place::Side, RowSize::default()), Put::Added);
        assert_eq!(keys(&tiles), vec![A, C, B]);
        assert_eq!(tiles.focused(), Some(C));
        assert_eq!(tiles.put(D, Place::At(0), RowSize::default()), Put::Added);
        assert_eq!(keys(&tiles), vec![D, A, C, B]);
        assert_eq!(tiles.focused(), Some(D));

        let mut tiles = row(&[A, B], A);
        stack(&mut tiles, A, C, 0.5);
        assert!(tiles.focus(C));
        assert_eq!(tiles.put(D, Place::Side, RowSize::default()), Put::Added);
        assert_eq!(shape(&tiles), vec![vec![A, C], vec![D], vec![B]]);
    }

    #[test]
    fn a_new_tile_takes_an_equal_part() {
        let uneven = Tiles::restored(vec![whole(A, 3.0), whole(B, 1.0)], Some(A));

        let mut tiles = uneven.clone();
        assert_eq!(tiles.put(C, Place::Side, RowSize::default()), Put::Added);
        assert_eq!(keys(&tiles), vec![A, C, B]);
        assert_widths(&tiles, 1600.0, &[800.0, 533.3, 266.7]);

        let mut tiles = uneven;
        assert_eq!(tiles.put(C, Place::Side, wide(1600.0)), Put::Added);
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
        assert_eq!(tiles.put(C, Place::Side, wide(1000.0)), Put::Replaced(A));
        assert_eq!(keys(&tiles), vec![C, B]);
        assert_eq!(tiles.put(D, Place::At(0), wide(1000.0)), Put::Nothing);
        assert_eq!(keys(&tiles), vec![C, B]);
        assert_eq!(tiles.focused(), Some(C));
    }

    #[test]
    fn room_is_420_points_a_tile() {
        let one = row(&[A], A);
        assert!(!one.fits_another(839.0));
        assert!(one.fits_another(840.0));
        assert!(Tiles::default().fits_another(100.0));

        let mut stacked = row(&[A], A);
        stack(&mut stacked, A, B, 0.5);
        assert!(!stacked.fits_another(839.0));
        assert!(stacked.fits_another(840.0));
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
        assert!(tiles.is_empty());
        assert_eq!(tiles.focused(), None);
        assert!(!tiles.remove(A));
    }

    #[test]
    fn closing_leaves_the_others_shares() {
        let mut tiles = Tiles::restored(vec![whole(A, 3.0), whole(B, 1.0), whole(C, 1.0)], Some(A));
        assert!(tiles.remove(B));
        assert_eq!(shares(&tiles), vec![3.0, 1.0]);
        assert_widths(&tiles, 1600.0, &[1200.0, 400.0]);
    }

    #[test]
    fn closing_one_of_two_gives_the_other_the_column() {
        for (gone, stays) in [(C, B), (B, C)] {
            let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 3.0)], Some(A));
            stack(&mut tiles, B, C, 0.25);
            assert!(tiles.focus(gone));
            assert!(tiles.remove(gone), "{gone:?}");
            assert_eq!(shape(&tiles), vec![vec![A], vec![stays]], "{gone:?}");
            assert_eq!(shares(&tiles), vec![1.0, 3.0], "{gone:?}");
            assert_eq!(tiles.focused(), Some(stays), "{gone:?}");
        }

        let mut tiles = row(&[A, B], A);
        stack(&mut tiles, B, C, 0.25);
        assert!(tiles.remove(C));
        assert_eq!(tiles.focused(), Some(A), "the focus was elsewhere");
    }

    #[test]
    fn closing_a_column_moves_the_focus_right_then_left() {
        let start = |focused: SessionKey| {
            let mut tiles = row(&[A, B, D], focused);
            stack(&mut tiles, B, C, 0.5);
            tiles
        };
        let mut tiles = start(A);
        assert!(tiles.remove(A));
        assert_eq!(shape(&tiles), vec![vec![B, C], vec![D]]);
        assert_eq!(tiles.focused(), Some(B));

        let mut tiles = start(D);
        assert!(tiles.remove(D));
        assert_eq!(shape(&tiles), vec![vec![A], vec![B, C]]);
        assert_eq!(tiles.focused(), Some(B));
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
    fn a_divider_moves_width_between_two_columns_of_any_shape() {
        let start = || {
            let mut tiles = row(&[A, C], A);
            stack(&mut tiles, A, B, 0.5);
            tiles
        };
        let (mut by_upper, mut by_lower) = (start(), start());
        assert!(by_upper.resize(A, 600.0, 1500.0));
        assert!(by_lower.resize(B, 600.0, 1500.0));
        assert_widths(&by_upper, 1500.0, &[600.0, 900.0]);
        assert_eq!(by_upper, by_lower);
        assert!(!by_lower.resize(C, 600.0, 1500.0), "the last column");

        assert!(by_upper.set_pair(A, [0.5, 1.5]));
        assert!(by_lower.set_pair(B, [0.5, 1.5]));
        assert_eq!(shares(&by_lower), vec![0.5, 1.5]);
        assert_eq!(by_upper, by_lower);
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
    fn evening_out_leaves_the_splits() {
        let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 3.0)], Some(A));
        stack(&mut tiles, B, C, 0.25);
        assert!(tiles.even());
        assert_eq!(shares(&tiles), vec![1.0, 1.0]);
        assert_eq!(tiles.split(B), Some(0.25));
    }

    #[test]
    fn the_tile_and_the_edge_under_a_point() {
        let tiles = Tiles::restored(vec![whole(A, 5.0), whole(B, 5.0), whole(C, 6.0)], Some(A));
        let at = |x: f32| {
            tiles
                .tile_at([1600.0, 900.0], [x, 10.0])
                .map(|laid| laid.key)
        };
        assert_widths(&tiles, 1600.0, &[500.0, 500.0, 600.0]);
        assert_eq!(at(499.0), Some(A));
        assert_eq!(at(500.0), Some(B));
        assert_eq!(at(1599.0), Some(C));
        assert_eq!(at(1700.0), None);
        assert_eq!(tiles.edge_at(1600.0, 10.0), 0);
        assert_eq!(tiles.edge_at(1600.0, 260.0), 1);
        assert_eq!(tiles.edge_at(1600.0, 1580.0), 3);
    }

    /// `[A share 1][B/C share 3, split 0.25]`.
    fn uneven_stack() -> Tiles {
        let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 3.0)], Some(A));
        stack(&mut tiles, B, C, 0.25);
        tiles
    }

    #[test]
    fn rects_tile_the_row() {
        let laid = |key, column, level, rect| Laid {
            key,
            column,
            level,
            rect,
        };
        assert_eq!(
            uneven_stack().rects([1600.0, 1600.0]),
            vec![
                laid(A, 0, Level::Whole, [0.0, 0.0, 400.0, 1600.0]),
                laid(B, 1, Level::Top, [400.0, 0.0, 1200.0, 400.0]),
                laid(C, 1, Level::Bottom, [400.0, 400.0, 1200.0, 1200.0]),
            ]
        );

        let rects = row(&[A, B, C], A).rects([1000.0, 900.0]);
        for pair in rects.windows(2) {
            let [left, right] = [pair[0].rect, pair[1].rect];
            assert_eq!(left[0] + left[2], right[0], "{rects:?}");
        }
        let last = rects[2].rect;
        assert_eq!(last[0] + last[2], 1000.0, "{rects:?}");
    }

    #[test]
    fn the_drawn_split_keeps_both_tiles_usable() {
        // The split, the row's height, and the two tiles' heights as drawn.
        for (split, height, upper, lower) in [
            (0.2, 600.0, 280.0, 320.0),
            (0.8, 600.0, 320.0, 280.0),
            (0.2, 500.0, 250.0, 250.0),
            (0.2, 1400.0, 280.0, 1120.0),
            (0.25, 1400.0, 350.0, 1050.0),
        ] {
            let tiles = Tiles::restored(vec![two(A, B, split)], Some(A));
            let rects: Vec<[f32; 4]> = tiles
                .rects([1600.0, height])
                .into_iter()
                .map(|laid| laid.rect)
                .collect();
            let what = format!("{split} of {height}");
            assert_eq!(
                rects,
                vec![[0.0, 0.0, 1600.0, upper], [0.0, upper, 1600.0, lower]],
                "{what}"
            );
            assert_eq!(rects[1][1] + rects[1][3], height, "{what}");
            assert_eq!(tiles.split(A), Some(split), "{what}: the split as set");
            let at = |y: f32| tiles.tile_at([1600.0, height], [800.0, y]).map(|l| l.key);
            assert_eq!(
                (at(upper - 0.5), at(upper + 0.5)),
                (Some(A), Some(B)),
                "{what}"
            );
        }
    }

    #[test]
    fn rects_end_exactly_at_the_row_edges() {
        let tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 5.0)], Some(A));
        let rects = tiles.rects([1234.567, 900.0]);
        let [left, last] = [rects[0].rect, rects[1].rect];
        assert_eq!(left[0] + left[2], last[0], "{rects:?}");
        assert_eq!(last[0] + last[2], 1234.567, "{rects:?}");

        let tiles = Tiles::restored(vec![two(A, B, 5.0 / 101.0)], Some(A));
        let rects = tiles.rects([1600.0, 900.3]);
        let [upper, lower] = [rects[0].rect, rects[1].rect];
        assert_eq!(upper[1] + upper[3], lower[1], "{rects:?}");
        assert_eq!(lower[1] + lower[3], 900.3, "{rects:?}");

        for size in [
            [1234.567, 900.3],
            [987.65, 611.1],
            [3001.7, 777.77],
            [999.99, 560.01],
        ] {
            for shares in [[1.0, 5.0, 0.3], [0.7, 0.01, 9.0], [1.0, 1.0, 1.0]] {
                let mut tiles = Tiles::restored(
                    shares
                        .iter()
                        .zip([A, B, D])
                        .map(|(&share, key)| whole(key, share))
                        .collect(),
                    Some(A),
                );
                stack(&mut tiles, B, C, shares[0] / 10.0);
                let rects = tiles.rects(size);
                let what = format!("{size:?} {shares:?}: {rects:?}");
                let ends: Vec<f32> = rects
                    .iter()
                    .map(|laid| laid.rect[0] + laid.rect[2])
                    .collect();
                assert_eq!(
                    [ends[0], ends[2]],
                    [rects[1].rect[0], rects[3].rect[0]],
                    "{what}"
                );
                assert_eq!(ends[3], size[0], "{what}");
                assert_eq!(
                    rects[1].rect[1] + rects[1].rect[3],
                    rects[2].rect[1],
                    "{what}"
                );
                assert_eq!(rects[2].rect[1] + rects[2].rect[3], size[1], "{what}");
            }
        }
    }

    #[test]
    fn rects_without_a_size_are_empty() {
        let mut tiles = row(&[A, B], A);
        stack(&mut tiles, B, C, 0.5);
        let rects = |size| -> Vec<[f32; 4]> {
            tiles
                .rects(size)
                .into_iter()
                .map(|laid| laid.rect)
                .collect()
        };
        assert_eq!(
            rects([0.0, 900.0]),
            vec![
                [0.0, 0.0, 0.0, 900.0],
                [0.0, 0.0, 0.0, 450.0],
                [0.0, 450.0, 0.0, 450.0]
            ]
        );
        assert_eq!(
            rects([1600.0, 0.0]),
            vec![
                [0.0, 0.0, 800.0, 0.0],
                [800.0, 0.0, 800.0, 0.0],
                [800.0, 0.0, 800.0, 0.0]
            ]
        );
        assert_eq!(rects([f32::NAN, f32::NAN]), vec![[0.0; 4]; 3]);
        for size in [[0.0, 900.0], [1600.0, 0.0], [f32::NAN, f32::NAN]] {
            assert_eq!(tiles.tile_at(size, [10.0, 10.0]), None, "{size:?}");
        }
    }

    #[test]
    fn the_tile_under_a_point() {
        let tiles = uneven_stack();
        let at = |x: f32, y: f32| tiles.tile_at([1600.0, 800.0], [x, y]).map(|laid| laid.key);
        assert_eq!(at(10.0, 10.0), Some(A));
        assert_eq!(at(500.0, 100.0), Some(B));
        assert_eq!(at(500.0, 300.0), Some(C));
        assert_eq!(at(399.9, 799.9), Some(A));
        assert_eq!(at(1600.0, 10.0), None);
        assert_eq!(at(-1.0, 5.0), None);
        assert_eq!(at(10.0, 800.0), None);
        assert_eq!(at(f32::NAN, 10.0), None);
        let laid = tiles.tile_at([1600.0, 800.0], [500.0, 300.0]).unwrap();
        assert_eq!((laid.column, laid.level), (1, Level::Bottom));
    }

    #[test]
    fn the_parts_of_a_tile() {
        let rect = [400.0, 0.0, 1200.0, 800.0];
        assert_eq!(part_at(rect, [500.0, 100.0]), Part::Left);
        assert_eq!(part_at(rect, [1500.0, 700.0]), Part::Right);
        assert_eq!(part_at(rect, [1000.0, 399.0]), Part::Upper);
        assert_eq!(part_at(rect, [1000.0, 400.0]), Part::Lower);
        assert_ne!(
            part_at(rect, [700.0, 100.0]),
            Part::Left,
            "the quarter line"
        );
    }

    #[test]
    fn keys_go_column_by_column() {
        let mut tiles = row(&[A, B, D], A);
        stack(&mut tiles, B, C, 0.5);
        assert_eq!(tiles.keys(), vec![A, B, C, D]);
        assert_eq!(tiles.len(), 4);
        assert!(!tiles.is_empty());
        assert!(Tiles::default().is_empty());
        assert_eq!(tiles.find(A), Some((0, Level::Whole)));
        assert_eq!(tiles.find(B), Some((1, Level::Top)));
        assert_eq!(tiles.find(C), Some((1, Level::Bottom)));
        assert_eq!(tiles.find(E), None);
        assert_eq!(tiles.mate(B), Some(C));
        assert_eq!(tiles.mate(C), Some(B));
        assert_eq!(tiles.mate(A), None);
        assert_eq!(tiles.mate(E), None);
        assert!(tiles.contains(C));
        assert_eq!(tiles.split(B), Some(0.5));
        assert_eq!(tiles.split(C), None, "the lower tile");
        assert_eq!(tiles.split(A), None, "a tile alone");
    }

    #[test]
    fn a_tile_opens_under_another() {
        let mut tiles = row(&[A, B], A);
        let placement = tiles.placement();
        assert_eq!(tiles.put(C, Place::Below(B), SIZE), Put::Added);
        assert_eq!(shape(&tiles), vec![vec![A], vec![B, C]]);
        assert_eq!(tiles.split(B), Some(0.5));
        assert_eq!(tiles.focused(), Some(C));
        assert_eq!(tiles.placement(), placement + 1);
    }

    #[test]
    fn a_tile_opens_over_another() {
        let mut tiles = row(&[A, B], A);
        assert_eq!(tiles.put(C, Place::Above(A), SIZE), Put::Added);
        assert_eq!(shape(&tiles), vec![vec![C, A], vec![B]]);
        assert_eq!(tiles.split(C), Some(0.5));
        assert_eq!(tiles.focused(), Some(C));
    }

    #[test]
    fn a_column_never_holds_three() {
        let mut tiles = row(&[A], A);
        stack(&mut tiles, A, B, 0.5);
        let before = tiles.clone();
        for spot in [
            Spot::Above(A),
            Spot::Below(A),
            Spot::Above(B),
            Spot::Below(B),
        ] {
            assert!(!tiles.takes(spot, SIZE), "{spot:?}");
            assert_eq!(tiles.put(C, place_of(spot), SIZE), Put::Nothing, "{spot:?}");
            assert_eq!(tiles, before, "{spot:?}");
        }
    }

    #[test]
    fn a_low_row_is_not_split() {
        for (height, put) in [
            (Some(559.0), Put::Nothing),
            (Some(560.0), Put::Added),
            (None, Put::Added),
        ] {
            let mut tiles = row(&[A, B], A);
            let size = RowSize {
                width: Some(1600.0),
                height,
            };
            assert_eq!(tiles.put(C, Place::Below(B), size), put, "{height:?}");
        }
        assert!(!Tiles::fits_two(559.0));
        assert!(Tiles::fits_two(560.0));
        assert!(!Tiles::fits_two(f32::NAN));
    }

    #[test]
    fn stacking_needs_no_room_for_a_column() {
        let mut tiles = row(&[A, B], A);
        assert_eq!(
            tiles.put(C, Place::At(1), sized(900.0, 900.0)),
            Put::Nothing
        );
        assert_eq!(
            tiles.put(C, Place::Below(A), sized(900.0, 900.0)),
            Put::Added
        );
        assert_eq!(shape(&tiles), vec![vec![A, C], vec![B]]);
    }

    #[test]
    fn a_saved_row_is_put_right() {
        let tiles = Tiles::restored(
            vec![
                whole(A, 0.0),
                whole(B, f32::NAN),
                whole(A, 4.0),
                whole(C, -2.0),
                whole(D, 2.5),
            ],
            Some(SessionKey(99)),
        );
        assert_eq!(keys(&tiles), vec![A, B, C, D]);
        assert_eq!(shares(&tiles), vec![1.0, 1.0, 1.0, 2.5]);
        assert_eq!(tiles.focused(), Some(A));
        assert_eq!(
            Tiles::restored(tiles.columns().to_vec(), Some(C)).focused(),
            Some(C)
        );
        assert_eq!(Tiles::restored(Vec::new(), Some(A)).focused(), None);
    }

    #[test]
    fn a_saved_row_with_columns_of_two_is_put_right() {
        // Its upper tile met before: the lower one has the column, with its share.
        let tiles = Tiles::restored(
            vec![
                whole(A, 1.0),
                Column {
                    share: 2.0,
                    ..two(A, B, 0.3)
                },
            ],
            Some(B),
        );
        assert_eq!(shape(&tiles), vec![vec![A], vec![B]]);
        assert_eq!(shares(&tiles), vec![1.0, 2.0]);
        assert_eq!(tiles.focused(), Some(B));

        // Its lower tile met before: the upper one alone.
        let tiles = Tiles::restored(vec![whole(A, 1.0), two(B, A, 0.3)], None);
        assert_eq!(shape(&tiles), vec![vec![A], vec![B]]);

        // Its lower tile its upper one again: the upper one alone.
        let tiles = Tiles::restored(vec![two(A, A, 0.3)], None);
        assert_eq!(shape(&tiles), vec![vec![A]]);

        // Both met before: the column goes.
        let tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 1.0), two(A, B, 0.3)], None);
        assert_eq!(shape(&tiles), vec![vec![A], vec![B]]);

        // A split that is not one becomes 0.5.
        for split in [0.0, 1.0, -0.5, 1.5, f32::NAN, f32::INFINITY] {
            let tiles = Tiles::restored(vec![two(A, B, split)], None);
            assert_eq!(tiles.split(A), Some(0.5), "{split}");
        }
        let tiles = Tiles::restored(
            vec![Column {
                share: f32::NAN,
                ..two(A, B, 0.3)
            }],
            Some(B),
        );
        assert_eq!(shape(&tiles), vec![vec![A, B]]);
        assert_eq!(shares(&tiles), vec![1.0]);
        assert_eq!(tiles.split(A), Some(0.3));
        assert_eq!(
            tiles.focused(),
            Some(B),
            "a lower tile can be the focused one"
        );

        let mut tiles = Tiles::default();
        tiles.restore(
            vec![
                Column {
                    share: 3.0,
                    ..two(A, B, 0.3)
                },
                whole(C, 1.0),
            ],
            Some(B),
        );
        assert_eq!(shape(&tiles), vec![vec![A, B], vec![C]]);
        assert_eq!(shares(&tiles), vec![1.5, 0.5]);
        assert_eq!(tiles.split(A), Some(0.3));
        assert_eq!(tiles.focused(), Some(B));
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

        tiles.put(A, Place::Focused, RowSize::default());
        changed(&tiles, true);
        tiles.put(B, Place::Side, RowSize::default());
        changed(&tiles, true);
        tiles.put(C, Place::Focused, RowSize::default());
        changed(&tiles, true);
        tiles.put(A, Place::Side, RowSize::default());
        changed(&tiles, true);
        tiles.put(A, Place::Side, RowSize::default());
        changed(&tiles, false);
        tiles.put(D, Place::Tile(SessionKey(99)), RowSize::default());
        changed(&tiles, false);
        tiles.put(D, Place::At(0), wide(500.0));
        changed(&tiles, false);
        tiles.put(D, Place::Below(SessionKey(99)), RowSize::default());
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

        tiles.restore(vec![whole(A, 1.0)], Some(A));
        changed(&tiles, true);

        // Each change of where the tiles are counts once on both.
        let mut tiles = row(&[A, B], A);
        let mut counts = (tiles.version(), tiles.placement());
        let mut once = |tiles: &Tiles, what: &str| {
            let now = (tiles.version(), tiles.placement());
            assert_eq!(now, (counts.0 + 1, counts.1 + 1), "{what}");
            counts = now;
        };
        assert_eq!(tiles.put(C, Place::Above(B), SIZE), Put::Added);
        once(&tiles, "a put above");
        assert!(tiles.move_to(B, Spot::Edge(0), SIZE));
        once(&tiles, "a move out");
        assert_eq!(shape(&tiles), vec![vec![B], vec![A], vec![C]]);
        assert!(tiles.move_to(A, Spot::Below(C), SIZE));
        once(&tiles, "a move into a column");
        assert!(tiles.move_to(A, Spot::Above(C), SIZE));
        once(&tiles, "a swap");
        assert_eq!(shape(&tiles), vec![vec![B], vec![A, C]]);
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
        tiles.put(C, Place::Side, RowSize::default());
        assert_eq!(tiles.placement(), placement + 3);
        tiles.put(D, Place::Focused, RowSize::default());
        assert_eq!(tiles.placement(), placement + 4);
        tiles.restore(vec![whole(A, 2.0)], None);
        assert_eq!(tiles.placement(), placement + 5);
    }

    #[test]
    fn a_restored_row_has_shares_of_mean_1() {
        let mut tiles = Tiles::default();
        tiles.restore(
            vec![whole(A, 3.0), whole(B, 1.0), whole(C, f32::NAN)],
            Some(B),
        );
        assert_eq!(keys(&tiles), vec![A, B, C]);
        assert_eq!(shares(&tiles), vec![1.8, 0.6, 0.6]);
        assert_eq!(tiles.focused(), Some(B));

        tiles.restore(vec![whole(A, f32::MIN_POSITIVE), whole(B, f32::MAX)], None);
        assert!(
            shares(&tiles).iter().all(|share| *share > 0.0),
            "{:?}",
            shares(&tiles)
        );
        assert_widths(&tiles, 1000.0, &[0.0, 1000.0]);
    }

    #[test]
    fn a_tile_moves_to_an_edge_with_its_share() {
        let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 2.0), whole(C, 3.0)], Some(B));
        let placement = tiles.placement();
        assert!(tiles.move_to(A, Spot::Edge(3), SIZE));
        assert_eq!(keys(&tiles), vec![B, C, A]);
        assert_eq!(shares(&tiles), vec![2.0, 3.0, 1.0]);
        assert_eq!(tiles.focused(), Some(A));
        assert_eq!(tiles.placement(), placement + 1);
    }

    #[test]
    fn a_tile_moves_left() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(C, Spot::Edge(0), SIZE));
        assert_eq!(keys(&tiles), vec![C, A, B]);
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(C, Spot::Edge(1), SIZE));
        assert_eq!(keys(&tiles), vec![A, C, B]);
    }

    #[test]
    fn a_tile_moves_one_place_right() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(A, Spot::Edge(2), SIZE));
        assert_eq!(keys(&tiles), vec![B, A, C]);
    }

    #[test]
    fn its_own_edges_move_nothing() {
        let mut tiles = row(&[A, B, C], A);
        let (version, placement) = (tiles.version(), tiles.placement());
        for edge in [1, 2] {
            assert!(!tiles.move_to(B, Spot::Edge(edge), SIZE), "edge {edge}");
            assert_eq!(keys(&tiles), vec![A, B, C], "edge {edge}");
            assert_eq!(tiles.focused(), Some(A), "edge {edge}");
            assert_eq!((tiles.version(), tiles.placement()), (version, placement));
        }
    }

    #[test]
    fn an_edge_past_the_end_is_the_end() {
        let mut tiles = row(&[A, B, C], A);
        assert!(tiles.move_to(A, Spot::Edge(9), SIZE));
        assert_eq!(keys(&tiles), vec![B, C, A]);
        let mut tiles = row(&[A, B, C], A);
        assert!(!tiles.move_to(C, Spot::Edge(9), SIZE));
        assert_eq!(keys(&tiles), vec![A, B, C]);
    }

    #[test]
    fn a_session_with_no_tile_moves_nothing() {
        let mut tiles = row(&[A, B, C], A);
        let version = tiles.version();
        assert!(!tiles.move_to(D, Spot::Edge(0), SIZE));
        assert!(!tiles.would_move(D, Spot::Edge(0), SIZE));
        assert_eq!(keys(&tiles), vec![A, B, C]);
        assert_eq!((tiles.focused(), tiles.version()), (Some(A), version));
    }

    #[test]
    fn would_move_says_whether_a_tile_would_move() {
        let tiles = row(&[A, B, C], B);
        let at = |key: SessionKey| -> Vec<bool> {
            (0..=3)
                .map(|edge| tiles.would_move(key, Spot::Edge(edge), SIZE))
                .collect()
        };
        assert_eq!(at(A), vec![false, false, true, true]);
        assert_eq!(at(C), vec![true, true, false, false]);
    }

    #[test]
    fn a_tile_leaves_its_column_for_one_of_its_own() {
        let start = uneven_stack;
        let mut tiles = start();
        assert!(tiles.move_to(C, Spot::Edge(0), sized(2600.0, 900.0)));
        assert_eq!(shape(&tiles), vec![vec![C], vec![A], vec![B]]);
        assert_eq!(shares(&tiles), vec![2.0, 1.0, 3.0]);
        assert_eq!(tiles.focused(), Some(C));

        // Kept to the least width, as a tile added there would be.
        let mut tiles = start();
        assert!(tiles.move_to(C, Spot::Edge(0), SIZE));
        assert_eq!(shape(&tiles), vec![vec![C], vec![A], vec![B]]);
        assert_widths(&tiles, 1600.0, &[498.1, 420.0, 681.9]);

        // No room for a column.
        let mut tiles = start();
        let before = tiles.clone();
        assert!(!tiles.would_move(C, Spot::Edge(0), sized(900.0, 900.0)));
        assert!(!tiles.move_to(C, Spot::Edge(0), sized(900.0, 900.0)));
        assert_eq!(tiles, before);

        // The edges beside its old column.
        let mut tiles = start();
        assert!(tiles.move_to(C, Spot::Edge(1), sized(2600.0, 900.0)));
        assert_eq!(shape(&tiles), vec![vec![A], vec![C], vec![B]]);
        let mut tiles = start();
        assert!(tiles.move_to(C, Spot::Edge(2), sized(2600.0, 900.0)));
        assert_eq!(shape(&tiles), vec![vec![A], vec![B], vec![C]]);
        assert_eq!(shares(&tiles), vec![1.0, 3.0, 2.0]);
    }

    #[test]
    fn a_tile_joins_another_column() {
        let mut tiles = Tiles::restored(vec![whole(A, 1.0), whole(B, 2.0), whole(C, 3.0)], Some(B));
        assert!(tiles.move_to(A, Spot::Below(C), SIZE));
        assert_eq!(shape(&tiles), vec![vec![B], vec![C, A]]);
        assert_eq!(shares(&tiles), vec![2.0, 3.0]);
        assert_eq!(tiles.split(C), Some(0.5));
        assert_eq!(tiles.focused(), Some(A));
    }

    #[test]
    fn the_two_of_a_column_swap() {
        let mut tiles = row(&[A], A);
        stack(&mut tiles, A, B, 0.25);
        assert!(tiles.move_to(A, Spot::Below(B), SIZE));
        assert_eq!(shape(&tiles), vec![vec![B, A]]);
        assert_eq!(tiles.split(B), Some(0.75));
        assert!(!tiles.move_to(A, Spot::Below(B), SIZE));
        assert!(tiles.move_to(A, Spot::Above(B), SIZE));
        assert_eq!(shape(&tiles), vec![vec![A, B]]);
        assert_eq!(tiles.split(A), Some(0.25));
    }

    #[test]
    fn a_full_column_takes_no_tile() {
        let mut tiles = row(&[A, B], A);
        stack(&mut tiles, B, C, 0.5);
        let before = tiles.clone();
        for spot in [Spot::Above(B), Spot::Below(C)] {
            assert!(!tiles.would_move(A, spot, SIZE), "{spot:?}");
            assert!(!tiles.move_to(A, spot, SIZE), "{spot:?}");
            assert_eq!(tiles, before, "{spot:?}");
        }
    }

    #[test]
    fn a_column_passes_a_column_of_two() {
        let mut tiles = row(&[A, B, D], A);
        stack(&mut tiles, B, C, 0.5);
        assert!(tiles.move_to(A, Spot::Edge(2), SIZE));
        assert_eq!(shape(&tiles), vec![vec![B, C], vec![A], vec![D]]);
    }

    #[test]
    fn moves_of_nothing_change_nothing() {
        let mut tiles = row(&[A, B], A);
        stack(&mut tiles, B, C, 0.5);
        let before = tiles.clone();
        for (key, spot) in [
            (A, Spot::Edge(0)),
            (A, Spot::Edge(1)),
            (A, Spot::Above(A)),
            (A, Spot::Below(A)),
            (B, Spot::Above(B)),
            (C, Spot::Below(C)),
            (B, Spot::Above(C)),
            (C, Spot::Below(B)),
            (A, Spot::Below(E)),
            (A, Spot::Above(E)),
            (E, Spot::Edge(0)),
            (E, Spot::Below(A)),
        ] {
            assert!(!tiles.would_move(key, spot, SIZE), "{key:?} to {spot:?}");
            assert!(!tiles.move_to(key, spot, SIZE), "{key:?} to {spot:?}");
            assert_eq!(tiles, before, "{key:?} to {spot:?}");
        }
    }

    #[test]
    fn the_split_is_dragged() {
        let mut tiles = row(&[A], A);
        stack(&mut tiles, A, B, 0.5);
        let (version, placement) = (tiles.version(), tiles.placement());
        assert!(tiles.resize_split(A, 300.0, 900.0));
        assert_eq!(tiles.split(A), Some(300.0 / 900.0));
        assert!(tiles.resize_split(A, 100.0, 900.0));
        assert_eq!(tiles.split(A), Some(280.0 / 900.0));
        assert!(tiles.resize_split(A, 800.0, 900.0));
        assert_eq!(tiles.split(A), Some(620.0 / 900.0));
        assert!(!tiles.resize_split(A, 700.0, 900.0), "already as high");
        assert_eq!(tiles.version(), version + 3);
        assert_eq!(tiles.placement(), placement);
    }

    #[test]
    fn a_low_column_does_not_drag() {
        let mut tiles = row(&[A, C], A);
        stack(&mut tiles, A, B, 0.5);
        let before = tiles.clone();
        assert!(!tiles.resize_split(A, 250.0, 500.0));
        assert!(!tiles.resize_split(A, f32::NAN, 900.0));
        assert!(!tiles.resize_split(A, 300.0, f32::NAN));
        assert!(!tiles.resize_split(B, 300.0, 900.0), "the lower tile");
        assert!(!tiles.resize_split(C, 300.0, 900.0), "a tile alone");
        assert_eq!(tiles, before);
    }

    #[test]
    fn set_split_puts_a_split_back() {
        let mut tiles = row(&[A], A);
        stack(&mut tiles, A, B, 0.5);
        assert!(tiles.resize_split(A, 300.0, 900.0));
        let (version, placement) = (tiles.version(), tiles.placement());
        assert!(tiles.set_split(A, 0.5));
        assert_eq!(tiles.split(A), Some(0.5));
        assert_eq!(tiles.version(), version + 1);
        assert_eq!(tiles.placement(), placement);
        assert!(!tiles.set_split(A, 0.5));
        assert!(!tiles.set_split(B, 0.25), "the lower tile");
        assert!(!tiles.set_split(SessionKey(99), 0.25));
        for split in [0.0, 1.0, f32::NAN] {
            assert!(!tiles.set_split(A, split), "{split}");
        }
        assert_eq!(tiles.split(A), Some(0.5));
        assert_eq!(tiles.version(), version + 1);
    }

    #[test]
    fn takes_and_would_move_say_what_put_and_move_to_do() {
        let start = || {
            let mut tiles = row(&[A, B, D], A);
            stack(&mut tiles, B, C, 0.5);
            tiles
        };
        let spots: Vec<Spot> = (0..=4)
            .map(Spot::Edge)
            .chain(
                [A, B, C, D, E]
                    .into_iter()
                    .flat_map(|key| [Spot::Above(key), Spot::Below(key)]),
            )
            .collect();
        for size in [
            SIZE,
            RowSize::default(),
            sized(2000.0, 900.0),
            sized(1000.0, 500.0),
        ] {
            for &spot in &spots {
                let mut tiles = start();
                let takes = tiles.takes(spot, size);
                let put = tiles.put(E, place_of(spot), size);
                assert_eq!(put == Put::Added, takes, "{spot:?} in {size:?}");
                for key in [A, B, C, D, E] {
                    let mut tiles = start();
                    let before = tiles.clone();
                    let would = tiles.would_move(key, spot, size);
                    let what = format!("{key:?} to {spot:?} in {size:?}");
                    assert_eq!(tiles.move_to(key, spot, size), would, "{what}");
                    if would {
                        assert_ne!(
                            (shape(&tiles), shares(&tiles)),
                            (shape(&before), shares(&before)),
                            "{what}"
                        );
                    } else {
                        assert_eq!(tiles, before, "{what}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_row_without_width_does_not_panic() {
        let mut tiles = row(&[A, B], A);
        for width in [0.0, -10.0, f32::NAN, f32::INFINITY] {
            assert_eq!(tiles.widths(width), vec![0.0, 0.0], "{width}");
            assert!(!tiles.fits_another(width), "{width}");
            assert_eq!(tiles.tile_at([width, 900.0], [10.0, 10.0]), None, "{width}");
            assert_eq!(tiles.edge_at(width, 10.0), 0, "{width}");
            assert!(!tiles.resize(A, 300.0, width), "{width}");
            assert_eq!(
                tiles.put(C, Place::At(0), wide(width)),
                Put::Nothing,
                "{width}"
            );
        }
        assert_eq!(shares(&tiles), vec![1.0, 1.0]);
        assert!(!tiles.resize(A, f32::NAN, 1500.0));
        assert_eq!(tiles.tile_at([1500.0, 900.0], [f32::NAN, 10.0]), None);
        assert_eq!(tiles.edge_at(1500.0, f32::NAN), 0);
        assert!(Tiles::default().fits_another(f32::NAN));
        assert_eq!(Tiles::default().edge_at(1500.0, 10.0), 0);
    }
}
