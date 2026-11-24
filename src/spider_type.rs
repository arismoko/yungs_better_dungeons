//! The spider dungeon's structure type: the glue between [`crate::spider`]'s plan and carving and
//! the game's `StructureType`, `StructurePiece` and `PieceContext`.

use crate::spider::{
    self, Beside, Cell, EGG_ROOM_LOOT, Frame, Kind, LENGTH, Level, Piece, Put, TYPE_ID,
};
use crate::support::{compound, int_of, int_of_tag, lookup, string};
use mod_api::{
    self, BlockKey, BlockMetadata, BoundingBox, Compound, FoundStructure, GenerationContext,
    HeightmapType, PieceContext, RandomSource, Registrar, StructureError, StructurePiece,
    StructureType, Tag,
};
use std::sync::{Arc, OnceLock};

/// The blocks the carving reads or writes, requested once at registration.
#[derive(Clone, Copy, Debug)]
struct Keys {
    cave_air: BlockKey,
    cobblestone: BlockKey,
    cobweb: BlockKey,
    white_wool: BlockKey,
    spawner: BlockKey,
    chest: BlockKey,
    diamond_block: BlockKey,
    leaves: [BlockKey; 6],
    grass: [BlockKey; 2],
    /// The blocks that hold a fluid, besides anything waterlogged.
    fluids: [BlockKey; 7],
}

static KEYS: OnceLock<Keys> = OnceLock::new();

/// Register the type and request its blocks, once at startup.
pub fn register(r: &mut dyn Registrar) {
    let mut key = |name: &str| r.block_key(name);
    let keys = Keys {
        cave_air: key("minecraft:cave_air"),
        cobblestone: key("minecraft:cobblestone"),
        cobweb: key("minecraft:cobweb"),
        white_wool: key("minecraft:white_wool"),
        spawner: key("minecraft:spawner"),
        chest: key("minecraft:chest"),
        diamond_block: key("minecraft:diamond_block"),
        leaves: [
            key("minecraft:acacia_leaves"),
            key("minecraft:birch_leaves"),
            key("minecraft:oak_leaves"),
            key("minecraft:dark_oak_leaves"),
            key("minecraft:jungle_leaves"),
            key("minecraft:spruce_leaves"),
        ],
        grass: [key("minecraft:short_grass"), key("minecraft:tall_grass")],
        fluids: [
            key("minecraft:water"),
            key("minecraft:lava"),
            key("minecraft:bubble_column"),
            key("minecraft:kelp"),
            key("minecraft:kelp_plant"),
            key("minecraft:seagrass"),
            key("minecraft:tall_seagrass"),
        ],
    };
    let _ = KEYS.set(keys);
    r.register_structure_type(TYPE_ID, SpiderDungeonType);
}

fn keys() -> Result<Keys, StructureError> {
    KEYS.get()
        .copied()
        .ok_or_else(|| StructureError("the spider dungeon's blocks were not registered".to_owned()))
}

/// The registered type: `SpiderDungeonStructure`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpiderDungeonType;

impl StructureType for SpiderDungeonType {
    /// The start's middle block, a height drawn from the config, and the plan.
    fn find_generation_point(&self, context: &mut dyn GenerationContext) -> Option<FoundStructure> {
        let (low, high) = start_height(&context.config())?;
        let chunk = context.chunk();
        let frame = Frame {
            seed: context.seed(),
            min_y: context.min_y(),
            max_y: context.max_y() + 1,
        };
        let random = context.random();
        let y = low + random.next_int_bound(high - low + 1);
        let start = [chunk.x * 16 + 8, y, chunk.z * 16 + 8];
        let pieces = spider::plan(random, start)
            .into_iter()
            .map(|piece| Arc::new(SpiderPiece { piece, frame }) as Arc<dyn StructurePiece>)
            .collect();
        Some(FoundStructure {
            position: start,
            pieces,
            ..FoundStructure::default()
        })
    }

    fn load_piece(
        &self,
        _metadata: &dyn BlockMetadata,
        tag: &Compound,
    ) -> Result<Arc<dyn StructurePiece>, StructureError> {
        saved_piece(tag)
            .map(|piece| Arc::new(piece) as Arc<dyn StructurePiece>)
            .ok_or_else(|| {
                StructureError("the saved spider dungeon piece is incomplete".to_owned())
            })
    }
}

/// The structure JSON's uniform, absolute `start_height`.
fn start_height(config: &Tag) -> Option<(i32, i32)> {
    let height = config.get("start_height")?;
    let low = int_of_tag(height.get("min_inclusive")?, "absolute")?;
    let high = int_of_tag(height.get("max_inclusive")?, "absolute")?;
    (low <= high).then_some((low, high))
}

/// One planned piece with the level's limits and the world seed.
#[derive(Debug)]
struct SpiderPiece {
    piece: Piece,
    frame: Frame,
}

impl StructurePiece for SpiderPiece {
    fn bounding_box(&self) -> BoundingBox {
        BoundingBox::new(self.piece.min, self.piece.max)
    }

    fn post_process(&self, context: &mut dyn PieceContext) -> Result<(), StructureError> {
        let keys = keys()?;
        let mut level = ContextLevel { context, keys };
        self.piece.place(&mut level, self.frame);
        Ok(())
    }

    fn add_additional_save_data(&self, tag: &mut Compound) {
        let piece = &self.piece;
        tag.insert("Kind".into(), string(piece.kind.name()));
        tag.insert("Start".into(), Tag::IntArray(piece.start.to_vec()));
        tag.insert("End".into(), Tag::IntArray(piece.end.to_vec()));
        tag.insert("Pitch".into(), Tag::Float(piece.pitch.to_bits()));
        tag.insert(
            "Yaws".into(),
            Tag::List(
                piece
                    .yaws
                    .iter()
                    .map(|yaw| Tag::Float(yaw.to_bits()))
                    .collect(),
            ),
        );
        tag.insert(
            "Radii".into(),
            Tag::List(
                piece
                    .radii
                    .iter()
                    .map(|radius| Tag::Float(radius.to_bits()))
                    .collect(),
            ),
        );
        tag.insert("Seed".into(), Tag::Long(self.frame.seed));
        tag.insert("MinY".into(), Tag::Int(self.frame.min_y));
        tag.insert("MaxY".into(), Tag::Int(self.frame.max_y));
    }
}

/// A saved piece, from the tag `load_piece` is given.
fn saved_piece(tag: &Compound) -> Option<SpiderPiece> {
    let Tag::String(kind) = lookup(tag, "Kind")? else {
        return None;
    };
    let kind = Kind::from_name(&kind.to_string().ok()?)?;
    let triple = |name: &str| match lookup(tag, name)? {
        Tag::IntArray(values) if values.len() == 3 => Some([values[0], values[1], values[2]]),
        _ => None,
    };
    let floats = |name: &str| -> Option<Vec<f32>> {
        let Tag::List(values) = lookup(tag, name)? else {
            return None;
        };
        values
            .iter()
            .map(|value| match value {
                Tag::Float(bits) => Some(f32::from_bits(*bits)),
                _ => None,
            })
            .collect()
    };
    let Tag::IntArray(bounds) = lookup(tag, "BB")? else {
        return None;
    };
    if bounds.len() != 6 {
        return None;
    }
    let pitch = match lookup(tag, "Pitch")? {
        Tag::Float(bits) => f32::from_bits(*bits),
        _ => return None,
    };
    let yaws = floats("Yaws")?;
    let radii = floats("Radii")?;
    let tunnel = matches!(kind, Kind::BigTunnel | Kind::SmallTunnel);
    if (tunnel && yaws.len() != LENGTH) || radii.len() != 3 {
        return None;
    }
    let seed = match lookup(tag, "Seed")? {
        Tag::Long(value) => *value,
        _ => return None,
    };
    Some(SpiderPiece {
        piece: Piece {
            kind,
            start: triple("Start")?,
            end: triple("End")?,
            pitch,
            yaws,
            radii: [radii[0], radii[1], radii[2]],
            min: [bounds[0], bounds[1], bounds[2]],
            max: [bounds[3], bounds[4], bounds[5]],
        },
        frame: Frame {
            seed,
            min_y: int_of(tag, "MinY")?,
            max_y: int_of(tag, "MaxY")?,
        },
    })
}

/// `PieceContext`, as the carving's [`Level`].
struct ContextLevel<'a> {
    context: &'a mut dyn PieceContext,
    keys: Keys,
}

impl Level for ContextLevel<'_> {
    fn random(&mut self) -> &mut dyn RandomSource {
        self.context.random()
    }

    fn cell(&mut self, pos: [i32; 3]) -> Cell {
        if !self.inside(pos) {
            return Cell::OUTSIDE;
        }
        let Some(state) = self.context.block(pos) else {
            return Cell::OUTSIDE;
        };
        let keys = self.keys;
        let block = state.block();
        let waterlogged = matches!(
            self.context.metadata().property(state, "waterlogged"),
            Ok(Some("true"))
        );
        Cell {
            air: self.context.is_air(state),
            fluid: waterlogged || keys.fluids.contains(&block),
            blacklisted: [
                keys.diamond_block,
                keys.white_wool,
                keys.spawner,
                keys.chest,
            ]
            .contains(&block)
                || keys.leaves.contains(&block)
                || keys.grass.contains(&block),
            occludes: self.context.solid_render(state),
            chest: block == keys.chest,
        }
    }

    fn put(&mut self, pos: [i32; 3], what: Put) {
        let keys = self.keys;
        let state = match what {
            Put::CaveAir => self.context.state(keys.cave_air),
            Put::Cobblestone => self.context.state(keys.cobblestone),
            Put::Cobweb => self.context.state(keys.cobweb),
            Put::WhiteWool => self.context.state(keys.white_wool),
            Put::Spawner => self.context.state(keys.spawner),
            Put::Chest(facing) => self.context.state_with(
                keys.chest,
                &[("facing", ["north", "east", "south", "west"][facing % 4])],
            ),
        };
        self.context.set_block(pos, state);
    }

    fn chunk(&self) -> [i32; 2] {
        let chunk = self.context.chunk();
        [chunk.x, chunk.z]
    }

    fn inside(&self, pos: [i32; 3]) -> bool {
        self.context.chunk_box().is_inside(pos)
    }

    fn surface(&mut self, x: i32, z: i32) -> i32 {
        self.context
            .height(HeightmapType::WorldSurfaceWg, x, z)
            .unwrap_or(i32::MIN)
    }

    fn spawner(&mut self, pos: [i32; 3], mob: &'static str) {
        let entity = Tag::Compound(compound([("id", string(mob))]));
        let data = Tag::Compound(compound([("entity", entity)]));
        self.context.block_entity(
            pos,
            compound([("id", string("minecraft:mob_spawner")), ("SpawnData", data)]),
        );
    }

    fn chest(&mut self, pos: [i32; 3], loot_seed: i64) {
        self.context.block_entity(
            pos,
            compound([
                ("id", string("minecraft:chest")),
                ("LootTable", string(EGG_ROOM_LOOT)),
                ("LootTableSeed", Tag::Long(loot_seed)),
            ]),
        );
    }

    fn beside(&mut self, pos: [i32; 3]) -> Beside {
        match self.context.block(pos) {
            Some(state) if state.block() == self.keys.chest => Beside::Chest,
            Some(state) if self.context.solid_render(state) => Beside::Solid,
            _ => Beside::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_piece_round_trips_through_its_own_fields() {
        let mut random = mod_api::LegacyRandomSource::new(77);
        for piece in spider::plan(&mut random, [8, 70, 8]) {
            let saved = SpiderPiece {
                piece: piece.clone(),
                frame: Frame {
                    seed: 99,
                    min_y: -64,
                    max_y: 320,
                },
            };
            let mut tag = compound([(
                "BB",
                Tag::IntArray(vec![
                    piece.min[0],
                    piece.min[1],
                    piece.min[2],
                    piece.max[0],
                    piece.max[1],
                    piece.max[2],
                ]),
            )]);
            saved.add_additional_save_data(&mut tag);
            let restored = saved_piece(&tag).expect("a saved piece");
            assert_eq!(restored.piece, piece);
            assert_eq!(restored.frame, saved.frame);
        }
    }

    #[test]
    fn a_piece_missing_its_fields_is_a_typed_refusal_not_a_panic() {
        assert!(saved_piece(&compound([("Kind", string("nest"))])).is_none());
        assert!(saved_piece(&Compound::new()).is_none());
    }

    #[test]
    fn the_start_height_reads_the_uniform_absolute_range() {
        let bound = |value: i32| Tag::compound([("absolute", Tag::Int(value))]);
        let config = Tag::compound([(
            "start_height",
            Tag::compound([
                ("type", string("minecraft:uniform")),
                ("min_inclusive", bound(70)),
                ("max_inclusive", bound(71)),
            ]),
        )]);
        assert_eq!(start_height(&config), Some((70, 71)));
        assert_eq!(start_height(&Tag::compound([("size", Tag::Int(1))])), None);
    }
}
