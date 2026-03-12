//! [`Spell`] asset type and its [`AssetLoader`].


use bevy::{
    asset::{io::Reader, AssetLoader, LoadContext},
    prelude::*,
};
use serde::de::Error;
use thiserror::Error;

use crate::runes::{RuneDeserializeError, RuneRegistry, Rune};

// ---------------------------------------------------------------------------
// Spell
// ---------------------------------------------------------------------------

/// A spell asset, composed of an ordered list of [`Rune`]s.
///
/// Spells are loaded from `.spell` files (RON format) via [`SpellAssetLoader`], or
/// constructed programmatically with [`Spell::new`] / [`Spell::with_rune`].
///
/// # RON format
///
/// Each rune object carries a `"type"` discriminant followed by the rune's own fields.
///
/// ```ron
/// (
///   name: "Fireball",
///   description: "Hurls a ball of fire.",
///   runes: [
///     (type: "damage", amount: 75.0, damage_type: fire),
///     (type: "status", effect: (kind: burn), duration_secs: 5.0),
///   ],
/// )
/// ```
#[derive(Asset, TypePath)]
pub struct Spell {
    /// Human-readable name shown in UI.
    pub name: String,
    /// Flavour / tooltip text.
    pub description: String,
    /// Runes executed **in order** each time this spell is cast.
    pub runes: Vec<Box<dyn Rune>>,
}

impl Spell {
    /// Creates an empty spell.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            runes: Vec::new(),
        }
    }

    /// Builder-style method: appends `rune` to the rune list.
    pub fn with_rune(mut self, rune: impl Rune + 'static) -> Self {
        self.runes.push(Box::new(rune));
        self
    }
}

// ---------------------------------------------------------------------------
// AssetLoader
// ---------------------------------------------------------------------------

/// Errors produced while loading a `.spell` asset (RON).
#[derive(Error, Debug)]
pub enum SpellLoadError {
    #[error("I/O error reading spell asset: {0}")]
    Io(#[from] std::io::Error),
    #[error("RON parse error in spell asset: {0}")]
    Ron(#[from] ron::Error),
    #[error("rune deserialization error: {0}")]
    Rune(#[from] RuneDeserializeError),
}

/// Loads [`Spell`] assets from `.spell.json` files.
///
/// Registered automatically by [`crate::plugin::MagicPlugin`].
/// Uses the [`RuneRegistry`] provided at construction to deserialize runes.
#[derive(TypePath)]
pub struct SpellAssetLoader {
    pub(crate) registry: RuneRegistry,
}

impl AssetLoader for SpellAssetLoader {
    type Asset = Spell;
    type Settings = ();
    type Error = SpellLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        spell_from_ron(&bytes, &self.registry)
    }

    fn extensions(&self) -> &[&str] {
        &["spell"]
    }
}

fn spell_from_ron(bytes: &[u8], registry: &RuneRegistry) -> Result<Spell, SpellLoadError> {
    // ron::de::from_bytes returns a `SpannedError`, convert to general `ron::Error`
    let value: ron::value::Value = ron::de::from_bytes(bytes).map_err(|e| SpellLoadError::Ron(e.into()))?;
    let obj = if let ron::value::Value::Map(m) = value {
        m
    } else {
        return Err(SpellLoadError::Ron(ron::Error::custom(
            "expected top-level RON map",
        )));
    };
    // helper to fetch a string field from the map
    let get_str = |key: &str| {
        obj.get(&ron::value::Value::String(key.to_string()))
            .and_then(|v| if let ron::value::Value::String(s) = v { Some(s) } else { None })
    };
    let name = get_str("name").ok_or_else(|| {
        SpellLoadError::Ron(ron::Error::custom("missing or invalid 'name' field"))
    })?;
    let description = get_str("description").ok_or_else(|| {
        SpellLoadError::Ron(ron::Error::custom("missing or invalid 'description' field"))
    })?;
    let runes_value = obj
        .get(&ron::value::Value::String("runes".to_string()))
        .ok_or_else(|| SpellLoadError::Ron(ron::Error::custom("missing 'runes' field")))?;
    let runes_array = if let ron::value::Value::Seq(s) = runes_value {
        s
    } else {
        return Err(SpellLoadError::Ron(ron::Error::custom(
            "invalid 'runes' field (expected sequence)",
        )));
    };
    let mut runes = Vec::new();
    for rune_value in runes_array {
        let rune = registry.deserialize_rune(rune_value.clone())?;
        runes.push(rune);
    }
    Ok(Spell {
        name: name.to_string(),
        description: description.to_string(),
        runes,
    })
}
