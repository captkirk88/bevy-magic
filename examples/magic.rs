//! `magic_demo` — a complete runnable example for bevy-magic.
//!
//! Demonstrates:
//!   * Defining custom [`Rune`] types and registering them with [`MagicPlugin`].
//!   * Loading spells from `.spell` asset files (RON).
//!   * Casting spells via [`CommandsExt::cast_magic`] / [`CastSpellMessage`].
//!   * Applying and removing [`Enchantment`]s on [`Enchantable`] entities.
//!   * Enchanting an **item** entity (Sword) with [`EnchantmentTrigger::OnDemand`].
//!   * Manually firing an on-demand enchantment via [`CommandsExt::trigger_enchantment`].
//!   * Reacting to game state driven by the magic system.
//!
//! # Running
//!
//! ```sh
//! cargo run --example magic_demo
//! ```
//!
//! The example runs for a fixed number of frames then exits.  All output is
//! printed to stdout so you can follow along without a GUI window.

use std::time::Duration;

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    ecs::system::BoxedSystem,
    prelude::*,
};
use serde::{Deserialize, Serialize};

use bevy_magic::{
    enchanting::prelude::*,
    prelude::*,
};

// ─────────────────────────────────────────────────────────────────────────────
// Domain components
// ─────────────────────────────────────────────────────────────────────────────

/// Player marker.
#[derive(Component)]
struct Player;

/// Sword item marker — no separate `Item` component needed; [`Enchantable`] is sufficient.
#[derive(Component)]
struct Sword;

/// Enemy marker.
#[derive(Component)]
struct Enemy;

/// Simple hit-point component shared by both player and enemies.
#[derive(Component, Debug)]
struct Health {
    current: f32,
    max: f32,
}

impl Health {
    fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    fn apply_damage(&mut self, amount: f32) {
        self.current = (self.current - amount).max(0.0);
    }

    fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    #[allow(unused)]
    fn is_alive(&self) -> bool {
        self.current > 0.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Custom Rune definitions
// ─────────────────────────────────────────────────────────────────────────────
// These are the "game logic" runes. Each interacts with ECS components
// (Health, Transform, etc.) via normal Bevy system parameters.

// --- DamageRune ---------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize, TypePath)]
pub struct DamageRune {
    pub amount: f32,
    pub damage_type: DamageType,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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
        let amount = self.amount;
        let dtype = format!("{:?}", self.damage_type);
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, mut commands: Commands, mut query: Query<(&mut Health, &Name)>| {
                for &target in &ctx.targets {
                    if let Ok((mut hp, name)) = query.get_mut(target) {
                        if !hp.is_alive() {
                            continue;
                        }
                        hp.apply_damage(amount);
                        println!(
                            "  [DamageRune] {} takes {:.0} {} damage  → HP {:.0}/{:.0}",
                            name, amount, dtype, hp.current, hp.max
                        );
                        if !hp.is_alive() {
                            println!("  [DamageRune] {} has died", name);
                            commands.entity(target).despawn();
                        }
                    }
                }
            },
        ))
    }
}

// --- HealRune -----------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize, TypePath)]
pub struct HealRune {
    pub amount: f32,
}

impl Rune for HealRune {
    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let amount = self.amount;
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, mut query: Query<(&mut Health, &Name)>| {
                if let Ok((mut hp, name)) = query.get_mut(ctx.caster) {
                    hp.heal(amount);
                    println!(
                        "  [HealRune] {} restores {:.0} HP  → HP {:.0}/{:.0}",
                        name, amount, hp.current, hp.max
                    );
                }
            },
        ))
    }
}

// --- StatusRune ---------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize, TypePath)]
pub struct StatusRune {
    pub effect: StatusEffect,
    pub duration_secs: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatusEffect {
    Burn,
    Freeze,
    Stun,
    Slow { factor: f32 },
    Poison { damage_per_sec: f32 },
}

impl Rune for StatusRune {
    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let effect = format!("{:?}", self.effect);
        let duration = self.duration_secs;
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, names: Query<&Name>| {
                for &target in &ctx.targets {
                    let label = names
                        .get(target)
                        .map(|n| n.as_str())
                        .unwrap_or("<unnamed>");
                    println!(
                        "  [StatusRune] applying {:?} ({:.1}s) to {} (entity={:?})",
                        effect, duration, label, target
                    );
                }
            },
        ))
    }
}

// --- TeleportRune -------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize, TypePath)]
pub struct TeleportRune {
    pub offset: [f32; 3],
}

impl Rune for TeleportRune {
    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let offset = self.offset;
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>,
                  mut transforms: Query<(&mut Transform, &Name)>| {
                if let Ok((mut tf, name)) = transforms.get_mut(ctx.caster) {
                    tf.translation += Vec3::from(offset);
                    println!(
                        "  [TeleportRune] {} blinks to {:?}",
                        name, tf.translation
                    );
                }
            },
        ))
    }
}

// --- FlameBrandRune ----------------------------------------------------------
// Used as the rune inside the spell-backed Flame Brand enchantment.
// Fires every 400 ms — short enough to be visible during the demo.

#[derive(Clone, TypePath)]
struct FlameBrandRune {
    damage: f32,
}

impl Rune for FlameBrandRune {
    fn interval(&self) -> Duration {
        Duration::from_millis(400)
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let dmg = self.damage;
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, mut commands: Commands, mut query: Query<(&mut Health, &Name)>| {
                for &target in &ctx.targets {
                    if let Ok((mut hp, name)) = query.get_mut(target) {
                        if !hp.is_alive() {
                            continue;
                        }
                        hp.apply_damage(dmg);
                        println!(
                            "  [FlameBrand] {} scorched for {:.0} fire  → HP {:.0}/{:.0}",
                            name, dmg, hp.current, hp.max
                        );
                        if !hp.is_alive() {
                            println!("  [FlameBrand] {} has died", name);
                            commands.entity(target).despawn();
                        }
                    }
                }
            },
        ))
    }
}

// --- PoisonTickRune ----------------------------------------------------------
// A custom rune used for an inline enchantment (not loaded from a file).

#[derive(Clone, TypePath)]
struct PoisonTickRune {
    damage_per_tick: f32,
}

impl Rune for PoisonTickRune {
    /// Fires every 500 ms (repeating).
    fn interval(&self) -> Duration {
        Duration::from_millis(500)
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let dmg = self.damage_per_tick;
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, mut query: Query<(&mut Health, &Name)>| {
                for &target in &ctx.targets {
                    if let Ok((mut hp, name)) = query.get_mut(target) {
                        hp.apply_damage(dmg);
                        println!(
                            "  [PoisonTick] {} takes {:.0} poison  → HP {:.0}/{:.0}",
                            name, dmg, hp.current, hp.max
                        );
                    }
                }
            },
        ))
    }
}

#[derive(Clone, TypePath)]
struct ExplosionRune {
    damage: f32,
    radius: f32,
}

impl Rune for ExplosionRune {
    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let dmg = self.damage;
        let radius_sq = self.radius * self.radius;
        Box::new(IntoSystem::into_system(
            move |In(ctx): In<CastContext>, mut commands: Commands, caster_tf: Query<&Transform>, mut query: Query<(Entity, &mut Health, &Name, &Transform), With<Enemy>>| {
                println!("  [ExplosionRune] entity {:?} is exploding", ctx.caster);

                // Use the live transform if available, otherwise fall back to
                // the pre-death origin snapshot stored in the context (set for
                // OnDespawn enchantments by flush_despawn_triggers).
                let caster_transform = if let Ok(tf) = caster_tf.get(ctx.caster) {
                    *tf
                } else if let Some(pos) = ctx.origin {
                    Transform::from_translation(pos)
                } else {
                    return;
                };

                for (target_ent, mut hp, name, target_tf) in &mut query {
                    if target_ent == ctx.caster {
                        continue;
                    }
                    let dist_sq = caster_transform.translation.distance_squared(target_tf.translation);
                    if dist_sq > radius_sq {
                        continue;
                    }

                    hp.apply_damage(dmg);
                    println!(
                        "    [ExplosionRune] {} ({:?}) takes {:.0} explosion -> HP {:.0}/{:.0}",
                        name, target_ent, dmg, hp.current, hp.max
                    );
                    if !hp.is_alive() {
                        println!("    [ExplosionRune] {} ({:?}) has died", name, target_ent);
                        commands.entity(target_ent).despawn();
                    }
                }
            },
        ))
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// State & Resources
// ─────────────────────────────────────────────────────────────────────────────

/// Spell handles built programmatically at startup.
#[derive(Resource)]
struct SpellHandles {
    fireball: Handle<Spell>,
    healing_wave: Handle<Spell>,
    frost_nova: Handle<Spell>,
    arcane_blink: Handle<Spell>,
    /// Spell used as an enchantment source — its rune has a repeating interval.
    flame_brand: Handle<Spell>,
}

/// Entity handles populated during setup.
#[derive(Resource)]
struct SceneEntities {
    player: Entity,
    goblin: Entity,
    orc: Entity,
    boss: Entity,
    sword: Entity,
}

/// Simple frame counter; the example exits after a fixed number of frames.
#[derive(Resource, Default)]
struct FrameCounter(u32);

/// At ~60 fps this is 5 seconds — long enough to observe repeating enchantments.
const MAX_FRAMES: u32 = 300;

// ─────────────────────────────────────────────────────────────────────────────
// Setup
// ─────────────────────────────────────────────────────────────────────────────

fn setup(mut commands: Commands, mut spell_assets: ResMut<Assets<Spell>>) {
    println!("\n=== bevy-magic demo ===\n");

    // Build spells programmatically so they are immediately available — no
    // async file I/O needed.  The same spells could be loaded from RON files
    // with `asset_server.load("spells/fireball.spell")` in a full game.
    let handles = SpellHandles {
        fireball: spell_assets.add(
            Spell::new("Fireball", "Hurls a ball of fire.")
                .with_rune(DamageRune { amount: 75.0, damage_type: DamageType::Fire })
                .with_rune(StatusRune { effect: StatusEffect::Burn, duration_secs: 5.0 }),
        ),
        healing_wave: spell_assets.add(
            Spell::new("Healing Wave", "Restores health to the caster.")
                .with_rune(HealRune { amount: 50.0 }),
        ),
        frost_nova: spell_assets.add(
            Spell::new("Frost Nova", "Freezes all nearby targets.")
                .with_rune(DamageRune { amount: 30.0, damage_type: DamageType::Ice })
                .with_rune(StatusRune { effect: StatusEffect::Freeze, duration_secs: 2.5 }),
        ),
        arcane_blink: spell_assets.add(
            Spell::new("Arcane Blink", "Teleport forward and blast the target.")
                .with_rune(TeleportRune { offset: [0.0, 0.0, 5.0] })
                .with_rune(DamageRune { amount: 20.0, damage_type: DamageType::Arcane }),
        ),
        // This spell drives the Flame Brand enchantment.  FlameBrandRune has a
        // repeating interval so the enchantment ticks periodically.
        flame_brand: spell_assets.add(
            Spell::new("Flame Brand", "Sears the target with repeating fire.")
                .with_rune(FlameBrandRune { damage: 8.0 }),
        ),
    };
    commands.insert_resource(handles);

    // Player: can cast spells and be enchanted.
    let player = commands.spawn((
        Name::new("Player"),
        Player,
        Enchantable,
        Health::new(200.0),
        Transform::default(),
        Spellbook::new(),
    )).id();

    // Enemies: enchantable targets.
    let goblin = commands.spawn((
        Name::new("Goblin"),
        Enemy,
        Enchantable,
        Health::new(60.0),
        Transform::from_xyz(5.0, 0.0, 0.0),
    )).id();

    let orc = commands.spawn((
        Name::new("Orc Warlord"),
        Enemy,
        Enchantable,
        Health::new(150.0),
        Transform::from_xyz(10.0, 0.0, 0.0),
    )).id();

    let boss = commands.spawn((
        Name::new("Dreadnought"),
        Enemy,
        Enchantable,
        Health::new(300.0),
        Transform::from_xyz(15.0, 0.0, 0.0),
    )).id();

    // Sword item — Enchantable works on items just like characters.
    // No dedicated Item component needed.
    let sword = commands.spawn((
        Name::new("Flameburst Sword"),
        Sword,
        Enchantable,
    )).id();

    commands.insert_resource(SceneEntities { player, goblin, orc, boss, sword });
}

// ─────────────────────────────────────────────────────────────────────────────
// Demo driver systems
// ─────────────────────────────────────────────────────────────────────────────

/// Runs once on the first Update tick; kicks off all demo casts and enchantments.
/// Spells are built programmatically in setup, so no asset-loading wait is needed.
fn demo_driver(
    mut commands: Commands,
    handles: Res<SpellHandles>,
    scene: Res<SceneEntities>,
    mut has_run: Local<bool>,
) -> Result<(), BevyError> {
    if *has_run {
        return Ok(());
    }
    *has_run = true;

    let player_e = scene.player;
    let goblin = scene.goblin;
    let orc = scene.orc;
    let sword = scene.sword;
    let enemy_list = vec![goblin, orc];

    println!("--- Starting demo ---\n");

    // 1. Cast Fireball at both enemies.
    println!("[Cast] Fireball → Goblin + Orc Warlord");
    commands.cast_magic(player_e, handles.fireball.clone(), Some(enemy_list.clone()));

    // 2. Cast Frost Nova at the Goblin.
    println!("[Cast] Frost Nova → Goblin");
    commands.cast_magic(player_e, handles.frost_nova.clone(), Some(vec![goblin]));

    // 3. Cast Healing Wave (self-heal).
    println!("[Cast] Healing Wave (self)");
    commands.cast_magic(player_e, handles.healing_wave.clone(), None);

    // 4. Cast Arcane Blink (self-teleport + arcane damage to Orc).
    println!("[Cast] Arcane Blink → Orc Warlord");
    commands.cast_magic(player_e, handles.arcane_blink.clone(), Some(vec![orc]));

    // 5. Apply an inline rune-based Poison enchantment to the Goblin (Timed).
    println!("[Enchant] Applying 'Venom Curse' (inline runes, timed) to Goblin");
    commands.apply_enchantment(
        goblin,
        Enchantment::from_runes(
            "Venom Curse",
            "Deals poison damage every second.",
            player_e,
            vec![Box::new(PoisonTickRune { damage_per_tick: 5.0 })],
        ),
    );

    // 6. Apply a spell-asset-backed Flame Brand enchantment to the Orc (Timed).
    println!("[Enchant] Applying 'Flame Brand' (spell asset, timed) to Orc Warlord");
    commands.apply_enchantment(
        orc,
        Enchantment::from_spell(
            "Flame Brand",
            "Burns the target periodically.",
            player_e,
            handles.flame_brand.clone(),
        ),
    );

    // 6a. Apply an OnDespawn explosion enchantment to the Orc.
    println!("[Enchant] Applying 'Death Explosion' (on despawn) to Orc Warlord");
    commands.apply_enchantment(
        orc,
        Enchantment::from_runes(
            "Death Explosion",
            "Explodes and damages nearby enemies on death.",
            orc,
            vec![Box::new(ExplosionRune { damage: 50.0, radius: 8.0 })],
        )
        .with_trigger(EnchantmentTrigger::OnDespawn),
    );

    // 7. Enchant the Sword item with an OnDemand trigger — fires only when the
    //    sword hits something, not on a fixed timer.
    println!("[Enchant] Enchanting 'Flameburst Sword' with 'Flame Edge' (OnDemand)");
    commands.apply_enchantment(
        sword,
        Enchantment::from_runes(
            "Flame Edge",
            "Deals fire damage on-hit.",
            player_e,
            vec![Box::new(PoisonTickRune { damage_per_tick: 12.0 })],
        )
        .with_trigger(EnchantmentTrigger::OnDemand),
    );

    Ok(())
}

/// Removes the Poison enchantment from the Goblin after 30 frames — shows
/// mid-game dispel.  Also fires the sword's OnDemand enchantment a few times
/// to simulate hit events.
fn dispel_after_delay(
    mut commands: Commands,
    frame: Res<FrameCounter>,
    scene: Res<SceneEntities>,
    mut done: Local<bool>,
) {
    // Dispel at ~3 s (180 frames × 16 ms).
    if frame.0 == 180 && !*done {
        *done = true;
        println!("\n[Dispel] Removing 'Venom Curse' from Goblin");
        commands.remove_enchantment(scene.goblin, "Venom Curse");
    }

    // Simulate sword hits at frames 60 and 120 on the boss (goblin may be dead by then).
    if frame.0 == 60 || frame.0 == 120 {
        println!("\n[Hit] Sword strikes — triggering 'Flame Edge' on {:?}", scene.sword);
        commands.trigger_enchantment(scene.sword, "Flame Edge", Some(vec![scene.boss]));
    }

    // Final boss extra strike at frame 240.
    if frame.0 == 240 {
        println!("\n[Boss Hit] Flameburst Sword hits Dreadnought again!");
        commands.trigger_enchantment(scene.sword, "Flame Edge", Some(vec![scene.boss]));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Status / HUD systems
// ─────────────────────────────────────────────────────────────────────────────

/// Prints a brief status table every 20 frames.
fn print_status(
    frame: Res<FrameCounter>,
    combatants: Query<(&Name, &Health, Option<&ActiveSpells>, Option<&ActiveEnchantments>)>,
    items: Query<(&Name, Option<&ActiveEnchantments>), With<Sword>>,
) {
    // Print every second (~60 frames at 60 fps).
    if frame.0 % 60 != 0 {
        return;
    }
    println!("\n--- Status (frame {}) ---", frame.0);
    for (name, hp, active_spells, enchants) in &combatants {
        let spell_count = active_spells.map(|a| a.spell_count()).unwrap_or(0);
        let enchant_count = enchants.map(|e| e.count()).unwrap_or(0);
        let enchant_names: Vec<&str> = enchants
            .map(|e| e.names().collect())
            .unwrap_or_default();
        println!(
            "  {:<20} HP {:.0}/{:.0}   spells in-flight: {}   enchantments: {} [{}]",
            name.as_str(),
            hp.current,
            hp.max,
            spell_count,
            enchant_count,
            enchant_names.join(", ")
        );
    }
    for (name, enchants) in &items {
        let enchant_names: Vec<&str> = enchants
            .map(|e| e.names().collect())
            .unwrap_or_default();
        println!(
            "  {:<20} [item]   enchantments: [{}]",
            name.as_str(),
            enchant_names.join(", ")
        );
    }
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame counter & exit
// ─────────────────────────────────────────────────────────────────────────────

fn tick_frame(mut frame: ResMut<FrameCounter>) {
    frame.0 += 1;
}

fn exit_after_max_frames(frame: Res<FrameCounter>, mut exit: MessageWriter<AppExit>) {
    if frame.0 >= MAX_FRAMES {
        println!("\n=== Demo complete ({} frames). Goodbye! ===", frame.0);
        exit.write(AppExit::Success);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// App entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    App::new()
        // Cap to ~60 fps so real-time timers used by enchantments advance at a
        // predictable rate.  Without this, MinimalPlugins runs uncapped and
        // Time::delta() is near-zero, preventing any timer from ever firing.
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))))
        .add_plugins(bevy::asset::AssetPlugin::default())
        // Register all rune types used in .spell files or inline enchantments.
        .add_plugins(
            MagicPlugin::default()
                .rune::<DamageRune>()
                .rune::<HealRune>()
                .rune::<StatusRune>()
                .rune::<TeleportRune>(),
        )
        .init_resource::<FrameCounter>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                demo_driver,
                dispel_after_delay,
                print_status,
                tick_frame,
                exit_after_max_frames,
            )
                .chain(),
        )
        .run();
}
