//! [`MagicPlugin`], [`CastSpellMessage`], and the exclusive cast-dispatch system.

use std::collections::HashMap;

use bevy::{
    ecs::{
        message::MessageCursor,
        system::{BoxedSystem, SystemId},
    },
    prelude::*,
};

use crate::{
    runes::{CastContext, Rune, RuneRegistry},
    spell::{Spell, SpellAssetLoader},
};

// ---------------------------------------------------------------------------
// Public message
// ---------------------------------------------------------------------------

/// Send this message to cast a spell.
///
/// [`execute_cast_spell_events`] picks it up each frame and runs every
/// [`crate::runes::Rune`] in the spell in declaration order.
///
/// # Multi-target support
///
/// `targets` is a `Vec<Entity>`, so a single cast can hit any number of
/// entities.  Pass an empty vec for self-cast / area spells that determine
/// targets inside their own rune logic.
#[derive(Message, Clone, Debug)]
pub struct CastSpellMessage {
    /// The entity casting the spell.
    pub caster: Entity,
    /// All targeted entities (empty = self-cast or rune determines targets).
    pub targets: Vec<Entity>,
    /// Handle to the spell asset to cast.
    pub spell: Handle<Spell>,
}

// ---------------------------------------------------------------------------
// Internal resources
// ---------------------------------------------------------------------------

/// Stores this system's position in the [`Messages<CastSpellMessage>`] ring
/// buffer so messages are never processed twice across frames.
#[derive(Resource, Default)]
struct SpellCastCursor(MessageCursor<CastSpellMessage>);

/// Caches the registered [`SystemId`] for each rune in each loaded spell.
///
/// Keys are `(AssetId<Spell>, rune_index)`.  An entry is created the first
/// time a spell is cast after load, and removed when the asset is modified or
/// unloaded.  Keeping one-shot system entities alive between casts means every
/// rune is initialized exactly once.
#[derive(Resource, Default)]
pub struct RuneSystemCache {
    pub systems: HashMap<(AssetId<Spell>, usize), SystemId<In<CastContext>>>,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Bevy plugin that wires up the entire magic system.
///
/// # Prerequisites
/// [`bevy::asset::AssetPlugin`] (included in [`bevy::DefaultPlugins`]) **must**
/// be present before `MagicPlugin`.
///
/// # What it registers
/// | Kind | Name |
/// |------|------|
/// | Asset | `Spell` (loaded from `.spell.json`) |
/// | AssetLoader | `SpellAssetLoader` |
/// | Message | `CastSpellMessage` |
/// | Resource | `SpellCastCursor` (internal) |
/// | Resource | `RuneSystemCache` |
/// | System | `invalidate_spell_cache` (Update) |
/// | System | `execute_cast_spell_events` (Update, after invalidate) |
pub struct MagicPlugin {
    rune_registrations: Vec<Box<dyn Fn(&RuneRegistry) + Send + Sync>>,
}

impl Default for MagicPlugin {
    fn default() -> Self {
        MagicPlugin {
            rune_registrations: Vec::new(),
        }
    }
}

impl MagicPlugin {
    /// Add a custom rune type to be registered when the plugin initializes.
    ///
    /// **`name`** must match the string returned by [`Rune::name`] on the
    /// corresponding implementation.  The rune type must also implement
    /// `serde::Deserialize` so it can be loaded from JSON.
    pub fn register_rune<R>(mut self, name: &'static str) -> Self
    where
        R: Rune + for<'de> serde::Deserialize<'de> + 'static,
    {
        self.rune_registrations
            .push(Box::new(move |registry: &RuneRegistry| {
                registry.register::<R>(name);
            }));
        self
    }
}

impl Plugin for MagicPlugin {
    fn build(&self, app: &mut App) {
        let registry = RuneRegistry::default();
        // register any user-added runes
        for f in &self.rune_registrations {
            f(&registry);
        }

        app.init_asset::<Spell>()
            .insert_resource(registry.clone())
            .register_asset_loader(SpellAssetLoader { registry: registry.clone() })
            .add_message::<CastSpellMessage>()
            .init_resource::<SpellCastCursor>()
            .init_resource::<RuneSystemCache>()
            .add_systems(
                Update,
                (invalidate_spell_cache, execute_cast_spell_events).chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Cache-invalidation system
// ---------------------------------------------------------------------------

/// Removes cache entries when a [`Spell`] asset is modified or unloaded.
///
/// Runs in `Update`, always **before** [`execute_cast_spell_events`].  When a
/// spell is hot-reloaded the stale [`SystemId`]s are evicted; the next cast
/// rebuilds them via `Rune::build`.
///
/// The orphaned one-shot system entities are left in the world intentionally —
/// unregistering requires exclusive world access; the footprint of a few extra
/// ECS entities is negligible and they are never run again.
pub fn invalidate_spell_cache(
    mut events: MessageReader<AssetEvent<Spell>>,
    mut cache: ResMut<RuneSystemCache>,
) {
    for event in events.read() {
        match event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => {
                cache.systems.retain(|(spell_id, _), _| spell_id != id);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch system
// ---------------------------------------------------------------------------

/// Exclusive-world system that processes every pending [`CastSpellMessage`].
///
/// ### Cache miss (first cast after load)
/// 1. Borrows `Assets<Spell>` + `RuneSystemCache` to find missing rune indices
///    and calls `Rune::build` for each, collecting `BoxedSystem` values.
/// 2. Drops both borrows, then calls `world.register_boxed_system` and stores
///    the resulting `SystemId` in [`RuneSystemCache`].
///
/// ### Cache hit (all subsequent casts)
/// Reads the cached `SystemId` values and calls `world.run_system_with(id, ctx)`
/// — no allocation, no re-initialization.
pub fn execute_cast_spell_events(world: &mut World) {
    // --- Move cursor out so we can borrow Messages at the same time ----------
    let mut cursor = world
        .remove_resource::<SpellCastCursor>()
        .unwrap_or_default();

    let messages: Vec<CastSpellMessage> = {
        let messages_res = world.resource::<Messages<CastSpellMessage>>();
        cursor.0.read(messages_res).cloned().collect()
    };

    world.insert_resource(cursor);

    // --- Process each cast message -------------------------------------------
    for message in messages {
        let spell_id = message.spell.id();
        let context = CastContext {
            caster: message.caster,
            targets: message.targets.clone(),
        };

        // 1. Build boxed systems for cache misses while holding borrows.
        let missing: Vec<(usize, BoxedSystem<In<CastContext>, ()>)> = {
            let cache = world.resource::<RuneSystemCache>();
            match world.resource::<Assets<Spell>>().get(&message.spell) {
                None => continue,
                Some(spell) => (0..spell.runes.len())
                    .filter(|&i| !cache.systems.contains_key(&(spell_id, i)))
                    .map(|i| (i, spell.runes[i].build()))
                    .collect(),
            }
        }; // borrows on Assets<Spell> and RuneSystemCache dropped here

        // 2. Register new systems and cache their SystemIds.
        for (i, boxed) in missing {
            let id = world.register_boxed_system(boxed);
            world
                .resource_mut::<RuneSystemCache>()
                .systems
                .insert((spell_id, i), id);
        }

        // 3. Gather cached SystemIds (safe re-borrow after step 2 mutations).
        let ids: Vec<SystemId<In<CastContext>>> = {
            let cache = world.resource::<RuneSystemCache>();
            let rune_count = world
                .resource::<Assets<Spell>>()
                .get(&message.spell)
                .map(|s| s.runes.len())
                .unwrap_or(0);
            (0..rune_count)
                .map(|i| cache.systems[&(spell_id, i)])
                .collect()
        };

        // 4. Run each rune system with the cast context.
        for id in ids {
            let _ = world.run_system_with(id, context.clone());
        }
    }
}
