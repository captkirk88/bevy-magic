//! [`Spellbook`] component — stores all [`Spell`] handles an entity can cast.

use bevy::prelude::*;

use crate::spell::Spell;

/// Component that holds every [`Spell`] an entity is capable of casting.
///
/// Attach this to player characters, NPCs, or any entity you want to give
/// casting ability.  Individual spells are referenced as asset [`Handle`]s so
/// the same spell asset can be shared across many entities with no duplication.
///
/// # Example
///
/// ```rust,ignore
/// fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
///     let fireball = asset_server.load("spells/fireball.spell");
///     commands.spawn((Player, Spellbook::new().with_spell(fireball)));
/// }
/// ```
#[derive(Component, Default, Debug)]
pub struct Spellbook {
    /// Ordered spell slots.  The index acts as the "slot number."
    pub spells: Vec<Handle<Spell>>,
}

impl Spellbook {
    /// Creates an empty spellbook.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style: add a spell handle before spawning.
    pub fn with_spell(mut self, handle: Handle<Spell>) -> Self {
        self.spells.push(handle);
        self
    }

    /// Appends `handle` to the next available slot.
    pub fn add_spell(&mut self, handle: Handle<Spell>) {
        self.spells.push(handle);
    }

    /// Removes the first slot whose handle matches `handle`.
    pub fn remove_spell(&mut self, handle: &Handle<Spell>) {
        self.spells.retain(|h| h != handle);
    }

    /// Returns `true` if any slot holds `handle`.
    pub fn contains(&self, handle: &Handle<Spell>) -> bool {
        self.spells.contains(handle)
    }

    /// Number of spells currently in the spellbook.
    pub fn len(&self) -> usize {
        self.spells.len()
    }

    /// Returns `true` when the spellbook has no spells.
    pub fn is_empty(&self) -> bool {
        self.spells.is_empty()
    }
}
