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
    enchanting::{
        ActiveEnchantments, ApplyEnchantmentCursor, ApplyEnchantmentMessage,
        EnchantmentTrigger, PendingDespawnTriggers, RemoveEnchantmentCursor,
        RemoveEnchantmentMessage, TriggerEnchantmentCursor, TriggerEnchantmentMessage,
        apply_enchantments, flush_despawn_triggers, ondespawn_trigger_enchantments,
        remove_enchantments, tick_enchantments, trigger_enchantments,
    },
    runes::{ActiveSpells, CastContext, Rune, RuneRegistry},
    spell::{Spell, SpellAssetLoader},
};

// ---------------------------------------------------------------------------
// Public message
// ---------------------------------------------------------------------------

/// Send this message to cast a spell.
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
struct RuneSystemCache {
    systems: HashMap<(AssetId<Spell>, usize), SystemId<In<CastContext>>>,
}

impl RuneSystemCache {
    /// Returns the cached `SystemId` for the given spell and rune index, if it
    /// exists.
    pub fn get(&self, spell_id: AssetId<Spell>, rune_index: usize) -> Option<SystemId<In<CastContext>>> {
        self.systems.get(&(spell_id, rune_index)).cloned()
    }

    /// Inserts a `SystemId` into the cache for the given spell and rune index.
    pub fn insert(&mut self, spell_id: AssetId<Spell>, rune_index: usize, system_id: SystemId<In<CastContext>>) {
        self.systems.insert((spell_id, rune_index), system_id);
    }

    #[allow(unused)]
    /// Removes the cache entry for the given spell and rune index.
    pub fn remove(&mut self, spell_id: AssetId<Spell>, rune_index: usize) {
        self.systems.remove(&(spell_id, rune_index));
    }

    /// Clears all cache entries for the given spell.
    pub fn clear_spell(&mut self, spell_id: AssetId<Spell>) {
        self.systems.retain(|(id, _), _| *id != spell_id);
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Bevy plugin that wires up the entire magic system.
///
/// # Prerequisites
/// [`bevy::asset::AssetPlugin`] (included in [`bevy::DefaultPlugins`]) **must**
/// be present before `MagicPlugin`.
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
    pub fn rune<R>(mut self) -> Self
    where
        R: Rune + TypePath + for<'de> serde::Deserialize<'de> + 'static,
    {
        self.rune_registrations
            .push(Box::new(move |registry: &RuneRegistry| {
                registry.register::<R>();
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
            .add_message::<ApplyEnchantmentMessage>()
            .add_message::<RemoveEnchantmentMessage>()
            .add_message::<TriggerEnchantmentMessage>()
            .init_resource::<SpellCastCursor>()
            .init_resource::<RuneSystemCache>()
            .init_resource::<ApplyEnchantmentCursor>()
            .init_resource::<RemoveEnchantmentCursor>()
            .init_resource::<TriggerEnchantmentCursor>()
            .init_resource::<PendingDespawnTriggers>()
            .add_observer(ondespawn_trigger_enchantments)
            .add_systems(
                Update,
                (
                    invalidate_spell_cache,
                    execute_cast_spell_events,
                    tick_spell_executions,
                    apply_enchantments,
                    remove_enchantments,
                    tick_enchantments,
                    trigger_enchantments,
                    flush_despawn_triggers,
                )
                    .chain(),
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
fn invalidate_spell_cache(
    mut events: MessageReader<AssetEvent<Spell>>,
    mut cache: ResMut<RuneSystemCache>,
) {
    for event in events.read() {
        match event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => {
                cache.clear_spell(*id);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Rune execution ticking system
// ---------------------------------------------------------------------------

/// Ticks all active spell executions and runs rune systems when their timers finish.
fn tick_spell_executions(world: &mut World) {
    let time_delta = world.resource::<Time>().delta();
    
    // Collect systems to run separately to avoid borrow conflicts
    let mut systems_to_run: Vec<(SystemId<In<CastContext>>, CastContext)> = Vec::new();
    
    // Collect entities with ActiveSpells to avoid borrow conflicts
    let casters: Vec<Entity> = world
        .query_filtered::<Entity, With<ActiveSpells>>()
        .iter(world)
        .collect();

    // Tick timers and collect systems to run
    for caster in casters.iter() {
        if let Some(mut active) = world.entity_mut(*caster).get_mut::<ActiveSpells>() {
            active.spells.retain_mut(|spell| {
                spell.runes.retain_mut(|rune| {
                    rune.timer.tick(time_delta);
                    if rune.timer.just_finished() {
                        systems_to_run.push((rune.system, spell.ctx.clone()));
                        if rune.repeating {
                            rune.timer.reset();
                            true  // keep the rune
                        } else {
                            false  // remove after one shot
                        }
                    } else {
                        true  // keep, still waiting
                    }
                });
                !spell.runes.is_empty()  // remove spell when all runes are done
            });
        }
    }

    // Remove ActiveSpells component from entities that completed all spells
    for caster in casters {
        if let Some(active) = world.entity(caster).get::<ActiveSpells>() {
            if active.spells.is_empty() {
                world.entity_mut(caster).remove::<ActiveSpells>();
            }
        }
    }

    // Run collected systems after releasing borrows
    for (system_id, mut context) in systems_to_run {
        context.targets.retain(|&e| world.get_entity(e).is_ok());
        let _ = world.run_system_with(system_id, context);
    }
}

// ---------------------------------------------------------------------------
// Dispatch system
// ---------------------------------------------------------------------------

/// Exclusive-world system that processes every pending [`CastSpellMessage`].
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
            origin: None,
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
                .insert(spell_id, i, id);
        }

        // 3. Gather cached SystemIds with timing metadata.
        let spell_opt = world.resource::<Assets<Spell>>().get(&message.spell);
        if spell_opt.is_none() {
            continue;
        }
        let spell = spell_opt.unwrap();
        
        let cache = world.resource::<RuneSystemCache>();
        let mut rune_systems: Vec<(SystemId<In<CastContext>>, Timer, bool)> = Vec::new();
        
        for i in 0..spell.runes.len() {
            if let Some(sys_id) = cache.get(spell_id, i) {
                let rune = &spell.runes[i];
                let delay = rune.delay();
                let interval = rune.interval();
                
                // Create a timer initialized to the delay duration
                let timer = Timer::from_seconds(
                    delay.as_secs_f32(),
                    if interval.is_zero() {
                        TimerMode::Once
                    } else {
                        TimerMode::Repeating
                    },
                );
                
                let repeating = !interval.is_zero();
                rune_systems.push((sys_id, timer, repeating));
            }
        }

        // 4. Add spell execution to caster's ActiveSpells, or create if missing.
        if let Ok(mut entity) = world.get_entity_mut(message.caster) {
            if let Some(mut active) = entity.get_mut::<ActiveSpells>() {
                active.add_spell(context.clone(), rune_systems);
            } else {
                let mut active = ActiveSpells::new();
                active.add_spell(context.clone(), rune_systems);
                entity.insert(active);
            }
        }

        // 5. Fire any OnCast enchantments on the caster immediately.
        let mut oncast_systems: Vec<(SystemId<In<CastContext>>, CastContext)> = Vec::new();
        if let Some(active_enchantments) = world.entity(message.caster).get::<ActiveEnchantments>() {
            for enchantment in active_enchantments.enchantments.iter() {
                if matches!(enchantment.trigger, EnchantmentTrigger::OnCast) {
                    for rune in &enchantment.rune_executions {
                        oncast_systems.push((
                            rune.system_id,
                            CastContext {
                                caster: message.caster,
                                targets: message.targets.clone(),
                                origin: None,
                            },
                        ));
                    }
                }
            }
        }

        for (system_id, context) in oncast_systems {
            let _ = world.run_system_with(system_id, context);
        }
    }
}
