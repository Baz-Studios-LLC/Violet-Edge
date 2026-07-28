# VIOLET EDGE — Rust / Bevy

A native Rust + [Bevy 0.16](https://bevy.org) game: a neon-vector love letter to
*Asteroids*. Ported and grown from an earlier JS/Canvas prototype (kept as the
reference at `../neon-asteroids/`).

> **Status: content-complete and beatable, in balance/polish.** The full
> **30-wave run** is in: **six bosses**, seven asteroid types, five earnable
> powerups, a story told through a decrypting **Pilot Log**, achievements,
> top-5 high scores, and fully procedural audio. A win rolls the real ending —
> and a hook for what comes next. Compiles on stable Rust with Bevy 0.16;
> `cargo test` green.

## Play (no build)

Grab the latest build from **[Releases](https://github.com/Baz-Studios-LLC/Violet-Edge/releases)**:
Windows `.zip` (unzip, run **`violet-edge.exe`**), macOS `.dmg` (Apple Silicon), or Linux `.zip`.
Self-contained — no install, no data files. On first launch Windows SmartScreen may warn about an
unsigned exe: *More info → Run anyway*.

## Build from source

### Prerequisites
1. **Rust** — install via [rustup.rs](https://rustup.rs) (Windows: `rustup-init.exe`, MSVC toolchain).
2. **Windows — C++ build tools.** Bevy compiles native code, so install the
   *"Desktop development with C++"* workload from
   [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022).
   (A `link.exe` / "cannot find linker" error means this is missing.)

### Run
```bash
cargo run            # debug (includes the dev keys below)
cargo run --release  # smooth framerate, no dev keys
```
The first build compiles Bevy + dependencies — several minutes and a few hundred
MB in `target/`. Rebuilds after that are fast.

## Controls

| Action | Keys |
| --- | --- |
| Rotate | `←` / `→` (or `A` / `D`) |
| Thrust | `↑` (or `W`) |
| Fire | `Space` or left-click |
| Warp (black hole) | `Shift` |
| Chain beam | right-click *(once earned)* |
| Cycle shot mode (standard / mass / Warhead) | `Q` *(once earned)* |
| Pause / resume | `Esc` |
| Mute music | `M` |

Everything is **rebindable** (keyboard/mouse and controller) on the CONTROLS screen; a full
controller works out of the box. The pause menu shows your current binds mid-run. Menus are
mouse-clickable, or use `Enter`/`Space` to play and `Esc` to go back.

## The run

**Survive 30 timed waves.** Every 5th wave is a **boss**; each boss's defeat drops a **powerup**
that echoes its mechanic. The belt escalates in three acts — new asteroid types keep debuting all
the way to the finale — and the story assembles in the **PILOT LOG** as your field reports decrypt
with each boss you fell.

### Asteroids (the stars)

| Rock | Debut | Behaviour |
| --- | --- | --- |
| **Blue** | 1 | The standard: splits large → mid → small. |
| **Green** | 6 | Dense — takes hits equal to its size; chipped by bullets, sheared whole by chain/mine/mass. Retires in Act III. |
| **Orange** | 11 | Explosive — detonates in a big AOE that obliterates and chains other oranges. |
| **Pulser** | 16 | Pulses lit ↔ dark on a slow beat; **invulnerable while lit** — hit the dark beat. Splits into smaller pulsers. |
| **Red** | 21 | Grows — absorbs nearby rocks to swell; a plain shot splits it into more reds (whack-a-mole). Blasts kill it clean. |
| **Beacon** | 23 | Teal aura warden — rocks inside its aura are **immune to your guns until it falls**. Blasts and the warp ignore the aura. |
| **Cluster** | 26 | Fractured ice — **shatters into a ring of fast shards**. Mass shot vaporizes it clean; the warp swallows it whole. |
| **Gold** | any | The 1UP: destroy the whole lineage before a piece escapes for +1 life. More frequent early, rare late. |

### Bosses & their drops

| Wave | Boss | Drop |
| --- | --- | --- |
| 5 | **The Warden** — fights behind a shield of captured rocks | **Chain beam** |
| 10 | **The Glutton** — eats rocks to grow; starve it or feed it into overload | **Mass shot** |
| 15 | **The Slinger** — a gunship that loads and fires cannonball rocks | **Drone** wingman |
| 20 | **The Detonator** — primes field rocks into live bombs; only vulnerable mid-channel | **Warhead rounds** |
| 25 | **The Pulsar** — lit/dark like its rocks; shockwaves fling the field | **Nova Shield** (regenerating one-hit barrier) |
| 30 | **THE PHANTOM** — the finale: a three-phase spectral steersman | *the ending* |

Also in the field: **mines** (wave 2+), brief **enemy mob** windows (3–4, 8–9), rock-riding
**limpets** (12–13), and **gravity wells** (18–19) — garnish only; the asteroids stay the show.

## Meta

- **Pilot Log** — the story, as transmissions home; each boss's record decrypts when it first falls.
- **12 achievements** with unlock toasts (boss ladder, lifetime grinds, and the real wave-30 capstones).
- **Top-5 high scores**, persisted. **HUD** with named ability slots that appear as you earn them.
- **Any screen size** — the camera scale-to-fits, the field fills your aspect, the HUD scales.
- **Photosensitivity-aware** — all flashing is kept at or under 3 flashes/sec by design.
- **Procedural audio** — every sound synthesized at runtime, no asset files: a full-length
  club-techno track, a distinct boss track, a pre-boss riser, and per-event SFX.

## Roadmap

The 30-wave run is done end-to-end (see [`DESIGN.md`](DESIGN.md) for the full design):

- **Magnet** powerup (the Phantom's drop) — needs its re-theme first.
- **New Game+** — replay waves 1–30 harder (the victory screen already teases it).
- Balance passes from playtesting.

## Notes

- **Purple is the player.** Reserved for the ship and its kit — nothing else uses it.
- **Rendering** is immediate-mode gizmos with an HDR + `Bloom` camera for the glow.
- **Motion is delta-time based** (framerate-independent).
- **Pinned to Bevy 0.16** deliberately — don't bump the version without updating the code.
- **Dev keys (debug builds only, compiled out of release):** `F1` invincibility, `F2` skip
  wave / kill boss, `F3` drop an orange rock, `F4` jump to the wave-30 finale.
