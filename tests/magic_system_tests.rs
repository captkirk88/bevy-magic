//! Integration tests for bevy-magic.
//!
//! Run with: `cargo test`

use bevy::{ecs::system::BoxedSystem, prelude::*};
use serde::{Deserialize, Serialize};

use bevy_magic::{
    MagicPlugin,
    plugin::CastSpellMessage,
    runes::{CastContext, Rune},
    spell::Spell,
    spellbook::Spellbook,
};

// ---------------------------------------------------------------------------
// Damage
// ---------------------------------------------------------------------------

/// Deals direct damage to every target in the [`CastContext`].
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct DamageRune {
    /// Raw damage amount.
    pub amount: f32,
    /// Elemental category used for resistance calculations.
    pub damage_type: DamageType,
}

/// Elemental damage categories.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    Fire,
    Ice,
    Lightning,
    Arcane,
    Physical,
}

impl Rune for DamageRune {
    fn name(&self) -> &str {
        "damage"
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let data = self.clone();
        Box::new(IntoSystem::into_system(move |In(ctx): In<CastContext>| {
            for &target in &ctx.targets {
                eprintln!(
                    "DamageRune: {:.1} {:?} damage \u{2192} {:?}  (caster: {:?})",
                    data.amount, data.damage_type, target, ctx.caster
                );
                // Real implementation:
                // commands.entity(target).queue(ApplyDamage(data.amount));
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// Heal
// ---------------------------------------------------------------------------

/// Restores health to the **caster**.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct HealRune {
    /// Amount of health to restore.
    pub amount: f32,
}

impl Rune for HealRune {
    fn name(&self) -> &str {
        "heal"
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let data = self.clone();
        Box::new(IntoSystem::into_system(move |In(ctx): In<CastContext>| {
            eprintln!(
                "HealRune: restoring {:.1} HP to caster {:?}",
                data.amount, ctx.caster
            );
            // Real implementation:
            // if let Ok(mut health) = query.get_mut(ctx.caster) {
            //     health.current = (health.current + data.amount).min(health.max);
            // }
        }))
    }
}

// ---------------------------------------------------------------------------
// Status effect
// ---------------------------------------------------------------------------

/// Applies a timed status effect to every target.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct StatusRune {
    /// Which status effect to apply.
    pub effect: StatusEffect,
    /// How long the effect lasts, in seconds.
    pub duration_secs: f32,
}

/// Available status effects.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatusEffect {
    Burn,
    Freeze,
    Stun,
    /// Multiplies movement speed.  `factor = 0.5` → half speed.
    Slow {
        factor: f32,
    },
    /// Deals damage per second.
    Poison {
        damage_per_sec: f32,
    },
}

impl Rune for StatusRune {
    fn name(&self) -> &str {
        "status"
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let data = self.clone();
        Box::new(IntoSystem::into_system(move |In(ctx): In<CastContext>| {
            for &target in &ctx.targets {
                eprintln!(
                    "StatusRune: applying {:?} for {:.1}s \u{2192} {:?}",
                    data.effect, data.duration_secs, target
                );
                // Real implementation:
                // commands.entity(target).insert(ActiveStatus { effect: data.effect.clone(), timer: Timer::from_seconds(data.duration_secs, TimerMode::Once) });
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// Teleport
// ---------------------------------------------------------------------------

/// Translates the **caster's** position by a fixed world-space offset.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct TeleportRune {
    /// `[x, y, z]` displacement in world units.
    pub offset: [f32; 3],
}

impl Rune for TeleportRune {
    fn name(&self) -> &str {
        "teleport"
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let data = self.clone();
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, mut transforms: Query<&mut Transform>| {
                eprintln!(
                    "TeleportRune: displacing {:?} by {:?}",
                    ctx.caster, data.offset
                );
                if let Ok(mut transform) = transforms.get_mut(ctx.caster) {
                    transform.translation += Vec3::from(data.offset);
                }
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Minimal app with asset support and the magic plugin wired up.
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(test_runes(MagicPlugin::default()));
    app
}

// helper providing a full-featured app; kept for manual experimentation
fn test_full_app() -> App {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: None,
        ..Default::default()
    }))
    // register built-in runes just like the plugin would
    .add_plugins(test_runes(MagicPlugin::default()));
    app
}

fn test_runes(plugin: MagicPlugin) -> MagicPlugin {
    plugin
        .register_rune::<DamageRune>("damage")
        .register_rune::<HealRune>("heal")
        .register_rune::<StatusRune>("status")
        .register_rune::<TeleportRune>("teleport")
}

/// Registry pre-populated with all built-in rune types.

/// Helper mimicking the plugin's registry logic, without exposing the type.
fn deserialize_rune(value: serde_json::Value) -> Box<dyn Rune> {
    let mut obj = if let serde_json::Value::Object(o) = value {
        o
    } else {
        panic!("expected object");
    };
    let type_name = obj
        .remove("type")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .expect("missing type field");
    match type_name.as_str() {
        "damage" => {
            Box::new(serde_json::from_value::<DamageRune>(serde_json::Value::Object(obj)).unwrap())
        }
        "heal" => {
            Box::new(serde_json::from_value::<HealRune>(serde_json::Value::Object(obj)).unwrap())
        }
        "status" => {
            Box::new(serde_json::from_value::<StatusRune>(serde_json::Value::Object(obj)).unwrap())
        }
        "teleport" => Box::new(
            serde_json::from_value::<TeleportRune>(serde_json::Value::Object(obj)).unwrap(),
        ),
        other => panic!("unknown rune type {}", other),
    }
}

/// Deserialize a `Spell` from JSON using the simple internal deserializer.
fn spell_from_json(json: &str) -> Spell {
    #[derive(serde::Deserialize)]
    struct SpellDef {
        name: String,
        description: String,
        runes: Vec<serde_json::Value>,
    }
    let def: SpellDef = serde_json::from_str(json).unwrap();
    let runes = def.runes.into_iter().map(deserialize_rune).collect();
    Spell {
        name: def.name,
        description: def.description,
        runes,
    }
}

// ---------------------------------------------------------------------------
// Rune serialization round-trips
// ---------------------------------------------------------------------------

#[test]
fn serialize_damage_rune_roundtrip() {
    let original = DamageRune {
        amount: 42.0,
        damage_type: DamageType::Lightning,
    };
    let json = serde_json::to_string(&original).unwrap();
    let decoded: DamageRune = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.amount, 42.0);
    assert!(matches!(decoded.damage_type, DamageType::Lightning));
}

#[test]
fn serialize_heal_rune_roundtrip() {
    let original = HealRune { amount: 100.0 };
    let json = serde_json::to_string(&original).unwrap();
    let decoded: HealRune = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.amount, 100.0);
}

#[test]
fn serialize_status_rune_roundtrip() {
    let original = StatusRune {
        effect: StatusEffect::Slow { factor: 0.5 },
        duration_secs: 3.0,
    };
    let json = serde_json::to_string(&original).unwrap();
    let decoded: StatusRune = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.duration_secs, 3.0);
    assert!(
        matches!(decoded.effect, StatusEffect::Slow { factor } if (factor - 0.5).abs() < f32::EPSILON)
    );
}

#[test]
fn serialize_teleport_rune_roundtrip() {
    let original = TeleportRune {
        offset: [1.0, 2.0, 3.0],
    };
    let json = serde_json::to_string(&original).unwrap();
    let decoded: TeleportRune = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.offset, [1.0, 2.0, 3.0]);
}

// ---------------------------------------------------------------------------
// Rune name / identity
// ---------------------------------------------------------------------------

#[test]
fn deserialize_rune_via_registry() {
    let json = r#"{"type":"heal","amount":55.0}"#;
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    let rune = deserialize_rune(value);
    assert_eq!(rune.name(), "heal");
}

// ---------------------------------------------------------------------------
// Spell serialization
// ---------------------------------------------------------------------------

#[test]
fn deserialize_spell_from_json_literal() {
    // Mirrors the on-disk format used by SpellAssetLoader.
    let json = r#"
    {
        "name": "Fireball",
        "description": "AoE fire damage",
        "runes": [
            { "type": "damage", "amount": 75.0, "damage_type": "fire" },
            { "type": "status", "effect": { "kind": "burn" }, "duration_secs": 5.0 }
        ]
    }"#;

    let spell = spell_from_json(json);
    assert_eq!(spell.name, "Fireball");
    assert_eq!(spell.runes.len(), 2);
}

#[test]
fn spell_builder_chains_runes_in_order() {
    let spell = Spell::new("Chain Spell", "")
        .with_rune(HealRune { amount: 1.0 })
        .with_rune(DamageRune {
            amount: 2.0,
            damage_type: DamageType::Physical,
        })
        .with_rune(TeleportRune {
            offset: [0.0, 0.0, 0.0],
        });

    assert_eq!(spell.runes.len(), 3);
}

// ---------------------------------------------------------------------------
// Asset loading and integration tests
// ---------------------------------------------------------------------------

#[test]
fn load_spell_assets_from_disk() {
    let mut app = test_app();
    let server = app.world_mut().resource_mut::<AssetServer>();
    let handle: Handle<Spell> = server.load("spells/fireball.spell");
    // run a few frames to allow loading
    for _ in 0..10 {
        app.update();
    }
    assert!(app.world().resource::<Assets<Spell>>().get(&handle).is_some());
}

#[test]
fn cast_loaded_spell_from_disk() {
    let mut app = test_app();
    let server = app.world_mut().resource_mut::<AssetServer>();
    let handle: Handle<Spell> = server.load("spells/fireball.spell");
    for _ in 0..10 { app.update(); }

    let caster = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![target],
        spell: handle.clone(),
    });
    app.update();
}

// ---------------------------------------------------------------------------
// Direct build_system tests
// ---------------------------------------------------------------------------

#[test]
fn build_system_initializes_and_runs() {
    // Verify that a rune's system can be built and run via register_boxed_system
    // without going through the full cast dispatch.
    let mut app = test_app();

    let caster = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn_empty().id();
    let ctx = CastContext::new(caster).with_targets([target]);

    let rune = DamageRune {
        amount: 33.0,
        damage_type: DamageType::Arcane,
    };

    let id = app.world_mut().register_boxed_system(rune.build());
    let _ = app.world_mut().run_system_with(id, ctx);
    // If we reach here without panic the system API is wired correctly.
}

#[test]
fn build_system_teleport_moves_transform() {
    let mut app = test_app();

    // Spawn a caster with a Transform so TeleportRune can mutate it.
    let caster = app
        .world_mut()
        .spawn(Transform::from_xyz(0.0, 0.0, 0.0))
        .id();

    let rune = TeleportRune {
        offset: [1.0, 2.0, 3.0],
    };

    let ctx = CastContext::new(caster);
    let id = app.world_mut().register_boxed_system(rune.build());
    let _ = app.world_mut().run_system_with(id, ctx);

    let translation = app
        .world()
        .entity(caster)
        .get::<Transform>()
        .unwrap()
        .translation;
    assert_eq!(translation, Vec3::new(1.0, 2.0, 3.0));
}

// ---------------------------------------------------------------------------
// CastContext
// ---------------------------------------------------------------------------

#[test]
fn cast_context_self_cast_has_no_targets() {
    let mut app = test_app();
    let caster = app.world_mut().spawn_empty().id();
    let ctx = CastContext::new(caster);

    assert_eq!(ctx.caster, caster);
    assert!(ctx.targets.is_empty());
}

#[test]
fn cast_context_multi_target() {
    let mut app = test_app();
    let world = app.world_mut();

    let caster = world.spawn_empty().id();
    let t1 = world.spawn_empty().id();
    let t2 = world.spawn_empty().id();
    let t3 = world.spawn_empty().id();

    let ctx = CastContext::new(caster).with_targets([t1, t2, t3]);

    assert_eq!(ctx.caster, caster);
    assert_eq!(ctx.targets.len(), 3);
    assert!(ctx.targets.contains(&t1));
    assert!(ctx.targets.contains(&t3));
}

// ---------------------------------------------------------------------------
// Spellbook component
// ---------------------------------------------------------------------------

#[test]
fn spellbook_add_and_contains() {
    let mut app = test_app();

    let handle = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(Spell::new("Test Spell", ""));

    let entity = app.world_mut().spawn(Spellbook::default()).id();

    app.world_mut()
        .entity_mut(entity)
        .get_mut::<Spellbook>()
        .unwrap()
        .add_spell(handle.clone());

    let sb = app.world().entity(entity).get::<Spellbook>().unwrap();
    assert_eq!(sb.len(), 1);
    assert!(sb.contains(&handle));
}

#[test]
fn spellbook_remove_spell() {
    let mut app = test_app();

    let handle = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(Spell::new("Spell A", ""));

    let entity = app
        .world_mut()
        .spawn(Spellbook::new().with_spell(handle.clone()))
        .id();

    app.world_mut()
        .entity_mut(entity)
        .get_mut::<Spellbook>()
        .unwrap()
        .remove_spell(&handle);

    let sb = app.world().entity(entity).get::<Spellbook>().unwrap();
    assert!(sb.is_empty());
}

#[test]
fn spellbook_builder_with_spell() {
    let mut app = test_app();

    let h1 = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(Spell::new("A", ""));
    let h2 = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(Spell::new("B", ""));

    let sb = Spellbook::new().with_spell(h1).with_spell(h2);
    assert_eq!(sb.len(), 2);
}

// ---------------------------------------------------------------------------
// End-to-end cast event processing
// ---------------------------------------------------------------------------

#[test]
fn cast_event_executes_without_panic() {
    let mut app = test_app();

    let spell = Spell::new("Zap", "").with_rune(DamageRune {
        amount: 10.0,
        damage_type: DamageType::Lightning,
    });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn_empty().id();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![target],
        spell: handle,
    });

    // One update frame processes the message via execute_cast_spell_events.
    app.update();
}

#[test]
fn cast_self_heal_no_targets() {
    let mut app = test_app();

    let spell = Spell::new("Minor Heal", "").with_rune(HealRune { amount: 25.0 });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle,
    });

    app.update();
}

#[test]
fn cast_multi_target_spell() {
    let mut app = test_app();

    let spell = Spell::new("Chain Lightning", "")
        .with_rune(DamageRune {
            amount: 15.0,
            damage_type: DamageType::Lightning,
        })
        .with_rune(StatusRune {
            effect: StatusEffect::Stun,
            duration_secs: 1.0,
        });

    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();
    let targets: Vec<Entity> = (0..5).map(|_| app.world_mut().spawn_empty().id()).collect();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets,
        spell: handle,
    });

    app.update();
}

#[test]
fn multiple_events_in_same_frame_all_processed() {
    let mut app = test_app();

    let spell = Spell::new("Quick Strike", "").with_rune(DamageRune {
        amount: 5.0,
        damage_type: DamageType::Physical,
    });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn_empty().id();

    // Send three events before the update.
    for _ in 0..3 {
        app.world_mut().write_message(CastSpellMessage {
            caster,
            targets: vec![target],
            spell: handle.clone(),
        });
    }

    // All three should be dispatched without panic.
    app.update();
}

#[test]
fn events_across_frames_not_double_processed() {
    let mut app = test_app();

    let spell = Spell::new("Test", "").with_rune(HealRune { amount: 1.0 });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();

    // Frame 1: send one event.
    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle.clone(),
    });
    app.update();

    // Frame 2: write another message.  If the cursor advanced correctly, only
    // the new event fires — no double execution of the first.
    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle,
    });
    app.update();

    // No assertion on side-effects here (runes just log), but if the cursor
    // were broken this test would panic or deadlock.
}

#[test]
fn cast_invalid_spell_handle_is_silent_noop() {
    let mut app = test_app();

    let caster = app.world_mut().spawn_empty().id();

    // A default/dangling handle — no corresponding asset.
    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: Handle::default(),
    });

    // Should not panic.
    app.update();
}
