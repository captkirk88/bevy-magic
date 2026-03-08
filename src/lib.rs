pub mod plugin;
pub mod runes;
pub mod spell;
pub mod spellbook;

pub use plugin::{CastSpellMessage, MagicPlugin, RuneSystemCache};

pub use spell::Spell;
pub use spellbook::Spellbook;