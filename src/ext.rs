use bevy::prelude::*;

use crate::{CastSpellMessage, Spell};

pub trait CommandsExt {
    fn cast_magic(&mut self, caster: Entity, spell: Handle<Spell>, targets: Option<Vec<Entity>>) -> &mut Self;
}

impl CommandsExt for Commands<'_, '_> {
    fn cast_magic(&mut self, caster: Entity, spell: Handle<Spell>, targets: Option<Vec<Entity>>) -> &mut Self {
        self.write_message(CastSpellMessage{
            caster,
            targets: targets.unwrap_or_else(Vec::new),
            spell,
        })
    }
}