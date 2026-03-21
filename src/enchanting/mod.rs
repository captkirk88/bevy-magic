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
//! [`Enchantable`] to the item entity — no separate `Item` component is needed.
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
    runes::{BoxedRune, CastContext},
    spell::Spell,
};

pub mod prelude {
    pub use crate::enchanting::{
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
    OnDemand,
}

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
/// All runes on the named enchantment run once; the enchantment itself
/// persists.  Use [`RemoveEnchantmentMessage`] to remove it entirely.
///
/// # Example
/// ```rust,ignore
/// // Inside an "on-hit" observer or system:
/// commands.trigger_enchantment(sword_entity, "Flame Edge");
/// ```
#[derive(Message, Clone, Debug)]
pub struct TriggerEnchantmentMessage {
    /// Entity that carries the enchantment.
    pub target: Entity,
    /// Name of the enchantment to fire (case-sensitive).
    pub name: String,
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

    for (entity, mut active) in world.query::<(Entity, &mut ActiveEnchantments)>().iter_mut(world) {
        active.enchantments.retain_mut(|enchantment| {
            // OnDemand enchantments are never pruned by the timer path.
            if matches!(enchantment.trigger, EnchantmentTrigger::OnDemand) {
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
        let target = msg.target;
        let Ok(mut entity_mut) = world.get_entity_mut(target) else {
            continue;
        };
        let Some(mut active) = entity_mut.get_mut::<ActiveEnchantments>() else {
            continue;
        };

        for enchantment in active.enchantments.iter_mut() {
            if enchantment.name != msg.name {
                continue;
            }
            if !matches!(enchantment.trigger, EnchantmentTrigger::OnDemand) {
                warn_once!(
                    "TriggerEnchantmentMessage: enchantment '{}' on {:?} is not OnDemand — ignoring.",
                    msg.name, target
                );
                continue;
            }
            let applier = enchantment.applier;
            for rune in &enchantment.rune_executions {
                systems_to_run.push((
                    rune.system_id,
                    CastContext {
                        caster: applier,
                        targets: vec![target],
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
