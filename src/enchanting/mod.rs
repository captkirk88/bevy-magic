//! Enchanting system — attaches persistent magical effects to [`Enchantable`] entities.
//!
//! An [`Enchantment`] is a named, persistent effect whose magic comes from either
//! inline [`Rune`]s or a loaded [`Spell`] asset.  Any entity that bears the
//! [`Enchantable`] marker component can receive enchantments via
//! [`ApplyEnchantmentMessage`] and have them removed via [`RemoveEnchantmentMessage`].
//!
//! # Items
//!
//! Items (weapons, armour, consumables, etc.) are enchantable by simply adding
//! [`Enchantable`] to the item entity.
//! The enchantment's [`CastContext`] will name the item as the target; rune
//! systems are free to walk equipped-item relationships with normal queries.
//!
//! # Triggers
//!
//! By default enchantments are **timed** — each rune fires according to its own
//! `delay()` / `interval()` schedule.  Set [`EnchantmentTrigger::OnDemand`] on
//! an [`Enchantment`] to suppress timer-driven firing; instead the runes run
//! only when a [`TriggerEnchantmentMessage`] naming that enchantment is sent,
//! which is useful for "on-hit", "on-equip", or other event-driven effects.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use bevy::prelude::*;
//! use bevy_magic::{
//!     enchanting::prelude::*,
//!     prelude::*,
//! };
//!
//! fn setup(mut commands: Commands) {
//!     // Spawn an enchantable sword entity.
//!     commands.spawn((Name::new("Sword"), Enchantable));
//! }
//!
//! fn enchant_sword(mut commands: Commands, sword: Query<Entity, With<Enchantable>>) {
//!     let applier = Entity::PLACEHOLDER;
//!     for entity in &sword {
//!         let enchantment = Enchantment::from_runes(
//!             "Flame Edge",
//!             "Deals periodic fire damage.",
//!             applier,
//!             vec![Box::new(my_fire_rune)],
//!         );
//!         commands.apply_enchantment(entity, enchantment);
//!     }
//! }
//! ```

use std::sync::Arc;

use bevy::{
    ecs::{
        message::MessageCursor,
        system::{BoxedSystem, SystemId},
    },
    prelude::*,
};

use crate::{
    runes::{BoxedRune, CastContext}, spell::Spell
};

pub mod prelude {
    pub use super::{
        ActiveEnchantments, Enchantable, Enchantment, EnchantmentSource, EnchantmentTrigger,
        ApplyEnchantmentMessage, RemoveEnchantmentMessage, TriggerEnchantmentMessage,
    };
}

// ---------------------------------------------------------------------------
// EnchantmentTrigger
// ---------------------------------------------------------------------------

/// Determines when an enchantment's runes fire.
///
/// The default is [`EnchantmentTrigger::Timed`], which preserves the existing
/// timer-driven behaviour where each rune fires according to its `delay()` and
/// `interval()`.
///
/// Use [`EnchantmentTrigger::OnDemand`] for event-driven effects (on-hit,
/// on-equip, etc.) — runes only run when a [`TriggerEnchantmentMessage`] is
/// sent that names this enchantment.
#[derive(Clone, Debug, Default)]
pub enum EnchantmentTrigger {
    /// Runes fire according to each rune's own `delay()` / `interval()` timers
    /// (default behaviour).
    #[default]
    Timed,
    /// Runes fire only when a [`TriggerEnchantmentMessage`] names this enchantment.
    /// The enchantment persists until explicitly removed.
    OnDemand,    /// Runes fire whenever `source` casts a spell.
    ///
    /// `source` is the entity holding the enchantment. Targets are the spell targets.
    OnCast,
    /// Runes fire when the source entity dies/despawns.
    ///
    /// This can be used for death throes and explosive weapons.
    OnDespawn,
    /// Runes fire when the source enters/triggers an area event.
    ///
    /// Use `commands.trigger_enchantment(source, name, Some(area_targets))`.
    OnTriggerArea,}

// ---------------------------------------------------------------------------
// EnchantmentSource
// ---------------------------------------------------------------------------

/// Defines where an enchantment's effects come from.
pub enum EnchantmentSource {
    /// Inline [`Rune`]s executed when the enchantment ticks.
    Runes(Vec<BoxedRune>),
    /// A loaded [`Spell`] asset whose runes drive the enchantment effect.
    Spell(Handle<Spell>),
}

// ---------------------------------------------------------------------------
// Enchantment
// ---------------------------------------------------------------------------

/// A persistent magical effect to be applied to an [`Enchantable`] entity.
///
/// Construct with [`Enchantment::from_runes`] or [`Enchantment::from_spell`], then
/// send an [`ApplyEnchantmentMessage`] (or use [`crate::CommandsExt::apply_enchantment`])
/// to attach it to an entity.
pub struct Enchantment {
    /// Human-readable name — also used as the key for removal.
    pub name: String,
    /// Flavour / tooltip text.
    pub description: String,
    /// Entity that applied this enchantment; passed as `caster` in [`CastContext`].
    pub applier: Entity,
    /// Source of this enchantment's effects.
    pub source: EnchantmentSource,
    /// When runes should fire (default: [`EnchantmentTrigger::Timed`]).
    pub trigger: EnchantmentTrigger,
}

impl Enchantment {
    /// Creates an enchantment driven by inline runes.
    pub fn from_runes(
        name: impl Into<String>,
        description: impl Into<String>,
        applier: Entity,
        runes: Vec<BoxedRune>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            applier,
            source: EnchantmentSource::Runes(runes),
            trigger: EnchantmentTrigger::default(),
        }
    }

    /// Creates an enchantment driven by a loaded [`Spell`] asset.
    pub fn from_spell(
        name: impl Into<String>,
        description: impl Into<String>,
        applier: Entity,
        spell: Handle<Spell>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            applier,
            source: EnchantmentSource::Spell(spell),
            trigger: EnchantmentTrigger::default(),
        }
    }

    /// Override the trigger mode for this enchantment.
    ///
    /// # Example
    /// ```rust,ignore
    /// Enchantment::from_runes("On-Hit Burn", "...", applier, runes)
    ///     .with_trigger(EnchantmentTrigger::OnDemand)
    /// ```
    pub fn with_trigger(mut self, trigger: EnchantmentTrigger) -> Self {
        self.trigger = trigger;
        self
    }
}

// ---------------------------------------------------------------------------
// Enchantable component
// ---------------------------------------------------------------------------

/// Marker component. Only entities with this component accept [`ApplyEnchantmentMessage`].
///
/// Add it to any entity you want to be enchantable — weapons, armour, characters, etc.
#[derive(Component, Default, Debug)]
pub struct Enchantable;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Send this message to apply an [`Enchantment`] to an [`Enchantable`] entity.
///
/// The enchantment is wrapped in [`Arc`] so the message is cheaply `Clone`.
///
/// # Example
///
/// ```rust,ignore
/// commands.apply_enchantment(entity, my_enchantment);
/// ```
#[derive(Message, Clone)]
pub struct ApplyEnchantmentMessage {
    /// Target entity — must have [`Enchantable`].
    pub target: Entity,
    /// The enchantment to apply.
    pub enchantment: Arc<Enchantment>,
}

/// Send this message to remove a named enchantment from an entity.
#[derive(Message, Clone, Debug)]
pub struct RemoveEnchantmentMessage {
    /// The entity to un-enchant.
    pub target: Entity,
    /// Name of the enchantment to remove (case-sensitive).
    pub name: String,
}

/// Send this message to fire the runes of an [`EnchantmentTrigger::OnDemand`]
/// enchantment.
///
/// `source` is the entity that hosts the enchantment. `targets` are the entity
/// or entities affected by the triggered effect.
///
/// # Example
/// ```rust,ignore
/// // Inside an "on-hit" observer or system:
/// commands.trigger_enchantment(sword_entity, "Flame Edge", Some(vec![goblin]))
/// ```
#[derive(Message, Clone, Debug)]
pub struct TriggerEnchantmentMessage {
    /// Entity that carries the enchantment.
    pub source: Entity,
    /// Name of the enchantment to fire (case-sensitive).
    pub name: String,
    /// Targets to receive the triggered effect.
    pub targets: Vec<Entity>,
}

// ---------------------------------------------------------------------------
// Internal tracking
// ---------------------------------------------------------------------------

/// One active enchantment slot on an entity — carries public metadata.
pub struct ActiveEnchantmentEntry {
    /// The name of this enchantment (matches what was applied).
    pub name: String,
    /// Description of this enchantment.
    pub description: String,
    pub(crate) applier: Entity,
    pub(crate) trigger: EnchantmentTrigger,
    pub(crate) rune_executions: Vec<ActiveEnchantRune>,
}

impl ActiveEnchantmentEntry {
    pub fn trigger(&self) -> EnchantmentTrigger {
        self.trigger.clone()
    }
}

pub(crate) struct ActiveEnchantRune {
    pub system_id: SystemId<In<CastContext>>,
    pub timer: Timer,
    pub repeating: bool,
}

/// Tracks all active enchantments on an entity.
///
/// Added automatically when a first enchantment is applied; removed when the
/// last one completes or is explicitly removed.
#[derive(Component, Default)]
pub struct ActiveEnchantments {
    /// All currently active enchantment slots.
    pub enchantments: Vec<ActiveEnchantmentEntry>,
}

impl ActiveEnchantments {
    /// Returns `true` if the entity bears an enchantment with the given name.
    pub fn has_enchantment(&self, name: &str) -> bool {
        self.enchantments.iter().any(|e| e.name == name)
    }

    /// Number of currently active enchantments.
    pub fn count(&self) -> usize {
        self.enchantments.len()
    }

    /// Iterates over the names of all active enchantments.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.enchantments.iter().map(|e| e.name.as_str())
    }
}

// ---------------------------------------------------------------------------
// Message cursors (internal)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub(crate) struct ApplyEnchantmentCursor(pub MessageCursor<ApplyEnchantmentMessage>);

#[derive(Resource, Default)]
pub(crate) struct RemoveEnchantmentCursor(pub MessageCursor<RemoveEnchantmentMessage>);

#[derive(Resource, Default)]
pub(crate) struct TriggerEnchantmentCursor(pub MessageCursor<TriggerEnchantmentMessage>);

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Processes pending [`ApplyEnchantmentMessage`]s, building rune systems and
/// inserting [`ActiveEnchantments`] onto target entities.
///
/// Silently ignores messages whose target lacks the [`Enchantable`] component or
/// whose [`Spell`] asset is not yet loaded.
pub(crate) fn apply_enchantments(world: &mut World) {
    let mut cursor = world
        .remove_resource::<ApplyEnchantmentCursor>()
        .unwrap_or_default();

    let messages: Vec<ApplyEnchantmentMessage> = {
        let messages_res = world.resource::<Messages<ApplyEnchantmentMessage>>();
        cursor.0.read(messages_res).cloned().collect()
    };

    world.insert_resource(cursor);

    for msg in messages {
        let target = msg.target;
        let enchantment = Arc::clone(&msg.enchantment);

        // Only process entities that opted in.
        let target_entity = match world.get_entity(target) {
            Ok(ent) => ent,
            Err(_) => continue,
        };

        if target_entity.get::<Enchantable>().is_none() {
            warn_once!(
                "ApplyEnchantmentMessage: entity {:?} does not have the Enchantable component — ignoring.",
                target
            );
            continue;
        }

        // Build boxed systems from the enchantment source.  The result is a Vec
        // of (system, delay_secs, repeating) tuples, all with 'static lifetimes so
        // no borrow from Assets<Spell> escapes this block.
        let opt_systems: Option<Vec<(BoxedSystem<In<CastContext>, ()>, f32, bool)>> =
            match &enchantment.source {
                EnchantmentSource::Runes(runes) => Some(
                    runes
                        .iter()
                        .map(|r| (r.build(), r.delay().as_secs_f32(), !r.interval().is_zero()))
                        .collect(),
                ),
                EnchantmentSource::Spell(spell_handle) => {
                    let assets = world.resource::<Assets<Spell>>();
                    match assets.get(spell_handle) {
                        None => {
                            warn_once!(
                                "ApplyEnchantmentMessage: spell asset {:?} is not loaded — ignoring enchantment '{}'.",
                                spell_handle, enchantment.name
                            );
                            None
                        }
                        Some(spell) => Some(
                            spell
                                .runes
                                .iter()
                                .map(|r| {
                                    (r.build(), r.delay().as_secs_f32(), !r.interval().is_zero())
                                })
                                .collect(),
                        ),
                    }
                    // `assets` borrow released here.
                }
            };

        let boxed_systems = match opt_systems {
            None => continue,
            Some(v) => v,
        };

        // Register each system and build execution entries.
        let mut rune_executions = Vec::with_capacity(boxed_systems.len());
        for (system, delay, repeating) in boxed_systems {
            let system_id: SystemId<In<CastContext>> = world.register_boxed_system(system);
            let timer = Timer::from_seconds(
                delay,
                if repeating {
                    TimerMode::Repeating
                } else {
                    TimerMode::Once
                },
            );
            rune_executions.push(ActiveEnchantRune {
                system_id,
                timer,
                repeating,
            });
        }

        let entry = ActiveEnchantmentEntry {
            name: enchantment.name.clone(),
            description: enchantment.description.clone(),
            applier: enchantment.applier,
            trigger: enchantment.trigger.clone(),
            rune_executions,
        };

        // Insert or update the component on the target.
        if let Ok(mut entity_mut) = world.get_entity_mut(target) {
            if let Some(mut active) = entity_mut.get_mut::<ActiveEnchantments>() {
                active.enchantments.push(entry);
            } else {
                entity_mut.insert(ActiveEnchantments {
                    enchantments: vec![entry],
                });
            }
        }
    }
}

/// Processes pending [`RemoveEnchantmentMessage`]s, dropping matching enchantment
/// slots from the target entity's [`ActiveEnchantments`].
pub(crate) fn remove_enchantments(world: &mut World) {
    let mut cursor = world
        .remove_resource::<RemoveEnchantmentCursor>()
        .unwrap_or_default();

    let messages: Vec<RemoveEnchantmentMessage> = {
        let messages_res = world.resource::<Messages<RemoveEnchantmentMessage>>();
        cursor.0.read(messages_res).cloned().collect()
    };

    world.insert_resource(cursor);

    for msg in messages {
        if let Ok(mut entity_mut) = world.get_entity_mut(msg.target) {
            if let Some(mut active) = entity_mut.get_mut::<ActiveEnchantments>() {
                active.enchantments.retain(|e| e.name != msg.name);
            }
        }
    }
}

/// Ticks all **timed** enchantment rune timers and runs rune systems when they fire.
///
/// [`EnchantmentTrigger::OnDemand`] enchantments are skipped here — their runes
/// fire only via [`trigger_enchantments`].  Non-repeating runes are dropped after
/// firing.  Enchantment slots with no remaining rune executions are pruned, and
/// the [`ActiveEnchantments`] component is removed when it becomes empty.
pub(crate) fn tick_enchantments(world: &mut World) {
    let delta = world.resource::<Time>().delta();

    let mut systems_to_run: Vec<(SystemId<In<CastContext>>, CastContext)> = Vec::new();
    let mut entities_to_cleanup: Vec<Entity> = Vec::new();
    // (entity, spawn_origin, [(applier, system_id)]) — snapshotted before rune systems run
    let mut despawn_watchlist: Vec<(Entity, Vec3, Vec<(Entity, SystemId<In<CastContext>>)>)> =
        Vec::new();

    for (entity, mut active, maybe_tf) in world
        .query::<(Entity, &mut ActiveEnchantments, Option<&Transform>)>()
        .iter_mut(world)
    {
        // Snapshot OnDespawn rune info while the entity + transform are still live.
        let ondespawn_runes: Vec<(Entity, SystemId<In<CastContext>>)> = active
            .enchantments
            .iter()
            .filter(|e| matches!(e.trigger, EnchantmentTrigger::OnDespawn))
            .flat_map(|e| {
                let applier = e.applier;
                e.rune_executions.iter().map(move |r| (applier, r.system_id))
            })
            .collect();
        if !ondespawn_runes.is_empty() {
            let origin = maybe_tf.map(|tf| tf.translation).unwrap_or(Vec3::ZERO);
            despawn_watchlist.push((entity, origin, ondespawn_runes));
        }

        active.enchantments.retain_mut(|enchantment| {
            // Only Timed enchantments are driven by the timer path.
            // OnDemand, OnDespawn, OnCast, and OnTriggerArea fire through
            // their own dedicated paths and must never be auto-ticked here.
            if !matches!(enchantment.trigger, EnchantmentTrigger::Timed) {
                return true;
            }

            let applier = enchantment.applier;
            enchantment.rune_executions.retain_mut(|rune| {
                rune.timer.tick(delta);
                if rune.timer.just_finished() {
                    systems_to_run.push((
                        rune.system_id,
                        CastContext {
                            caster: applier,
                            targets: vec![entity],
                            origin: None,
                        },
                    ));
                    if rune.repeating {
                        rune.timer.reset();
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            });
            !enchantment.rune_executions.is_empty()
        });

        if active.enchantments.is_empty() {
            entities_to_cleanup.push(entity);
        }
    }

    for entity in entities_to_cleanup {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.remove::<ActiveEnchantments>();
        }
    }

    for (system_id, mut context) in systems_to_run {
        context.targets.retain(|&e| world.get_entity(e).is_ok());
        let _ = world.run_system_with(system_id, context);
    }

    // Fire OnDespawn runes for any entity that was just killed by the rune systems above.
    // The entity is gone from the world — use the pre-death origin snapshot in CastContext.
    for (entity, origin, rune_infos) in despawn_watchlist {
        if world.get_entity(entity).is_err() {
            for (caster, system_id) in rune_infos {
                let ctx = CastContext {
                    caster,
                    targets: vec![],
                    origin: Some(origin),
                };
                let _ = world.run_system_with(system_id, ctx);
            }
        }
    }
}

/// Processes pending [`TriggerEnchantmentMessage`]s, firing the runes of each
/// named [`EnchantmentTrigger::OnDemand`] enchantment once.
///
/// The enchantment persists after firing; send a [`RemoveEnchantmentMessage`]
/// to tear it down explicitly.
pub(crate) fn trigger_enchantments(world: &mut World) {
    let mut cursor = world
        .remove_resource::<TriggerEnchantmentCursor>()
        .unwrap_or_default();

    let messages: Vec<TriggerEnchantmentMessage> = {
        let messages_res = world.resource::<Messages<TriggerEnchantmentMessage>>();
        cursor.0.read(messages_res).cloned().collect()
    };

    world.insert_resource(cursor);

    let mut systems_to_run: Vec<(SystemId<In<CastContext>>, CastContext)> = Vec::new();

    for msg in messages {
        let source = msg.source;
        let Ok(mut entity_mut) = world.get_entity_mut(source) else {
            continue;
        };
        let Some(mut active) = entity_mut.get_mut::<ActiveEnchantments>() else {
            continue;
        };

        for enchantment in active.enchantments.iter_mut() {
            if enchantment.name != msg.name {
                continue;
            }
            if matches!(enchantment.trigger, EnchantmentTrigger::Timed) {
                warn_once!(
                    "TriggerEnchantmentMessage: enchantment '{}' on {:?} is Timed — use direct timed flow.",
                    msg.name, source
                );
                continue;
            }
            let applier = enchantment.applier;
            for rune in &enchantment.rune_executions {
                systems_to_run.push((
                    rune.system_id,
                    CastContext {
                        caster: applier,
                        targets: msg.targets.clone(),
                        origin: None,
                    },
                ));
            }
        }
    }

    for (system_id, mut context) in systems_to_run {
        context.targets.retain(|&e| world.get_entity(e).is_ok());
        let _ = world.run_system_with(system_id, context);
    }
}

/// Snapshot of one `OnDespawn` rune to be fired after the entity is gone.
pub(crate) struct PendingDespawnRune {
    pub system_id: SystemId<In<CastContext>>,
    pub caster: Entity,
    pub origin: Vec3,
}

/// Buffer populated by [`ondespawn_trigger_enchantments`] and drained by
/// [`flush_despawn_triggers`] later in the same frame.
#[derive(Resource, Default)]
pub(crate) struct PendingDespawnTriggers(pub Vec<PendingDespawnRune>);

/// Observer that fires when an entity with `OnDespawn` enchantments is
/// despawned.  Snapshots the rune system IDs and the entity's world position
/// into [`PendingDespawnTriggers`] so they can be executed after the entity
/// is fully removed.
pub(crate) fn ondespawn_trigger_enchantments(
    event: On<Despawn>,
    mut pending: ResMut<PendingDespawnTriggers>,
    query: Query<(Entity, &ActiveEnchantments, Option<&Transform>)>,
) {
    let Ok((_entity, active, maybe_tf)) = query.get(event.entity) else {
        return;
    };
    let origin = maybe_tf.map(|tf| tf.translation).unwrap_or(Vec3::ZERO);

    for enchantment in &active.enchantments {
        if !matches!(enchantment.trigger, EnchantmentTrigger::OnDespawn) {
            continue;
        }
        let caster = enchantment.applier;
        for rune in &enchantment.rune_executions {
            pending.0.push(PendingDespawnRune {
                system_id: rune.system_id,
                caster,
                origin,
            });
        }
    }
}

/// Drains [`PendingDespawnTriggers`] and runs each queued rune system.
///
/// Runs at the end of the `Update` chain — after the tick and trigger systems
/// — so it executes in the same frame as the despawn.
pub(crate) fn flush_despawn_triggers(world: &mut World) {
    let pending = world
        .remove_resource::<PendingDespawnTriggers>()
        .unwrap_or_default();
    world.insert_resource(PendingDespawnTriggers::default());

    for rune in pending.0 {
        let ctx = CastContext {
            caster: rune.caster,
            targets: vec![],
            origin: Some(rune.origin),
        };
        let _ = world.run_system_with(rune.system_id, ctx);
    }
}