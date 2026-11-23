//! The spider dungeon: a cave of carved tunnels, a nest with a cave spider spawner and egg rooms,
//! built by the mod's own structure type `betterdungeons:spider_dungeon_type`.
//!
//! The original (`SpiderDungeonStructure` and its five pieces) is traditional code, not jigsaw:
//! a big tunnel leaves the start, a second one follows it, three or four small tunnels branch off
//! each big tunnel's end, every small tunnel ends in an egg room four times out of five, and a
//! nest closes each big tunnel. This file is that plan and the carving of each piece, written from
//! the behaviour the source describes.
//!
//! Two halves, kept apart so the plan and the carving are proved by `cargo test` without a game:
//!
//! * [`plan`] is the layout (`addChildren`): every draw in the original's order, every box.
//! * [`Piece::place`] is `postProcess` for one chunk, against the small [`Level`] trait that
//!   [`crate::support`]'s glue answers from `PieceContext`.
//!
//! Where the mod API differs from the original, the difference is named at the line: the original's
//! `canOcclude` is asked as `isSolidRender`, and a fluid is a water-like block or a waterlogged one.

use mod_api::RandomSource;
use std::sync::OnceLock;

/// The structure type's id, the `type` a data-pack structure names.
pub const TYPE_ID: &str = "spider_dungeon_type";

/// The loot table of the egg room's chest.
pub const EGG_ROOM_LOOT: &str = "betterdungeons:spider_dungeon/chests/egg_room";

/// Steps of a tunnel (`LENGTH`), one carved ellipsoid each.
pub const LENGTH: usize = 30;

/// A tunnel's radii: `[x_min, x_max, y_min, y_max, z_min, z_max]`.
const BIG_RADII: [f32; 6] = [2.0, 2.5, 2.0, 2.5, 2.0, 2.5];
const SMALL_RADII: [f32; 6] = [1.0, 1.5, 2.0, 2.0, 1.0, 1.5];
/// A nest's radii, as `[min, max]` per axis.
const NEST_RADII: [[f32; 2]; 3] = [[6.0, 10.0], [4.0, 6.0], [6.0, 10.0]];
/// An egg room's radii, as `[min, max]` per axis.
const EGG_RADII: [[f32; 2]; 3] = [[2.0, 3.0], [2.0, 3.0], [2.0, 3.0]];

/// `Math.PI` as the original's `(float) Math.PI`.
const PI: f32 = std::f32::consts::PI;

/// Which of the four pieces this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    BigTunnel,
    SmallTunnel,
    Nest,
    EggRoom,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::BigTunnel => "big_tunnel",
            Self::SmallTunnel => "small_tunnel",
            Self::Nest => "nest",
            Self::EggRoom => "egg_room",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::BigTunnel,
            Self::SmallTunnel,
            Self::Nest,
            Self::EggRoom,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
    }
}

/// One piece as planned: where it starts and how it is shaped, and the box that holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub kind: Kind,
    pub start: [i32; 3],
    /// Where a tunnel ends (the next piece starts here); the start for a room.
    pub end: [i32; 3],
    /// A tunnel's pitch; zero for a room.
    pub pitch: f32,
    /// A tunnel's yaw at each step; empty for a room.
    pub yaws: Vec<f32>,
    /// A room's radii; zero for a tunnel.
    pub radii: [f32; 3],
    pub min: [i32; 3],
    pub max: [i32; 3],
}

/// `Mth`'s sine table: `sin` and `cos` of a float as vanilla reads them, from 65536 entries.
fn sine_table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..65536)
            .map(|i| (f64::from(i) * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
            .collect()
    })
}

/// `Mth.sin`.
pub fn sin(value: f32) -> f32 {
    sine_table()[((value * 10430.378_f32) as i32 & 65535) as usize]
}

/// `Mth.cos`.
pub fn cos(value: f32) -> f32 {
    sine_table()[((value * 10430.378_f32 + 16384.0_f32) as i32 & 65535) as usize]
}

/// `Mth.floor` of a float.
fn floor(value: f32) -> i32 {
    let truncated = value as i32;
    if value < truncated as f32 {
        truncated - 1
    } else {
        truncated
    }
}

/// `Mth.lerp(delta, start, end)`.
fn lerp(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * (end - start)
}

/// `Mth.clamp` of floats.
fn clamp(value: f32, low: f32, high: f32) -> f32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// The original's running min/max over a tunnel's step centres, with its `(int)` casts.
struct Extent {
    min: [i32; 3],
    max: [i32; 3],
}

impl Extent {
    fn new() -> Self {
        Self {
            min: [i32::MAX; 3],
            max: [i32::MIN; 3],
        }
    }

    /// Fold one step centre in; `radii` is the tunnel's maximum on each axis.
    fn include(&mut self, centre: [f32; 3], radii: [f32; 3]) {
        for axis in 0..3 {
            let reach = radii[axis] as i32 + 4;
            if centre[axis] - radii[axis] - 4.0 < self.min[axis] as f32 {
                self.min[axis] = centre[axis] as i32 - reach;
            }
            if centre[axis] + radii[axis] + 4.0 > self.max[axis] as f32 {
                self.max[axis] = centre[axis] as i32 + reach;
            }
        }
    }
}

/// `SpiderDungeonStructure.findGenerationPoint`'s piece plan, from the start position: the big
/// tunnel and everything it leaves behind, in the order the original adds them.
pub fn plan(random: &mut dyn RandomSource, start: [i32; 3]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    big_tunnel(random, &mut pieces, start, 0, 0.0, 0.0);
    pieces
}

/// `SpiderDungeonBigTunnelPiece.addChildren`, with the piece itself added first.
fn big_tunnel(
    random: &mut dyn RandomSource,
    pieces: &mut Vec<Piece>,
    start: [i32; 3],
    depth: i32,
    initial_pitch: f32,
    initial_yaw: f32,
) {
    let mut pitch = initial_pitch;
    let mut yaws = vec![0.0_f32; LENGTH];
    yaws[0] = initial_yaw;
    if pitch == 0.0 {
        pitch = clamp(random.next_float() * -PI, -2.6, -0.6);
    }
    // Don't let the cave go straight down.
    if pitch > -2.2 && pitch < -1.0 {
        pitch = -2.2;
    }
    if yaws[0] == 0.0 {
        yaws[0] = random.next_float() * (PI * 2.0);
    }
    let (end, min, max) = walk(random, start, pitch, &mut yaws, 0.01, BIG_RADII);
    pieces.push(Piece {
        kind: Kind::BigTunnel,
        start,
        end,
        pitch,
        yaws: yaws.clone(),
        radii: [0.0; 3],
        min,
        max,
    });
    let last_yaw = yaws[LENGTH - 1];

    // A second big tunnel, when this is the first.
    if depth == 0 {
        big_tunnel(random, pieces, end, depth + 1, 0.0, last_yaw);
    }

    // Small tunnels leaving the nest, offset a little to clear the main tunnel.
    let mut angle = last_yaw + 0.3;
    for _ in 0..3 {
        angle += random.next_float() * 0.4 + 0.9;
        small_tunnel(random, pieces, end, angle);
    }
    // A fourth small tunnel, or a chance of another big one.
    angle += random.next_float() * 0.4 + 0.9;
    if random.next_float() < 0.5 {
        small_tunnel(random, pieces, end, angle);
    } else if depth == 0 {
        let extra_pitch = random.next_float() * PI / 6.0 - PI / 6.0;
        big_tunnel(random, pieces, end, depth + 1, extra_pitch, angle);
    }

    room(random, pieces, Kind::Nest, end);
}

/// `SpiderDungeonSmallTunnelPiece.addChildren`.
fn small_tunnel(random: &mut dyn RandomSource, pieces: &mut Vec<Piece>, start: [i32; 3], yaw: f32) {
    let pitch = random.next_float() * PI / 4.0 - PI / 6.0;
    let mut yaws = vec![0.0_f32; LENGTH];
    yaws[0] = yaw;
    let (end, min, max) = walk(random, start, pitch, &mut yaws, 0.02, SMALL_RADII);
    pieces.push(Piece {
        kind: Kind::SmallTunnel,
        start,
        end,
        pitch,
        yaws,
        radii: [0.0; 3],
        min,
        max,
    });
    if random.next_float() < 0.8 {
        room(random, pieces, Kind::EggRoom, end);
    }
}

/// The yaw walk both tunnels share: the centre of each step, the box over them, and where the
/// tunnel ends. `drift` is the yaw change's scale (0.01 for big tunnels, 0.02 for small).
fn walk(
    random: &mut dyn RandomSource,
    start: [i32; 3],
    pitch: f32,
    yaws: &mut [f32],
    drift: f32,
    radii: [f32; 6],
) -> ([i32; 3], [i32; 3], [i32; 3]) {
    let pitch_y = sin(pitch);
    let pitch_xz = cos(pitch);
    let max_radii = [radii[1], radii[3], radii[5]];
    let mut extent = Extent::new();
    let mut centre = [start[0] as f32, start[1] as f32, start[2] as f32];
    let step = |centre: &mut [f32; 3], yaw: f32| {
        centre[0] += cos(yaw) * pitch_xz;
        centre[1] += sin(pitch_y);
        centre[2] += sin(yaw) * pitch_xz;
    };
    step(&mut centre, yaws[0]);
    extent.include(centre, max_radii);
    let mut modifier = 0.0_f32;
    for i in 1..LENGTH {
        modifier *= 0.75;
        modifier += random.next_float() * random.next_float();
        yaws[i] = yaws[i - 1] + modifier * drift;
        step(&mut centre, yaws[i]);
        extent.include(centre, max_radii);
    }
    (
        [centre[0] as i32, centre[1] as i32, centre[2] as i32],
        extent.min,
        extent.max,
    )
}

/// `SpiderDungeonNestPiece` and `SpiderDungeonEggRoomPiece`'s `addChildren`: three radii.
fn room(random: &mut dyn RandomSource, pieces: &mut Vec<Piece>, kind: Kind, start: [i32; 3]) {
    let ranges = if kind == Kind::Nest {
        NEST_RADII
    } else {
        EGG_RADII
    };
    let mut radii = [0.0_f32; 3];
    for (radius, [low, high]) in radii.iter_mut().zip(ranges) {
        *radius = random.next_float() * (high - low) + low;
    }
    let reach = radii.map(|radius| radius as i32 + 4);
    pieces.push(Piece {
        kind,
        start,
        end: start,
        pitch: 0.0,
        yaws: Vec::new(),
        radii,
        min: [
            start[0] - reach[0],
            start[1] - reach[1],
            start[2] - reach[2],
        ],
        max: [
            start[0] + reach[0],
            start[1] + reach[1],
            start[2] + reach[2],
        ],
    });
}

/// What a cell holds, as the carving asks of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub air: bool,
    /// Water, lava or a block holding water.
    pub fluid: bool,
    /// The original's `BLOCK_BLACKLIST`: what the carving never overwrites.
    pub blacklisted: bool,
    /// The original's `canOcclude`.
    pub occludes: bool,
    pub chest: bool,
}

impl Cell {
    /// What `StructurePiece.getBlock` answers outside the writable box: air.
    pub const OUTSIDE: Self = Self {
        air: true,
        fluid: false,
        blacklisted: false,
        occludes: false,
        chest: false,
    };
}

/// What the carving writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Put {
    CaveAir,
    Cobblestone,
    Cobweb,
    WhiteWool,
    Spawner,
    /// A chest facing `0..4` in north, east, south, west order.
    Chest(usize),
}

/// What lies next to a chest being placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beside {
    Chest,
    Solid,
    Open,
}

/// The world as the carving sees it: one chunk's writable box.
pub trait Level {
    /// The piece's own random.
    fn random(&mut self) -> &mut dyn RandomSource;
    fn cell(&mut self, pos: [i32; 3]) -> Cell;
    fn put(&mut self, pos: [i32; 3], what: Put);
    /// The chunk the writable box is.
    fn chunk(&self) -> [i32; 2];
    /// Whether `pos` is in the writable box.
    fn inside(&self, pos: [i32; 3]) -> bool;
    /// `Heightmap.Types.WORLD_SURFACE_WG`: the first free height at the column.
    fn surface(&mut self, x: i32, z: i32) -> i32;
    /// The spawner block entity at `pos`, set to `mob`.
    fn spawner(&mut self, pos: [i32; 3], mob: &'static str);
    /// The chest block entity at `pos`, with its loot table and the seed it rolls with.
    fn chest(&mut self, pos: [i32; 3], loot_seed: i64);
    /// What is at `pos`, whether or not it is in the box (`level.getBlockState`).
    fn beside(&mut self, pos: [i32; 3]) -> Beside;
}

/// The chunk-local carving mask, one bit per cell the piece carved.
struct Mask {
    bits: Vec<u64>,
}

impl Mask {
    fn new(height: i32) -> Self {
        let cells = 256 * height.max(1) as usize;
        Self {
            bits: vec![0; cells.div_ceil(64)],
        }
    }

    fn get(&self, index: usize) -> bool {
        self.bits[index / 64] >> (index % 64) & 1 == 1
    }

    fn set(&mut self, index: usize) {
        self.bits[index / 64] |= 1 << (index % 64);
    }

    /// The set indices, ascending, as `BitSet.stream` gives them.
    fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.iter().enumerate().flat_map(|(word, bits)| {
            (0..64)
                .filter(move |bit| bits >> bit & 1 == 1)
                .map(move |bit| word * 64 + bit)
        })
    }
}

/// The level's own limits and where the dungeon is, which the carving needs and a chunk's
/// `PieceContext` does not give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub seed: i64,
    pub min_y: i32,
    /// `getMaxBuildHeight`: one above the highest block.
    pub max_y: i32,
}

/// `WorldgenRandom.setLargeFeatureSeed` over a `LegacyRandomSource`, as the pieces seed their
/// decoration random with the dungeon's block position in place of a chunk.
pub fn decoration_random(seed: i64, x: i32, z: i32) -> mod_api::LegacyRandomSource {
    let mut random = mod_api::LegacyRandomSource::new(0);
    random.set_seed(seed);
    let a = random.next_long();
    let b = random.next_long();
    random.set_seed(i64::from(x).wrapping_mul(a) ^ i64::from(z).wrapping_mul(b) ^ seed);
    random
}

/// How an ellipsoid treats its inside and its shell.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A tunnel: cave air, and a cobblestone shell over a fifth of what it meets.
    Tunnel,
    /// The nest: a wool column up to the ceiling, and a shell over air.
    Nest,
    /// An egg room: cave air, and a shell over air.
    Egg,
}

impl Piece {
    /// `postProcess` for the chunk `level` writes: the carving, then what the room holds, then
    /// the cobwebs along the rim. The piece's own random (`randomSource`) is the level's.
    pub fn place(&self, level: &mut dyn Level, frame: Frame) {
        let chunk = level.chunk();
        let mut deco = decoration_random(frame.seed, self.start[0], self.start[2]);
        let mut mask = Mask::new(frame.max_y - frame.min_y + 1);
        let mut surface = [0_i32; 256];
        for x in 0..16 {
            for z in 0..16 {
                surface[(x * 16 + z) as usize] =
                    level.surface(chunk[0] * 16 + x, chunk[1] * 16 + z);
            }
        }
        let mut carve = Carver {
            level,
            deco: &mut deco,
            mask: &mut mask,
            surface: &surface,
            chunk,
            frame,
            start: self.start,
        };
        match self.kind {
            Kind::BigTunnel | Kind::SmallTunnel => self.carve_tunnel(&mut carve),
            Kind::Nest => self.fill_nest(&mut carve),
            Kind::EggRoom => self.fill_egg_room(&mut carve),
        }
        carve.decorate();
    }

    /// A tunnel's thirty ellipsoids.
    fn carve_tunnel(&self, carve: &mut Carver<'_>) {
        let radii = if self.kind == Kind::BigTunnel {
            BIG_RADII
        } else {
            SMALL_RADII
        };
        let pitch_y = sin(self.pitch);
        let pitch_xz = cos(self.pitch);
        let mut centre = self.start.map(|value| value as f32);
        for (i, yaw) in self.yaws.iter().enumerate() {
            centre[0] += cos(*yaw) * pitch_xz;
            centre[1] += sin(pitch_y);
            centre[2] += sin(*yaw) * pitch_xz;
            // Widest in the middle, smallest at the ends.
            let delta = sin(i as f32 * PI / LENGTH as f32);
            let step = [
                lerp(delta, radii[0], radii[1]),
                lerp(delta, radii[2], radii[3]),
                lerp(delta, radii[4], radii[5]),
            ];
            carve.ellipsoid(centre, step, Mode::Tunnel);
        }
    }

    /// `SpiderDungeonNestPiece.postProcess` after the carving: the wool cocoon, the webs around
    /// it and the cave spider spawner at its heart.
    fn fill_nest(&self, carve: &mut Carver<'_>) {
        carve.ellipsoid(self.start.map(|value| value as f32), self.radii, Mode::Nest);
        let [x, y, z] = self.start;
        carve.sphere([x, y + 1, z], 2, 0.5, Put::WhiteWool, true);
        for pos in [
            [x + 1, y + 1, z],
            [x - 1, y + 1, z],
            [x, y + 1, z + 1],
            [x, y + 1, z - 1],
            [x, y, z],
        ] {
            carve.level.put(pos, Put::WhiteWool);
        }
        carve.sphere([x, y + 1, z], 3, 0.5, Put::Cobweb, true);
        carve.level.put([x, y + 1, z], Put::Spawner);
        if carve.level.inside(self.start) {
            carve.level.spawner([x, y + 1, z], "minecraft:cave_spider");
        }
    }

    /// `SpiderDungeonEggRoomPiece.postProcess` after the carving: a wool egg round a chest, or
    /// a spider spawner in its place.
    fn fill_egg_room(&self, carve: &mut Carver<'_>) {
        carve.ellipsoid(self.start.map(|value| value as f32), self.radii, Mode::Egg);
        let [x, y, z] = self.start;
        let chest = [x, y - self.radii[1] as i32 + 1, z];
        carve.sphere(chest, 2, 0.5, Put::WhiteWool, false);
        for pos in [
            [chest[0] + 1, chest[1], chest[2]],
            [chest[0] - 1, chest[1], chest[2]],
            [chest[0], chest[1], chest[2] + 1],
            [chest[0], chest[1], chest[2] - 1],
            [chest[0], chest[1] - 1, chest[2]],
            [chest[0], chest[1] + 1, chest[2]],
        ] {
            carve.level.put(pos, Put::WhiteWool);
        }
        carve.sphere(chest, 2, 0.4, Put::Cobweb, true);
        if carve.level.random().next_float() < 0.6 {
            create_chest(carve.level, chest);
        } else if carve.level.inside(chest) {
            carve.level.put(chest, Put::Spawner);
            carve.level.spawner(chest, "minecraft:spider");
        }
    }
}

/// `StructurePiece.createChest`: a chest, turned as `reorient` turns it, with a loot seed.
fn create_chest(level: &mut dyn Level, pos: [i32; 3]) {
    if !level.inside(pos) || level.cell(pos).chest {
        return;
    }
    let facing = reorient(level, pos);
    level.put(pos, Put::Chest(facing));
    let seed = level.random().next_long();
    level.chest(pos, seed);
}

/// `StructurePiece.reorient` for a chest (default facing north): away from the one solid wall,
/// or the first open side.
fn reorient(level: &mut dyn Level, pos: [i32; 3]) -> usize {
    // Horizontal directions in vanilla's order: north, east, south, west.
    const STEP: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
    let mut wall: Option<usize> = None;
    for (direction, step) in STEP.iter().enumerate() {
        match level.beside([pos[0] + step[0], pos[1], pos[2] + step[1]]) {
            Beside::Chest => return 0,
            Beside::Solid => {
                if wall.is_some() {
                    wall = None;
                    break;
                }
                wall = Some(direction);
            }
            Beside::Open => {}
        }
    }
    if let Some(direction) = wall {
        return (direction + 2) % 4;
    }
    let solid = |level: &mut dyn Level, direction: usize| {
        let step = STEP[direction];
        level.beside([pos[0] + step[0], pos[1], pos[2] + step[1]]) == Beside::Solid
    };
    let mut facing = 0;
    if solid(level, facing) {
        facing = (facing + 2) % 4;
        if solid(level, facing) {
            facing = (facing + 1) % 4;
            if solid(level, facing) {
                facing = (facing + 2) % 4;
            }
        }
    }
    facing
}

/// One piece's carving state for one chunk.
struct Carver<'a> {
    level: &'a mut dyn Level,
    deco: &'a mut mod_api::LegacyRandomSource,
    mask: &'a mut Mask,
    surface: &'a [i32; 256],
    chunk: [i32; 2],
    frame: Frame,
    start: [i32; 3],
}

impl Carver<'_> {
    /// One ellipsoid at `centre` with `radii`: its inside to cave air, its shell to cobblestone.
    fn ellipsoid(&mut self, centre: [f32; 3], radii: [f32; 3], mode: Mode) {
        let [cx, cy, cz] = centre;
        let [xr, yr, zr] = radii;
        let origin = [self.chunk[0] * 16, self.chunk[1] * 16];
        let local = |value: f32, radius: f32, base: i32| {
            (
                floor(value - radius) - base - 1,
                floor(value + radius) - base + 1,
            )
        };
        let (min_x, max_x) = local(cx, xr, origin[0]);
        let (min_z, max_z) = local(cz, zr, origin[1]);
        let min_y = (floor(cy - yr) - 1).clamp(self.frame.min_y, self.frame.max_y);
        let max_y = (floor(cy + yr) + 1).clamp(self.frame.min_y, self.frame.max_y);
        let (min_x, max_x) = (min_x.clamp(0, 15), max_x.clamp(0, 15));
        let (min_z, max_z) = (min_z.clamp(0, 15), max_z.clamp(0, 15));
        let column = [self.start[0] as f32, self.start[2] as f32];

        for x in min_x..=max_x {
            let gx = x + origin[0];
            let rx = (gx as f32 - cx + 0.5) / xr;
            for z in min_z..=max_z {
                let gz = z + origin[1];
                let rz = (gz as f32 - cz + 0.5) / zr;
                for y in min_y..=max_y {
                    // Don't go above the surface.
                    if y > self.surface[(x * 16 + z) as usize] {
                        break;
                    }
                    let ry = (y as f32 - cy - 0.5) / yr;
                    let index = (x | z << 4 | (y - self.frame.min_y) << 8) as usize;
                    let pos = [gx, y, gz];
                    let dist = rx * rx + ry * ry + rz * rz;
                    // The nest's wool column, from the centre to the ceiling.
                    let on_column =
                        mode == Mode::Nest && gx as f32 == column[0] && gz as f32 == column[1];
                    let above_centre = on_column && y as f32 > cy;
                    if dist < 1.0 {
                        if above_centre {
                            self.level.put(pos, Put::WhiteWool);
                        } else if !self.mask.get(index) {
                            self.carve_inside(pos, index);
                        }
                        continue;
                    }
                    if mode == Mode::Tunnel && self.mask.get(index) {
                        continue;
                    }
                    let sx = (gx as f32 - cx + 0.5) / (xr + 1.2);
                    let sy = (y as f32 - cy - 0.5) / (yr + 1.2);
                    let sz = (gz as f32 - cz + 0.5) / (zr + 1.2);
                    if sx * sx + sy * sy + sz * sz < 1.0 {
                        if above_centre {
                            // Guarantee wool up to the ceiling.
                            self.level.put(pos, Put::WhiteWool);
                        } else if !self.mask.get(index) {
                            self.shell(pos, mode);
                        }
                    }
                }
            }
        }
    }

    fn carve_inside(&mut self, pos: [i32; 3], index: usize) {
        if !self.level.cell(pos).blacklisted {
            self.level.put(pos, Put::CaveAir);
            self.mask.set(index);
        }
    }

    /// The cobblestone just outside the ellipsoid.
    fn shell(&mut self, pos: [i32; 3], mode: Mode) {
        let cell = self.level.cell(pos);
        if cell.blacklisted {
            return;
        }
        match mode {
            Mode::Tunnel => {
                if cell.air || cell.fluid || self.deco.next_float() < 0.2 {
                    self.level.put(pos, Put::Cobblestone);
                }
            }
            // Only over air, so the shell never closes a tunnel's mouth. A shell over fluid
            // cannot happen here: fluid is not air.
            Mode::Nest => {
                if cell.air && self.deco.next_float() < 0.8 {
                    // The original's shell selector draws once more to pick its one block.
                    let _ = self.deco.next_float();
                    self.level.put(pos, Put::Cobblestone);
                }
            }
            Mode::Egg => {
                if cell.air {
                    self.level.put(pos, Put::Cobblestone);
                }
            }
        }
    }

    /// `placeSphereRandomized`: each cell of a ball takes `what` with `chance`; the selector's own
    /// draw follows the chance's.
    fn sphere(&mut self, centre: [i32; 3], radius: i32, chance: f32, what: Put, only_air: bool) {
        let bound = (radius * radius) as f32;
        for x in -radius..=radius {
            for z in -radius..=radius {
                for y in -radius..=radius {
                    if ((x * x + y * y + z * z) as f32) < bound {
                        let pos = [centre[0] + x, centre[1] + y, centre[2] + z];
                        if (!only_air || self.level.cell(pos).air)
                            && self.deco.next_float() < chance
                        {
                            let _ = self.deco.next_float();
                            self.level.put(pos, what);
                        }
                    }
                }
            }
        }
    }

    /// `decorateCave`: a cobweb on a fifth of a tenth of the carved cells that touch a wall.
    fn decorate(&mut self) {
        let indices: Vec<usize> = self.mask.indices().collect();
        for index in indices {
            if self.deco.next_float() < 0.15 {
                let x = (index & 0xF) as i32;
                let z = (index >> 4 & 0xF) as i32;
                let y = (index >> 8) as i32 + self.frame.min_y;
                let (gx, gz) = (x + self.chunk[0] * 16, z + self.chunk[1] * 16);
                let touches_wall = [
                    [gx - 1, y, gz],
                    [gx + 1, y, gz],
                    [gx, y, gz - 1],
                    [gx, y, gz + 1],
                    [gx, y - 1, gz],
                    [gx, y + 1, gz],
                ]
                .into_iter()
                .any(|pos| self.level.cell(pos).occludes);
                if touches_wall && !self.level.cell([gx, y, gz]).blacklisted {
                    self.level.put([gx, y, gz], Put::Cobweb);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::LegacyRandomSource;
    use std::collections::HashMap;

    /// A world of stone under a surface at `top`, one chunk writable.
    struct Stone {
        chunk: [i32; 2],
        top: i32,
        blocks: HashMap<[i32; 3], Put>,
        spawners: Vec<([i32; 3], &'static str)>,
        chests: Vec<[i32; 3]>,
        random: LegacyRandomSource,
    }

    impl Stone {
        fn new(chunk: [i32; 2], top: i32, random: i64) -> Self {
            Self {
                random: LegacyRandomSource::new(random),
                chunk,
                top,
                blocks: HashMap::new(),
                spawners: Vec::new(),
                chests: Vec::new(),
            }
        }

        fn in_chunk(&self, pos: [i32; 3]) -> bool {
            pos[0].div_euclid(16) == self.chunk[0] && pos[2].div_euclid(16) == self.chunk[1]
        }
    }

    impl Level for Stone {
        fn random(&mut self) -> &mut dyn RandomSource {
            &mut self.random
        }

        fn cell(&mut self, pos: [i32; 3]) -> Cell {
            if !self.in_chunk(pos) {
                return Cell::OUTSIDE;
            }
            match self.blocks.get(&pos) {
                Some(Put::CaveAir) | Some(Put::Cobweb) => Cell::OUTSIDE,
                Some(Put::WhiteWool) | Some(Put::Spawner) | Some(Put::Chest(_)) => Cell {
                    air: false,
                    fluid: false,
                    blacklisted: true,
                    occludes: false,
                    chest: matches!(self.blocks.get(&pos), Some(Put::Chest(_))),
                },
                _ if pos[1] > self.top => Cell::OUTSIDE,
                _ => Cell {
                    air: false,
                    fluid: false,
                    blacklisted: false,
                    occludes: true,
                    chest: false,
                },
            }
        }

        fn put(&mut self, pos: [i32; 3], what: Put) {
            if self.in_chunk(pos) {
                self.blocks.insert(pos, what);
            }
        }

        fn chunk(&self) -> [i32; 2] {
            self.chunk
        }

        fn inside(&self, pos: [i32; 3]) -> bool {
            self.in_chunk(pos)
        }

        fn surface(&mut self, _x: i32, _z: i32) -> i32 {
            self.top + 1
        }

        fn spawner(&mut self, pos: [i32; 3], mob: &'static str) {
            self.spawners.push((pos, mob));
        }

        fn chest(&mut self, pos: [i32; 3], _loot_seed: i64) {
            self.chests.push(pos);
        }

        fn beside(&mut self, pos: [i32; 3]) -> Beside {
            match self.blocks.get(&pos) {
                Some(Put::Chest(_)) => Beside::Chest,
                Some(Put::CaveAir) | Some(Put::Cobweb) => Beside::Open,
                _ if pos[1] > self.top => Beside::Open,
                _ => Beside::Solid,
            }
        }
    }

    const FRAME: Frame = Frame {
        seed: 1234,
        min_y: -64,
        max_y: 320,
    };

    /// A well-mixed seed: Java's `Random` gives near-identical first draws for small seeds.
    fn mixed(seed: i64) -> i64 {
        seed.wrapping_mul(-7_046_029_254_386_353_131)
    }

    fn planned(seed: i64) -> Vec<Piece> {
        let mut random = LegacyRandomSource::new(mixed(seed));
        plan(&mut random, [8, 70, 8])
    }

    #[test]
    fn the_sine_table_reads_as_vanilla_does() {
        assert!((sin(PI / 2.0) - 1.0).abs() < 1e-3);
        assert_eq!(cos(0.0), 1.0);
        assert!((cos(PI) + 1.0).abs() < 1e-3);
        // Negative angles wrap through the table's mask.
        assert!((sin(-PI / 2.0) + 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_plan_follows_the_original_chain() {
        for seed in 0..60 {
            let pieces = planned(seed);
            let count = |kind| pieces.iter().filter(|p| p.kind == kind).count();
            // The start is a big tunnel, and a nest closes each big tunnel.
            assert_eq!(pieces[0].kind, Kind::BigTunnel, "seed {seed}");
            assert_eq!(pieces[0].start, [8, 70, 8]);
            assert_eq!(count(Kind::Nest), count(Kind::BigTunnel), "seed {seed}");
            // Two big tunnels, three of them when the first one's fourth branch is a big tunnel.
            assert!((2..=3).contains(&count(Kind::BigTunnel)), "seed {seed}");
            // Every big tunnel leaves three or four small ones.
            let small = count(Kind::SmallTunnel);
            assert!(
                small >= 3 * count(Kind::BigTunnel) && small <= 4 * count(Kind::BigTunnel),
                "seed {seed}: {small} small tunnels"
            );
            assert!(count(Kind::EggRoom) <= small);
        }
    }

    #[test]
    fn the_second_big_tunnel_starts_where_the_first_ends() {
        let pieces = planned(3);
        let second = pieces
            .iter()
            .filter(|p| p.kind == Kind::BigTunnel)
            .nth(1)
            .expect("a second big tunnel");
        assert_eq!(second.start, pieces[0].end);
    }

    #[test]
    fn every_box_holds_its_piece() {
        for seed in 0..60 {
            for piece in planned(seed) {
                // The original's running box compares against its own truncated corners, so it
                // can sit a block inside the walk; it is never empty and a room's holds its centre.
                for axis in 0..3 {
                    assert!(piece.min[axis] <= piece.max[axis], "{piece:?}");
                    if matches!(piece.kind, Kind::Nest | Kind::EggRoom) {
                        assert!(piece.min[axis] < piece.start[axis], "{piece:?}");
                        assert!(piece.max[axis] > piece.start[axis], "{piece:?}");
                    }
                }
                if piece.kind == Kind::BigTunnel {
                    // A big tunnel always drops.
                    assert!(piece.end[1] < piece.start[1], "seed {seed}");
                    assert_eq!(piece.yaws.len(), LENGTH);
                }
            }
        }
    }

    #[test]
    fn the_plan_is_deterministic_and_seed_dependent() {
        assert_eq!(planned(9), planned(9));
        assert_ne!(planned(9), planned(10));
    }

    #[test]
    fn a_nest_is_carved_walled_and_holds_a_cave_spider_spawner() {
        let pieces = planned(5);
        let nest = pieces.iter().find(|p| p.kind == Kind::Nest).unwrap();
        let chunk = [nest.start[0].div_euclid(16), nest.start[2].div_euclid(16)];
        let mut level = Stone::new(chunk, 200, 1);
        nest.place(&mut level, FRAME);
        assert!(level.blocks.values().any(|put| *put == Put::CaveAir));
        assert_eq!(
            level
                .blocks
                .get(&[nest.start[0], nest.start[1] + 1, nest.start[2]]),
            Some(&Put::Spawner)
        );
        assert_eq!(
            level.spawners,
            [(
                [nest.start[0], nest.start[1] + 1, nest.start[2]],
                "minecraft:cave_spider"
            )]
        );
        // The wool column reaches up from the spawner.
        assert_eq!(
            level
                .blocks
                .get(&[nest.start[0], nest.start[1] + 3, nest.start[2]]),
            Some(&Put::WhiteWool)
        );
    }

    #[test]
    fn carving_never_goes_above_the_surface() {
        let pieces = planned(5);
        let nest = pieces.iter().find(|p| p.kind == Kind::Nest).unwrap();
        let chunk = [nest.start[0].div_euclid(16), nest.start[2].div_euclid(16)];
        let top = nest.start[1] + 1;
        let mut level = Stone::new(chunk, top, 1);
        nest.place(&mut level, FRAME);
        let carved: Vec<_> = level
            .blocks
            .iter()
            .filter(|(_, put)| **put == Put::CaveAir)
            .collect();
        assert!(!carved.is_empty());
        assert!(carved.iter().all(|(pos, _)| pos[1] <= top + 1));
    }

    #[test]
    fn an_egg_room_holds_a_chest_or_a_spider_spawner() {
        let mut chests = 0;
        let mut spawners = 0;
        for seed in 0..40 {
            let pieces = planned(seed);
            let Some(room) = pieces.iter().find(|p| p.kind == Kind::EggRoom) else {
                continue;
            };
            let chunk = [room.start[0].div_euclid(16), room.start[2].div_euclid(16)];
            let mut level = Stone::new(chunk, 200, mixed(seed));
            room.place(&mut level, FRAME);
            chests += level.chests.len();
            spawners += level.spawners.len();
            assert!(level.chests.len() + level.spawners.len() <= 1);
        }
        assert!(
            chests > 0 && spawners > 0,
            "{chests} chests, {spawners} spawners"
        );
    }

    #[test]
    fn a_tunnel_carves_a_connected_passage_with_a_cobblestone_shell() {
        let pieces = planned(2);
        let tunnel = &pieces[0];
        let mut carved = 0;
        let mut shell = 0;
        for cx in (tunnel.min[0].div_euclid(16))..=(tunnel.max[0].div_euclid(16)) {
            for cz in (tunnel.min[2].div_euclid(16))..=(tunnel.max[2].div_euclid(16)) {
                let mut level = Stone::new([cx, cz], 300, 1);
                tunnel.place(&mut level, FRAME);
                carved += level
                    .blocks
                    .values()
                    .filter(|p| **p == Put::CaveAir)
                    .count();
                shell += level
                    .blocks
                    .values()
                    .filter(|p| **p == Put::Cobblestone)
                    .count();
            }
        }
        assert!(carved > 100, "{carved}");
        assert!(shell > 0);
    }

    #[test]
    fn a_chest_turns_its_back_to_the_one_wall() {
        struct Wall(LegacyRandomSource);
        impl Level for Wall {
            fn random(&mut self) -> &mut dyn RandomSource {
                &mut self.0
            }
            fn cell(&mut self, _: [i32; 3]) -> Cell {
                Cell::OUTSIDE
            }
            fn put(&mut self, _: [i32; 3], _: Put) {}
            fn chunk(&self) -> [i32; 2] {
                [0, 0]
            }
            fn inside(&self, _: [i32; 3]) -> bool {
                true
            }
            fn surface(&mut self, _: i32, _: i32) -> i32 {
                0
            }
            fn spawner(&mut self, _: [i32; 3], _: &'static str) {}
            fn chest(&mut self, _: [i32; 3], _: i64) {}
            fn beside(&mut self, pos: [i32; 3]) -> Beside {
                // A wall to the east only.
                if pos[0] == 1 {
                    Beside::Solid
                } else {
                    Beside::Open
                }
            }
        }
        // The wall is east, so the chest faces west.
        assert_eq!(
            reorient(&mut Wall(LegacyRandomSource::new(0)), [0, 0, 0]),
            3
        );
    }
}
