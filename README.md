# NEON EDGE — Bevy port (Rust)

A native Rust + [Bevy](https://bevy.org) port of NEON EDGE (the JS/Canvas game
lives at `../neon-asteroids/` and stays the reference/fallback while this grows).

> **Status: vertical slice (milestone 1).** Window, neon grid, ship
> (rotate/thrust/fire), and asteroids that drift, recycle at the edges, and split
> when shot. It proves the ECS architecture + the bloom-neon rendering. **It has
> not been compiled** — there's no Rust toolchain on the dev machine it was
> written on — so the first `cargo run` may surface a small fix or two. See
> "If it doesn't build" below.

## Prerequisites

1. **Rust** — install via [rustup.rs](https://rustup.rs). On Windows, download
   and run `rustup-init.exe`; take the defaults (MSVC toolchain).
2. **Windows only — C++ build tools.** Bevy compiles native code, so you need the
   *"Desktop development with C++"* workload from
   [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022).
   (If `cargo run` fails with a `link.exe` / `cannot find linker` error, this is why.)

## Run

```bash
cd neon-edge-bevy
cargo run
```

The **first** build downloads and compiles Bevy + its dependencies (hundreds of
crates) — expect several minutes and a few hundred MB in `target/`. After that,
rebuilds of just this game are fast.

## Controls

| Action | Keys |
| --- | --- |
| Rotate | `←` / `→` (or `A` / `D`) |
| Thrust | `↑` (or `W`) |
| Fire | `Space` |

## What's here vs. the full game

**In this slice:** HDR+bloom camera, grid, ship movement/fire, blue asteroids
(spawn → drift → edge-recycle → split on hit), bullet lifetime, score (resource).

**Not yet ported** (next milestones, in rough order): dense/orange asteroid
types, particles + screen shake, on-screen HUD/score text, mines + enemies, the
three bosses (octopus / devourer / raider), the power-ups (chain / mass / drone /
vortex), procedural audio, and the menu/pause system.

## Notes

- **Rendering:** wireframes are drawn with Bevy **gizmos** (immediate mode) for
  the vector look; the camera is HDR with `Bloom` for the glow. If the gizmo
  lines don't visibly bloom on your GPU/driver, we'll switch the key shapes to
  emissive meshes (guaranteed bloom) — tell me what you see.
- **Motion is per-second** (delta-time based, framerate-independent). The exact
  feel from the JS game (which was tuned per-frame at 60fps) will be re-tuned
  once it runs.
- **Pinned to Bevy 0.16** deliberately — the code targets 0.16's API. Don't bump
  the version without updating the code.

## If it doesn't build

Paste me the compiler errors. The most likely spots (noted in `src/main.rs`):
a `Time` method name (`delta_secs`), a gizmo signature, or an `Isometry2d`
constructor — all isolated one-liners if a 0.16 API detail differs.
