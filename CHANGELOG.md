# VIOLET EDGE — Changelog

Patch notes for the Rust + Bevy build. Newest first. (Releases are cut to GitHub and picked up by the
Baz Studios launcher.)

## v0.2.7 — Volatile: waves 16–20, the Slinger, pulsers & wells (2026-07-21)

The full Act II arc (waves 11–20) is now in.

**New content**
- **The Slinger** — wave-15 boss. A large ice-blue gunship that hovers high, tractor-beams a field
  rock to its muzzle, and fires it at you like a cannonball. Dodge the shot or shoot the loaded rock;
  chip its exposed core. Drops the **Drone**.
- **Drone** — the Slinger's pickup: an ally craft that follows you and mops up rocks you miss.
- **Pulser asteroids** (waves 16–20) — pulse bright white and are **invulnerable while lit**; hit them
  on the dark beat. They split into smaller pulsers. Wave 16 is pulser-only.
- **Gravity Well** (waves 18–19) — an "opposite warp" hazard that pops in at random and drags your
  ship. Weaker than your thrust, so you can always fly out.

**Changes**
- No blue asteroids past wave 10 (they harden to green).
- Non-boss waves shortened 180s → 120s (reaching wave 15 is ~28 min, not ~40).
- Post-boss flow: a "NEXT WAVE IN n" countdown, then the WAVE banner (no more overlap).
- The boss run-up now previews the *actual* incoming boss, and stray mobs retreat off-screen first.
- Warping the gold 1UP grants the life (it's a player action).
- Dev **F2** wave-skip now works on every boss wave.
- Fixes/polish: Slinger sparse field, pulser sparks white, rocks dissolve when a boss clears the
  field, Devourer size floor, orange blast VFX, persistent shot-mode indicator.
- CI: macOS `.dmg` "no space" fixed (strip the binary + free cargo caches).

## v0.2.6 — Controls, waves 11–15, the Limpet & Glutton (2026-07-20)

- **Waves 11–15** as bespoke content: orange **explosive** asteroids wired into the field (all-orange
  wave 14), and **The Limpet** — a parasite mob (waves 12–13) that tethers to a rock and peeks out to
  fire; break the rock or flank it.
- The **Glutton** (wave-10 boss) now starts at full health and heals less; the **warp** grid glows and
  crackles and its pull is stronger.
- **Controls screen** does full input rebinding for keyboard/mouse *and* controller (the separate
  Settings screen was merged in).
- CI emits launcher-compatible native assets; wired into the Baz Studios launcher.

## v0.2.5 — Controller support (2026-07-20)

- Play with a controller *or* keyboard/mouse (both live at once); input-method auto-detect + a full
  rebinding screen.

## v0.2.0–v0.2.4 — Initial native port (2026-07-17–18)

- First GitHub releases of the Rust + Bevy port (from the JS/Canvas original): core Asteroids loop,
  the Warden (w5) and Glutton (w10) bosses, chain + mass pickups, the gold 1UP economy, top-5 high
  scores, menus/achievements, procedural audio.
- Release pipeline: CI builds Windows / macOS / Linux; macOS ships a real `VIOLET EDGE.app` in a
  drag-to-Applications `.dmg`; embedded exe icon + window logo.
