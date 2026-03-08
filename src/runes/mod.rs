use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bevy::ecs::system::BoxedSystem;
use bevy::prelude::*;
use serde::de::Error;

/// Context passed to each [`Rune`] when a spell is executed.
///
/// Contains the caster and an arbitrary number of targets, supporting
/// single-target, multi-target, and self-cast spells uniformly.
#[derive(Clone, Debug)]
pub struct CastContext {
    /// The entity that is casting the spell.
    pub caster: Entity,
    /// Every entity targeted by this cast.  May be empty for self-cast spells.
    pub targets: Vec<Entity>,
}

impl CastContext {
    /// Creates a self-cast context with no targets.
    pub fn new(caster: Entity) -> Self {
        Self {
            caster,
            targets: Vec::new(),
        }
    }

    /// Builder-style: attach targets to the context.
    pub fn with_targets(mut self, targets: impl IntoIterator<Item = Entity>) -> Self {
        self.targets = targets.into_iter().collect();
        self
    }
}

// ---------------------------------------------------------------------------
// RuneRegistry
// ---------------------------------------------------------------------------

type RuneDeserFn = fn(serde_json::Value) -> Result<Box<dyn Rune>, serde_json::Error>;

#[derive(Default)]
struct RuneRegistryInner {
    deserializers: HashMap<String, RuneDeserFn>,
}

impl RuneRegistryInner {
    fn register<R>(&mut self, name: &str)
    where
        R: Rune + for<'de> serde::Deserialize<'de>,
    {
        fn deser<R: Rune + for<'de> serde::Deserialize<'de>>(
            v: serde_json::Value,
        ) -> Result<Box<dyn Rune>, serde_json::Error> {
            serde_json::from_value::<R>(v).map(|r| Box::new(r) as Box<dyn Rune>)
        }
        self.deserializers.insert(name.to_string(), deser::<R>);
    }

    fn deserialize_rune(&self, mut value: serde_json::Value) -> Result<Box<dyn Rune>, RuneDeserializeError> {
        let type_name = value
            .as_object_mut()
            .and_then(|obj| obj.remove("type"))
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s),
                _ => None,
            })
            .ok_or(RuneDeserializeError::MissingType)?;

        let deser_fn = self
            .deserializers
            .get(&type_name)
            .ok_or_else(|| RuneDeserializeError::UnknownType(type_name))?;

        deser_fn(value).map_err(RuneDeserializeError::Json)
    }
}

/// A shared, thread-safe registry mapping rune type names to their deserializers.
///
/// [`crate::plugin::MagicPlugin`] inserts this as a Bevy [`Resource`].
/// The plugin automatically registers all built-in rune types during setup;
/// additional custom runes may be added by calling [`RuneRegistry::register`]
/// before their spells are loaded.
///
/// # Registering custom runes
///
/// ```rust,ignore
/// // In a startup system or plugin build:
/// fn register_custom(registry: Res<RuneRegistry>) {
///     registry.register::<KnockbackRune>("knockback");
/// }
/// ```
#[derive(Resource, Clone, Default)]
pub(crate) struct RuneRegistry(Arc<RwLock<RuneRegistryInner>>);

impl RuneRegistry {
    /// Register a concrete rune type so it can be deserialized from JSON.
    ///
    /// `name` must match the string returned by [`Rune::name`] for that type.
    pub fn register<R>(&self, name: &str)
    where
        R: Rune + for<'de> serde::Deserialize<'de>,
    {
        self.0.write().unwrap().register::<R>(name);
    }


    /// Deserialize a single rune from a JSON object that must include a `"type"` field.
    ///
    /// The `"type"` field is consumed and used to look up the registered deserializer.
    pub fn deserialize_rune(&self, value: serde_json::Value) -> Result<Box<dyn Rune>, RuneDeserializeError> {
        match self.0.read() {
            Ok(registry) => registry.deserialize_rune(value),
            Err(_) => Err(RuneDeserializeError::Json(serde_json::Error::custom(
                "rune registry lock poisoned",
            ))),
        }
    }
}

/// Errors produced while deserializing a [`Rune`] from JSON.
#[derive(Debug, thiserror::Error)]
pub enum RuneDeserializeError {
    #[error("rune JSON object is missing the required \"type\" field")]
    MissingType,
    #[error("unknown rune type \"{0}\" — was it registered with RuneRegistry?")]
    UnknownType(String),
    #[error("JSON error deserializing rune: {0}")]
    Json(serde_json::Error),
}

// ---------------------------------------------------------------------------
// Rune trait
// ---------------------------------------------------------------------------

/// The atomic unit of a magic spell.
///
/// A [`crate::spell::Spell`] is composed of an ordered sequence of [`Rune`]s.
/// Each rune's effect is a standard Bevy one-shot system that receives the
/// [`CastContext`] via [`In<CastContext>`] and may freely declare [`Query`],
/// [`Res`], [`Commands`], and any other system params.
///
/// # Implementing a custom rune
///
/// 1. Derive `serde::Deserialize` on your struct so it can be loaded from JSON.
/// 2. Implement the two required trait methods.
/// 3. Register the type before spell assets are loaded:
///    `registry.register::<MyRune>("my_rune")`.
///
/// ```rust,ignore
/// use serde::Deserialize;
/// use bevy::prelude::*;
/// use bevy::ecs::system::BoxedSystem;
/// use bevy_magic::runes::{CastContext, Rune};
///
/// #[derive(Clone, Deserialize)]
/// pub struct KnockbackRune { pub force: f32 }
///
/// impl Rune for KnockbackRune {
///     fn name(&self) -> &str { "knockback" }
///
///     fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
///         let data = self.clone();
///         Box::new(IntoSystem::into_system(
///             move |In(ctx): In<CastContext>| {
///                 for &target in &ctx.targets {
///                     info!("knockback {} → {:?}", data.force, target);
///                 }
///             },
///         ))
///     }
/// }
/// ```
///
/// # Serialization format
///
/// Runes are deserialized from a JSON object with a `"type"` discriminant key:
///
/// ```json
/// { "type": "damage", "amount": 50.0, "damage_type": "fire" }
/// ```
pub trait Rune: Send + Sync + 'static {
    /// The unique type name used to identify this rune in serialized spells.
    ///
    /// Must match the string passed to [`RuneRegistry::register`] for this type.
    fn name(&self) -> &str;

    /// Build a one-shot Bevy system that applies this rune's effect.
    ///
    /// Called at most once per (spell, rune-index) pair after load.  The
    /// returned system is registered and its [`SystemId`] is cached in
    /// [`crate::plugin::RuneSystemCache`].
    fn build(&self) -> BoxedSystem<In<CastContext>, ()>;
}

