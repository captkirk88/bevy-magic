use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bevy::ecs::system::{BoxedSystem, SystemId};
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

    pub fn with_target(mut self, target: Entity) -> Self {
        self.targets.push(target);
        self
    }
}

// ---------------------------------------------------------------------------
// RuneRegistry
// ---------------------------------------------------------------------------


type RuneDeserializationFn = fn(ron::value::Value) -> Result<Box<dyn Rune>, ron::Error>;

#[derive(Default)]
struct RuneRegistryInner {
    deserializers: HashMap<String, RuneDeserializationFn>,
}

impl RuneRegistryInner {
    fn register<R>(&mut self, name: &str)
    where
        R: Rune + for<'de> serde::Deserialize<'de>,
    {
        fn deser<R: Rune + for<'de> serde::Deserialize<'de>>(
            v: ron::value::Value,
        ) -> Result<Box<dyn Rune>, ron::Error> {
            // `ron::value::Value` implements `Deserializer`, so we can hand it directly
            // to the type we want to build.  the error type is already `ron::Error`.
            let r: R = serde::Deserialize::deserialize(v)?;
            Ok(Box::new(r) as Box<dyn Rune>)
        }
        self.deserializers.insert(name.to_string(), deser::<R>);
    }

    fn deserialize_rune(&self, mut value: ron::value::Value) -> Result<BoxedRune, RuneDeserializeError> {
        // extract and consume the "type" field from the map
        let type_name = if let ron::value::Value::Map(ref mut map) = value {
            let key = ron::value::Value::String("type".to_string());
            match map.remove(&key) {
                Some(ron::value::Value::String(s)) => s,
                _ => return Err(RuneDeserializeError::MissingType(format!("{:?}", value))),
            }
        } else {
            return Err(RuneDeserializeError::MissingType(format!("{:?}", value)));
        };

        let deser_fn = self
            .deserializers
            .get(&type_name)
            .ok_or_else(|| RuneDeserializeError::UnknownType(type_name.clone()))?;

        deser_fn(value).map_err(RuneDeserializeError::Ron)
    }
}


#[derive(Resource, Clone, Default)]
pub(crate) struct RuneRegistry(Arc<RwLock<RuneRegistryInner>>);

impl RuneRegistry {
    /// Register a concrete rune type so it can be deserialized from RON.
    ///
    /// `name` must match the string returned by [`Rune::name`] for that type.
    ///
    /// The underlying representation is RON rather than JSON, but the procedure is
    /// otherwise the same.
    pub fn register<R: TypePath>(&self)
    where
        R: Rune + for<'de> serde::Deserialize<'de>,
    {
        let mut name = R::short_type_path().to_string();
        if name.ends_with("Rune") {
            name.truncate(name.len() - 4);
        }
        self.0.write().unwrap().register::<R>(&name.to_lowercase());
    }


    /// Deserialize a single rune from a RON value that must include a `"type"` field.
    ///
    /// The `"type"` field is consumed and used to look up the registered deserializer.
    pub fn deserialize_rune(&self, value: ron::value::Value) -> Result<BoxedRune, RuneDeserializeError> {
        match self.0.read() {
            Ok(registry) => registry.deserialize_rune(value),
            Err(_) => Err(RuneDeserializeError::Ron(ron::Error::custom(
                "rune registry lock poisoned",
            ))),
        }
    }
}

/// Errors produced while deserializing a [`Rune`] from RON.
#[derive(Debug, thiserror::Error)]
pub enum RuneDeserializeError {
    #[error("rune RON object is missing the required \"type\" field: {0}")]
    MissingType(String),
    #[error("unknown rune type \"{0}\" — was it registered with RuneRegistry?")]
    UnknownType(String),
    #[error("RON error deserializing rune: {0}")]
    Ron(ron::Error),
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
/// 1. Derive `serde::Deserialize` on your struct so it can be loaded from RON.
/// 2. Implement the two required trait methods.
/// 3. Register the type before spell assets are loaded:
///    `registry.register::<MyRune>()`.
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
///     fn name() -> &'static str { "knockback" }
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
/// Runes are deserialized from a RON map with a `"type"` discriminant key.
///
/// ```ron
/// (type: "damage", amount: 50.0, damage_type: fire)
/// ```
pub trait Rune:  Send + Sync + 'static {
    /// Build a one-shot Bevy system that applies this rune's effect.
    ///
    /// Called at most once per (spell, rune-index) pair after load.  The
    /// returned system is registered and its [`SystemId`] is cached in
    /// [`crate::plugin::RuneSystemCache`].
    fn build(&self) -> BoxedSystem<In<CastContext>, ()>;

    /// How long to wait before the first invocation (default = 0).
    fn delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    /// If non-zero, the rune will repeat at this interval (default = 0).
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}

pub type BoxedRune = Box<dyn Rune>;

// ---------------------------------------------------------------------------
// Active spell execution tracking
// ---------------------------------------------------------------------------

/// Tracks in-flight spell/rune executions on a caster entity.
#[derive(Component)]
pub struct ActiveSpells {
    pub(crate) spells: Vec<SpellExecution>,
}

pub(crate) struct SpellExecution {
    pub ctx: CastContext,
    pub runes: Vec<PendingRune>,
}

pub(crate) struct PendingRune {
    pub system: SystemId<In<CastContext>>,
    pub timer: Timer,
    pub repeating: bool,
}

impl ActiveSpells {
    pub fn new() -> Self {
        Self {
            spells: Vec::new(),
        }
    }

    /// Returns the number of active spell executions.
    pub fn spell_count(&self) -> usize {
        self.spells.len()
    }

    pub(crate) fn add_spell(
        &mut self,
        ctx: CastContext,
        rune_systems: Vec<(SystemId<In<CastContext>>, Timer, bool)>,
    ) {
        self.spells.push(SpellExecution {
            ctx,
            runes: rune_systems
                .into_iter()
                .map(|(sys, timer, repeating)| PendingRune {
                    system: sys,
                    timer,
                    repeating,
                })
                .collect(),
        });
    }
}

impl Default for ActiveSpells {
    fn default() -> Self {
        Self::new()
    }
}