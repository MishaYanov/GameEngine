use std::{
	collections::VecDeque,
	f32::consts::{FRAC_PI_2, PI},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cell {
	pub(super) column: usize,
	pub(super) row: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
	North,
	East,
	South,
	West,
}

impl Direction {
	const IN_ORDER: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

	pub(super) fn input_edge(previous: u8, movement: [f32; 3]) -> (u8, Option<Self>) {
		// Controlled Move receives the legacy XY axes. Positive keyboard Y is north.
		let held = u8::from(movement[1] > 0.0)
			| (u8::from(movement[0] > 0.0) << 1)
			| (u8::from(movement[1] < 0.0) << 2)
			| (u8::from(movement[0] < 0.0) << 3);
		(held, Self::first_in(held & !previous))
	}

	pub(super) fn first_in(directions: u8) -> Option<Self> {
		Self::IN_ORDER
			.into_iter()
			.enumerate()
			.find(|(index, _)| directions & (1 << index) != 0)
			.map(|(_, direction)| direction)
	}

	pub(super) fn between(from: Cell, to: Cell) -> Self {
		match (to.column.cmp(&from.column), to.row.cmp(&from.row)) {
			(std::cmp::Ordering::Greater, _) => Self::East,
			(std::cmp::Ordering::Less, _) => Self::West,
			(_, std::cmp::Ordering::Greater) => Self::South,
			_ => Self::North,
		}
	}

	pub(super) fn yaw(self) -> f32 {
		match self {
			Self::North => 0.0,
			Self::East => -FRAC_PI_2,
			Self::South => PI,
			Self::West => FRAC_PI_2,
		}
	}

	fn step(self, from: Cell) -> Option<Cell> {
		let (column, row) = match self {
			Self::North => (from.column, from.row.checked_sub(1)?),
			Self::East => (from.column + 1, from.row),
			Self::South => (from.column, from.row + 1),
			Self::West => (from.column.checked_sub(1)?, from.row),
		};
		Some(Cell { column, row })
	}
}

pub(super) struct Board {
	width: usize,
	height: usize,
	tile_size: f32,
	tiles: Vec<u8>,
	pub(super) player: Cell,
	pub(super) monster: Cell,
	pub(super) seals: [Cell; 3],
	pub(super) exit: Cell,
}

impl Board {
	const MAX_SIDE: usize = 31;

	pub(super) fn parse(rows: &[String], tile_size: f32) -> Result<Self, String> {
		let width = rows.first().map_or(0, String::len);
		let height = rows.len();
		if !(3..=Self::MAX_SIDE).contains(&width)
			|| !(3..=Self::MAX_SIDE).contains(&height)
			|| rows.iter().any(|row| row.len() != width)
			|| !(1.0..=6.0).contains(&tile_size)
		{
			return Err(
				"The Hollow Maze needs a rectangular 3–31 cell grid and 1–6 metre tiles".into(),
			);
		}

		let tiles: Vec<_> = rows.iter().flat_map(|row| row.bytes()).collect();
		if !tiles.iter().all(|tile| b"#.HPM123E".contains(tile)) {
			return Err("The Hollow Maze grid uses only # . H P M 1 2 3 E".into());
		}

		let locate = |marker: u8| {
			let mut matches = tiles
				.iter()
				.enumerate()
				.filter(|(_, tile)| **tile == marker);
			let (Some((index, _)), None) = (matches.next(), matches.next()) else {
				return Err(format!(
					"The Hollow Maze needs exactly one {} cell",
					char::from(marker)
				));
			};
			Ok(Cell {
				column: index % width,
				row: index / width,
			})
		};
		let board = Self {
			width,
			height,
			tile_size,
			player: locate(b'P')?,
			monster: locate(b'M')?,
			seals: [locate(b'1')?, locate(b'2')?, locate(b'3')?],
			exit: locate(b'E')?,
			tiles,
		};
		if board.seals.into_iter().any(|cell| {
			board
				.find_step(board.player, cell, false, Self::jump_destination)
				.is_none()
		}) || board
			.find_step(board.player, board.exit, true, Self::jump_destination)
			.is_none()
		{
			return Err("The Hollow Maze needs paths to all seals before its gate opens, and a path to the open exit".into());
		}
		Ok(board)
	}

	pub(super) fn center(&self, cell: Cell) -> [f32; 2] {
		[
			(cell.column as f32 - (self.width - 1) as f32 * 0.5) * self.tile_size,
			(cell.row as f32 - (self.height - 1) as f32 * 0.5) * self.tile_size,
		]
	}

	pub(super) fn neighbor(
		&self,
		from: Cell,
		direction: Direction,
		exit_open: bool,
	) -> Option<Cell> {
		let (cell, tile) = self.adjacent(from, direction)?;
		match tile {
			b'#' | b'H' => None,
			b'E' if !exit_open => None,
			_ => Some(cell),
		}
	}

	/// A jump lands one cell away, or one cell beyond an intervening hurdle.
	pub(super) fn jump_destination(
		&self,
		from: Cell,
		direction: Direction,
		exit_open: bool,
	) -> Option<Cell> {
		let (cell, tile) = self.adjacent(from, direction)?;
		if tile == b'H' {
			return self.neighbor(cell, direction, exit_open);
		}
		self.neighbor(from, direction, exit_open)
	}

	/// One ground step of a shortest route, with stable north/east/south/west ties.
	pub(super) fn shortest_step(&self, from: Cell, to: Cell, exit_open: bool) -> Option<Cell> {
		self.find_step(from, to, exit_open, Self::neighbor)
	}

	fn adjacent(&self, from: Cell, direction: Direction) -> Option<(Cell, u8)> {
		let cell = direction.step(from)?;
		if cell.column >= self.width || cell.row >= self.height {
			return None;
		}
		Some((cell, self.tiles[self.index(cell)]))
	}

	fn find_step(
		&self,
		from: Cell,
		to: Cell,
		exit_open: bool,
		destination: fn(&Self, Cell, Direction, bool) -> Option<Cell>,
	) -> Option<Cell> {
		let mut visited = vec![false; self.tiles.len()];
		let mut pending = VecDeque::from([(from, from)]);
		visited[self.index(from)] = true;
		while let Some((cell, first_step)) = pending.pop_front() {
			if cell == to {
				return Some(first_step);
			}
			for next in Direction::IN_ORDER
				.into_iter()
				.filter_map(|direction| destination(self, cell, direction, exit_open))
			{
				let visited = &mut visited[self.index(next)];
				if !*visited {
					*visited = true;
					pending.push_back((next, if cell == from { next } else { first_step }));
				}
			}
		}
		None
	}

	fn index(&self, cell: Cell) -> usize {
		cell.row * self.width + cell.column
	}
}
