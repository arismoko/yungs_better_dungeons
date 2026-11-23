# YUNG's Better Dungeons for Sunrise

Vanilla's monster rooms replaced by YUNG's small dungeons, with banners, spawners and loot piles.

A port of [YUNG's Better Dungeons](https://github.com/YUNG-GANG/YUNGs-Better-Dungeons) by YUNGNICKYOUNG and Acarii to [Sunrise](https://playsunrise.net) (Minecraft Java 26.3). This version carries the original's small dungeon; its structure type and processors are Rust, read from the original's behaviour.

## What it does

- The small dungeon underground: six room shells (5x5 to 9x9), zombie, skeleton and spider variants, with loot piles, chests, banners, ceiling props and a spawner, assembled from the original's templates and weights.
- The original's placement (its structure set and biome tag) and loot tables.
- Vanilla monster rooms no longer generate where the small dungeon can, as in the original (its "Remove Vanilla Dungeons" option is on by default).
- Operator command `/betterdungeons status` describes the generator.

## Differences from the original

- Only the small dungeon is ported: the zombie, skeleton and spider dungeons, the small Nether dungeon and their own processors are not yet.
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
