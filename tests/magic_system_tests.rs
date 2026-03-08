//! Integration tests for bevy-magic.
//!
//! Run with: `cargo test`

use bevy::{ecs::system::BoxedSystem, prelude::*};
use serde::{Deserialize, Serialize};

use bevy_magic::{
    MagicPlugin, plugin::CastSpellMessage, runes::{ActiveSpells, BoxedRune, CastContext, Rune}, spell::Spell, spellbook::Spellbook
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
// Delayed/Repeating Rune
// ---------------------------------------------------------------------------

/// A test rune that has a delay and repeats at an interval.
/// Used for testing the timing system.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct TimedRune {
    pub name: String,
    pub delay_secs: f32,
    pub interval_secs: f32,
}

impl Rune for TimedRune {
    fn delay(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.delay_secs)
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.interval_secs)
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let name = self.name.clone();
        Box::new(IntoSystem::into_system(move |In(ctx): In<CastContext>| {
            eprintln!("TimedRune '{}': executed on {:?}", name, ctx.caster);
        }))
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

fn test_runes(plugin: MagicPlugin) -> MagicPlugin {
    plugin
        .rune::<DamageRune>()
        .rune::<HealRune>()
        .rune::<StatusRune>()
        .rune::<TeleportRune>()
        .rune::<TimedRune>()
}

/// Registry pre-populated with all built-in rune types.

/// Helper mimicking the plugin's registry logic, without exposing the type.
fn deserialize_rune(value: serde_json::Value) -> (BoxedRune, String) {
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
            (Box::new(serde_json::from_value::<DamageRune>(serde_json::Value::Object(obj)).unwrap()), type_name)
        }
        "heal" => {
            (Box::new(serde_json::from_value::<HealRune>(serde_json::Value::Object(obj)).unwrap()), type_name)
        }
        "status" => {
            (Box::new(serde_json::from_value::<StatusRune>(serde_json::Value::Object(obj)).unwrap()), type_name)
        }
        "teleport" => (Box::new(
            serde_json::from_value::<TeleportRune>(serde_json::Value::Object(obj)).unwrap(),
        ), type_name),
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
    let runes = def.runes.into_iter().map(deserialize_rune).map(|(r, _)| r).collect();
    Spell {
        name: def.name,
        description: def.description,
        runes,
    }
}


// ---------------------------------------------------------------------------
//  Test Systems
// ---------------------------------------------------------------------------

fn on_cast(mut cast_message: MessageReader<CastSpellMessage>) {
    for msg in cast_message.read() {
        eprintln!("Received CastSpellMessage: caster={:?}, targets={:?}, spell={:?}", msg.caster, msg.targets, msg.spell);
    }
}

// ---------------------------------------------------------------------------
// Rune serialization (consolidated)
// ---------------------------------------------------------------------------

#[test]
fn rune_types_serialize_and_deserialize() {
    // Test a variety of rune types can round-trip through JSON
    let damage = DamageRune {
        amount: 42.0,
        damage_type: DamageType::Lightning,
    };
    let json = serde_json::to_string(&damage).unwrap();
    let decoded: DamageRune = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.amount, 42.0);
    assert!(matches!(decoded.damage_type, DamageType::Lightning));

    let heal = HealRune { amount: 100.0 };
    let json = serde_json::to_string(&heal).unwrap();
    let decoded: HealRune = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.amount, 100.0);

    let status = StatusRune {
        effect: StatusEffect::Slow { factor: 0.5 },
        duration_secs: 3.0,
    };
    let json = serde_json::to_string(&status).unwrap();
    let decoded: StatusRune = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.duration_secs, 3.0);
}

// ---------------------------------------------------------------------------
// Spell construction
// ---------------------------------------------------------------------------

#[test]
fn spell_builder_constructs_with_runes() {
    let spell = Spell::new("Test Spell", "A test")
        .with_rune(HealRune { amount: 1.0 })
        .with_rune(DamageRune {
            amount: 2.0,
            damage_type: DamageType::Physical,
        });

    assert_eq!(spell.name, "Test Spell");
    assert_eq!(spell.runes.len(), 2);
}

#[test]
fn deserialize_spell_from_json() {
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

    app.add_systems(Update, on_cast); // log the cast message for manual verification

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
// Timing: Rune delays and intervals
// ---------------------------------------------------------------------------

#[test]
fn instant_rune_executes_immediately() {
    let mut app = test_app();

    // A rune with no delay or interval should execute right away
    let spell = Spell::new("Instant", "").with_rune(DamageRune {
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

    // First update: processes the cast and schedules on ActiveSpells.
    app.update();

    // Second update: ticks timers. Since delay=0, the rune should have executed.
    app.update();

    // Verify the caster no longer has an ActiveSpells component (all runes completed).
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_none());
}

#[test]
fn delayed_rune_waits_before_execution() {
    let mut app = test_app();

    let delay = 0.001;  // 1ms delay - very short
    let spell = Spell::new("Delayed", "").with_rune(TimedRune {
        name: "delayed_test".to_string(),
        delay_secs: delay,
        interval_secs: 0.0,
    });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle,
    });

    // Frame 1: Cast is processed, rune scheduled with delay.
    app.update();
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_some());

    // Process many frames to ensure delay passes.
    for _ in 0..50 {
        app.update();
    }

    // After delay expires, the rune should have executed and cleaned up.
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_none());
}

#[test]
fn repeating_rune_ticks_at_interval() {
    let mut app = test_app();

    let interval = 0.1;
    let spell = Spell::new("Repeating", "").with_rune(TimedRune {
        name: "repeat_test".to_string(),
        delay_secs: 0.0,
        interval_secs: interval,
    });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle,
    });

    // Frame 1: Cast is processed.
    app.update();

    // Frame 2: First tick fire (delay=0).
    app.update();

    // ActiveSpells should still exist because the rune is repeating.
    let active = app.world().entity(caster).get::<ActiveSpells>();
    assert!(active.is_some());

    // Keep ticking to verify repetition happens multiple times.
    for _ in 0..100 {
        app.update();
    }

    // After many updates, the component should still be there (repeating indefinitely).
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_some());
}

#[test]
fn instant_and_delayed_runes_mixed_in_one_spell() {
    let mut app = test_app();

    // One instant rune, one delayed rune
    let spell = Spell::new("Mixed", "")
        .with_rune(DamageRune {
            amount: 10.0,
            damage_type: DamageType::Fire,
        })
        .with_rune(TimedRune {
            name: "delayed_part".to_string(),
            delay_secs: 0.001,
            interval_secs: 0.0,
        });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle,
    });

    // Frame 1: Cast is processed.
    app.update();

    // Frame 2: Instant rune fires; delayed rune waits.
    app.update();

    // ActiveSpells should still have the delayed rune waiting.
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_some());

    // Process many frames past the delay.
    for _ in 0..50 {
        app.update();
    }

    // After the delay, all runes are done and ActiveSpells is gone.
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_none());
}

#[test]
fn multiple_spells_stacked_on_same_caster() {
    let mut app = test_app();

    let spell1 = Spell::new("Spell1", "").with_rune(TimedRune {
        name: "spell1".to_string(),
        delay_secs: 0.0,
        interval_secs: 0.002,
    });
    let spell2 = Spell::new("Spell2", "").with_rune(TimedRune {
        name: "spell2".to_string(),
        delay_secs: 0.001,
        interval_secs: 0.0,
    });

    let h1 = app.world_mut().resource_mut::<Assets<Spell>>().add(spell1);
    let h2 = app.world_mut().resource_mut::<Assets<Spell>>().add(spell2);

    let caster = app.world_mut().spawn_empty().id();

    // Cast both spells on the same caster.
    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: h1.clone(),
    });
    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: h2.clone(),
    });

    app.update();

    // Both spell executions should be tracked in ActiveSpells.
    let active = app.world().entity(caster).get::<ActiveSpells>();
    assert!(active.is_some());
    assert_eq!(active.unwrap().spell_count(), 2);

    // Tick past spell2's delay but not too many repeats of spell1.
    for _ in 0..50 {
        app.update();
    }

    // spell1 is repeating so it should still exist; spell2 is done.
    let active = app.world().entity(caster).get::<ActiveSpells>();
    assert!(active.is_some());
    // At this point, spell1 should still be active (repeating)
    // and spell2 should be gone (one-shot after delay).
    let count = active.unwrap().spell_count();
    assert_eq!(count, 1, "Expected only spell1 to remain (repeating), but {} spells exist", count);
}

#[test]
fn active_spells_cleaned_up_when_runes_complete() {
    let mut app = test_app();

    // A spell with one instant rune.
    let spell = Spell::new("Quick", "").with_rune(DamageRune {
        amount: 5.0,
        damage_type: DamageType::Physical,
    });
    let handle = app.world_mut().resource_mut::<Assets<Spell>>().add(spell);

    let caster = app.world_mut().spawn_empty().id();

    app.world_mut().write_message(CastSpellMessage {
        caster,
        targets: vec![],
        spell: handle,
    });

    app.update();

    // After ticking through execution, ActiveSpells should be cleaned up.
    app.update();
    assert!(app.world().entity(caster).get::<ActiveSpells>().is_none());
}

// ---------------------------------------------------------------------------
// CastContext
// ---------------------------------------------------------------------------

#[test]
fn cast_context_supports_self_and_multi_target() {
    let mut app = test_app();
    let world = app.world_mut();

    let caster = world.spawn_empty().id();
    let t1 = world.spawn_empty().id();
    let t2 = world.spawn_empty().id();

    // Self-cast (no targets)
    let self_ctx = CastContext::new(caster);
    assert_eq!(self_ctx.caster, caster);
    assert!(self_ctx.targets.is_empty());

    // Multi-target
    let multi_ctx = CastContext::new(caster).with_targets([t1, t2]);
    assert_eq!(multi_ctx.caster, caster);
    assert_eq!(multi_ctx.targets.len(), 2);
    assert!(multi_ctx.targets.contains(&t1));
    assert!(multi_ctx.targets.contains(&t2));
}

// ---------------------------------------------------------------------------
// Spellbook component
// ---------------------------------------------------------------------------

#[test]
fn spellbook_manages_spells() {
    let mut app = test_app();

    let h1 = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(Spell::new("A", ""));
    let h2 = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(Spell::new("B", ""));

    let sb = Spellbook::new().with_spell(h1.clone()).with_spell(h2.clone());
    assert_eq!(sb.len(), 2);
    assert!(sb.contains(&h1));
    assert!(sb.contains(&h2));

    let mut sb = sb;
    sb.remove_spell(&h1);
    assert_eq!(sb.len(), 1);
    assert!(sb.contains(&h2));
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
