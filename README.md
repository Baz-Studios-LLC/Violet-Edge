# VIOLET EDGE — Rust / Bevy

A native Rust + [Bevy 0.16](https://bevy.org) game: a neon-vector love letter to
*Asteroids*. Ported and grown from an earlier JS/Canvas prototype (kept as the
reference at `../neon-asteroids/`).

> **Status: playable, in active development.** The full **waves 1–10 arc** with
> **two bosses** loops, and menus, achievements, top-5 high scores, and fully
> procedural audio are in. Next up is the wave 11–15 content arc (a new mob, a
> third boss, and explosive asteroids into the rotation) — see the roadmap.
> Compiles on stable Rust with Bevy 0.16; `cargo test` green.

## Play (no build)

Grab the latest Windows build from **[Releases](https://github.com/Baz-Studios-LLC/Violet-Edge/releases)**:
download the `.zip`, unzip, and run **`violet-edge.exe`**. It's self-contained —
no install, no Rust, no data files (Windows x64). On first launch SmartScreen may
warn about an unsigned exe: *More info → Run anyway*.

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
| Chain shot | right-click *(once earned)* |
| Standard ↔ mass shot | `Q` *(once earned)* |
| Pause / resume | `Esc` |
| Mute music | `M` |

Menus (main / controls / briefing / achievements / pause) are mouse-clickable, or
use `Enter`/`Space` to play and `Esc` to go back.

## What's in

- **Ship** — momentum flight, lives, respawn invulnerability, tuned for precise aiming.
- **Asteroids** — blue rocks split 3 → 2 → 1; dense **green** rocks (wave 6+) take
  several hits; **orange** explosive rocks detonate in a blast that obliterates
  everything nearby and chains other oranges *(mechanic done; not yet in the wave
  rotation — try one with dev `F3`)*; rare **gold** rocks grant a life if you
  destroy the whole lineage.
- **Mines** (wave 2+) — drift in, chain-detonate, blast nearby rocks.
- **Enemy mobs** (waves 3–4 and 8–9) — fly in, strafe the ship, lob shots, then flee.
- **Warp** — fire a missile that flies to the wall you aim at and tears open a black
  hole; it devours rocks, mines and enemies (never the player) and bends the grid.
- **Chain shot** — a beam that shears everything along its length; earned from a
  pickup after boss 1 (grab it by flying into it *or* shooting it).
- **Mass shot** — a bigger, slower, harder-hitting primary; earned after boss 2.
  Toggle it against the fast standard shot with `Q`.
- **Boss 1 — the Warden** (wave 5): roams behind a shield of captured rocks and
  hurls them; strip the shield and hit the exposed core.
- **Boss 2 — the Glutton** (wave 10): a red seeker that eats rocks to grow bigger
  and tankier. Starve it and shoot it down — gunfire chips *and* shrinks it. Let it
  gorge and it overloads: swells huge, detonates a near screen-wide blast, then
  shrinks and starts over. Both bosses show an HP bar.
- **Progression & meta** — main menu, controls & briefing screens, an achievements
  screen with unlock toasts, a pause menu, and a persisted **top-5 high-score** table.
- **Procedural audio** — every sound is synthesized at runtime, no asset files:
  fire / rock-break / mine / ship-death / enemy fire+death / warp / 1-up, plus a
  full-length club-techno track and a distinct boss track.
- **Presentation** — HDR + bloom neon (Bevy gizmos), on-screen HUD, timed waves.

## Roadmap

Waves 1–10 currently loop. Next is the **11–15 content arc** (see
[`DESIGN.md`](DESIGN.md) for the full 50-level plan):

- Wire the **explosive orange** rock into waves 11–14.
- New mob: the **Darter** (fast interceptor that charges) — waves 12–13.
- Boss 3: **the Slinger** (wave 15) — a ship that fires asteroids at you.
- Beyond: red (growing) & pulser asteroids, more bosses/pickups, the full arc.

## Notes

- **Purple is the player.** Reserved for the ship and its kit — nothing else uses it.
- **Rendering** is immediate-mode gizmos with an HDR + `Bloom` camera for the glow.
- **Motion is delta-time based** (framerate-independent).
- **Pinned to Bevy 0.16** deliberately — don't bump the version without updating the code.
- **Dev keys (debug builds only, compiled out of release):** `F1` toggles
  invincibility, `F2` skips to the next wave (kills the boss on a boss wave), `F3`
  drops an explosive orange rock mid-field.
