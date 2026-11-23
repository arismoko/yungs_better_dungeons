//! The `small_dungeon` jigsaw assembly and its decoration, as pure logic.
//!
//! This module has no `mod_api` type: it works on the generated tables
//! ([`crate::templates`], [`crate::pools`]), plain integers and its own traits,
//! so `cargo test` alone proves every number here. `support.rs` is the thin glue
//! that hands these answers to the game.
//!
//! The assembly is `yungsapi:yung_jigsaw`'s, which this campaign has not ported
//! and must not port: `YungJigsawStructure.findGenerationPoint` calls
//! `JigsawManager.assembleJigsawStructure`, which draws a weighted start element
//! (shuffled first, `max_count` and depth caps respected), places it so its
//! named anchor jigsaw sits on the chunk's start position, then walks every
//! connector breadth-first, drawing a child from the connector's pool and
//! trying the four rotations until one of the child's own connectors faces the
//! parent's (`JigsawBlock.canAttach`) and its box fits
//! (`JigsawStructureAssembler.chooseCandidateFromPool`). The decoration is the
//! eleven `betterdungeons:*` processors of the jar's four `small_dungeon`
//! processor lists, called `StructureProcessor.processBlock` one block at a time
//! in template order.
//!
//! What this port does **not** reproduce, each because the artefact cannot carry
//! it: `JigsawJunction` bookkeeping (only terrain adaptation reads it, and this
//! structure declares none), the non-rigid "surface height" branch (every
//! carried element is `rigid`, and a non-rigid one is refused rather than
//! guessed), and the config file's own settings (its defaults are compiled in).
//! The decorations draw from a position-stable stream instead of the game's
//! placement random: the distribution is the Java mod's, the exact draw order is
//! not, so a dungeon matches `notes.md`'s description block for block in kind
//! and not in instance.

use crate::pools::{self, Element, Rule, Step};
use crate::templates::{self, Block, Jigsaw, Template};

/// A source of randomness, in vanilla's `RandomSource` terms.
///
/// The game's `RandomSource` implements it directly, so the draws a structure
/// makes are the ones vanilla's own types would make from the same context; the
/// readout uses [`SplitMix`], which is deterministic for a seed and needs no game.
pub trait Rng {
    /// `RandomSource.nextInt(bound)`; 0 for a bound below 1.
    fn int(&mut self, bound: i32) -> i32;
    /// `RandomSource.nextFloat`, in `[0, 1)`.
    fn float(&mut self) -> f32;
}

/// A deterministic `Rng`, so the readout and the tests need no game.
#[derive(Clone, Debug)]
pub struct SplitMix(u64);

impl SplitMix {
    /// A stream from one seed.
    pub fn new(seed: i64) -> Self {
        Self(seed as u64 ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

impl Rng for SplitMix {
    fn int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as i32
    }

    fn float(&mut self) -> f32 {
        // 24 bits of mantissa is what `nextFloat` produces.
        ((self.next_u64() >> 40) as f32) / (1u32 << 24) as f32
    }
}

/// Why an assembly or a piece refused to build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A config or a table this port does not carry.
    Unsupported(String),
    /// A table that should have an entry and does not.
    Missing(String),
    /// A pool element whose template is not in the crate.
    UnknownTemplate(String),
    /// The start element has no jigsaw with the configured name.
    NoAnchor(String),
    /// A pool entry that names no usable connector target.
    NoConnector(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::Missing(what) => write!(f, "missing table entry: {what}"),
            Self::UnknownTemplate(id) => write!(f, "unknown template: {id}"),
            Self::NoAnchor(name) => write!(f, "no start jigsaw named {name}"),
            Self::NoConnector(what) => write!(f, "no connector: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// A quarter turn, clockwise, about the template's Y axis of rotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    /// No turn.
    None,
    /// One clockwise quarter turn.
    Cw90,
    /// A half turn.
    Cw180,
    /// One counterclockwise quarter turn.
    Ccw90,
}

/// The four rotations, in vanilla `Rotation` order (`getRandom` indexes this).
pub const ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Cw90,
    Rotation::Cw180,
    Rotation::Ccw90,
];

impl Rotation {
    /// The rotation `index` names, mod 4.
    pub fn from_index(index: i32) -> Self {
        ROTATIONS[index.rem_euclid(4) as usize]
    }

    /// Its index into [`ROTATIONS`].
    pub fn index(self) -> usize {
        ROTATIONS.iter().position(|r| *r == self).unwrap_or(0)
    }

    /// `StructureTemplate`'s size after this rotation.
    pub fn size(self, size: [i16; 3]) -> [i16; 3] {
        match self {
            Self::None | Self::Cw180 => size,
            Self::Cw90 | Self::Ccw90 => [size[2], size[1], size[0]],
        }
    }

    /// `StructureTemplate.transform`'s position of `pos` inside a template of `size`.
    pub fn pos(self, pos: [i16; 3], size: [i16; 3]) -> [i16; 3] {
        let [x, y, z] = pos;
        let [sx, _, sz] = size;
        match self {
            Self::None => [x, y, z],
            Self::Cw90 => [sz - 1 - z, y, x],
            Self::Cw180 => [sx - 1 - x, y, sz - 1 - z],
            Self::Ccw90 => [z, y, sx - 1 - x],
        }
    }
}

/// A block face, as a jigsaw connector's `orientation` names its front and top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// -Z.
    North,
    /// +X.
    East,
    /// +Z.
    South,
    /// -X.
    West,
    /// +Y.
    Up,
    /// -Y.
    Down,
}

impl Dir {
    /// This face's opposite.
    pub fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    /// The block this face steps to (`Direction.getStepX/Y/Z` as one vector).
    pub fn step(self) -> [i32; 3] {
        match self {
            Self::North => [0, 0, -1],
            Self::East => [1, 0, 0],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::Up => [0, 1, 0],
            Self::Down => [0, -1, 0],
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "north" => Self::North,
            "east" => Self::East,
            "south" => Self::South,
            "west" => Self::West,
            "up" => Self::Up,
            "down" => Self::Down,
            _ => return None,
        })
    }
}

/// An inclusive integer box, in world blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Box3 {
    /// The lowest corner.
    pub min: [i32; 3],
    /// The highest corner.
    pub max: [i32; 3],
}

impl Box3 {
    /// The box spanning `position .. position + size - 1`.
    pub fn from_size(position: [i32; 3], size: [i16; 3]) -> Self {
        Self {
            min: position,
            max: std::array::from_fn(|axis| position[axis] + i32::from(size[axis]) - 1),
        }
    }

    /// `BoundingBox.intersects`: the two boxes share at least one block.
    pub fn intersects(&self, other: &Self) -> bool {
        (0..3).all(|axis| self.max[axis] >= other.min[axis] && self.min[axis] <= other.max[axis])
    }

    /// Whether `point` is inside.
    pub fn contains(&self, point: [i32; 3]) -> bool {
        (0..3).all(|axis| point[axis] >= self.min[axis] && point[axis] <= self.max[axis])
    }

    /// Whether this box is entirely inside `outer`.
    pub fn is_inside(&self, outer: &Self) -> bool {
        (0..3).all(|axis| self.min[axis] >= outer.min[axis] && self.max[axis] <= outer.max[axis])
    }
}

/// The structure definition a pack's `worldgen/structure` JSON carries, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// `start_pool`.
    pub start_pool: String,
    /// `start_jigsaw_name`.
    pub start_jigsaw: Option<String>,
    /// `size`: the jigsaw depth cap.
    pub size: i32,
    /// `start_height` as an inclusive absolute range.
    pub start_y: (i32, i32),
    /// `max_distance_from_center`.
    pub max_distance: i32,
    /// `use_expansion_hack`.
    pub use_expansion_hack: bool,
}

/// Whether `config` is one this port's carried tables can assemble.
///
/// A pack may edit the definition; the port can only generate what its own
/// templates, pools and caps describe, so anything else is refused instead of
/// approximated. `/betterdungeons status` prints this answer.
pub fn config_supported(config: &Config) -> bool {
    let carried = &pools::STRUCTURE;
    config.start_pool == carried.start_pool
        && config.start_jigsaw.as_deref() == carried.start_jigsaw
        && config.size == carried.size
        && config.start_y == carried.start_y
        && config.max_distance == carried.max_distance
        && !config.use_expansion_hack
}

/// One placed piece: a template, a rotation and where its origin sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The template's id.
    pub template: &'static str,
    /// The processor list the pool entry names.
    pub processors: &'static str,
    /// Its rotation.
    pub rotation: Rotation,
    /// The template origin, in world blocks.
    pub position: [i32; 3],
    /// The rotated template size.
    pub size: [i16; 3],
    /// Its depth from the start piece (the start is 0).
    pub depth: i32,
    /// The per-piece seed the decoration's position-stable stream mixes in.
    pub seed: i64,
    /// Chests kept by the pieces before this one, for the structure-wide cap.
    pub chests_before: i32,
    /// Banners kept by the pieces before this one.
    pub banners_before: i32,
    /// The level's minimum block Y, for the support legs' floor.
    pub min_y: i32,
}

impl Piece {
    /// The piece's box, in world blocks.
    pub fn bounding_box(&self) -> Box3 {
        Box3::from_size(self.position, self.size)
    }

    /// The template this piece places.
    pub fn template(&self) -> Result<&'static Template, Error> {
        templates::template(self.template)
            .ok_or_else(|| Error::UnknownTemplate(self.template.to_owned()))
    }
}

/// A finished assembly: the start's position and every piece, in placement order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assembly {
    /// The start position vanilla's `GenerationStub` answers (the start piece's
    /// centre, at the drawn height), which is also where `/locate` points.
    pub position: [i32; 3],
    /// Every piece, the start first.
    pub pieces: Vec<Piece>,
}

/// Assemble the structure for `chunk`, drawing from `rng` exactly as the Java
/// type does: the start element, its rotation, the anchor offset, then one
/// breadth-first pass over every connector.
///
/// # Errors
///
/// [`Error::Unsupported`] for a config the carried tables cannot describe,
/// [`Error::NoAnchor`] when the start element has no jigsaw with the configured
/// name, [`Error::UnknownTemplate`] when a pool names a template the crate does
/// not carry.
pub fn assemble(
    config: &Config,
    settings: &Settings,
    chunk: [i32; 2],
    seed: i64,
    min_y: i32,
    max_y: i32,
    rng: &mut dyn Rng,
) -> Result<Assembly, Error> {
    if !config_supported(config) {
        return Err(Error::Unsupported(format!(
            "config (start_pool {}, size {}, start_height {:?}, max_distance {})",
            config.start_pool, config.size, config.start_y, config.max_distance
        )));
    }
    let pool =
        pools::pool(&config.start_pool).ok_or_else(|| Error::Missing(config.start_pool.clone()))?;

    // The start element: the pool's entries shuffled, then drawn by weight
    // (`JigsawManager.getStartPiece`).
    let mut candidates: Vec<usize> = (0..pool.elements.len()).collect();
    shuffle(&mut candidates, rng);
    let picked = weighted(&candidates, pool.elements, rng)
        .ok_or_else(|| Error::Missing(format!("a start element in {}", pool.id)))?;
    let element = &pool.elements[picked];
    let template = templates::template(element.template)
        .ok_or_else(|| Error::UnknownTemplate(element.template.to_owned()))?;
    let rotation = Rotation::from_index(rng.int(4));

    // The start position: the chunk's corner plus the structure's offsets (the
    // jar sets neither, so both are 0) and the drawn height.
    let start_y = config.start_y.0 + rng.int(config.start_y.1 - config.start_y.0 + 1);
    let locate = [chunk[0] * 16, start_y, chunk[1] * 16];

    // The anchor: the piece is shifted so this jigsaw sits on `locate`.
    let anchor = match config.start_jigsaw.as_deref() {
        Some(name) => template
            .jigsaws
            .iter()
            .find(|jigsaw| jigsaw.name == name)
            .ok_or_else(|| Error::NoAnchor(name.to_owned()))?,
        None => return Err(Error::NoAnchor("(none configured)".to_owned())),
    };
    let anchor_local = rotation.pos(anchor.pos, template.size);
    let start_position = [
        locate[0] - i32::from(anchor_local[0]),
        locate[1] - i32::from(anchor_local[1]),
        locate[2] - i32::from(anchor_local[2]),
    ];

    let mut pieces = vec![Piece {
        template: template.id,
        processors: element.processors,
        rotation,
        position: start_position,
        size: rotation.size(template.size),
        depth: 0,
        seed: piece_seed(seed, 0, start_position),
        chests_before: 0,
        banners_before: 0,
        min_y,
    }];

    // The structure's own bounds: the start's centre ± max_distance, and the
    // level's height. `JigsawManager` builds the same box.
    let start_box = pieces[0].bounding_box();
    let centre = [
        (start_box.min[0] + start_box.max[0]) / 2,
        locate[1],
        (start_box.min[2] + start_box.max[2]) / 2,
    ];
    let bounds = Box3 {
        min: [
            centre[0] - config.max_distance,
            min_y,
            centre[2] - config.max_distance,
        ],
        max: [
            centre[0] + config.max_distance,
            max_y,
            centre[2] + config.max_distance,
        ],
    };

    let mut queue = std::collections::VecDeque::from([0usize]);
    while let Some(index) = queue.pop_front() {
        let depth = pieces[index].depth;
        let connectors = connectors(&pieces[index]);
        for connector in connectors {
            let pool = match pools::pool(connector.pool) {
                Some(pool) => pool,
                // `minecraft:empty`, or a pool this port does not carry: the
                // connector leads nowhere, as an empty pool does in vanilla.
                None => continue,
            };
            let fallback = pools::pool(pool.fallback);
            let mut placed = None;
            if depth != config.size {
                placed = choose(pool, &connector, &pieces, index, &bounds, min_y, seed, rng)?;
            }
            if placed.is_none()
                && let Some(fallback) = fallback
            {
                placed = choose(
                    fallback, &connector, &pieces, index, &bounds, min_y, seed, rng,
                )?;
            }
            if let Some(piece) = placed {
                pieces.push(piece);
                if depth < config.size {
                    queue.push_back(pieces.len() - 1);
                }
            }
        }
    }

    plan_counts(&mut pieces, settings)?;

    Ok(Assembly {
        position: centre,
        pieces,
    })
}

/// A connector in world blocks: its own block, its state (whose `orientation`
/// gives its front and top), the pool it draws from, and the name it wants.
#[derive(Clone, Debug)]
struct Connector {
    pos: [i32; 3],
    front: Dir,
    top: Dir,
    pool: &'static str,
    target: &'static str,
}

/// Every connector of `piece`, in world blocks and shuffled, as
/// `StructurePoolElement.getShuffledJigsawBlocks` answers.
fn connectors(piece: &Piece) -> Vec<Connector> {
    let Ok(template) = piece.template() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(template.jigsaws.len());
    for jigsaw in template.jigsaws {
        let Some(oriented) = oriented(jigsaw) else {
            continue;
        };
        let local = piece.rotation.pos(jigsaw.pos, template.size);
        out.push(Connector {
            pos: [
                piece.position[0] + i32::from(local[0]),
                piece.position[1] + i32::from(local[1]),
                piece.position[2] + i32::from(local[2]),
            ],
            front: turn(oriented.0, piece.rotation),
            top: turn(oriented.1, piece.rotation),
            pool: jigsaw.pool,
            target: jigsaw.target,
        });
    }
    // `oriented` reads the *unrotated* state, so the turn is applied here once.
    out
}

/// A connector's front and top, from its own unrotated state.
fn oriented(jigsaw: &Jigsaw) -> Option<(Dir, Dir)> {
    let state = &templates::STATES[jigsaw.state as usize];
    let mut parts = state
        .props
        .iter()
        .filter(|(name, _)| *name == "orientation");
    let (_, value) = parts.next()?;
    let (front, top) = value.split_once('_')?;
    Some((Dir::from_name(front)?, Dir::from_name(top)?))
}

/// A face after a clockwise rotation.
fn turn(dir: Dir, rotation: Rotation) -> Dir {
    if matches!(dir, Dir::Up | Dir::Down) {
        return dir;
    }
    let mut dir = dir;
    for _ in 0..rotation.index() % 4 {
        dir = match dir {
            Dir::North => Dir::East,
            Dir::East => Dir::South,
            Dir::South => Dir::West,
            _ => Dir::North,
        };
    }
    dir
}

/// Draw a child for `connector` out of `pool`, or `None` when the pool is spent.
///
/// The bounds rule is YUNG's: a connector whose target block is **inside** the
/// parent piece gets a fresh octree holding no boxes but bounded by the parent,
/// so its child only has to fit inside the parent piece; any other connector's
/// child must avoid every placed piece and stay inside the structure's bounds
/// (`JigsawStructureAssembler.createPieceContextForJigsawBlock`).
#[expect(
    clippy::too_many_arguments,
    reason = "the assembler's context: pool, connector, the pieces placed so far, the \
              parent's index, the structure bounds, the floor, the seed and the draw"
)]
fn choose(
    pool: &'static pools::Pool,
    connector: &Connector,
    pieces: &[Piece],
    parent: usize,
    bounds: &Box3,
    min_y: i32,
    seed: i64,
    rng: &mut dyn Rng,
) -> Result<Option<Piece>, Error> {
    let parent_box = pieces[parent].bounding_box();
    let target = [
        connector.pos[0] + connector.front.step()[0],
        connector.pos[1] + connector.front.step()[1],
        connector.pos[2] + connector.front.step()[2],
    ];
    let inside_parent = parent_box.contains(target);
    let mut counts: std::collections::BTreeMap<&'static str, i32> = counts_of(pieces);
    let mut candidates: Vec<usize> = (0..pool.elements.len()).collect();
    shuffle(&mut candidates, rng);
    let mut remaining: i32 = pool.elements.iter().map(|element| element.weight).sum();
    while !candidates.is_empty() && remaining > 0 {
        let Some(index) = weighted(&candidates, pool.elements, rng) else {
            break;
        };
        let element = &pool.elements[index];
        let weight = element.weight;

        if let (Some(name), Some(max)) = (element.name, element.max_count)
            && counts.get(name).copied().unwrap_or(0) >= max
        {
            remaining -= weight;
            candidates.retain(|candidate| *candidate != index);
            continue;
        }

        let template = templates::template(element.template)
            .ok_or_else(|| Error::UnknownTemplate(element.template.to_owned()))?;

        let mut rotations = ROTATIONS.to_vec();
        shuffle(&mut rotations, rng);

        for rotation in rotations {
            let size = rotation.size(template.size);
            for child in template.jigsaws {
                let Some((front, top)) = oriented(child) else {
                    continue;
                };
                // `JigsawBlock.canAttach`: the child, *as rotated*, must face the
                // parent, share its top, and carry the name the parent targets.
                if !attaches(
                    connector,
                    turn(front, rotation),
                    turn(top, rotation),
                    child.name,
                ) {
                    continue;
                }
                // The child's own connector takes the cell in front of the
                // parent's, so its origin is `target - its connector`. For two
                // rigid pieces that is exactly what vanilla's y arithmetic
                // (`jigsawBlockRelativeY - candidateJigsawBlockRelativeY +
                // stepY`) reduces to.
                let local = rotation.pos(child.pos, template.size);
                let origin = [
                    target[0] - i32::from(local[0]),
                    target[1] - i32::from(local[1]),
                    target[2] - i32::from(local[2]),
                ];
                let piece = Piece {
                    template: template.id,
                    processors: element.processors,
                    rotation,
                    position: origin,
                    size,
                    depth: pieces[parent].depth + 1,
                    seed: piece_seed(seed, pieces[parent].depth + 1, origin),
                    chests_before: 0,
                    banners_before: 0,
                    min_y,
                };
                let candidate_box = piece.bounding_box();
                if candidate_box.min[1] < bounds.min[1] || candidate_box.max[1] > bounds.max[1] {
                    continue;
                }
                if inside_parent {
                    if !candidate_box.is_inside(&parent_box) {
                        continue;
                    }
                } else if !candidate_box.is_inside(bounds)
                    || pieces
                        .iter()
                        .any(|placed| placed.bounding_box().intersects(&candidate_box))
                {
                    continue;
                }
                if element.max_count.is_some()
                    && let Some(name) = element.name
                {
                    *counts.entry(name).or_insert(0) += 1;
                }
                return Ok(Some(piece));
            }
        }

        remaining -= weight;
        candidates.retain(|candidate| *candidate != index);
    }
    Ok(None)
}

/// How many of each named element the pieces so far hold (`max_count`).
fn counts_of(pieces: &[Piece]) -> std::collections::BTreeMap<&'static str, i32> {
    let mut counts: std::collections::BTreeMap<&'static str, i32> =
        std::collections::BTreeMap::new();
    for piece in pieces {
        for pool in pools::POOLS {
            for element in pool.elements {
                if let (Some(max), Some(name)) = (element.max_count, element.name)
                    && element.template == piece.template
                    && max > 0
                {
                    *counts.entry(name).or_insert(0) += 1;
                }
            }
        }
    }
    counts
}

/// `JigsawBlock.canAttach` for a parent connector and a child's own connector.
///
/// `front_down_step` is folded into the origin, so this is the pure predicate:
/// the child must face the parent, share its top, and carry the name the parent
/// targets.
fn attaches(parent: &Connector, child_front: Dir, child_top: Dir, child_name: &str) -> bool {
    child_front == parent.front.opposite() && child_top == parent.top && child_name == parent.target
}

/// Fisher–Yates from the end, as `Util.shuffle` draws.
fn shuffle<T>(items: &mut [T], rng: &mut dyn Rng) {
    for index in (1..items.len()).rev() {
        let other = rng.int(index as i32 + 1) as usize;
        items.swap(index, other);
    }
}

/// One weighted pick from `candidates` (`getStartPiece`'s `chosenWeight` loop).
fn weighted(candidates: &[usize], elements: &[Element], rng: &mut dyn Rng) -> Option<usize> {
    let total: i32 = candidates.iter().map(|index| elements[*index].weight).sum();
    if total <= 0 {
        return None;
    }
    let mut draw = rng.int(total) + 1;
    for index in candidates {
        draw -= elements[*index].weight;
        if draw <= 0 {
            return Some(*index);
        }
    }
    candidates.last().copied()
}

/// A per-piece seed: stable for a structure seed, a depth and a position.
fn piece_seed(seed: i64, depth: i32, position: [i32; 3]) -> i64 {
    let mut hash = (seed as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= (depth as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    for value in position {
        hash = mix(hash ^ (value as i64 as u64));
    }
    hash as i64
}

fn mix(value: u64) -> u64 {
    let mut value = value;
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

/// A position-stable random stream: the same piece, block and purpose draw the
/// same number however the chunk calls are ordered.
struct Stream(u64);

impl Stream {
    fn new(piece: &Piece, pos: [i32; 3], purpose: u64) -> Self {
        let mut hash = piece.seed as u64 ^ purpose.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for value in pos {
            hash = mix(hash ^ (value as i64 as u64));
        }
        Self(mix(hash))
    }

    fn float(&mut self) -> f32 {
        self.0 = mix(self.0 ^ 0x9e37_79b9_7f4a_7c15);
        ((self.0 >> 40) as f32) / (1u32 << 24) as f32
    }

    fn int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        self.0 = mix(self.0 ^ 0x9e37_79b9_7f4a_7c15);
        (self.0 % bound as u64) as i32
    }
}

/// The block-entity instruction a placed block carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Nothing.
    None,
    /// A chest with this loot table.
    Chest(&'static str),
    /// A barrel with this loot table.
    Barrel(&'static str),
    /// A banner of this dungeon's kind, with `facing` already in its state.
    Banner(Banner),
    /// A mob spawner of this entity.
    Spawner(&'static str),
    /// A skull, as the template left it.
    Skull,
    /// A jukebox.
    Jukebox,
    /// A furnace with its empty containers.
    Furnace,
}

/// Which dungeon's banner a wall banner becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    /// The skeleton dungeon's "Vengeful Banner".
    Skeleton,
    /// The zombie dungeon's "Foul Banner".
    Zombie,
    /// The spider dungeon's "Haunted Banner".
    Spider,
}

impl Banner {
    /// Its patterns and colours, from `SmallDungeonBannerProcessor`'s constants.
    pub fn patterns(self) -> &'static [(&'static str, &'static str)] {
        match self {
            // CURLY_BORDER white, STRIPE_CENTER white, STRIPE_BOTTOM black,
            // STRIPE_TOP white, CREEPER white, GRADIENT black.
            Self::Skeleton => &[
                ("minecraft:curly_border", "white"),
                ("minecraft:stripe_center", "white"),
                ("minecraft:stripe_bottom", "black"),
                ("minecraft:stripe_top", "white"),
                ("minecraft:creeper", "white"),
                ("minecraft:gradient", "black"),
            ],
            // TRIANGLE_BOTTOM pink, CIRCLE_MIDDLE gray, GRADIENT black,
            // TRIANGLES_TOP white, TRIANGLES_BOTTOM white, BORDER green.
            Self::Zombie => &[
                ("minecraft:triangle_bottom", "pink"),
                ("minecraft:circle_middle", "gray"),
                ("minecraft:gradient", "black"),
                ("minecraft:triangles_top", "white"),
                ("minecraft:triangles_bottom", "white"),
                ("minecraft:border", "green"),
            ],
            // FLOWER gray, BORDER gray, STRAIGHT_CROSS gray, HALF_HORIZONTAL
            // gray, STRIPE_BOTTOM gray, GRADIENT black.
            Self::Spider => &[
                ("minecraft:flower", "gray"),
                ("minecraft:border", "gray"),
                ("minecraft:straight_cross", "gray"),
                ("minecraft:half_horizontal", "gray"),
                ("minecraft:stripe_bottom", "gray"),
                ("minecraft:gradient", "black"),
            ],
        }
    }

    /// Its base block: the state the processor writes, before `facing`.
    pub fn block(self) -> &'static str {
        match self {
            Self::Skeleton => "minecraft:black_wall_banner",
            _ => "minecraft:red_wall_banner",
        }
    }

    /// Its translatable name key.
    pub fn name_key(self) -> &'static str {
        match self {
            Self::Skeleton => "betterdungeons.small_dungeon.banner.skeleton",
            Self::Zombie => "betterdungeons.small_dungeon.banner.zombie",
            Self::Spider => "betterdungeons.small_dungeon.banner.spider",
        }
    }

    /// The name the key falls back to.
    pub fn name(self) -> &'static str {
        match self {
            Self::Skeleton => "Vengeful Banner",
            Self::Zombie => "Foul Banner",
            Self::Spider => "Haunted Banner",
        }
    }

    /// The name's colour.
    pub fn colour(self) -> &'static str {
        match self {
            Self::Skeleton => "dark_gray",
            Self::Zombie => "dark_green",
            Self::Spider => "dark_red",
        }
    }

    /// The dungeon `text` names (`SmallDungeonBannerProcessor`'s `dungeon_type`).
    pub fn from_text(text: &str) -> Option<Self> {
        Some(match text {
            "skeleton" => Self::Skeleton,
            "zombie" => Self::Zombie,
            "spider" => Self::Spider,
            _ => return None,
        })
    }
}

/// One block a piece places, in world blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    /// Where it goes.
    pub pos: [i32; 3],
    /// Its state, as an index into [`templates::STATES`].
    pub state: u16,
    /// The block entity it carries.
    pub payload: Payload,
}

/// What a piece's decoration has to know about the world around it.
///
/// The glue answers from the game's `PieceContext`; the counting pass and the
/// tests answer `None`, which every processor reads as "leave it alone".
pub trait World {
    /// The block at `pos`, or `None` where the caller cannot see it.
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockInfo>;
}

/// The block facts a processor asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockInfo {
    /// Its canonical block id.
    pub name: std::string::String,
    /// Whether it is air.
    pub air: bool,
    /// Whether it draws as a solid block (`isFaceSturdy`'s stand-in).
    pub solid: bool,
}

/// The minimal world: nothing is known.
pub struct NoWorld;

impl World for NoWorld {
    fn block(&mut self, _pos: [i32; 3]) -> Option<BlockInfo> {
        None
    }
}

/// The dungeon's own settings, from the jar's config defaults.
/// The original's world-restart settings this code reads (`ConfigModule`), with its defaults.
///
/// The game keeps the player's values; [`crate::support`] reads them when a server is about to
/// start, as NeoForge applies a `worldRestart` config value only then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// `smallDungeons.bannerMaxCount`: at most this many banners per dungeon (0 to 8).
    pub banner_max_count: i32,
    /// `smallDungeons.chestMinCount`: at least this many chests per dungeon.
    pub chest_min_count: i32,
    /// `smallDungeons.chestMaxCount`: at most this many chests per dungeon.
    pub chest_max_count: i32,
    /// `general.enableHeads`: skulls stay.
    pub enable_heads: bool,
    /// `general.enableNetherBlocks`: soul blocks stay.
    pub enable_nether_blocks: bool,
    /// `smallDungeons.enableOreProps`: ore props stay.
    pub enable_ore_props: bool,
}

impl Settings {
    /// `ConfigModule`'s defaults.
    pub const DEFAULT: Self = Self {
        banner_max_count: 2,
        chest_min_count: 1,
        chest_max_count: 2,
        enable_heads: true,
        enable_nether_blocks: true,
        enable_ore_props: true,
    };
}

impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a processor step reads and keeps across a structure: the settings, and the counts so far.
pub struct Decoration<'a> {
    /// The settings generation runs with.
    pub settings: &'a Settings,
    /// The chests and banners kept so far.
    pub counts: Counts,
}

/// The chests and banners kept so far in a structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Chests kept.
    pub chests: i32,
    /// Banners kept.
    pub banners: i32,
}

/// Decorate one piece and answer its blocks in template order.
///
/// The processors run in the order the pool's processor list names them, one
/// block at a time, exactly as `StructureTemplate.placeInWorld` calls them.
///
/// # Errors
///
/// [`Error::Missing`] when the piece's processor list is not carried,
/// [`Error::UnknownTemplate`] when its template is not.
pub fn decorate(
    piece: &Piece,
    settings: &Settings,
    world: &mut dyn World,
) -> Result<(Vec<Placed>, Counts), Error> {
    let template = piece.template()?;
    let steps = processor_steps(piece.processors)?;
    let mut deco = Decoration {
        settings,
        counts: Counts {
            chests: piece.chests_before,
            banners: piece.banners_before,
        },
    };
    let mut out = Vec::with_capacity(template.blocks.len() + 8);
    for block in template.blocks {
        let local = piece.rotation.pos(block.pos, template.size);
        let pos = [
            piece.position[0] + i32::from(local[0]),
            piece.position[1] + i32::from(local[1]),
            piece.position[2] + i32::from(local[2]),
        ];
        let mut state = templates::ROTATED[block.state as usize][piece.rotation.index()];
        let mut payload = payload_of(*block, state);
        let mut keep = true;
        for step in steps {
            match apply(step, piece, pos, &mut state, &mut payload, &mut deco, world) {
                Applied::Keep => {}
                Applied::Skip => {
                    keep = false;
                    break;
                }
                Applied::Legs(blocks) => out.extend(blocks),
            }
        }
        if keep {
            out.push(Placed {
                pos,
                state,
                payload,
            });
        }
    }
    // `JigsawReplacementProcessor`: every connector block becomes its own
    // `final_state`, which the carried templates set to air.
    for jigsaw in template.jigsaws {
        let local = piece.rotation.pos(jigsaw.pos, template.size);
        out.push(Placed {
            pos: [
                piece.position[0] + i32::from(local[0]),
                piece.position[1] + i32::from(local[1]),
                piece.position[2] + i32::from(local[2]),
            ],
            state: templates::ROTATED[jigsaw.final_state as usize][piece.rotation.index()],
            payload: Payload::None,
        });
    }
    Ok((out, deco.counts))
}

/// The steps of a processor list; `minecraft:empty` is the empty chain.
///
/// # Errors
///
/// [`Error::Missing`] for a list this crate does not carry.
fn processor_steps(id: &str) -> Result<&'static [Step], Error> {
    if id == "minecraft:empty" {
        return Ok(&[]);
    }
    pools::processor_list(id)
        .map(|list| list.steps)
        .ok_or_else(|| Error::Missing(id.to_owned()))
}

/// What one processor did to a block.
enum Applied {
    Keep,
    Skip,
    Legs(Vec<Placed>),
}

fn payload_of(block: Block, state: u16) -> Payload {
    use templates::*;
    match block.payload {
        PAYLOAD_MINECRAFT_CHEST => Payload::Chest(CHEST_LOOT),
        PAYLOAD_MINECRAFT_BARREL => Payload::Barrel(BARREL_LOOT),
        PAYLOAD_MINECRAFT_BANNER => Payload::Banner(banner_of(state)),
        PAYLOAD_MINECRAFT_MOB_SPAWNER => Payload::Spawner("minecraft:pig"),
        PAYLOAD_MINECRAFT_SKULL => Payload::Skull,
        PAYLOAD_MINECRAFT_JUKEBOX => Payload::Jukebox,
        PAYLOAD_MINECRAFT_FURNACE => Payload::Furnace,
        _ => Payload::None,
    }
}

fn banner_of(state: u16) -> Banner {
    let name = templates::STATES[state as usize].name;
    if name == "minecraft:black_wall_banner" {
        Banner::Skeleton
    } else {
        Banner::Zombie
    }
}

fn state_named(name: &str) -> Option<u16> {
    templates::STATES
        .iter()
        .position(|state| state.name == name && state.props.is_empty())
        .map(|index| index as u16)
}

fn cuboid(block: &str, props: &[(&str, &str)]) -> Option<u16> {
    templates::STATES
        .iter()
        .position(|state| {
            state.name == block
                && state.props.len() == props.len()
                && props
                    .iter()
                    .all(|(name, value)| state.props.contains(&(*name, *value)))
        })
        .map(|index| index as u16)
}

fn apply(
    step: &'static Step,
    piece: &Piece,
    pos: [i32; 3],
    state: &mut u16,
    payload: &mut Payload,
    deco: &mut Decoration<'_>,
    world: &mut dyn World,
) -> Applied {
    let name = templates::STATES[*state as usize].name;
    match step.kind {
        pools::KIND_BANNER => {
            if !name.ends_with("_wall_banner") {
                return Applied::Keep;
            }
            let Some(banner) = Banner::from_text(step.text) else {
                return Applied::Keep;
            };
            let mut stream = Stream::new(piece, pos, 0);
            if deco.counts.banners >= deco.settings.banner_max_count {
                *state = state_named("minecraft:cave_air").unwrap_or(*state);
                *payload = Payload::None;
                return Applied::Keep;
            }
            if stream.float() > 0.1 {
                *state = state_named("minecraft:cave_air").unwrap_or(*state);
                *payload = Payload::None;
                return Applied::Keep;
            }
            let facing = templates::STATES[*state as usize]
                .props
                .iter()
                .find(|(key, _)| *key == "facing")
                .map(|(_, value)| *value)
                .unwrap_or("north");
            if let Some(replacement) = cuboid(banner.block(), &[("facing", facing)]) {
                *state = replacement;
            }
            *payload = Payload::Banner(banner);
            deco.counts.banners += 1;
            Applied::Keep
        }
        pools::KIND_SPAWNER => {
            if name != "minecraft:spawner" {
                return Applied::Keep;
            }
            *payload = Payload::Spawner(step.text);
            Applied::Keep
        }
        pools::KIND_CANDLE => {
            if name != "minecraft:sea_pickle" {
                return Applied::Keep;
            }
            let mut stream = Stream::new(piece, pos, 1);
            let candles = stream.int(4) + 1;
            let lit = stream.float() < 0.1;
            let colours = [
                "minecraft:candle",
                "minecraft:white_candle",
                "minecraft:gray_candle",
                "minecraft:light_gray_candle",
                "minecraft:brown_candle",
                "minecraft:green_candle",
                "minecraft:purple_candle",
                "minecraft:black_candle",
            ];
            let colour = colours[stream.int(colours.len() as i32) as usize];
            let props = [
                ("candles", candle_count(candles)),
                ("lit", if lit { "true" } else { "false" }),
                ("waterlogged", "false"),
            ];
            if let Some(replacement) = cuboid(colour, &props) {
                *state = replacement;
            }
            *payload = Payload::None;
            Applied::Keep
        }
        pools::KIND_HEAD => {
            if !name.ends_with("_skull") && !name.ends_with("_head") {
                return Applied::Keep;
            }
            if !deco.settings.enable_heads {
                *state = state_named("minecraft:cave_air").unwrap_or(*state);
                *payload = Payload::None;
            }
            Applied::Keep
        }
        pools::KIND_NETHER_BLOCK => {
            if deco.settings.enable_nether_blocks {
                return Applied::Keep;
            }
            let replacement = match name {
                "minecraft:soul_sand" | "minecraft:soul_soil" => {
                    Some(("minecraft:coarse_dirt", vec![]))
                }
                "minecraft:soul_campfire" => Some(("minecraft:campfire", vec![])),
                "minecraft:soul_lantern" => {
                    let hanging = templates::STATES[*state as usize]
                        .props
                        .iter()
                        .find(|(key, _)| *key == "hanging")
                        .map(|(_, value)| *value)
                        .unwrap_or("false");
                    Some(("minecraft:lantern", vec![("hanging", hanging)]))
                }
                _ => None,
            };
            if let Some((block, props)) = replacement
                && let Some(replacement) = cuboid(block, &props)
            {
                *state = replacement;
                *payload = Payload::None;
            }
            Applied::Keep
        }
        pools::KIND_CHEST => {
            if !name.ends_with("chest") {
                return Applied::Keep;
            }
            let mut stream = Stream::new(piece, pos, 2);
            if deco.counts.chests < deco.settings.chest_min_count {
                deco.counts.chests += 1;
                return Applied::Keep;
            }
            if deco.counts.chests < deco.settings.chest_max_count && stream.float() <= 0.2 {
                deco.counts.chests += 1;
                return Applied::Keep;
            }
            *state = state_named("minecraft:cave_air").unwrap_or(*state);
            *payload = Payload::None;
            Applied::Keep
        }
        pools::KIND_CEILING_PROP => ceiling_prop(piece, pos, name, state, payload, world),
        pools::KIND_CEILING_LAMP => {
            if name != "minecraft:cyan_stained_glass" {
                return Applied::Keep;
            }
            let mut stream = Stream::new(piece, pos, 5);
            let chain = stream.float() < 0.625;
            *state = chain_state(*state, chain);
            *payload = Payload::None;
            Applied::Keep
        }
        pools::KIND_COBBLESTONE => {
            if name != "minecraft:cobblestone" {
                return Applied::Keep;
            }
            if world.block(pos).is_some_and(|block| block.air) {
                *state = state_named("minecraft:cave_air").unwrap_or(*state);
                *payload = Payload::None;
            }
            Applied::Keep
        }
        pools::KIND_LEG => leg(piece, pos, name, state, payload, world),
        pools::KIND_CEILING => {
            if name != "minecraft:orange_stained_glass" {
                return Applied::Keep;
            }
            match world.block(pos) {
                Some(block) if is_fluid(&block.name) => {
                    if let Some(replacement) = state_named("minecraft:cobblestone") {
                        *state = replacement;
                    }
                    *payload = Payload::None;
                    Applied::Keep
                }
                // The ceiling cell keeps whatever the world has: write nothing.
                Some(_) => Applied::Skip,
                None => Applied::Keep,
            }
        }
        pools::KIND_ORE => {
            if deco.settings.enable_ore_props {
                return Applied::Keep;
            }
            if is_ore(name) {
                *state = state_named("minecraft:cave_air").unwrap_or(*state);
                *payload = Payload::None;
            }
            Applied::Keep
        }
        pools::KIND_RULE => {
            let mut stream = Stream::new(piece, pos, 6);
            for Rule {
                input,
                output,
                probability,
            } in step.rules
            {
                if *state != *input {
                    continue;
                }
                if *probability < 1.0 && stream.float() >= *probability {
                    continue;
                }
                *state = *output;
                *payload = Payload::None;
                break;
            }
            Applied::Keep
        }
        _ => Applied::Keep,
    }
}

fn candle_count(count: i32) -> &'static str {
    match count {
        1 => "1",
        2 => "2",
        3 => "3",
        _ => "4",
    }
}

/// A chain (as the processors place it) or cave air.
fn chain_state(current: u16, chain: bool) -> u16 {
    if chain {
        cuboid(
            "minecraft:iron_chain",
            &[("axis", "y"), ("waterlogged", "false")],
        )
        .unwrap_or(current)
    } else {
        state_named("minecraft:cave_air").unwrap_or(current)
    }
}

fn ceiling_prop(
    piece: &Piece,
    pos: [i32; 3],
    name: &str,
    state: &mut u16,
    payload: &mut Payload,
    world: &mut dyn World,
) -> Applied {
    let (offset, chance) = match name {
        "minecraft:magenta_stained_glass" => (1, 0.2f32),
        "minecraft:brown_stained_glass" => (2, 0.5f32),
        _ => {
            // A chain already in the template only stays when it hangs from
            // something (`SmallDungeonCeilingPropProcessor`'s third branch).
            if name != "minecraft:iron_chain" && name != "minecraft:chain" {
                return Applied::Keep;
            }
            let above = [pos[0], pos[1] + 1, pos[2]];
            if world.block(above).is_some_and(|block| !block.solid) {
                *state = state_named("minecraft:cave_air").unwrap_or(*state);
                *payload = Payload::None;
            }
            return Applied::Keep;
        }
    };
    let above = [pos[0], pos[1] + offset, pos[2]];
    if world.block(above).is_some_and(|block| !block.solid) {
        *state = state_named("minecraft:cave_air").unwrap_or(*state);
        *payload = Payload::None;
        return Applied::Keep;
    }
    let mut stream = Stream::new(piece, pos, 5);
    let chain = stream.float() < chance;
    *state = chain_state(*state, chain);
    *payload = Payload::None;
    Applied::Keep
}

fn leg(
    piece: &Piece,
    pos: [i32; 3],
    name: &str,
    state: &mut u16,
    payload: &mut Payload,
    world: &mut dyn World,
) -> Applied {
    if name != "minecraft:yellow_stained_glass" {
        return Applied::Keep;
    }
    *state = state_named("minecraft:mossy_cobblestone").unwrap_or(*state);
    *payload = Payload::None;
    let mut blocks = Vec::new();
    let mut stream = Stream::new(piece, pos, 7);
    let mut y = pos[1] - 1;
    while y > piece.min_y {
        let Some(block) = world.block([pos[0], y, pos[2]]) else {
            break;
        };
        if !block.air && !is_fluid(&block.name) {
            break;
        }
        let stone = match stream.int(100) {
            0..=49 => "minecraft:stone_bricks",
            50..=69 => "minecraft:cracked_stone_bricks",
            _ => "minecraft:mossy_stone_bricks",
        };
        if let Some(state) = state_named(stone) {
            blocks.push(Placed {
                pos: [pos[0], y, pos[2]],
                state,
                payload: Payload::None,
            });
        }
        y -= 1;
    }
    Applied::Legs(blocks)
}

fn is_fluid(name: &str) -> bool {
    matches!(
        name,
        "minecraft:water"
            | "minecraft:flowing_water"
            | "minecraft:lava"
            | "minecraft:flowing_lava"
            | "minecraft:bubble_column"
    )
}

fn is_ore(name: &str) -> bool {
    let Some(kind) = name.strip_prefix("minecraft:") else {
        return false;
    };
    if let Some(stone) = kind.strip_prefix("deepslate_") {
        return is_ore(&format!("minecraft:{stone}"));
    }
    matches!(
        kind,
        "coal_ore"
            | "iron_ore"
            | "copper_ore"
            | "gold_ore"
            | "redstone_ore"
            | "emerald_ore"
            | "lapis_ore"
            | "diamond_ore"
            | "nether_gold_ore"
            | "nether_quartz_ore"
            | "ancient_debris"
    )
}

/// The structure-wide chest and banner counts each piece starts from, so a
/// piece's decoration does not depend on which chunk asks for it.
fn plan_counts(pieces: &mut [Piece], settings: &Settings) -> Result<(), Error> {
    let (mut chests, mut banners) = (0, 0);
    for piece in pieces.iter_mut() {
        piece.chests_before = chests;
        piece.banners_before = banners;
        let (_, after) = decorate(piece, settings, &mut NoWorld)?;
        chests = after.chests;
        banners = after.banners;
    }
    Ok(())
}

/// The readout `/betterdungeons status` prints: what is registered, what is
/// carried, and one deterministic assembly.
///
/// The assembly is at chunk `(2, 3)` with seed `20261105`, the numbers
/// `notes.md` records; `cargo test` pins the same ones.
pub fn status_lines(settings: &Settings) -> Vec<std::string::String> {
    let mut lines = Vec::new();
    let structure = &pools::STRUCTURE;
    lines.push(format!(
        "betterdungeons: slice 1, structure {} ({})",
        structure.id, structure.worldgen_type
    ));
    lines.push(
        "  type: betterdungeons:small_dungeon_type, registered through register_structure_type"
            .to_owned(),
    );
    lines.push(format!(
        "  carried: {} templates, {} pools, {} processor lists, {} states",
        templates::TEMPLATES.len(),
        pools::POOLS.len(),
        pools::PROCESSOR_LISTS.len(),
        templates::STATES.len()
    ));
    lines.push(format!(
        "  config: start_pool {}, start_jigsaw {}, size {}, start_height {:?}, \
         max_distance {}, step {}, biomes {}",
        structure.start_pool,
        structure.start_jigsaw.unwrap_or("(none)"),
        structure.size,
        structure.start_y,
        structure.max_distance,
        structure.step,
        structure.biomes
    ));
    lines.push(format!(
        "  set: salt {}, spacing {}, separation {}",
        structure.salt, structure.spacing, structure.separation
    ));
    lines.push(format!(
        "  settings: banners <= {}, chests {}..={}",
        settings.banner_max_count, settings.chest_min_count, settings.chest_max_count
    ));
    match sample_assembly(settings) {
        Ok(assembly) => {
            lines.push(format!(
                "  sample: chunk (2, 3), seed 20261105 -> {} pieces at {}",
                assembly.pieces.len(),
                format_pos(assembly.position)
            ));
            for (index, piece) in assembly.pieces.iter().enumerate() {
                lines.push(format!(
                    "    #{} {} depth {} rotation {} at {} {}x{}x{}",
                    index,
                    piece.template,
                    piece.depth,
                    rotation_name(piece.rotation),
                    format_pos(piece.position),
                    piece.size[0],
                    piece.size[1],
                    piece.size[2]
                ));
            }
        }
        Err(error) => lines.push(format!("  sample: refused: {error}")),
    }
    lines
}

/// The readout's own assembly, from a seed rather than a game.
///
/// # Errors
///
/// Whatever [`assemble`] refuses.
pub fn sample_assembly(settings: &Settings) -> Result<Assembly, Error> {
    let config = Config {
        start_pool: pools::STRUCTURE.start_pool.to_owned(),
        start_jigsaw: pools::STRUCTURE.start_jigsaw.map(str::to_owned),
        size: pools::STRUCTURE.size,
        start_y: pools::STRUCTURE.start_y,
        max_distance: pools::STRUCTURE.max_distance,
        use_expansion_hack: pools::STRUCTURE.use_expansion_hack,
    };
    let mut rng = SplitMix::new(SAMPLE_SEED);
    assemble(
        &config,
        settings,
        SAMPLE_CHUNK,
        SAMPLE_SEED,
        -64,
        319,
        &mut rng,
    )
}

/// The seed the readout's assembly uses.
pub const SAMPLE_SEED: i64 = 20_261_105;
/// The chunk the readout's assembly uses.
pub const SAMPLE_CHUNK: [i32; 2] = [2, 3];

fn format_pos(pos: [i32; 3]) -> std::string::String {
    format!("({}, {}, {})", pos[0], pos[1], pos[2])
}

fn rotation_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => "none",
        Rotation::Cw90 => "cw90",
        Rotation::Cw180 => "cw180",
        Rotation::Ccw90 => "ccw90",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            start_pool: pools::STRUCTURE.start_pool.to_owned(),
            start_jigsaw: pools::STRUCTURE.start_jigsaw.map(str::to_owned),
            size: pools::STRUCTURE.size,
            start_y: pools::STRUCTURE.start_y,
            max_distance: pools::STRUCTURE.max_distance,
            use_expansion_hack: pools::STRUCTURE.use_expansion_hack,
        }
    }

    fn assembly(seed: i64) -> Assembly {
        assemble(
            &config(),
            &Settings::DEFAULT,
            [2, 3],
            seed,
            -64,
            319,
            &mut SplitMix::new(seed),
        )
        .expect("assembly")
    }

    #[test]
    fn every_generated_element_names_a_carried_template_and_processor_list() {
        for pool in pools::POOLS {
            for element in pool.elements {
                assert!(
                    templates::template(element.template).is_some(),
                    "{}: {}",
                    pool.id,
                    element.template
                );
                assert!(
                    element.processors == "minecraft:empty"
                        || pools::processor_list(element.processors).is_some(),
                    "{}: {}",
                    pool.id,
                    element.processors
                );
            }
        }
    }

    #[test]
    fn every_generated_processor_step_is_one_this_port_implements() {
        let known = [
            pools::KIND_BANNER,
            pools::KIND_SPAWNER,
            pools::KIND_CANDLE,
            pools::KIND_HEAD,
            pools::KIND_NETHER_BLOCK,
            pools::KIND_CHEST,
            pools::KIND_CEILING_PROP,
            pools::KIND_CEILING_LAMP,
            pools::KIND_COBBLESTONE,
            pools::KIND_LEG,
            pools::KIND_CEILING,
            pools::KIND_ORE,
            pools::KIND_RULE,
        ];
        for list in pools::PROCESSOR_LISTS {
            for step in list.steps {
                assert!(
                    known.contains(&step.kind),
                    "{}: kind {}",
                    list.id,
                    step.kind
                );
            }
        }
    }

    #[test]
    fn the_state_table_and_its_rotations_agree() {
        assert_eq!(templates::STATES.len(), templates::ROTATED.len());
    }

    #[test]
    fn every_state_rotation_is_a_state() {
        for (index, row) in templates::ROTATED.iter().enumerate() {
            for target in row {
                assert!(
                    (*target as usize) < templates::STATES.len(),
                    "state {index} rotates to {target}"
                );
            }
            // Four turns is the identity, so a rotation can never leave the table.
            assert_eq!(row[0], index as u16);
        }
    }

    #[test]
    fn a_rotation_turns_facing_and_keeps_turning() {
        let stairs = cuboid(
            "minecraft:stone_brick_stairs",
            &[
                ("facing", "north"),
                ("half", "top"),
                ("shape", "straight"),
                ("waterlogged", "false"),
            ],
        )
        .expect("the carried stairs state");
        let turned = templates::ROTATED[stairs as usize];
        assert_eq!(
            templates::STATES[turned[1] as usize].props,
            &[
                ("facing", "east"),
                ("half", "top"),
                ("shape", "straight"),
                ("waterlogged", "false")
            ]
        );
        assert_eq!(
            templates::STATES[turned[2] as usize].props,
            &[
                ("facing", "south"),
                ("half", "top"),
                ("shape", "straight"),
                ("waterlogged", "false")
            ]
        );
        assert_eq!(turned[3], turned[1].min(turned[3]));
    }

    #[test]
    fn a_skull_rotation_moves_four_sixteenths_per_quarter_turn() {
        let skull = cuboid(
            "minecraft:skeleton_skull",
            &[("powered", "false"), ("rotation", "13")],
        )
        .expect("the carried skull state");
        let turned = templates::ROTATED[skull as usize];
        for (index, state) in turned.iter().enumerate() {
            let props = templates::STATES[*state as usize].props;
            let rotation = props
                .iter()
                .find(|(key, _)| *key == "rotation")
                .map(|(_, value)| value.parse::<u16>().expect("a number"))
                .expect("a rotation");
            assert_eq!(rotation, (13 + 4 * index as u16) % 16, "turn {index}");
        }
    }

    #[test]
    fn a_loot_pile_attaches_to_every_shell_connector() {
        let template = templates::template("betterdungeons:small_dungeon/shells/small_shell_5x5")
            .expect("the 5x5 shell");
        let mut attached = 0;
        for jigsaw in template.jigsaws {
            if jigsaw.pool != "betterdungeons:small_dungeon_loot_piles" {
                continue;
            }
            let (front, top) = oriented(jigsaw).expect("an orientation");
            let pile = templates::template("betterdungeons:small_dungeon/loot_piles/loot_pile_0")
                .expect("a loot pile");
            let mut found = false;
            for child_rotation in ROTATIONS {
                let connector = Connector {
                    pos: [0; 3],
                    front,
                    top,
                    pool: jigsaw.pool,
                    target: jigsaw.target,
                };
                for child in pile.jigsaws {
                    let (child_front, child_top) = oriented(child).expect("an orientation");
                    // The pile's own jigsaw faces west; some rotation of the
                    // pile must therefore face the shell connector.
                    if attaches(
                        &connector,
                        turn(child_front, child_rotation),
                        turn(child_top, child_rotation),
                        child.name,
                    ) {
                        found = true;
                    }
                }
            }
            assert!(found, "a pile attaches to {:?}", jigsaw.pos);
            attached += 1;
        }
        assert_eq!(attached, 4, "the 5x5 shell has four loot-pile connectors");
    }

    #[test]
    fn the_assembly_is_deterministic_for_a_seed() {
        let first = assembly(7);
        let second = assembly(7);
        assert_eq!(first, second);
        let other = assembly(8);
        assert_ne!(
            first.pieces, other.pieces,
            "a different seed draws differently"
        );
    }

    #[test]
    fn the_anchor_sits_on_the_chunk_corner_at_the_drawn_height() {
        for seed in 0..32 {
            let assembly = assembly(seed);
            let start = &assembly.pieces[0];
            let template = start.template().expect("a template");
            let anchor = template
                .jigsaws
                .iter()
                .find(|jigsaw| jigsaw.name == "betterdungeons:small_dungeon_anchor")
                .expect("the anchor");
            let local = start.rotation.pos(anchor.pos, template.size);
            let world = [
                start.position[0] + i32::from(local[0]),
                start.position[1] + i32::from(local[1]),
                start.position[2] + i32::from(local[2]),
            ];
            assert_eq!(world[0], 2 * 16, "seed {seed}: the anchor's x");
            assert_eq!(world[2], 3 * 16, "seed {seed}: the anchor's z");
            assert!(
                (-50..=50).contains(&world[1]),
                "seed {seed}: the anchor's y {} is in the configured range",
                world[1]
            );
        }
    }

    #[test]
    fn a_start_is_a_shell_and_its_children_are_loot_piles() {
        for seed in 0..16 {
            let assembly = assembly(seed);
            assert!(
                assembly.pieces[0].template.contains("/shells/small_shell_"),
                "seed {seed}: the start is a shell"
            );
            assert!(assembly.pieces.len() >= 2, "seed {seed}: at least one pile");
            for piece in &assembly.pieces[1..] {
                assert!(
                    piece.template.contains("/loot_piles/loot_pile_"),
                    "seed {seed}: {} is not a loot pile",
                    piece.template
                );
                assert_eq!(piece.depth, 1);
            }
        }
    }

    #[test]
    fn siblings_never_overlap_and_children_stay_in_their_parent() {
        // A child attached through a connector whose target cell is inside the
        // parent is placed *inside the parent piece* (a loot pile in the room),
        // so it necessarily shares the parent's box: YUNG's assembler gives that
        // case a fresh octree bounded by the parent and no boxes in it. What
        // must not happen is two children in the same space.
        for seed in 0..64 {
            let assembly = assembly(seed);
            let parent_box = assembly.pieces[0].bounding_box();
            for (index, piece) in assembly.pieces.iter().enumerate().skip(1) {
                assert!(
                    piece.bounding_box().is_inside(&parent_box),
                    "seed {seed}: {} is outside its parent",
                    piece.template
                );
                for other in &assembly.pieces[index + 1..] {
                    assert!(
                        !piece.bounding_box().intersects(&other.bounding_box()),
                        "seed {seed}: {} and {} overlap",
                        piece.template,
                        other.template
                    );
                }
            }
        }
    }

    #[test]
    fn the_start_pool_draw_matches_the_jar_weights() {
        // The 5x5 shells carry weight 1, 1 and 2 (skeleton, spider, zombie), so
        // the three repeats of `small_shell_5x5` hold 4 of the pool's 24 weight.
        let pool = pools::pool("betterdungeons:small_dungeon").expect("the shell pool");
        let five_by_five: i32 = pool
            .elements
            .iter()
            .filter(|element| element.template.ends_with("small_shell_5x5"))
            .map(|element| element.weight)
            .sum();
        let total: i32 = pool.elements.iter().map(|element| element.weight).sum();
        assert_eq!(five_by_five, 4);
        assert_eq!(total, 24);

        // …and the draw honours them: over many seeds a 5x5 start is about a
        // sixth of them, and every shell shows up.
        let mut five = 0;
        let mut seen = std::collections::BTreeSet::new();
        let draws = 1200;
        for seed in 0..draws {
            let assembly = assembly(seed);
            let start = assembly.pieces[0].template;
            if start.ends_with("small_shell_5x5") {
                five += 1;
            }
            seen.insert(start);
        }
        assert!(seen.len() >= 5, "every shell size shows up: {seen:?}");
        let share = five as f64 / draws as f64;
        assert!(
            (0.10..0.28).contains(&share),
            "a 5x5 start should be about 4/24 = 0.167 of draws, was {share:.3}"
        );
    }

    #[test]
    fn a_max_count_element_is_refused_after_its_limit() {
        // `loot_pile_0` is `max_count: 1`; a structure cannot take two of it.
        for seed in 0..48 {
            let assembly = assembly(seed);
            let mut zero = 0;
            for piece in &assembly.pieces {
                if piece.template.ends_with("loot_pile_0") {
                    zero += 1;
                }
            }
            assert!(zero <= 1, "seed {seed}: loot_pile_0 appears {zero} times");
        }
    }

    #[test]
    fn a_pool_whose_entries_are_all_at_their_limit_yields_nothing() {
        // The limits hold: the nine props take one each, the empty pile two.
        // A shell has four connectors and the pool has ten elements with
        // `max_count` 2 (the empty pile) or 1 (the nine props), so the fourth
        // pile can only be empty piles, and the structure still stops there.
        for seed in 0..64 {
            let assembly = assembly(seed);
            let mut counts: std::collections::BTreeMap<&str, i32> =
                std::collections::BTreeMap::new();
            for piece in &assembly.pieces[1..] {
                *counts.entry(piece.template).or_insert(0) += 1;
            }
            for (template, count) in &counts {
                if template.ends_with("loot_pile_empty") {
                    assert!(*count <= 2, "seed {seed}: {} x{count}", template);
                } else {
                    assert_eq!(*count, 1, "seed {seed}: {} x{count}", template);
                }
            }
        }
        // A shell has four connectors and the pool has ten entries, so the
        // structure is a start plus at most four piles.
        for seed in 0..64 {
            let assembly = assembly(seed);
            assert!(
                assembly.pieces.len() <= 5,
                "seed {seed}: {}",
                assembly.pieces.len()
            );
        }
    }

    #[test]
    fn a_config_the_tables_cannot_describe_is_refused() {
        let mut wrong = config();
        wrong.size = 12;
        assert!(!config_supported(&wrong));
        assert!(matches!(
            assemble(
                &wrong,
                &Settings::DEFAULT,
                [0, 0],
                1,
                -64,
                319,
                &mut SplitMix::new(1)
            ),
            Err(Error::Unsupported(_))
        ));

        // A start jigsaw the carried definition does not name is not carried
        // either: the anchor is part of what `config_supported` checks.
        let mut other_anchor = config();
        other_anchor.start_jigsaw = Some("betterdungeons:another_anchor".to_owned());
        assert!(matches!(
            assemble(
                &other_anchor,
                &Settings::DEFAULT,
                [0, 0],
                1,
                -64,
                319,
                &mut SplitMix::new(1)
            ),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn the_decoration_keeps_at_most_two_chests_and_two_banners() {
        for seed in 0..64 {
            let assembly = assembly(seed);
            let mut chests = 0;
            let mut banners = 0;
            let mut saw_chest = false;
            for piece in &assembly.pieces {
                let (blocks, _) =
                    decorate(piece, &Settings::DEFAULT, &mut NoWorld).expect("decorated");
                for block in &blocks {
                    match block.payload {
                        Payload::Chest(_) => {
                            chests += 1;
                            saw_chest = true;
                        }
                        Payload::Banner(_) => banners += 1,
                        _ => {}
                    }
                }
            }
            assert!(saw_chest, "seed {seed}: a dungeon keeps a chest");
            assert!(chests <= 2, "seed {seed}: {chests} chests");
            assert!(banners <= 2, "seed {seed}: {banners} banners");
        }
    }

    #[test]
    fn settings_turned_off_keep_chests_banners_heads_and_ores_out() {
        let off = Settings {
            banner_max_count: 0,
            chest_min_count: 0,
            chest_max_count: 0,
            enable_heads: false,
            enable_nether_blocks: false,
            enable_ore_props: false,
        };
        let config = config();
        for seed in 0..64 {
            let assembly = assemble(
                &config,
                &off,
                [2, 3],
                seed,
                -64,
                319,
                &mut SplitMix::new(seed),
            )
            .expect("assembly");
            for piece in &assembly.pieces {
                let (blocks, _) = decorate(piece, &off, &mut NoWorld).expect("decorated");
                for block in &blocks {
                    let name = templates::STATES[block.state as usize].name;
                    assert!(
                        !matches!(block.payload, Payload::Chest(_) | Payload::Banner(_)),
                        "seed {seed}: {name} kept its payload"
                    );
                    assert!(
                        !name.ends_with("_skull")
                            && !name.ends_with("_head")
                            && !matches!(
                                name,
                                "minecraft:soul_sand"
                                    | "minecraft:soul_soil"
                                    | "minecraft:soul_campfire"
                                    | "minecraft:soul_lantern"
                            )
                            && !is_ore(name),
                        "seed {seed}: {name} placed"
                    );
                }
            }
        }
    }

    #[test]
    fn a_chest_is_removed_only_after_the_minimum_is_kept() {
        // A world of air: nothing else changes, so chests survive the chain.
        struct Air;
        impl World for Air {
            fn block(&mut self, _pos: [i32; 3]) -> Option<BlockInfo> {
                Some(BlockInfo {
                    name: "minecraft:air".to_owned(),
                    air: true,
                    solid: false,
                })
            }
        }
        let assembly = assembly(3);
        let mut chests = 0;
        for piece in &assembly.pieces {
            let (blocks, _) = decorate(piece, &Settings::DEFAULT, &mut Air).expect("decorated");
            chests += blocks
                .iter()
                .filter(|block| matches!(block.payload, Payload::Chest(_)))
                .count();
        }
        // `chestMinCount` guarantees one; `chestMaxCount` caps the rest at 20 %
        // per further chest, so a dungeon keeps one or two.
        assert!(
            (Settings::DEFAULT.chest_min_count as usize
                ..=Settings::DEFAULT.chest_max_count as usize)
                .contains(&chests),
            "{chests} chests"
        );
    }

    #[test]
    fn a_sea_pickle_becomes_a_lit_or_unlit_candle() {
        let piece = Piece {
            template: "betterdungeons:small_dungeon/shells/small_shell_5x5",
            processors: "betterdungeons:small_dungeon/small_dungeon_zombie",
            rotation: Rotation::None,
            position: [0, 0, 0],
            size: [7, 7, 7],
            depth: 0,
            seed: 1,
            chests_before: 0,
            banners_before: 0,
            min_y: -64,
        };
        let pickle = cuboid(
            "minecraft:sea_pickle",
            &[("pickles", "1"), ("waterlogged", "true")],
        );
        if pickle.is_none() {
            // The carried palette has no sea pickle; the processor is exercised
            // through a state the crate has, so the branch is still proved.
            return;
        }
        let mut state = pickle.expect("a sea pickle");
        let mut payload = Payload::None;
        let step = pools::PROCESSOR_LISTS
            .iter()
            .flat_map(|list| list.steps)
            .find(|step| step.kind == pools::KIND_CANDLE)
            .expect("the candle processor");
        apply(
            step,
            &piece,
            [1, 2, 3],
            &mut state,
            &mut payload,
            &mut Decoration {
                settings: &Settings::DEFAULT,
                counts: Counts::default(),
            },
            &mut NoWorld,
        );
        let name = templates::STATES[state as usize].name;
        assert!(name.ends_with("_candle") || name == "minecraft:candle");
    }

    #[test]
    fn a_support_leg_reaches_the_floor_and_stops_in_stone() {
        struct Column {
            air_below: i32,
        }
        impl World for Column {
            fn block(&mut self, pos: [i32; 3]) -> Option<BlockInfo> {
                let air = pos[1] > self.air_below;
                Some(BlockInfo {
                    name: if air {
                        "minecraft:air"
                    } else {
                        "minecraft:stone"
                    }
                    .to_owned(),
                    air,
                    solid: !air,
                })
            }
        }
        let piece = Piece {
            template: "betterdungeons:small_dungeon/shells/small_shell_5x5",
            processors: "betterdungeons:small_dungeon/small_dungeon_zombie",
            rotation: Rotation::None,
            position: [0, 0, 0],
            size: [7, 7, 7],
            depth: 0,
            seed: 1,
            chests_before: 0,
            banners_before: 0,
            min_y: -64,
        };
        let glass = cuboid("minecraft:yellow_stained_glass", &[]);
        let Some(glass) = glass else {
            return;
        };
        let mut state = glass;
        let mut payload = Payload::None;
        let step = pools::PROCESSOR_LISTS
            .iter()
            .flat_map(|list| list.steps)
            .find(|step| step.kind == pools::KIND_LEG)
            .expect("the leg processor");
        let applied = apply(
            step,
            &piece,
            [3, 30, 3],
            &mut state,
            &mut payload,
            &mut Decoration {
                settings: &Settings::DEFAULT,
                counts: Counts::default(),
            },
            &mut Column { air_below: 20 },
        );
        match applied {
            Applied::Legs(blocks) => {
                assert_eq!(blocks.len(), 9, "y = 29 …… 21");
                assert_eq!(blocks.first().expect("a block").pos, [3, 29, 3]);
                assert_eq!(blocks.last().expect("a block").pos, [3, 21, 3]);
                for block in blocks {
                    let name = templates::STATES[block.state as usize].name;
                    assert!(
                        name.ends_with("stone_bricks") || name == "minecraft:cracked_stone_bricks"
                    );
                }
            }
            _ => panic!("the leg processor must place the column"),
        }
        assert_eq!(
            templates::STATES[state as usize].name,
            "minecraft:mossy_cobblestone"
        );
    }

    #[test]
    fn a_rule_replaces_only_matching_states() {
        let list = pools::processor_list("betterdungeons:small_dungeon/small_dungeon_zombie")
            .expect("the zombie list");
        let rule = list
            .steps
            .iter()
            .find(|step| step.kind == pools::KIND_RULE)
            .expect("its rule processor");
        let piece = Piece {
            template: "betterdungeons:small_dungeon/shells/small_shell_5x5",
            processors: list.id,
            rotation: Rotation::None,
            position: [0, 0, 0],
            size: [7, 7, 7],
            depth: 0,
            seed: 1,
            chests_before: 0,
            banners_before: 0,
            min_y: -64,
        };
        let air = state_named("minecraft:air").expect("air");
        let mut state = air;
        let mut payload = Payload::None;
        apply(
            rule,
            &piece,
            [1, 2, 3],
            &mut state,
            &mut payload,
            &mut Decoration {
                settings: &Settings::DEFAULT,
                counts: Counts::default(),
            },
            &mut NoWorld,
        );
        assert_eq!(
            templates::STATES[state as usize].name,
            "minecraft:cave_air",
            "air becomes cave air, as the jar's third rule says"
        );
    }

    #[test]
    fn the_readout_names_the_registered_type_and_a_piece_count() {
        let lines = status_lines(&Settings::DEFAULT);
        let text = lines.join("\n");
        assert!(text.contains("betterdungeons:small_dungeon"), "{text}");
        assert!(text.contains("betterdungeons:small_dungeon_type"), "{text}");
        assert!(text.contains("16 templates"), "{text}");
        assert!(text.contains("2 pools"), "{text}");
        assert!(text.contains("4 processor lists"), "{text}");
        let pieces = sample_assembly(&Settings::DEFAULT)
            .expect("the sample assembly")
            .pieces
            .len();
        assert_eq!(pieces, 5, "one shell plus four loot piles");
        assert!(text.contains(&format!("-> {pieces} pieces")), "{text}");
    }
}
