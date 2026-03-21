//! Enchanting system — attaches persistent magical effects to [`Enchantable`] entities.
//!
//! An [`Enchantment`] is a named, persistent effect whose magic comes from either
//! inline [`Rune`]s or a loaded [`Spell`] asset.  Any entity that bears the
//! [`Enchantable`] marker component can receive enchantments via
//! [`ApplyEnchantmentMessage`] and have them removed via [`RemoveEnchantmentMessage`].
//!
//! # Quick start
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use bevy::prelude::*;
//! use bevy_magic::{
//!     enchanting::{Enchantable, Enchantment},
//!     CommandsExt,
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
        }
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
        if world.entity(target).get::<Enchantable>().is_none() {
            warn!(
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
                            warn!(
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

/// Ticks all active enchantment rune timers and runs rune systems when they fire.
///
/// Non-repeating runes are dropped after firing. Enchantment slots with no
/// remaining rune executions are pruned. The [`ActiveEnchantments`] component
/// is removed when it becomes empty.
pub(crate) fn tick_enchantments(world: &mut World) {
    let delta = world.resource::<Time>().delta();

    let targets: Vec<Entity> = world
        .query_filtered::<Entity, With<ActiveEnchantments>>()
        .iter(world)
        .collect();

    let mut systems_to_run: Vec<(SystemId<In<CastContext>>, CastContext)> = Vec::new();

    for &target in &targets {
        if let Some(mut active) = world.entity_mut(target).get_mut::<ActiveEnchantments>() {
            active.enchantments.retain_mut(|enchantment| {
                let applier = enchantment.applier;
                enchantment.rune_executions.retain_mut(|rune| {
                    rune.timer.tick(delta);
                    if rune.timer.just_finished() {
                        systems_to_run.push((
                            rune.system_id,
                            CastContext {
                                caster: applier,
                                targets: vec![target],
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
        }
    }

    // Remove the component when all enchantments are exhausted.
    for target in targets {
        if let Some(active) = world.entity(target).get::<ActiveEnchantments>() {
            if active.enchantments.is_empty() {
                world.entity_mut(target).remove::<ActiveEnchantments>();
            }
        }
    }

    for (system_id, context) in systems_to_run {
        let _ = world.run_system_with(system_id, context);
    }
}
