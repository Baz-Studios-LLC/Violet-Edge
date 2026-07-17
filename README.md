# VIOLET EDGE — Rust / Bevy

A native Rust + [Bevy 0.16](https://bevy.org) game: a neon-vector love letter to
*Asteroids*. Ported and grown from an earlier JS/Canvas prototype (kept as the
reference at `../neon-asteroids/`).

> **Status: playable, in active development.** The core loop, one boss, and the
> full audio are in. The mid/late wave content and the second boss are next
> (see the roadmap). Compiles on stable Rust with Bevy 0.16; `cargo test` green.

## Prerequisites

1. **Rust** — install via [rustup.rs](https://rustup.rs) (Windows: `rustup-init.exe`, MSVC toolchain).
2. **Windows only — C++ build tools.** Bevy compiles native code, so install the
   *"Desktop development with C++"* workload from
   [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022).
   (A `link.exe` / "cannot find linker" error means this is missing.)

## Run

```bash
cargo run            # debug
cargo run --release  # smooth framerate
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
| Chain shot | right-click *(once unlocked)* |
| Pause | `Esc` |
| Mute music | `M` |
| Skip track | `N` |

## What's in

- **Ship** — momentum flight, lives, respawn invulnerability.
- **Asteroids** — blue rocks that split 3 → 2 → 1, plus dense **green** rocks
  (wave 6+) that take several hits before they break.
- **Mines** (wave 2+) — drift in, chain-detonate, and blast nearby rocks.
- **Enemy mobs** (waves 3–5) — fly in, strafe the ship, lob shots, then flee.
- **Warp** — fire a missile that tears open a black hole; it devours rocks,
  mines and enemies (never the player), and bends the grid. Stays on-screen.
- **Chain shot** — a beam that shears everything along its length; unlocked by a
  pickup dropped after the first boss (grab it by flying into it *or* shooting it).
- **Boss 1** — a roaming "shield-shaman" that orbits a shield of captured rocks
  and hurls them; beaten by clearing its shield and hitting the exposed core.
- **Procedural audio** — every sound is synthesized at runtime, no asset files:
  fire / rock-break / mine / ship-death / enemy fire+death / warp effects, a
  full-length club-techno track, and a distinct boss track.
- **Presentation** — HDR + bloom neon (Bevy gizmos), on-screen HUD, timed waves.

## Roadmap

- Wave 6–10 content arc (green → +mines → +mobs), then **loop waves 1–10**.
- **Boss 2** (wave 10) — a red seeker that eats asteroids to grow bigger and tankier.
- More pickups (mass shot, assist drone).
- Menus, and an audio-polish pass (effects chain / produced tracks).

## Notes

- **Purple is the player.** It's reserved for the ship and its kit — nothing else uses it.
- **Rendering** is immediate-mode gizmos with an HDR + `Bloom` camera for the glow.
- **Motion is delta-time based** (framerate-independent).
- **Pinned to Bevy 0.16** deliberately — don't bump the version without updating the code.
- Dev builds only: `F1` toggles invincibility (compiled out of release).
