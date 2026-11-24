//! The glue: the registrar call, the structure type and its pieces, and the readout.
//!
//! Every decision is in [`crate::runtime`]; this file only speaks the game's
//! vocabulary. The junction between the two is three small adapters:
//!
//! * [`ContextRandom`] makes the game's `RandomSource` the assembly's [`runtime::Rng`];
//! * [`ContextWorld`] answers the processors' block questions from `PieceContext`;
//! * [`states`] resolves the generated state table to the game's `BlockState`s once
//!   per assembly, from the metadata the worker context lends.

use crate::pools;
use crate::runtime::{self, Payload, Piece, Rotation, Settings};
use crate::templates;
use mod_api::{
    self, BlockKey, BlockMetadata, BlockState, BoundingBox, Command, CommandRegistration, Compound,
    FoundStructure, GenerationContext, Host, PieceContext, RandomSource, Registrar,
    RegistrationContext, ServerHost, SettingSpec, StateConstraint, StructureError, StructurePiece,
    StructureType, Tag, Text,
};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// The mod's id, which is also its resource namespace.
const MOD_ID: &str = "betterdungeons";

/// The structure type's id, the `type` a data-pack structure names.
pub const TYPE_ID: &str = "small_dungeon_type";

/// The blocks the templates and the processor chains name, in [`templates::STATES`]
/// order: requested once at registration, so every state has a key.
static BLOCKS: OnceLock<Vec<BlockKey>> = OnceLock::new();

/// The last reason generation refused, for `/betterdungeons status`.
static REFUSAL: Mutex<Option<String>> = Mutex::new(None);

/// The settings the server that runs now started with; the original's defaults until one starts.
/// Worldgen workers read it, and only [`read_settings`] writes it.
static SETTINGS: RwLock<Settings> = RwLock::new(Settings::DEFAULT);

/// The setting keys, one per `ConfigModule` value this code reads.
const BANNER_MAX: &str = "small_dungeon_max_banners";
const CHEST_MIN: &str = "small_dungeon_min_chests";
const CHEST_MAX: &str = "small_dungeon_max_chests";
const ORE_PROPS: &str = "small_dungeon_ore_props";
const HEADS: &str = "enable_heads";
const NETHER_BLOCKS: &str = "enable_nether_blocks";

/// The chest counts' slider top: the original's `define` takes any integer, and a small dungeon
/// has fewer chest props than this.
const CHEST_COUNT_MAX: f64 = 8.0;

fn declare_settings(r: &mut dyn Registrar) {
    let defaults = Settings::DEFAULT;
    let label = |key: &str| format!("{MOD_ID}.configuration.{key}");
    let specs = [
        SettingSpec::toggle(HEADS, label(HEADS), defaults.enable_heads),
        SettingSpec::toggle(
            NETHER_BLOCKS,
            label(NETHER_BLOCKS),
            defaults.enable_nether_blocks,
        ),
        // `defineInRange("Small Dungeon Max Banner Count", 2, 0, 8)`.
        SettingSpec::range(
            BANNER_MAX,
            label(BANNER_MAX),
            0.0,
            8.0,
            1.0,
            f64::from(defaults.banner_max_count),
        ),
        SettingSpec::range(
            CHEST_MIN,
            label(CHEST_MIN),
            0.0,
            CHEST_COUNT_MAX,
            1.0,
            f64::from(defaults.chest_min_count),
        ),
        SettingSpec::range(
            CHEST_MAX,
            label(CHEST_MAX),
            0.0,
            CHEST_COUNT_MAX,
            1.0,
            f64::from(defaults.chest_max_count),
        ),
        SettingSpec::toggle(ORE_PROPS, label(ORE_PROPS), defaults.enable_ore_props),
    ];
    for spec in specs {
        let tooltip = format!("{}.tooltip", spec.label);
        let _ = r.register_setting(spec.tooltip(tooltip));
    }
}

/// Take the player's settings for the server about to start: NeoForge reads a `worldRestart`
/// value when the world loads, and generation reads [`settings`] from then on.
pub fn read_settings(host: &dyn Host) {
    let toggle = |key: &str, default: bool| match host.setting(MOD_ID, key).as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    };
    let count = |key: &str, default: i32| {
        host.setting(MOD_ID, key)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .map_or(default, |value| value.round() as i32)
    };
    let defaults = Settings::DEFAULT;
    let read = Settings {
        banner_max_count: count(BANNER_MAX, defaults.banner_max_count),
        chest_min_count: count(CHEST_MIN, defaults.chest_min_count),
        chest_max_count: count(CHEST_MAX, defaults.chest_max_count),
        enable_heads: toggle(HEADS, defaults.enable_heads),
        enable_nether_blocks: toggle(NETHER_BLOCKS, defaults.enable_nether_blocks),
        enable_ore_props: toggle(ORE_PROPS, defaults.enable_ore_props),
    };
    *SETTINGS.write().expect("the settings lock is not poisoned") = read;
}

/// The settings generation uses now.
fn settings() -> Settings {
    *SETTINGS.read().expect("the settings lock is not poisoned")
}

/// Register the structure type and request the blocks it places, once at startup.
pub fn register(r: &mut dyn Registrar, _cx: &RegistrationContext) {
    declare_settings(r);
    let mut keys = Vec::with_capacity(templates::STATES.len());
    let mut seen = std::collections::BTreeMap::new();
    for state in templates::STATES {
        let key = *seen
            .entry(state.name)
            .or_insert_with(|| r.block_key(state.name));
        keys.push(key);
    }
    let _ = BLOCKS.set(keys);
    r.register_structure_type(TYPE_ID, SmallDungeonType);
    crate::spider_type::register(r);
}

/// The registered type: `yungsapi:yung_jigsaw`'s assembly, as this port's own code.
#[derive(Clone, Copy, Debug, Default)]
pub struct SmallDungeonType;

impl StructureType for SmallDungeonType {
    /// Assemble the start and its pieces for `context`'s chunk.
    fn find_generation_point(&self, context: &mut dyn GenerationContext) -> Option<FoundStructure> {
        match generate(context) {
            Ok(found) => Some(found),
            Err(error) => {
                *REFUSAL.lock().expect("the refusal slot is not poisoned") = Some(error);
                None
            }
        }
    }

    /// Rebuild one saved piece: the template, its rotation and where its box is.
    fn load_piece(
        &self,
        metadata: &dyn BlockMetadata,
        tag: &Compound,
    ) -> Result<Arc<dyn StructurePiece>, StructureError> {
        let states = states(metadata)?;
        let spec = saved_spec(tag).ok_or_else(|| {
            StructureError("the saved piece has no Template/Rotation/BB".to_owned())
        })?;
        Ok(Arc::new(SmallDungeonPiece {
            spec,
            states: Arc::new(states),
        }))
    }
}

/// The assembly, or the reason it refused.
fn generate(context: &mut dyn GenerationContext) -> Result<FoundStructure, String> {
    let config_tag = context.config();
    let config = decode_config(&config_tag)
        .ok_or_else(|| "the structure's config is not a uniform-start jigsaw".to_owned())?;
    if !runtime::config_supported(&config) {
        return Err(format!(
            "config not carried: start_pool {}, size {}, start_height {:?}",
            config.start_pool, config.size, config.start_y
        ));
    }
    let chunk = context.chunk();
    let seed = context.seed();
    let min_y = context.min_y();
    let max_y = context.max_y();
    let assembly = {
        let mut rng = ContextRandom {
            random: context.random(),
        };
        runtime::assemble(
            &config,
            &settings(),
            [chunk.x, chunk.z],
            seed,
            min_y,
            max_y,
            &mut rng,
        )
        .map_err(|error| error.to_string())?
    };
    let states = states(context.metadata()).map_err(|error| error.0)?;
    let states = Arc::new(states);

    let mut pieces: Vec<Arc<dyn StructurePiece>> = Vec::with_capacity(assembly.pieces.len());
    for spec in &assembly.pieces {
        pieces.push(Arc::new(SmallDungeonPiece {
            spec: spec.clone(),
            states: Arc::clone(&states),
        }));
    }
    let position = assembly.position;
    *REFUSAL.lock().expect("the refusal slot is not poisoned") = None;
    Ok(FoundStructure {
        position,
        pieces,
        ..FoundStructure::default()
    })
}

/// One piece of the dungeon: its template, rotation, origin and decoration counts.
#[derive(Debug)]
pub struct SmallDungeonPiece {
    /// What the template and rotation say, and where the piece sits.
    spec: Piece,
    /// Every state the templates name, resolved against this worker's metadata.
    states: Arc<Vec<BlockState>>,
}

impl StructurePiece for SmallDungeonPiece {
    /// The piece's box, in world blocks.
    fn bounding_box(&self) -> BoundingBox {
        let box3 = self.spec.bounding_box();
        BoundingBox::new(box3.min, box3.max)
    }

    /// Place the part of this piece that falls in the chunk `context` clips to.
    fn post_process(&self, context: &mut dyn PieceContext) -> Result<(), StructureError> {
        let placed = {
            let mut world = ContextWorld { context };
            let (placed, _) = runtime::decorate(&self.spec, &settings(), &mut world)
                .map_err(|error| StructureError(error.to_string()))?;
            placed
        };
        let clip = context.chunk_box();
        for block in placed {
            if !clip.is_inside(block.pos) {
                continue;
            }
            let Some(state) = self.states.get(block.state as usize) else {
                return Err(StructureError(format!(
                    "state {} is not in the resolved table",
                    block.state
                )));
            };
            context.set_block(block.pos, *state);
            match block.payload {
                Payload::None => {}
                Payload::Chest(loot) => {
                    context.block_entity(
                        block.pos,
                        compound([
                            ("id", string("minecraft:chest")),
                            ("LootTable", string(loot)),
                        ]),
                    );
                }
                Payload::Barrel(loot) => {
                    context.block_entity(
                        block.pos,
                        compound([
                            ("id", string("minecraft:barrel")),
                            ("LootTable", string(loot)),
                        ]),
                    );
                }
                Payload::Banner(banner) => {
                    context.block_entity(block.pos, banner_nbt(banner));
                }
                Payload::Spawner(mob) => {
                    context.block_entity(block.pos, spawner_nbt(mob));
                }
                Payload::Skull => {
                    context.block_entity(block.pos, compound([("id", string("minecraft:skull"))]));
                }
                Payload::Jukebox => {
                    context
                        .block_entity(block.pos, compound([("id", string("minecraft:jukebox"))]));
                }
                Payload::Furnace => {
                    context.block_entity(
                        block.pos,
                        compound([
                            ("id", string("minecraft:furnace")),
                            ("BurnTime", Tag::Short(0)),
                            ("CookTime", Tag::Short(0)),
                            ("CookTimeTotal", Tag::Short(0)),
                            ("Items", Tag::List(Vec::new())),
                            ("RecipesUsed", compound_tag([])),
                        ]),
                    );
                }
            }
        }
        Ok(())
    }

    /// The piece's own fields, beside the engine's `BB`/`O`/`GD`.
    fn add_additional_save_data(&self, tag: &mut Compound) {
        let spec = &self.spec;
        tag.insert("Template".into(), string(spec.template));
        tag.insert("Processors".into(), string(spec.processors));
        tag.insert("Rotation".into(), Tag::Byte(spec.rotation.index() as i8));
        tag.insert("Depth".into(), Tag::Int(spec.depth));
        tag.insert("Seed".into(), Tag::Long(spec.seed));
        tag.insert("Chests".into(), Tag::Int(spec.chests_before));
        tag.insert("Banners".into(), Tag::Int(spec.banners_before));
        tag.insert("MinY".into(), Tag::Int(spec.min_y));
    }
}

/// A saved piece's own fields, from the tag `load_piece` is given.
fn saved_spec(tag: &Compound) -> Option<Piece> {
    let template = templates::TEMPLATES
        .iter()
        .find(|template| Some(template.id) == text_of(tag, "Template").as_deref())?;
    let processors_name = text_of(tag, "Processors")?;
    let processors = pools::PROCESSOR_LISTS
        .iter()
        .find(|list| list.id == processors_name)
        .map(|list| list.id)
        // `minecraft:empty` is the pile entries' "no processors".
        .or((processors_name == "minecraft:empty").then_some("minecraft:empty"))?;
    let rotation = Rotation::from_index(int_of(tag, "Rotation").unwrap_or(0));
    let Tag::IntArray(bounds) = lookup(tag, "BB")? else {
        return None;
    };
    if bounds.len() != 6 {
        return None;
    }
    Some(Piece {
        template: template.id,
        processors,
        rotation,
        position: [bounds[0], bounds[1], bounds[2]],
        size: rotation.size(template.size),
        depth: int_of(tag, "Depth").unwrap_or(0),
        seed: match lookup(tag, "Seed")? {
            Tag::Long(value) => *value,
            _ => return None,
        },
        chests_before: int_of(tag, "Chests").unwrap_or(0),
        banners_before: int_of(tag, "Banners").unwrap_or(0),
        min_y: int_of(tag, "MinY").unwrap_or(-64),
    })
}

/// The generated state table as the game's `BlockState`s.
fn states(metadata: &dyn BlockMetadata) -> Result<Vec<BlockState>, StructureError> {
    let keys = BLOCKS
        .get()
        .ok_or_else(|| StructureError("the block keys were not registered".to_owned()))?;
    let mut out = Vec::with_capacity(templates::STATES.len());
    for (index, state) in templates::STATES.iter().enumerate() {
        let key = *keys
            .get(index)
            .ok_or_else(|| StructureError(format!("{} has no block key", state.name)))?;
        let constraints: Vec<StateConstraint> = state
            .props
            .iter()
            .map(|(name, value)| StateConstraint::new(*name, *value))
            .collect();
        let resolved = BlockState::of(metadata, key, &constraints).map_err(|error| {
            StructureError(format!("{} {:?}: {error:?}", state.name, state.props))
        })?;
        out.push(resolved);
    }
    Ok(out)
}

/// The game's structure random, as the assembly's [`runtime::Rng`].
struct ContextRandom<'a> {
    random: &'a mut dyn RandomSource,
}

impl runtime::Rng for ContextRandom<'_> {
    fn int(&mut self, bound: i32) -> i32 {
        self.random.next_int_bound(bound)
    }

    fn float(&mut self) -> f32 {
        self.random.next_float()
    }
}

/// `PieceContext`, as the processors' [`runtime::World`].
struct ContextWorld<'a> {
    context: &'a mut dyn PieceContext,
}

impl runtime::World for ContextWorld<'_> {
    fn block(&mut self, pos: [i32; 3]) -> Option<runtime::BlockInfo> {
        let state = self.context.block(pos)?;
        let air = self.context.is_air(state);
        let solid = self.context.solid_render(state);
        let name = {
            let metadata = self.context.metadata();
            metadata.name(state.block()).ok().map(str::to_owned)?
        };
        Some(runtime::BlockInfo { name, air, solid })
    }
}

/// The structure JSON's own fields, as [`runtime::Config`].
fn decode_config(tag: &Tag) -> Option<runtime::Config> {
    let start_pool = text_of_tag(tag, "start_pool")?;
    let start_jigsaw = text_of_tag(tag, "start_jigsaw_name");
    let size = int_of_tag(tag, "size")?;
    let max_distance = int_of_tag(tag, "max_distance_from_center")?;
    let use_expansion_hack = matches!(tag.get("use_expansion_hack"), Some(Tag::Byte(1)));
    let height = tag.get("start_height")?;
    if text_of_tag(height, "type").as_deref() != Some("minecraft:uniform") {
        return None;
    }
    let low = int_of_tag(height.get("min_inclusive")?, "absolute")?;
    let high = int_of_tag(height.get("max_inclusive")?, "absolute")?;
    Some(runtime::Config {
        start_pool,
        start_jigsaw,
        size,
        start_y: (low, high),
        max_distance,
        use_expansion_hack,
    })
}

pub(crate) fn string(value: &str) -> Tag {
    Tag::String(value.into())
}

/// A compound as a value.
fn compound_tag<const N: usize>(entries: [(&str, Tag); N]) -> Tag {
    Tag::Compound(compound(entries))
}

/// A compound on its own, for `PieceContext::block_entity`.
pub(crate) fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Compound {
    let mut out = Compound::new();
    for (name, value) in entries {
        // `Compound`'s key type is the SDK's Java string; it converts from `&str`.
        out.insert(name.into(), value);
    }
    out
}

/// A compound's entry. `Compound`'s key type is not re-exported by the API, so
/// the lookup compares the stored keys' text instead of naming one.
pub(crate) fn lookup<'a>(tag: &'a Compound, name: &str) -> Option<&'a Tag> {
    tag.iter()
        .find(|(key, _)| key.to_string_lossy() == name)
        .map(|(_, value)| value)
}

/// A banner's saved data: its patterns and its named, coloured display.
fn banner_nbt(banner: runtime::Banner) -> Compound {
    let patterns = banner
        .patterns()
        .iter()
        .map(|(pattern, colour)| {
            Tag::Compound(compound([
                ("pattern", string(pattern)),
                ("color", string(colour)),
            ]))
        })
        .collect();
    let mut out = compound([
        ("id", string("minecraft:banner")),
        ("patterns", Tag::List(patterns)),
    ]);
    out.insert(
        "CustomName".into(),
        string(&format!(
            r#"{{"translate":"{}","fallback":"{}","color":"{}","italic":false}}"#,
            banner.name_key(),
            banner.name(),
            banner.colour()
        )),
    );
    out
}

/// A spawner's saved data, in the shape the jar's `MobSpawnerProcessor` writes.
fn spawner_nbt(mob: &str) -> Compound {
    let entity = || Tag::Compound(compound([("id", string(mob))]));
    let spawn_data = || Tag::Compound(compound([("entity", entity())]));
    compound([
        ("id", string("minecraft:mob_spawner")),
        ("Delay", Tag::Short(0)),
        ("MinSpawnDelay", Tag::Short(200)),
        ("MaxSpawnDelay", Tag::Short(800)),
        ("SpawnCount", Tag::Short(4)),
        ("MaxNearbyEntities", Tag::Short(6)),
        ("RequiredPlayerRange", Tag::Short(16)),
        ("SpawnRange", Tag::Short(4)),
        ("SpawnData", spawn_data()),
        (
            "SpawnPotentials",
            Tag::List(vec![Tag::Compound(compound([
                ("data", spawn_data()),
                ("weight", Tag::Int(1)),
            ]))]),
        ),
    ])
}

fn text_of(tag: &Compound, name: &str) -> Option<String> {
    match lookup(tag, name)? {
        Tag::String(value) => value.to_string().ok(),
        _ => None,
    }
}

pub(crate) fn int_of(tag: &Compound, name: &str) -> Option<i32> {
    match lookup(tag, name)? {
        Tag::Int(value) => Some(*value),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Byte(value) => Some(i32::from(*value)),
        _ => None,
    }
}

fn text_of_tag(tag: &Tag, name: &str) -> Option<String> {
    match tag.get(name)? {
        Tag::String(value) => value.to_string().ok(),
        _ => None,
    }
}

pub(crate) fn int_of_tag(tag: &Tag, name: &str) -> Option<i32> {
    match tag.get(name)? {
        Tag::Int(value) => Some(*value),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Byte(value) => Some(i32::from(*value)),
        _ => None,
    }
}

/// The operator commands (level 2, as vanilla's debug commands): `/betterdungeons` (usage) and `/betterdungeons status`.
pub fn declare_commands(commands: &mut Vec<CommandRegistration>) {
    commands.push(Command::op("betterdungeons", 2).executes(|host, _, _| Ok(send(host, usage()))));
    commands.push(
        Command::op("betterdungeons status", 2).executes(|host, _, _| Ok(send(host, status()))),
    );
}

/// Send `lines` to the command's source; the command's result is their count.
fn send(host: &mut dyn ServerHost, lines: Vec<String>) -> i32 {
    let count = lines.len() as i32;
    for line in lines {
        let _ = host.commands().feedback(Text::literal(line), false);
    }
    count
}

fn usage() -> Vec<String> {
    vec![format!("{MOD_ID}: /betterdungeons status")]
}

fn status() -> Vec<String> {
    let mut lines = runtime::status_lines(&settings());
    if let Some(refusal) = REFUSAL
        .lock()
        .expect("the refusal slot is not poisoned")
        .clone()
    {
        lines.push(format!("  last refusal: {refusal}"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_piece_round_trips_through_its_own_fields() {
        let spec = Piece {
            template: "betterdungeons:small_dungeon/shells/small_shell_5x5",
            processors: "betterdungeons:small_dungeon/small_dungeon_zombie",
            rotation: Rotation::Cw180,
            position: [10, -20, 30],
            size: Rotation::Cw180.size(
                templates::template("betterdungeons:small_dungeon/shells/small_shell_5x5")
                    .expect("the shell")
                    .size,
            ),
            depth: 1,
            seed: 42,
            chests_before: 1,
            banners_before: 0,
            min_y: -64,
        };
        let piece = SmallDungeonPiece {
            spec: spec.clone(),
            states: Arc::new(Vec::new()),
        };
        let mut tag = compound([("BB", Tag::IntArray(vec![10, -20, 30, 16, -14, 36]))]);
        piece.add_additional_save_data(&mut tag);
        let restored = saved_spec(&tag).expect("a saved piece");
        assert_eq!(restored, spec);
    }

    #[test]
    fn a_config_without_a_uniform_start_height_is_refused() {
        let tag = Tag::compound([
            ("start_pool", string("betterdungeons:small_dungeon")),
            ("size", Tag::Int(10)),
            ("max_distance_from_center", Tag::Int(80)),
            (
                "start_height",
                Tag::compound([("type", string("minecraft:constant"))]),
            ),
        ]);
        assert!(decode_config(&tag).is_none());
    }

    #[test]
    fn the_carried_config_decodes_to_the_carried_cap() {
        let structure = &pools::STRUCTURE;
        let tag = Tag::compound([
            ("start_pool", string(structure.start_pool)),
            (
                "start_jigsaw_name",
                string(structure.start_jigsaw.expect("an anchor")),
            ),
            ("size", Tag::Int(structure.size)),
            (
                "start_height",
                Tag::compound([
                    ("type", string("minecraft:uniform")),
                    (
                        "min_inclusive",
                        Tag::compound([("absolute", Tag::Int(structure.start_y.0))]),
                    ),
                    (
                        "max_inclusive",
                        Tag::compound([("absolute", Tag::Int(structure.start_y.1))]),
                    ),
                ]),
            ),
            ("max_distance_from_center", Tag::Int(structure.max_distance)),
            ("use_expansion_hack", Tag::Byte(0)),
        ]);
        let config = decode_config(&tag).expect("the carried config");
        assert!(runtime::config_supported(&config));
    }

    #[test]
    fn a_spawner_carries_its_mob_in_both_fields() {
        let nbt = spawner_nbt("minecraft:skeleton");
        assert_eq!(
            text_of(&nbt, "id").as_deref(),
            Some("minecraft:mob_spawner")
        );
        let Tag::Compound(data) = lookup(&nbt, "SpawnData").expect("SpawnData") else {
            panic!("SpawnData is a compound");
        };
        let Tag::Compound(entity) = lookup(data, "entity").expect("entity") else {
            panic!("entity is a compound");
        };
        assert_eq!(text_of(entity, "id").as_deref(), Some("minecraft:skeleton"));
        let Tag::List(potentials) = lookup(&nbt, "SpawnPotentials").expect("potentials") else {
            panic!("potentials is a list");
        };
        assert_eq!(potentials.len(), 1);
    }

    #[test]
    fn a_banner_carries_its_patterns_and_name() {
        let nbt = banner_nbt(runtime::Banner::Zombie);
        let Tag::List(patterns) = lookup(&nbt, "patterns").expect("patterns") else {
            panic!("patterns is a list");
        };
        assert_eq!(patterns.len(), 6);
        let name = text_of(&nbt, "CustomName").expect("a name");
        assert!(
            name.contains("betterdungeons.small_dungeon.banner.zombie"),
            "{name}"
        );
        assert!(name.contains("Foul Banner"), "{name}");
    }
}
