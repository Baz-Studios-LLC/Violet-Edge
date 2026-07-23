# VIOLET EDGE — Changelog

Patch notes for the Rust + Bevy build. Newest first. (Releases are cut to GitHub and picked up by the
Baz Studios launcher.) **Keep this current with every change** — it's the record testers read.

## Unreleased

- **The wave-30 finale is THE PHANTOM — reborn as THE HAUNT** (the channel-the-old-bosses design is out): a
  spectral predator **too arrogant to be touched**, with its own mechanics per phase. The fight is you
  stripping that arrogance away. The deliberate *exception* to "asteroids are the star."
  - **It's INTANGIBLE** — your shots pass straight through, and its ghost-body drifts harmlessly through you.
    **Firing its Sweep Ray forces it to SURFACE**: for a short window after each beam it's solid, still, and
    hittable (and lethal to touch). **Bait the ray, punish the recovery** — that's the fight.
  - **Per-phase health with a RESET:** deplete a phase → it reforms and the next begins. Three phases; the
    **Sweep Ray** (telegraphed quadrant → lethal sweeping beam) runs through all of them, faster each.
  - **Phase 1 — HAUNT:** the ghost + the ray. Learn the rhythm.
  - **Phase 2 — SPLIT:** it **fractures into identical apparitions**. The decoys roam and shimmer exactly
    like the real one and never attack — **only the real one fires (watch for the blazing eyes)**, and after
    every surface window it **shuffles places with a decoy**, so you can't camp it.
  - **Phase 3 — HUNT:** cornered, **the mask drops** — it turns solid full-time (always hittable, kills on
    contact) and **charges across the arena** on a telegraphed lock-on line, **searing a wake of spectral
    afterimages** that linger and kill on touch — the arena shrinks around you while the ray fires at its
    fastest.
  - **The look:** the spectral skull (angry brow, ember eyes that blaze as it attacks) — **ghostly-faint and
    wavering** while intangible; when it surfaces, **the skull CRACKS OPEN and a molten core burns through**,
    the fractures sealing as your window closes (read the boss, not a UI ring); a flash on each phase break;
    **three phase pips** by its health bar.
  - **The finale field arrives in mono-type GROUPS of ten** — ten blue drift in; once the field is clear,
    ten green, then orange, then pulser, then red, and around again. Far less crowded, one colour at a time.
- **Finale fixes + code-health pass** (from an adversarial review):
  - The **win is now truly guaranteed** — a last-life death landing on the exact frame the Phantom dies can
    no longer flip your victory into a Game Over.
  - **Beating the Phantom awards score** now (it was the only boss kill worth 0 points).
  - The **ally Drone no longer targets the Phantom** — it was firing at the intangible ghost and, worse,
    its tracers gave away the real skull among the phase-2 decoys. The finale is your fight.
  - A **live gold 1UP no longer vanishes into the wave-30 slate-wipe** (which used to read as "cleared" and
    hand you a free life at the door of the finale).
  - **Boss music no longer loops over the Victory / menu screens** after a win.
  - Dev keys are gated to gameplay (no more F3 leaking a rock onto the menu); F2 reliably ends the fight.
  - Removed dead code + stale comments left by the finale's earlier designs.
- **Dev F4 — face the Phantom:** jumps straight to the wave-30 finale (wipes the field but keeps your ship,
  then spawns a fresh Phantom) so the final boss can be tested without clearing 29 waves. Debug builds only;
  pair with **F1** (invincibility). *(F1 invincible · F2 wave-skip · F3 spawn-orange · F4 face-the-Phantom.)*
- **The ally Drone now fires at bosses too**, not just asteroids — it targets the nearest asteroid *or*
  boss in range (except the finale Phantom), so it helps in boss fights (where the field is sparse).

- **Shot modes are now a 3-way cycle (Q):** Standard → Mass → Warhead (through whatever you've unlocked).
  - **Mass** is no longer an instant wipe — it's just **stronger** (`MASS_POWER=3`: one-shots a dense rock,
    which then splits normally).
  - **Warhead** is now a **toggle** and *the* instant-destroy tool: a **piercing** round that **passes
    through asteroids**, deleting each one it touches, keeping its **violet blast ring** — and still **no
    chaining**.
  - The ally **Drone** fires plain standard bullets, so it can never be a Warhead machine (no special-case
    needed anymore).
- **Dev F2** steps **one wave at a time** again (no longer jumping straight to bosses) and reliably kills
  every boss type along the way.

- **More playtest tuning + fixes:**
  - **Boss HP now ascends** wave-to-wave — 26 / 34 / 40 / 46 / 52 / 60 (Warden → Singularity). *(This drops
    the Glutton from 70; it's a healer, so it still plays tanky — say the word to keep it beefier.)*
  - **Bosses never touch the gold 1UP:** the Warden won't shield it, the Slinger won't tractor it, the
    Detonator won't prime it, the Pulsar won't fling it, the Singularity won't crush it (the Glutton already
    ignored it).
  - **Warhead nerfed** — its blast is now LOCAL (~110px vs the orange 250) and **no longer chains**, so
    spraying shots doesn't clear the screen. And the **ally Drone's shots are exempt** from Warhead.
  - **Victory screen reveals slowly**, credits-style (lines fade in on a stagger) instead of popping in.
  - **Gravity-Well render** redesigned into a whirlpool — it was a 4-arm cross that read like a swastika.

- **Playtest fixes + finale hardening (waves 12–30):**
  - **Limpets leave the arena** once their waves (12–13) are over, instead of lingering into later waves.
  - Detonator's **primed rocks are now hot red** (were gold — read like the 1UP).
  - **Red asteroids recolored** to a cool crimson (clearly apart from orange), and they no longer throw
    blue sparks or split into blue rocks — a red stays red from every weapon and can grow back to large.
  - **Pulsar (boss 5) is meaner** — stronger, more frequent shockwaves that shove rocks (and you) harder.
  - **Singularity redesigned** — the 3-arm spiral (too swastika-like) is now a 7-arm whirlpool, with a
    stronger, wider pull.
  - **Dev F2** now skips through every boss to the finale (it was stalling at wave 20).
  - **Finale hardening (from an adversarial review):** killing the Singularity now **wins instantly** (a
    stray rock during its death throes could previously flip the win into a Game Over); quitting/restarting
    mid-boss-fight no longer leaves a boss alive (a stale one could fire a false victory next run); and it
    no longer takes "fed-an-orange" damage during its intro invulnerability.

- **The finale is in — the run is beatable start to finish.** Wave 30 is now the **Singularity** (boss 6):
  a gravity core that drags every rock and your ship toward it. Chip its core while you fight the pull, or
  **feed it an orange** (let one get pulled in) for big damage; contact crushes you. Beating it triggers a
  **Victory** screen — *"YOU SAVED THE PLANET"* — that teases the New Game+ unlock. Six bosses, waves 1–30.
  (The boss powerup drops — Nova & Magnet — and difficulty tuning are the remaining work.)

- **The Pulsar (boss 5, wave 25) is in.** An electric white-cyan core that's **invulnerable while lit /
  open on the dark beat**, and on a beat it **shockwaves every rock and your ship outward** — shoot it in
  a dark window and don't get pinned to a wall. (Its Nova-pulse drop and the wave-30 Singularity are next.)

- **Act III begins — the run now reaches wave 30.** The wave engine authors content through **wave 30** —
  the standard run's full six-boss arc, ending there (…Detonator, then Pulsar at 25 and Singularity at 30
  — *bosses still placeholders, landing next*). **Green asteroids retire** across Act III and **orange +
  pulser become the standard field.** New **Red (growing)** asteroids debut in Act III: they absorb nearby
  rocks to swell, and a plain shot splits one into more reds (whack-a-mole) — mass / warhead / chain / mine
  clear them outright.

- **The Detonator — boss 4, wave 20** (closes out Act II). It's **armored except while it PRIMES a
  rock**: it drifts in to a rock, halts, and **beams it** (the beam shows which rock), its chartreuse core
  opening for ~1.5s — that channel is your only damage window. Each primed rock becomes a **live bomb**
  you must dodge. Wave 20 is now all-orange (the bombs it primes). Unique colour: hazard chartreuse.
- **Warhead rounds** (the Detonator's drop) — a permanent passive: every primary shot makes the rock it
  hits **detonate and chain**. The blast is **violet and safe to you** (your own explosions no longer kill
  you) — distinct from the orange, lethal bombs the boss and rocks throw. Echoes the primed bombs.
- **Mass shot reworked** — it now **destroys any asteroid in one hit, with no chunks left**, making it a
  genuine field-clearing tool instead of a slower standard shot. Lit (white, invulnerable) pulsers still
  shrug it off. Against bosses it's only a bit stronger than standard per hit, so its slow fire rate keeps
  the standard shot the better boss DPS.
- **Boss run-up warning** — the 10s before a boss now names the incoming boss on screen ("WARNING:
  THE WARDEN INCOMING") and pulses a full-screen tint in that boss's colour, rising in intensity as the
  wave nears. The faint background cameo silhouette is no longer the only telegraph. The in-fight HUD
  line also names the boss now (e.g. "WAVE 10    THE GLUTTON") instead of a generic "BOSS".
- **Mines toned down** — they no longer scale to a wall: fewer per wave, capped at 30% of the rock
  count and a hard cap of 6, so they stay a garnish instead of a constant swarm.

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
