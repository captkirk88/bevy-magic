use std::sync::Arc;

use bevy::prelude::*;

use crate::{
    enchanting::{ApplyEnchantmentMessage, Enchantment, RemoveEnchantmentMessage, TriggerEnchantmentMessage},
    CastSpellMessage, Spell,
};

pub trait CommandsExt {
    /// Cast a spell from `caster` at optional `targets`.
    fn cast_magic(
        &mut self,
        caster: Entity,
        spell: Handle<Spell>,
        targets: Option<Vec<Entity>>,
    ) -> &mut Self;

    /// Apply `enchantment` to `target`. The target must have the [`crate::enchanting::Enchantable`] component.
    fn apply_enchantment(&mut self, target: Entity, enchantment: Enchantment) -> &mut Self;

    /// Remove the enchantment named `name` from `target`.
    fn remove_enchantment(&mut self, target: Entity, name: impl Into<String>) -> &mut Self;

    /// Manually fire the runes of an [`crate::enchanting::EnchantmentTrigger::OnDemand`]
    /// enchantment named `name` on `target`.
    fn trigger_enchantment(&mut self, target: Entity, name: impl Into<String>) -> &mut Self;
}

impl CommandsExt for Commands<'_, '_> {
    fn cast_magic(
        &mut self,
        caster: Entity,
        spell: Handle<Spell>,
        targets: Option<Vec<Entity>>,
    ) -> &mut Self {
        self.write_message(CastSpellMessage {
            caster,
            targets: targets.unwrap_or_else(Vec::new),
            spell,
        })
    }

    fn apply_enchantment(&mut self, target: Entity, enchantment: Enchantment) -> &mut Self {
        self.write_message(ApplyEnchantmentMessage {
            target,
            enchantment: Arc::new(enchantment),
        })
    }

    fn remove_enchantment(&mut self, target: Entity, name: impl Into<String>) -> &mut Self {
        self.write_message(RemoveEnchantmentMessage {
            target,
            name: name.into(),
        })
    }

    fn trigger_enchantment(&mut self, target: Entity, name: impl Into<String>) -> &mut Self {
        self.write_message(TriggerEnchantmentMessage {
            target,
            name: name.into(),
        })
    }
}