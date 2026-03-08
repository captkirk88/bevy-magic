# bevy-magic

`bevy-magic` is a lightweight spellcasting system built on [Bevy](https://bevyengine.org/).
It provides abstractions for spells, runes, and a simple message-driven cast pipeline.

> [!NOTE]
> This is mainly for my personal use and future use, however, you may use it in your projects or contribute if you find it useful!

## Use

```toml
[dependencies]
bevy = "0.18"
bevy-magic = { version = "0.18", git = "https://github.com/captkirk88/bevy-magic" }
```

## Example

Below is a minimal example illustrating configuration of the plugin, definition of
custom runes, creation of a spell, and triggering a cast in a Bevy app.

```rust
use bevy::prelude::*;
use bevy_magic::{MagicPlugin, CastSpellMessage, Spell, Spellbook, Rune, CastContext};

// --- custom rune types -----------------------------------------------------

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
struct DamageRune { amount: f32 }

impl Rune for DamageRune {
    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let data = self.clone();
        Box::new(IntoSystem::into_system(move |In(ctx): In<CastContext>| {
            for &target in &ctx.targets {
                println!("hit {:?} for {} damage", target, data.amount);
            }
        }))
    }
}

// --- app setup -------------------------------------------------------------

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(MagicPlugin::default()
            .rune::<DamageRune>()
            )
        .add_startup_system(setup)
        .add_system(on_cast)
        .run();
}

fn setup(mut commands: Commands, mut assets: ResMut<Assets<Spell>>) {
    // build a spell in code (could also be loaded from `assets/` folder)
    let fireball = Spell::new("Fireball", "Exploding orb")
        .with_rune(DamageRune { amount: 50.0 });

    let handle = assets.add(fireball);

    let caster = commands.spawn_empty().id();
    let target = commands.spawn_empty().id();

    // queue a cast message; will run on the next update tick
    commands.write_message(CastSpellMessage {
        caster,
        targets: vec![target],
        spell: handle,
    });
}

fn on_cast(mut reader: MessageReader<CastSpellMessage>) {
    for msg in reader.read() {
        println!("spell cast by {:?} on {:?}", msg.caster, msg.targets);
    }
}
```

You can also drop spell files (JSON) into `assets/spells/` and load them with `AssetServer`.

## Timing: Delays and Intervals

Runes support per-rune delays and intervals. When a spell is cast, the plugin automatically schedules each rune on the caster's `ActiveSpells` component and ticks them each frame.

```rust
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
struct BurningRune {
    pub damage_per_tick: f32,
}

impl Rune for BurningRune {
    /// Half-second delay before damage starts ticking.
    fn delay(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(0.5)
    }

    /// Tick every 1 second (repeating).
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(1.0)
    }

    fn build(&self) -> BoxedSystem<In<CastContext>, ()> {
        let data = self.clone();
        Box::new(IntoSystem::into_system(move |In(ctx): In<CastContext>| {
            for &target in &ctx.targets {
                println!("burn: {} takes {} damage", target, data.damage_per_tick);
            }
        }))
    }
}
```

When this rune is part of a spell:
- The first execution is delayed by 0.5 seconds.
- Then it repeats every 1.0 second.

Runes without timing (both `delay()` and `interval()` return `Duration::ZERO`) execute
immediately and once, just like the `DamageRune` example above.

---

## Contributing
Contributions are welcome! Please open an issue or submit a pull request.

## License
MIT License. See the [LICENSE](LICENSE) file for details.