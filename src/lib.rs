pub mod enchanting;

pub mod plugin;
pub mod runes;
pub mod spell;
pub mod spellbook;

pub use plugin::{CastSpellMessage, MagicPlugin, MagicSystems};

mod ext;
pub use ext::CommandsExt;
pub use spell::Spell;
pub use spellbook::Spellbook;

#[allow(unused)]
pub mod prelude {
    pub use crate::ext::*;
    pub use crate::{runes::Rune, runes::CastContext, runes::ActiveSpells, CastSpellMessage, CommandsExt, MagicPlugin, MagicSystems, Spell, Spellbook};
}