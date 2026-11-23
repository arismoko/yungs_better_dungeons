//! `betterdungeons` — YUNG's Better Dungeons: the small dungeon.
//!
//! | Moment | What the port does |
//! |---|---|
//! | [`Mod::register`] | registers the structure types `betterdungeons:small_dungeon_type` and `betterdungeons:spider_dungeon_type` and requests the blocks they place |
//! | [`Mod::register_commands`] | registers `/betterdungeons status`, answered from [`runtime`] |
//!
//! The structure's own code lives on the registered type: [`support::SmallDungeonType`]
//! assembles the jar's carried jigsaw templates through [`runtime::assemble`], and
//! [`support::SmallDungeonPiece`] places one piece into the chunk the engine asks
//! about. Every number is in [`runtime`], which has no `mod_api` type and is
//! proved by `cargo test`; [`support`] is the thin glue.
//!
//! The spider dungeon is code, not jigsaw: [`spider`] plans its tunnels and carves them against a
//! small `Level` trait, and [`spider_type`] answers that trait from `PieceContext`.
//!
//! The carried tables ([`pools`], [`templates`]) are generated from the jar by
//! `tools/gen_tables.py`.
pub mod pools;
pub mod runtime;
pub mod spider;
pub mod spider_type;
pub mod support;
pub mod templates;

use mod_api::{self, Hooks, RegisterCommandsEvent, Registrar, RegistrationContext, ServerHost};

#[derive(Default)]
struct Mod;

#[mod_api::sunrise_mod]
impl Hooks for Mod {
    /// Once, at startup: the structure type and the blocks it places.
    fn register(&self, r: &mut dyn Registrar, cx: &RegistrationContext) {
        support::register(r, cx);
    }

    /// Declare `/betterdungeons …` in the server's command tree.
    fn register_commands(&self, _host: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        support::declare_commands(event.commands);
    }
}
