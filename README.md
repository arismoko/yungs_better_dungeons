# YUNG's Better Dungeons for Sunrise

Vanilla's monster rooms replaced by YUNG's dungeons: small dungeons, zombie crypts and skeleton halls, with spawners and loot.

A port of [YUNG's Better Dungeons](https://github.com/YUNG-GANG/YUNGs-Better-Dungeons) by YUNGNICKYOUNG and Acarii to [Sunrise](https://playsunrise.net) (Minecraft Java 26.3). The small dungeon's and the spider dungeon's structure types and the processors are Rust, read from the original's behaviour; the zombie and skeleton dungeons are the original's jigsaw structures, with its processors written as data.

## What it does

- The small dungeon underground: six room shells (5x5 to 9x9), zombie, skeleton and spider variants, with loot piles, chests, banners, ceiling props and a spawner, assembled from the original's templates and weights.
- Zombie dungeons: crypts of tombs and cubbies near the surface, with zombie spawners, skeleton spawners in the tombstones, flower pots and worn cobblestone.
- Skeleton dungeons: deep halls of ruined stone brick with skeleton spawners, candles and legs down to the cave floor.
- Spider dungeons: a cave of two winding tunnels with side tunnels, cobwebbed and shelled in cobblestone, ending in a wool-wrapped nest with a cave spider spawner and egg rooms holding loot chests or spider spawners.
- The original's placement (its structure sets and biome tags) and loot tables.
- Vanilla monster rooms no longer generate where the small dungeon can, as in the original (its "Remove Vanilla Dungeons" option is on by default).
- Settings, applied when a world loads: skulls and heads, Nether blocks, the small dungeon's banner limit, its fewest and most chests, and its ore props.
- Operator command `/betterdungeons status` describes the generator.

## Differences from the original

- The spider cave's web check treats a block as solid where the original asks whether it occludes, and a fluid is water, lava, a water plant or a waterlogged block; the two differ only on unusual blocks such as slabs.
- The small Nether dungeon is included but off, as in the original's default: add biomes to the `betterdungeons:has_structure/small_nether_dungeon` tag with a data pack to turn it on. The nether brick stair in front of its entrance, the two-banner limit and the wither skull and blaze rod options are not carried.
- The skulls and Nether blocks settings change the small dungeon only; the zombie, skeleton and Nether dungeons keep the original's defaults. The zombie staircase length setting is not carried.
- The zombie dungeon's staircase up to the surface is not dug: its stairs end where the original's would when the surface is out of reach.
- The status command is this port's own; the original has none.

## Building

With the Sunrise mod tool (`cargo sunrise-mod`, from the Sunrise SDK) and Rust:

```sh
cargo sunrise-mod build    # compiles against the pinned game, packs target/sunrise-mod/betterdungeons.smod
cargo sunrise-mod run      # builds it and starts the game with it
cargo test
```

`sunrise-mod.toml` pins the Sunrise Dev build `dev-2026-11-22-7cfdf81` (game commit `7cfdf81463`). Sunrise builds native mods from source on the player's computer, against the exact game build.

## Licence and credits

YUNG's Better Dungeons is by YUNGNICKYOUNG and Acarii (https://github.com/YUNG-GANG/YUNGs-Better-Dungeons). Its templates, structure definitions, loot tables and strings are theirs and are used here under the **GNU LGPL v3.0 only**, the original's licence; `LICENSE` holds the text and `LICENSE-NOTICE.md` says what this port carries.

Carried to Minecraft 26.3 by AdLf0208 ([AdLf0208/yungs_better_dungeons](https://github.com/AdLf0208/yungs_better_dungeons)), and ported to Sunrise by arismoko. This port, its Rust included, is distributed under the same licence.
