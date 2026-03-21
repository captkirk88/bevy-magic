//! Integration tests for the enchanting system.
//!
//! Run with: `cargo test --release -- --nocapture`

use std::sync::{Arc, Mutex};

use bevy::{ecs::system::{BoxedSystem, SystemState}, prelude::*};
use bevy_magic::{
    enchanting::{
        ActiveEnchantments, ApplyEnchantmentMessage, Enchantable, Enchantment,
        RemoveEnchantmentMessage,
    },
    runes::{CastContext, Rune},
    CommandsExt, MagicPlugin,
};

// ---------------------------------------------------------------------------
// Test runes
// ---------------------------------------------------------------------------

/// Records every invocation into a shared counter for assertions.
#[derive(Clone)]
struct CountingRune {
    counter: Arc<Mutex<u32>>,
}

impl Rune for CountingRune {
    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let counter = Arc::clone(&self.counter);
        Box::new(IntoSystem::into_system(
            move |_ctx: In<CastContext>| {
                *counter.lock().expect("counter lock") += 1;
            },
        ))
    }
}

/// A rune that repeats at a short interval for timing tests.
#[derive(Clone)]
struct RepeatingRune {
    counter: Arc<Mutex<u32>>,
    interval_secs: f32,
}

impl Rune for RepeatingRune {
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.interval_secs)
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let counter = Arc::clone(&self.counter);
        Box::new(IntoSystem::into_system(
            move |_ctx: In<CastContext>| {
                *counter.lock().expect("counter lock") += 1;
            },
        ))
    }
}

/// A rune with a short delay before its first execution.
#[derive(Clone)]
struct DelayedRune {
    counter: Arc<Mutex<u32>>,
    delay_secs: f32,
}

impl Rune for DelayedRune {
    fn delay(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.delay_secs)
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let counter = Arc::clone(&self.counter);
        Box::new(IntoSystem::into_system(
            move |_ctx: In<CastContext>| {
                *counter.lock().expect("counter lock") += 1;
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(MagicPlugin::default());
    app
}

fn counter() -> Arc<Mutex<u32>> {
    Arc::new(Mutex::new(0))
}

fn read(c: &Arc<Mutex<u32>>) -> u32 {
    *c.lock().expect("counter lock")
}

// ---------------------------------------------------------------------------
// Enchantable component / entity filtering
// ---------------------------------------------------------------------------

#[test]
fn enchantment_is_rejected_without_enchantable_component() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    // No Enchantable component on target.
    let target = app.world_mut().spawn_empty().id();

    let hits = counter();
    let enchantment = Enchantment::from_runes(
        "Test",
        "Should not apply",
        applier,
        vec![Box::new(CountingRune { counter: Arc::clone(&hits) })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    app.update();
    app.update();

    // No ActiveEnchantments added.
    assert!(
        app.world().entity(target).get::<ActiveEnchantments>().is_none(),
        "ActiveEnchantments should not be added to non-Enchantable entity"
    );
    assert_eq!(read(&hits), 0, "Rune should not have fired");
}

#[test]
fn enchantment_is_accepted_with_enchantable_component() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits = counter();
    // Use a repeating rune so it persists after the first tick.
    let enchantment = Enchantment::from_runes(
        "Spark",
        "Repeats so it does not self-remove",
        applier,
        vec![Box::new(RepeatingRune {
            counter: Arc::clone(&hits),
            interval_secs: 0.001,
        })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    // One update: apply_enchantments and tick_enchantments both run (chained).
    // The repeating rune fires on its first tick and remains active.
    app.update();

    assert!(
        app.world().entity(target).get::<ActiveEnchantments>().is_some(),
        "Enchantable entity should have ActiveEnchantments with a repeating enchantment"
    );
    assert!(read(&hits) > 0, "Rune should have fired at least once");
}

// ---------------------------------------------------------------------------
// One-shot enchantments
// ---------------------------------------------------------------------------

#[test]
fn one_shot_enchantment_fires_and_cleans_up() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits = counter();
    let enchantment = Enchantment::from_runes(
        "Flash",
        "Single burst",
        applier,
        vec![Box::new(CountingRune { counter: Arc::clone(&hits) })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    // apply_enchantments and tick_enchantments run in the same chained update.
    // A 0-delay once-rune fires and cleans up within a single update.
    app.update();

    assert_eq!(read(&hits), 1);
    // Component removed once all non-repeating runes complete.
    assert!(
        app.world().entity(target).get::<ActiveEnchantments>().is_none(),
        "ActiveEnchantments should be cleaned up after all runes complete"
    );
}

// ---------------------------------------------------------------------------
// Repeating enchantments
// ---------------------------------------------------------------------------

#[test]
fn repeating_enchantment_fires_multiple_times() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits = counter();
    let enchantment = Enchantment::from_runes(
        "Burn",
        "Repeating fire damage",
        applier,
        vec![Box::new(RepeatingRune {
            counter: Arc::clone(&hits),
            interval_secs: 0.001,
        })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    // Apply + many ticks.
    for _ in 0..60 {
        app.update();
    }

    assert!(
        read(&hits) > 1,
        "Repeating rune should have fired more than once, got {}",
        read(&hits)
    );
    // Repeating enchantment remains active.
    assert!(
        app.world().entity(target).get::<ActiveEnchantments>().is_some(),
        "Repeating enchantment should still be active"
    );
}

// ---------------------------------------------------------------------------
// Delayed enchantments
// ---------------------------------------------------------------------------

#[test]
fn delayed_enchantment_waits_before_firing() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits = counter();
    let enchantment = Enchantment::from_runes(
        "Slow Burn",
        "Delayed fire",
        applier,
        vec![Box::new(DelayedRune {
            counter: Arc::clone(&hits),
            delay_secs: 0.001,
        })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    // Apply only — rune should not have fired yet.
    app.update();
    assert!(
        app.world().entity(target).get::<ActiveEnchantments>().is_some(),
        "ActiveEnchantments exists during delay"
    );

    // Tick past the delay.
    for _ in 0..50 {
        app.update();
    }

    assert_eq!(read(&hits), 1, "Delayed rune should have fired once after delay");
    assert!(
        app.world().entity(target).get::<ActiveEnchantments>().is_none(),
        "ActiveEnchantments should be removed after delayed rune completes"
    );
}

// ---------------------------------------------------------------------------
// Remove enchantment by name
// ---------------------------------------------------------------------------

#[test]
fn remove_enchantment_by_name_stops_further_ticks() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits = counter();
    let enchantment = Enchantment::from_runes(
        "Poison",
        "Repeating poison",
        applier,
        vec![Box::new(RepeatingRune {
            counter: Arc::clone(&hits),
            interval_secs: 0.001,
        })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    // Let it run a few times.
    for _ in 0..20 {
        app.update();
    }
    let count_before_remove = read(&hits);
    assert!(count_before_remove > 0, "Should have fired before removal");

    // Remove it.
    app.world_mut().write_message(RemoveEnchantmentMessage {
        target,
        name: "Poison".to_string(),
    });
    app.update(); // processes remove

    // Tick more — counter should not increase.
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(
        read(&hits),
        count_before_remove,
        "Rune must not fire after removal"
    );
}

#[test]
fn remove_nonexistent_enchantment_is_a_no_op() {
    let mut app = test_app();

    let target = app.world_mut().spawn((Enchantable,)).id();

    app.world_mut().write_message(RemoveEnchantmentMessage {
        target,
        name: "Ghost".to_string(),
    });

    // Should not panic.
    app.update();
    app.update();
}

// ---------------------------------------------------------------------------
// Multiple enchantments on one entity
// ---------------------------------------------------------------------------

#[test]
fn multiple_enchantments_stack_on_same_entity() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits_a = counter();
    let hits_b = counter();

    for (name, c) in [("Frost", Arc::clone(&hits_a)), ("Fire", Arc::clone(&hits_b))] {
        let enchantment = Enchantment::from_runes(
            name,
            "stacked",
            applier,
            vec![Box::new(RepeatingRune {
                counter: c,
                interval_secs: 0.001,
            })],
        );
        app.world_mut().write_message(ApplyEnchantmentMessage {
            target,
            enchantment: Arc::new(enchantment),
        });
    }

    for _ in 0..30 {
        app.update();
    }

    let active = app
        .world()
        .entity(target)
        .get::<ActiveEnchantments>()
        .expect("should have active enchantments");

    assert_eq!(active.count(), 2, "Both enchantments should be active");
    assert!(active.has_enchantment("Frost"));
    assert!(active.has_enchantment("Fire"));
    assert!(read(&hits_a) > 0);
    assert!(read(&hits_b) > 0);
}

#[test]
fn remove_one_of_many_enchantments_leaves_others_intact() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let hits_keep = counter();
    let hits_remove = counter();

    for (name, c) in [
        ("Keep", Arc::clone(&hits_keep)),
        ("Remove", Arc::clone(&hits_remove)),
    ] {
        let enchantment = Enchantment::from_runes(
            name,
            "",
            applier,
            vec![Box::new(RepeatingRune {
                counter: c,
                interval_secs: 0.001,
            })],
        );
        app.world_mut().write_message(ApplyEnchantmentMessage {
            target,
            enchantment: Arc::new(enchantment),
        });
    }

    for _ in 0..10 {
        app.update();
    }

    app.world_mut().write_message(RemoveEnchantmentMessage {
        target,
        name: "Remove".to_string(),
    });
    app.update(); // process removal

    let active = app
        .world()
        .entity(target)
        .get::<ActiveEnchantments>()
        .expect("Keep enchantment should still be active");

    assert_eq!(active.count(), 1);
    assert!(active.has_enchantment("Keep"));
    assert!(!active.has_enchantment("Remove"));
}

// ---------------------------------------------------------------------------
// CommandsExt
// ---------------------------------------------------------------------------

#[test]
fn commands_ext_apply_enchantment() {
    let mut app = test_app();

    let hits = counter();
    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let enchantment = Enchantment::from_runes(
        "Ext Test",
        "Via CommandsExt",
        applier,
        vec![Box::new(CountingRune {
            counter: Arc::clone(&hits),
        })],
    );

    // Get Commands via SystemState, call the trait method, then apply.
    {
        let mut state: SystemState<Commands> = SystemState::new(app.world_mut());
        let mut commands = state.get_mut(app.world_mut());
        commands.apply_enchantment(target, enchantment);
        state.apply(app.world_mut());
    }

    app.update(); // apply
    app.update(); // tick

    assert_eq!(read(&hits), 1, "CommandsExt::apply_enchantment should work");
}

#[test]
fn commands_ext_remove_enchantment() {
    let mut app = test_app();

    let hits = counter();
    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    let enchantment = Enchantment::from_runes(
        "Ext Remove",
        "",
        applier,
        vec![Box::new(RepeatingRune {
            counter: Arc::clone(&hits),
            interval_secs: 0.001,
        })],
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });
    for _ in 0..10 {
        app.update();
    }

    {
        let mut state: SystemState<Commands> = SystemState::new(app.world_mut());
        let mut commands = state.get_mut(app.world_mut());
        commands.remove_enchantment(target, "Ext Remove");
        state.apply(app.world_mut());
    }

    app.update(); // process removal

    let count_after = read(&hits);
    for _ in 0..10 {
        app.update();
    }

    assert_eq!(
        read(&hits),
        count_after,
        "CommandsExt::remove_enchantment should stop further ticks"
    );
}

// ---------------------------------------------------------------------------
// Spell-backed enchantment
// ---------------------------------------------------------------------------

#[test]
fn spell_backed_enchantment_fires_spell_runes() {
    use bevy_magic::Spell;

    let mut app = test_app();

    let hits = counter();
    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    // Build a spell with a counting rune and add it to the asset store.
    let spell = Spell::new("Enchant Spell", "A spell used as an enchantment")
        .with_rune(CountingRune { counter: Arc::clone(&hits) });

    let spell_handle = app
        .world_mut()
        .resource_mut::<Assets<Spell>>()
        .add(spell);

    let enchantment = Enchantment::from_spell(
        "Spell Enchant",
        "Drives effect from a Spell asset",
        applier,
        spell_handle,
    );

    app.world_mut().write_message(ApplyEnchantmentMessage {
        target,
        enchantment: Arc::new(enchantment),
    });

    app.update(); // apply
    app.update(); // tick

    assert_eq!(
        read(&hits),
        1,
        "Spell-backed enchantment rune should have fired"
    );
}

// ---------------------------------------------------------------------------
// ActiveEnchantments query helpers
// ---------------------------------------------------------------------------

#[test]
fn active_enchantments_names_iterator() {
    let mut app = test_app();

    let applier = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn((Enchantable,)).id();

    for name in ["Alpha", "Beta", "Gamma"] {
        let enchantment = Enchantment::from_runes(
            name,
            "",
            applier,
            vec![Box::new(RepeatingRune {
                counter: counter(),
                interval_secs: 0.5,
            })],
        );
        app.world_mut().write_message(ApplyEnchantmentMessage {
            target,
            enchantment: Arc::new(enchantment),
        });
    }

    app.update();

    let active = app
        .world()
        .entity(target)
        .get::<ActiveEnchantments>()
        .expect("active enchantments present");

    let names: Vec<&str> = active.names().collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"Alpha"));
    assert!(names.contains(&"Beta"));
    assert!(names.contains(&"Gamma"));
}
