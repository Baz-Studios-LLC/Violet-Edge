# VIOLET EDGE — design reference

Living roster of hazards and enemies. **✅ implemented · 🔷 planned.** Behaviour notes are the
intended design; implemented rows describe what the code does today.

## Design rules

- **Asteroids are the theme.** Every other hazard exists to change *how you engage the rocks*, never
  to replace them. Enemies stay a fraction of the live asteroid count and thin themselves out — the
  field is always mostly rocks.
- **Each boss weaponizes one *relationship to asteroids*** (a verb): hoard · eat · shoot · prime ·
  pulse · pull · split · reflect · link. The boss is a lens on the rock field, not a separate spectacle.
- **Bosses are SPECTACLE and never static (user rule, 2026-07-28).** Every boss carries an idle-motion
  layer (breathing shells, gnashing teeth, spinning drums, rippling tentacles, waving cloak wisps),
  movement character (lunges, recoil, sway — not flat drift), and a STAGED death (parts shear off one
  by one → core failure → the shared final blast). One canonical `draw_X_body()` per boss is shared by
  the fight, the run-up WARNING BANNER, and the background cameo, so the silhouette never drifts.
- **Each powerup is thematically derived from the boss whose defeat drops it** — the reward echoes the
  mechanic you just beat (chain shot ← the Warden *linking* rocks; mass shot ← the red Glutton gaining
  *mass*; drone ← the enemy *ship*). The base Warp weapon is core kit and exempt.
- **Difficult but fair.** Manageable chaos is welcome; only pull back at unavoidable/instant death.
  Leave skill-gated tricks for players to discover rather than hand-holding them.
- **Each new asteroid escalates.** Every new asteroid type should be genuinely dangerous and harder to
  manage than the last — the player must *learn* each one (spacing, engagement order, timing). A new
  asteroid that feels harmless is a bug; lean into the threat (bounded only by the instant-death line).

## Asteroids

**ACT OWNERSHIP (user rule, 2026-07-28): no rock type outlives its act.** Act I (1–10) = blue
(green bridges in at 6); Act II (11–20) = green (carrier) + orange + pulser; Act III (21–29) =
red (carrier/fallback) + beacon + cluster. Each act opens with an all-X teaching wave for its new
carrier (7 all-green, 16 all-pulser, 21 all-red). The **wave-30 finale is the ONE exception**: its
field rolls random across every type (`roll_finale_kind`). Boss fields stay themed within their
act's roster (10 = blue food, 20 = orange + green fodder, 25 = pure red).

| Colour | Status | Behaviour |
| --- | --- | --- |
| **Blue** | ✅ | The standard rock. Sizes L/M/S (radius 88 / 46 / 30); a hit splits it into two of the next size down, the smallest is destroyed. One bullet per break. |
| **Green** | ✅ | Dense — takes multiple hits (HP = size, so a large green needs 3). A normal bullet *chips* it; the chain beam or a mine blast shear it in one, and a **mass shot** destroys it outright. Bridges in wave 6, owns 7–9, CARRIES Act II (the 11–19 baseline), and retires with its act at 20. |
| **Orange** | ✅ | Explosive. Instead of splitting, a destroyed orange **detonates** after a brief fuse — a big AOE (`ORANGE_BLAST_R`) that **destroys everything inside outright** (rocks obliterated, not split), pops mines/enemies, kills the ship if caught, and lights other oranges (chain reaction). **Gold is spared**; bullet/chain/mine all *light* it. Act II only: debuts 11, peaks at 14, last seen 19 — its own boss's wave 20 is pure green fodder (the Detonator can't prime a rock that's already a bomb). |
| **Red** | ✅ | Grows, like the Glutton boss: absorbs nearby asteroids (other reds included) to gain size. A broken red splits into two reds, and *those* absorb and swell back — a whack-a-mole unless you clear the field around them. Soft (1 hp); mass/warhead/chain/mine destroy one outright, no regrow. **Act III's CARRIER**: wave 21 is all-red (the teaching wave), it's the 21–29 fallback, and the Pulsar's wave 25 is fought over a pure red field. |
| **Pulser** | ✅ | Pulses bright white ↔ dim on its own **slow** beat (`PULSE_RATE`, ~3.7s, per-rock phase); **invulnerable while LIT** — bullets/chain/mine-blast all no-op on it (a shot fizzles with a white spark). Hit it on the **dark** beat. Breaks into **smaller pulsers** (a sustained timing puzzle, not inert rubble); internally dense so there's never any blue. Act II only: debuts 16 (all-pulser), retires with its act at 20 — the Pulsar boss carries the beat into Act III itself. `pulser_lit()` derives the beat from global time. |
| **Cluster** | ✅ | Fractured pale-ICE rock (visible crack lines). Breaking it **SHATTERS it into a ring of ~7 tiny fast shards** instead of two chunks — point-blank shots become a bad habit; spacing matters. The **mass shot vaporizes it clean** (no shards) and the **warp swallows it whole** — the first rock where tool choice really matters. A mine-triggered shatter flings the ring even faster. Debuts wave 26 (splits Act III into two eras). |
| **Beacon** | ✅ | Teal **aura warden** (`BEACON_AURA_R` = 270, up from 200 — big enough to own a region; the reach ring is drawn soft-but-legible): every non-beacon rock inside its aura is **immune to gunfire and the chain** until the beacon falls — the field becomes a target-ORDER puzzle (the pulser gates *when* you shoot; this gates *what you shoot first*). Warhead rounds fizzle inside it too (test-pinned); the beacon itself is always shootable. Blasts, the warp, and red-absorption bypass the aura (counterplay). Spawns dense (chips like a green), **never splits** — it dies clean. Rare (~10-12%/roll). Debuts wave 23, replacing green's retired tank role with something smarter. |
| *Split economy* | ✅ | **(2026-07-31, user design)** Breaking a rock no longer guarantees two children: a LARGE sheds **1-2 mediums** (60% two), a MEDIUM sheds **2 smalls or dies clean** (55% split). Cuts a large's average lineage from 7 entities to ~4.4 — small debris stops silting the screen, and breaks have variance. Exempt: **gold** (the 1UP hunt's lineage length is tuned economy) and **red** (split-and-regrow IS its identity); the cluster keeps its own shatter rule. `split_children` in main.rs. |
| **Gold (1UP)** | ✅ | Not a hazard — a *reward*. A rare shimmering gold rock that drifts in at random times during play (any wave, boss waves included). Destroy the **whole lineage** (it + every fragment) for +1 life. Its pieces get a **long grace** (`GOLD_GRACE`, they recycle) so they're never lost *immediately* — but after that a piece that drifts off is culled and the life is **forfeit**, so clear them before they scatter. **Only your shots break it** — mines bounce off, the Devourer won't eat it. See Life economy below. |

## Enemies

Minor by design — capped as a fraction of the live asteroid count, and they flee/despawn over a
lifetime so the field stays mostly rocks. Three types total is the ceiling; more starts competing
with the asteroids for attention.

| Enemy | Status | Behaviour |
| --- | --- | --- |
| **Yellow mob** | ✅ | Standard enemy ship. Glides in, hovers and strafes around the player, steers clear of rocks/mines, lobs slow (dodgeable) shots, and flees off-screen after a lifetime. Runs in two windows: waves 3–4 and 8–9. |
| **Darter** | 🔷 SHELVED | Fast interceptor — telegraphs, then charges. Shelved with the mob de-emphasis (2026-07-28): the asteroids are the star; the Limpet was removed outright and no Act II/III mob replaces it. |

## Bosses — the 50-level ladder

Every 5th wave. Each weaponizes a different *relationship to asteroids* (see Design rules). **Every
boss shows a top-center HP bar** (shared `boss_hp_bar`, tinted to the boss) and STARTS full. The
Glutton starts full too (heal cap == starting HP); eating heals the damage you've dealt back toward
full (never past — it grows in *size*, not max HP), so letting it feed visibly refills the bar.

**Run-up telegraph (last 10s, `BOSS_CAMEO_SECS`):** the *actual* incoming boss drifts across the
background as a faint, ALIVE silhouette (`draw_boss_idle` — the same canonical body the fight uses),
a music riser builds, and stray mobs clear off. On top of that a **WARNING BANNER** frames the warning
line: a hazard band with marching edge-ticks and the boss's true mini body beside its name, plus the
full-screen tint pulse — all rates ≤3 Hz (`boss_warning_update` + the banner block in `render_boss`).

| Wave | Boss | Status | Verb → mechanic | Counterplay |
| --- | --- | --- | --- | --- |
| 5 | **The Warden** | ✅ | *Hoard* — shield of captured rocks on rotating arms; hurls the small ones | Strip the shield, shoot the core through the gaps (bullets hurt the core; chain/warp don't) |
| 10 | **The Glutton** | ✅ | *Eat* — red seeker devours free rocks to grow bigger & tankier; gorged to full it **OVERLOADS**: swells huge (flashing white as a tell), detonates a near screen-wide blast (wipes the field, kills you unless you're far — `DEVOURER_BURST_R`), then shrinks to nothing and starts over | Starve it (clear the field) + **shoot it down** — gunfire chips its HP *and* claws its size back (`DEVOURER_SHRINK_PER_HIT`), so active fire holds off the overload; when it's swollen and flashing, **get clear** before it bursts |
| 15 | **The Slinger** | ✅ | *Shoot* — a large gunship that hovers high and TRACTOR-BEAMS a field rock, reels it to its muzzle, then fires it at you like a cannonball; exposed core, no shield. Its wave is green (dense) rocks, so grabbed rounds resist being shot away | Dodge the fast shots (you can't reliably spam-break a dense round); chip the core between barrages |
| 20 | **The Detonator** | ✅ | *Prime* — primes nearby rocks into live bombs; **armored EXCEPT while priming** (its chartreuse core opens for the channel) | Punish the priming window — unload on the exposed core; dodge the bombs it plants |
| 25 | **The Pulsar** | 🔷 | *Pulse* — invulnerable while lit; shockwaves fling every rock (and you) outward | Hit only on the dark beat; don't get pinned to a wall |
| 30 | **The Phantom** ("The Haunt") | ✅ | *Untouchable* — an INTANGIBLE ghost (shots pass through) that must SURFACE to be hit; p1 sweeps a quadrant ray, p2 POSSESSES a homing rock (break the vessel to rip it out), p3 drops the mask — solid, charging, searing a lethal spectral wake | Bait the ray & punish the surface window; break the possessed vessel to expose it; sidestep the locked charge line and don't get walled in by the wake |
| 35 | **The Hive** | 🔷 | *Split* — the boss **is** an asteroid; every hit mitoses, fragments re-fuse if ignored | Burn all pieces down before they merge |
| 40 | **The Prism** | 🔷 | *Reflect* — facets bounce your shots; spawns crystal rocks that also reflect | Catch an open facet, or shoot it *through* a rock |
| 45 | **Gemini** | 🔷 | *Link* — twin ships tethered by a shared rock core; damage transfers between them | Break the core rock to sever them, then focus one |
| 50 | **The Progenitor** | 🔷 | *All* — the first asteroid; cycles the earlier verbs as phases | Apply each phase's counter in turn — a full-run mastery check |

## Pickups (powerups) ↔ boss mapping

Each boss drops a powerup that echoes its own mechanic.

| Boss (drop) | Powerup | Status | Thematic tie |
| --- | --- | --- | --- |
| Warden (W5) | **Chain Shot** | ✅ | beam arcs/*links* between rocks — the Warden links rocks on its arms |
| Glutton (W10) | **Mass Shot** | ✅ | fat, slow rounds that **destroy any rock in one hit, no chunks** (the field-clearing tool; only a bit stronger than standard vs bosses, and its slow rate keeps standard the better boss DPS) — the red Glutton gains *mass* |
| Slinger (W15) | **Drone** | ✅ | an ally craft that orbits the ship a short distance out and auto-fires the player's Bullet at the nearest asteroid in range — mops up rocks you left behind (one per run) |
| Detonator (W20) | **Warhead rounds** | ✅ | permanent passive — every primary shot makes the rock it hits **detonate & chain** in a **violet, player-SAFE blast** (gold is spared; your own boom won't kill you) — echoes the primed bombs |
| Pulsar (W25) | **Nova Shield** | ✅ | a regenerating **one-hit barrier**: while UP it eats one lethal hit and collapses; after `NOVA_REGEN` (~9s) it **flickers back on** (its ring blinks ≤3 Hz as it re-lights). A hit while it's DOWN costs the life as normal. The player inherits the Pulsar's lit-invulnerable ↔ dark-vulnerable identity. (Replaced the earlier "Nova pulse" shockwave sketch — playtest direction 2026-07-28.) |
| Phantom (W30) | ~~Magnet~~ **CUT** (2026-07-30) | ❌ | the wave-30 kill ends the game — the victory cinematic plays immediately, so there is nothing to pick a drop up WITH. The Phantom deliberately drops nothing; its "reward" is the ending + NEW GAME+ unlocking on the menu. |
| Hive (W35) | **Spread shot** | 🔷 | your shot *splits* into several — echoes mitosis |
| Prism (W40) | **Ricochet rounds** | 🔷 | bullets *reflect* off walls/rocks — echoes the facets |
| Gemini (W45) | **Twin cannons** | 🔷 | two linked fire streams — echoes the twins (kept distinct from the drone) |
| Progenitor (W50) | — | — | final boss; no drop (or a combined ultimate) |

> Resolve at implementation: Drone (W15) vs Twin cannons (W45) must feel distinct, and Magnet (W30)
> must not just re-skin the base Warp weapon.

## Progression — the standard run (3 acts, waves 1–30)

Each new asteroid debuts a few waves *before* the boss that weaponizes it: learn the toy, then fight the
thing made of it. Waves **1–30** are the whole standard run — a six-boss arc that **ends at wave 30**
(beating the Phantom → RUN COMPLETE). **There is no wave 31+.** A **New Game+** is planned as a
**separate mode** that replays waves 1–30 at higher difficulty — *deferred until the standard run is
perfected*. (The `content_wave` loop past 30 is only a technical fallback the standard run never reaches.)

| Act | Waves | New asteroid(s) | New enemy | Bosses |
| --- | --- | --- | --- | --- |
| I — The Field | 1–10 | Blue, Green ✅ | Yellow mob ✅ | Warden (5), Glutton (10) |
| II — Volatile | 11–20 | Orange (explosive) ✅, Pulser (invuln-lit) ✅ | — (Limpet REMOVED 2026-07-28) | Slinger (15) ✅, Detonator (20) ✅ |
| III — Unstable | 21–30 | **Red (growing)** — new; **green phases out** here (oldest type), leaving **orange + pulser as the standard field** | — | Pulsar (25), **The Phantom** (30) |
| IV — Deep Belt | 31–40 | **Crystal** (reflects) *or* **Ice** (shard-burst) — TBD | — | Hive (35), Prism (40) |
| V — The Core | 41–50 | **Void** (swallows bullets) *or* **Magnetic** (bends fire) — TBD | — | Gemini (45), Progenitor (50) |

> **Scope (updated):** the standard run **caps at wave 30**. In scope: Act III's **Pulsar (25) + Nova
> pulse** and **The Phantom (30) + Magnet**. **Acts IV–V (waves 31–50, bosses 35–50 + their powerups) are
> SHELVED** — New Game+ replays 1–30 harder instead of adding waves. The 31–50 rows in the ladder/pickup
> tables above are kept as *parked ideas only* (a maybe-someday beyond NG+), not current plan.

### Waves 11–15 ✅ (Act II front half)

Per-wave content plan. The rock mix lives in `roll_rock_kind` (orange fraction ~0.25 on 11–13, 1.0 on 14):

| Wave | Content | Section status |
| --- | --- | --- |
| 11 | green + orange | wired ✅ |
| 12 | green + orange (pure rock wave) | Limpet mob REMOVED 2026-07-28 |
| 13 | green + orange (as 12) | orange/green wired ✅ |
| 14 | orange only | wired ✅ |
| 15 | **The Slinger** (boss) + green only | green wired ✅ · Slinger ✅ · Drone drop ✅ |

Build order (one section at a time): **1. orange mechanic ✅ → 2. wave restructure + orange/green
wiring ✅ (§A) → 3. (Limpet mob — since removed) → 4. Slinger boss ✅ (§C) → 5. Slinger's Drone powerup ✅.**

### Waves 16–20 — building now

`content_wave` is now identity through **30** (`rem_euclid(30)` loop after 30). No blue past wave 10;
waves 11–20 harden leftovers to green, and from content 21 on (Act III) green retires so leftovers are
orange. The **Pulser** debuts here, and
the **gravity Well** hazard appears on 18–19 (no mobs in Act II at all — the Limpet was removed).

| Wave | Content | Status |
| --- | --- | --- |
| 16 | pulser ONLY (a pure timing wave to learn the beat) | ✅ Pulser mechanic + wiring |
| 17 | green + orange + pulser | ✅ wired |
| 18 | pulser-heavy + orange + **Well** | ✅ wired |
| 19 | green + orange + pulser + **Well** | ✅ wired |
| 20 | **The Detonator** (boss) + green fodder (no orange — the boss can't prime bombs) | Detonator ✅ · Warhead drop ✅ |

The **gravity Well** (`WELL_*`, ✅): an "opposite warp" HAZARD — a small, tight rose-red swirl that
**pops in at random intervals** (`WELL_MIN_GAP`..`WELL_MAX_GAP`), drags the *ship* toward it
(`well_pull`, under `THRUST` so you can always fly out — a compile-time invariant), and **collapses
after ~5s** (`WELL_LIFE`). A fleeting flight-disruptor, not a fixture: it doesn't kill on its own — the
threat is that it yanks your movement while you're dodging. Ship-only pull, ≤2 at a time. A
field-hazard preview of the Phantom's *Pull* (W30).

The Detonator (§D, ✅): boss 4, wave 20 — a hazard-**chartreuse** armored core. Invulnerable while it
drifts; it drifts UNTIL it reaches a rock (within `DETONATOR_ATTACH_R`), then HALTS and PRIMES that rock —
a `DETONATOR_PRIME_SECS` channel (2.5s, retuned from 1.5 — the window was too short to land real damage
once drift/search time was paid) with a chartreuse **beam** to the rock, its core OPENING (the ONLY
window to damage it, `det.prime > 0`). The primed rock becomes a live bomb (a `Detonating` rock on
`DETONATOR_BOMB_FUSE`) to dodge. It never primes "nothing" — no rock in reach ⇒ keep drifting in; it
never primes gold or an ORANGE (already a bomb). Wave 20 is therefore **all-green fodder** (retuned
2026-07-29: the old orange-heavy mix filled the field with unprimeable rocks, leaving the boss hunting —
armored — for a green; the boss brings the explosions now). On death it drops **Warhead rounds** (permanent passive: every primary shot
makes the rock it hits detonate + chain, reusing the orange pipeline; gold is spared). Warhead blasts are
tagged `Detonating { friendly: true }` — **violet and player-safe** (skips the ship-kill) — vs the orange,
lethal `friendly: false` bombs (boss primes, orange rocks, mines); the flag propagates through chains.

### Waves 21–30 — Act III "Unstable" (building now)

The standard run's finale act — it **ends at wave 30** (beating the Phantom → RUN COMPLETE); there is
no 31+. A New Game+ (a separate mode replaying 1–30 harder) is planned but **deferred until the standard
run is perfected**.

- **§A·1 ✅** — wave engine extended to author 21–30 (`content_wave` loop → 30); field rebalanced:
  **green retires** (thin transition on 21–22, gone after) and **orange + pulser are the standard field**
  (leftovers now fall back to orange, not green). Boss waves 25 & 30 are **Warden placeholders** until §B/§C.
- **§A·2 ✅** — the **Red (growing)** asteroid (`Red { cool }`, `RED_ABSORB_*`, `red_growth`): absorbs the
  nearest non-red rock within reach every ~2.6s to swell one size (cap large), staying soft (1 hp). A
  **plain shot splits it into smaller reds** (they eat the field back up — whack-a-mole); **mass / warhead
  / chain / mine destroy it outright, no regrow** (the counters). Never eats gold / live bombs / boss-held
  rocks. Debuts w21; ~25–40% of the non-boss Act III field.
- **§B — Pulsar boss (25) ✅ + Nova Shield drop ✅**: electric white-cyan; invulnerable while LIT / open
  while DARK (reuses `pulser_lit(phase, t)`); on a beat it emits a `Shockwave` that flings every rock +
  the ship outward (`PULSAR_SHOCK_*`). Counter: shoot it on the dark beat, don't get pinned to a wall.
  Slow drift-chase so it can't be camped; contact kills. On death it drops the **Nova Shield** orb (see
  the powerup table): a regenerating one-hit barrier, absorbed uniformly in `kill_ship` (every death path
  funnels there), state in `Run.nova`. Still open: the *W25 two-older-boss variant*.
- **§C — THE PHANTOM ("The Haunt"), boss 6 / wave 30 (the FINALE) + Victory finale**: a **spectral predator
  too arrogant to be touched** — its OWN mechanics per phase (the earlier channel-the-fallen-bosses design
  was cut: it played like a grab-bag). The fight's arc is stripping that arrogance away. The deliberate
  **exception to "asteroids are the star."** Beating it → **`GameState::Victory`** ("YOU SAVED THE PLANET" +
  NG+ teaser; Enter → Menu), latched immediately on the kill (a stray rock can't preempt the win).
  - **The core loop — INTANGIBILITY:** it's a ghost (`vuln <= 0`): shots pass straight through (`collisions`
    skips it), its body drifts harmlessly through the ship. **Firing the Sweep Ray forces it to SURFACE**
    (`vuln = PHANTOM_MATERIALIZE`, 1.6s): solid, still, hittable — and lethal to touch. **Bait the ray,
    punish the recovery.**
  - **Per-phase pool + RESET** (`PHANTOM_PHASE_HP = 30` refills each phase; `transition` reset beat between
    phases, `PHANTOM_RESET_SECS`; phase advances only via a completed reset). Clear phase 3 → win.
  - **The Sweep Ray** (`PHANTOM_RAY_*`, Idle→Telegraph→Fire; **PHASE 1's signature only** now — p2 possesses,
    p3 charges): **AIMS at the player** — the 90° quadrant centres on the ship's bearing (+ jitter), so it
    sweeps the corner you're nearest (~1.7s tell = the dodge window). The beam (swept-arc `angle_in_arc`,
    frame-rate-robust) spans the **full arena diagonal** (`arena.half.length()*2 + 40`, so it reaches the far
    edge from any position, not just centre) and vaporizes rocks + kills the ship. It roams an unhurried
    Lissajous (`PHANTOM_ROAM_EASE`), holding still while a beam is live or while surfaced. Its spectral body
    **dissolves any asteroid it drifts through** (`phantom_dissolve`, spares the p2 seek target) — never clips.
    Every cue is its **own sound** (`SoundFx::Haunt`), not the warp.
  - **P1 — HAUNT:** the ghost + the ray. Learn the bait-and-punish rhythm.
  - **P2 — POSSESSION:** it **SEEKS a real field rock**, glides to it (`PHANTOM_SEEK_SPEED`), and **dives IN**
    — that rock is consumed and reborn as a haunted **vessel** (`Possessed`) it hides inside, unhittable;
    **shots hit the vessel, not the ghost**. The vessel homes at the ship + kills on contact
    (`possessed_update`), and gunfire chips its `PHANTOM_POSSESS_HP` (a hook in `collisions`). **Break it and
    the Haunt is RIPPED OUT into the open** — it surfaces (`vuln`) for the punish window (the *same* damage
    path as P1's ray-recovery). Then it hunts the next rock (state: `seeking` / `possessed` / `dive`) until
    the pool's gone. No ray in P2. Its own new mechanic — a bookend that turns the belt back into the fight;
    retired the old decoy shell-game.
  - **P3 — HUNT:** the mask drops — **solid full-time** (always hittable, body kills on contact) and **NO
    beam**: a pure charger. It **locks the ship's bearing** (`PHANTOM_CHARGE_AIM` telegraph line, eyes
    blazing), then **DASHES** (`PHANTOM_CHARGE_SPEED/SECS`), **searing a wake of lethal spectral afterimages**
    (`SpectralTrail`, lethal `PHANTOM_TRAIL_R` for `PHANTOM_TRAIL_TTL`) that wall the no-wrap arena; the wake
    dies with it on the win. **Desperation:** the more of its final pool you strip, the less it waits between
    lunges (`PHANTOM_CHARGE_EVERY × (0.4 + 0.6·hpFrac)` — base gap down to ~40% at death's door; the aim/dodge
    window is FIXED, fairer-but-relentless), and it **stalks** — biasing its roam toward the ship (35%→75% as
    it nears death) so it's always closing in.
  - **Look:** the spectral skull (shared `draw_haunt_skull`: domed cranium, angry brow, ember eyes that blaze
    when it attacks or locks on, nasal, clenched teeth) — **ghost-faint + edge-wavering** while intangible;
    when surfaced the **skull CRACKS OPEN — jagged fractures + a molten core burn through, sealing as `vuln`
    runs out** (read the boss's own form, not a UI ring — the old containment ring was cut for being too
    gamey); solid all of P3; hue morphs per phase (spectral → chartreuse → hot rose); phase-break flash. (The
    phase pips were removed — they clipped the boss-name HUD text.)
  - **The finale field arrives in SEQUENTIAL mono-type GROUPS of ten** (`FinaleGroup{idx,remaining}` +
    `top_up_asteroids`, `FINALE_GROUP_SIZE`): 10 blue → (field clear) → 10 green → orange → pulser → red → …
    `boss_director` clears the field + resets the cycle on the Phantom's arrival. Each group **trickles in a
    rock at a time** (`FINALE_TRICKLE`), never a wall of ten drifting on together; a colour starts only once
    the field's clear (`FINALE_GROUP_GAP`). Reds absorb the nearest rock **including other reds**, so the
    all-red group **consolidates** into fewer, bigger threats instead of drifting inert.
  - **It's the player's fight:** the ally Drone is deliberately **excluded from targeting the Phantom**
    (`drone_update`'s boss query drops `With<Phantom>`) — no auto-fire at the ghost, no tracer giving away
    the ghost. Beating it awards `BOSS_SCORE`, then runs a **cinematic DEATH SCENE** — **event-driven, not a
    fixed timer** (`PHANTOM_VICTORY_SECS` is only a safety cap), in two beats (`Phantom.erupted`): (1) GATHER —
    the boss glides back to the **MIDDLE**; (2) ERUPT — it **pops every asteroid** and a **layered burst of
    light** (speeds 420→1400) **FILLS the screen** in all directions from `Vec2::ZERO`, and the **true-form
    core** tears free and **flees EAST** as the `EscapeShard` (ease-in `MIN→MAX` speed) — small + subtle, lost
    among the light (the sequel seed). Once the shard has **left the arena**, the hero's ship
    **warps off east after it** — a cosmetic `DepartingShip` (`departing_ship_update`, `SHIP_DEPART_SPEED`);
    the real `Ship` is despawned so control/bounds don't fight it. **Only once BOTH have cleared → Victory.**
    Latched HARD: zeroes `run.respawn`, holds the arena **calm** (`wave.calm` — the calm-countdown HUD is
    suppressed on wave 30, so no stray "NEXT WAVE IN"), AND **shields the ship** (`invuln`) so a last-life
    stray hit can't stomp the win into a GameOver.
  - **Dev F4 (`dev_face_phantom`, debug only)** — wipes the field (keeps the ship) + jumps to wave 30 (resets
    the group cycle) so the finale can be tested without clearing 29 waves. Dev F2 sets phase 3 + zero → the
    win path in one press. (Dev keys are gated to the Playing state.)
  **The standard run is beatable end-to-end — six bosses, waves 1–30.** The Magnet (§C) is CUT — the
  Phantom drops nothing by design (see the powerup table). Formerly listed as still open
  powerup drop (the Nova Shield ✅ shipped); then balance tuning. The **achievements pass ✅ shipped
  (2026-07-28), expanded to 23 (2026-07-29)**: every boss has one, *Edgelord*/"beat the game" keys on the
  real wave-30 Haunt kill (recorded at the erupt, with *Purist* and *Untouchable*/deathless riding the
  same moment — `Run.died` tracks real deaths, Nova absorbs don't count). One grind PER ROCK TYPE
  (`ACH_BLUE` 1000 / `ACH_GREEN` 500 / `ACH_ORANGE` 400 / `ACH_RED` 400 / `ACH_PULSER` 300 /
  `ACH_CLUSTER` 300 / `ACH_BEACON` 100 — scaled to how much of the run each type inhabits), lifetime
  grinds (`ACH_MINES` 250 / `ACH_GOLDS` 25 / `ACH_WAVES` 250 / `ACH_WARPS` 150), **PACIFIST**
  (2 straight waves, zero breaks of any kind — warp/chain/mass/warhead fires count even on a miss;
  dying does NOT reset it: restraint, not survival — per user call 2026-07-29), and the **restart
  ladder** (10/25/50 runs — *Back for More* / *Sisyphus* / *The Definition of Insanity*; every Start
  counts and saves immediately, celebrating the die-a-lot loop instead of punishing it). All are
  deliberately steep careers: players die a lot and stats span every run. `credit_rock_kill` is the one
  source of truth for kill → counter routing (beacon/pulser/red/cluster check before the dense/blue
  split, since specials are dense internally). Save format extended 6 → 12 → 21 fields, old saves load
  with the new fields defaulted (nothing lost, nothing wrongly granted).

The Slinger (§C, ✅): boss 3, wave 15 — a large **ice-blue gunship** (its nose/cannon tracks the
player; unique boss colour, apart from the Warden's magenta + Devourer's red). Glides in, then hovers
high mirroring the ship's x. **Tractor beam:** it grabs the nearest field rock (tags it `Cannonball`,
draws a beam), reels it to its muzzle at `SLINGER_REEL_SPEED`, holds `SLINGER_HOLD`s, then launches it
at the ship at `SLINGER_CANNON_SPEED`; grabs every `SLINGER_COOL`s. Because its wave is **green
(dense)** rocks, a grabbed round takes several hits to break — you can't spam it away, you *dodge*.
Grabs refill from the field (`top_up`) so it never runs dry; a launched round despawns off-screen. Its
wave runs a **sparse field** (`SLINGER_WAVE_ROCKS`, the beam's ammo reservoir), cleared when it arrives
(clean green-only slate — the Warden/Devourer keep their rocks). Exposed core (`SLINGER_HP`, no shield).
On death it drops the **Drone** pickup (`DRONE_*`): an ally that orbits the ship (`DRONE_FOLLOW_DIST`)
and auto-fires the player's Bullet at the nearest asteroid within `DRONE_RANGE` — one per run, cleared
on a field wipe. Rule of thumb going forward: **each
boss gets a unique colour** (Warden magenta · Devourer red · Slinger ice-blue · Detonator chartreuse).

The Limpet (§B, ✅ core): a cyan parasite that TETHERS to a large rock — it rigidly rides the rim
(glued to the rock's edge with little gripping claws, not floating near it). **Peek-to-fire:** it
hides on the FAR side (rock between it and the ship — protected), then POPS OUT around the rim to the
ship-side and fires the slow `EnemyBullet` only once the lane is clear of its host (never *through*
the rock), then ducks back. It's exposed on the near side while shooting — that's the kill window.
Its host is a shield — rock-side shots are blocked (`guard` half-plane); you kill it by catching it
popped-out/flanked, or EXPOSED while it transits between rocks. Break its host and it scrambles to
another large rock — it re-tethers until *it* is destroyed (**1 HP** — dies in one hit; a mob never
out-HPs the ship). Slide rate `LIMPET_TURN`. Gated to waves 12–13 (cap `LIMPET_MAX`); the old yellow
lobber stays off 11–15 via `enemy_target`. **Warp kills it**
✅ (yields to a nearby hole → dragged off its rock + consumed, like everything except the player,
bosses, and boss-held rocks). **Pass-2 TODO:** direct hits from the orange blast + chain beam (today
those kill it only by destroying whatever rock it's on).

## Lore (CANON — user-set 2026-07-28) ✅ implemented

**THE ARCHITECT** is the true antagonist: an entity that **destroys planets and shelves the pieces**
— the Belt is its *collection*, and **every asteroid is a chunk of a murdered world**. The Haunt
(the Phantom, boss 6) is its **steersman** — the Architect's hand, sent ahead to take the player's
world; each earlier boss is one of its instruments (Warden = the collection's keeper, Glutton =
renders the dead worlds down, Slinger = fires planet-shards as ammunition, Detonator = arms the
wreckage, Pulsar = the herdsman whose beat drives the Belt). The wave-30 death scene is canon: the
Haunt's **core flees east — it was NOT destroyed** — and the ship gives chase. **That escape is the
sequel hook**; the ending scene itself is final (user: "the ending is fine").

**KEEP THE MYSTERY (user rule, 2026-07-28):** the destroyed-worlds truth is never stated up front — it
is **paced**. The story is told as the **PILOT LOG**: field reports the Violet Cutter's pilot transmits
home, one per contact, written in first person. The arc: anomaly (a field *holding formation*) → a
keeper?? (Warden) → wrong minerals in the debris (Glutton — "rerunning the assay") → these things
*operate* the field (Slinger) → **strata in the rock** — dread, unsaid (Detonator) → the heading is HOME,
"this is a delivery" (Pulsar) → the steersman, "an acquisition", the core flees (Haunt) → only the FINAL
entry says it plainly: the ARCHITECT breaks worlds for parts, "every rock I've shot was somebody's
ground. I'm going after it." (the sequel).

Surfaces: the **BRIEFING** stays mystery-clean (user's text: a large mass approaching fast, the VIOLET
CUTTER deployed — a prototype ship, one pilot, possibly the only chance; objective: "Investigate and
hold back the approaching mass" — the old "Cut the field, hold the edge" closer was CUT as cheesy);
the **PILOT LOG screen** (main menu, `GameState::Lore`, button `PILOT LOG n/8`) holds the 8 reports —
**nothing readable on a virgin save**: THE BELT decrypts on the FIRST LAUNCH (`runs >= 1`; its locked
row reads "Awaiting deployment." — user call 2026-07-29: the log shouldn't show before the game is
even started), each boss's report **decrypting when that boss first falls** (gated on the lifetime
`Stats` flags), the wave-30 win opening THE HAUNT + THE ARCHITECT (`lore_entries`); other locked rows
read "▮▮▮ NO SIGNAL / Awaiting transmission — survive wave N." Every decrypt pops a **PILOT LOG
UPDATED toast** in-game (`lore_watch` + `LoreSeen`, seeded from the save at boot so nothing re-toasts;
title in the entry's accent; its own dry two-blip radio sfx `log_sfx_wav`, deliberately not the
achievement fanfare) — the story advancing is a reward, so it's announced the moment it happens. The
**victory screen** names the Architect once ("Far past the edge, the ARCHITECT is still building" — safe
there: the mystery is already resolved by then). Deliberate UX call: **no lore prose on the mid-combat
WARNING banners** (unreadable during a run-up; the log carries it).

## Life economy (implemented: gold 1UP rock)

50 levels on 3 lives is likely impossible, especially a no-powerup **Purist** run — so lives are
recoverable, but only by earning them, via a rare gold asteroid. ✅ Implemented:

- A gold rock **drifts in at a randomized time during play** (a countdown, not tied to wave starts) —
  a distinct shimmering gold large rock that otherwise behaves normally (splits when shot, spraying
  **gold** debris). One hunt at a time; a random gap measured from when it *appears* is armed on each
  spawn. The gap is **WAVE-TAPERED**: short/frequent through the early-mid game (`GOLD_GAP_EARLY_MIN`..
  `GOLD_GAP_EARLY_MAX`, ~2-3 min, waves ≤ `GOLD_TAPER_START`=16 — a spare life matters most then), ramping
  to rare (`GOLD_GAP_LATE_MIN`..`GOLD_GAP_LATE_MAX`, ~4.5-6 min) by wave `GOLD_TAPER_END`=30. **Gated out
  of wave 1** (a life that early is wasted; first one arrives wave 2). `GOLD_INITIAL_DELAY` graces the run
  start. Any wave 2+, boss waves included (the Devourer won't eat it; a rock the Warden grabs is just shot
  off its shield).
- You must **destroy the whole gold lineage** — the rock *and* every gold fragment (gold-ness is
  inherited through every break: bullet or chain) — to claim **+1 life**. `GoldRush` tracks it. The
  **warp counts too** (it's a player action): a hole that swallows the entire lineage grants the life.
- **Long grace, then forfeit.** Gold fragments carry a long grace (`GOLD_GRACE`) during which they
  recycle rather than being culled — so a shot gold never vanishes *immediately*, you get a fair
  window to catch every piece. After the grace, a piece that drifts off-screen IS culled and latches
  `forfeited` (the life is denied even if you clear the rest). So gold can be lost — just not instantly.
- Capped at `LIFE_CAP` (= `START_LIVES`, 3): a gold rock only restores a *lost* life, never above the
  starting count. Purist-safe: a life isn't a powerup.
- **Telegraph:** a single shimmering gold outline (same shape/chunkiness as any rock — the pulsing
  colour is what marks it); clearing it pops an "EXTRA LIFE" toast + a distinct 1UP jingle
  (`life_sfx_wav`, separate from the achievement chime).
- **Only player actions claim it** (shots, the chain beam, and the **warp** — the warp missile detonates
  on gold and the hole swallows it, paying out the life). Mine blasts spare gold rocks, and a drifting
  mine bounces off one instead of detonating — so a mine can't clear the lineage for you. The
  **Devourer** won't eat gold either (both would hand over a 1UP the player didn't earn).

Considered and shelved (could layer on later): score extends, boss-clear +1, perfect-wave meter.

## Related systems

- **Scoring** (classic-Asteroids values — smaller rock = more points, so *finishing* a rock beats cracking it):

  | Target | Points |
  | --- | --- |
  | Asteroid — large / mid / small | 20 / 50 / 100 |
  | Green (dense) | ×2 (40 / 100 / 200) |
  | Enemy mob | 300 |
  | Mine | 150 — but rocks a mine's BLAST breaks score 0 (2026-07-29: the bounty is for the aimed kill; blast collateral can't be farmed). Orange/Warhead blasts still score their rocks — the player lit those. |
  | Boss | 3000 |
  | Warden shield rock (small remnant) | 20 |
  | Rock swallowed by the warp | `WARP_ROCK_SCORE` (25, low flat — no farming) |

  Gold rocks score like normal rocks (their reward is the life, not points). Score is purely for
  ranking — it doesn't grant lives.
- **High scores:** a persisted **top 5** (numeric), saved to `violet-edge.hiscore`. On game over the
  final score slots into the table (`record_high_score`), the screen shows the board with the new
  entry lit and a **NEW BEST!** / **TOP 5!** banner, and the main menu shows a **BEST** line.
- **Death = progress (the anti-give-up system, 2026-07-29).** DECIDED: a game over is a full reset —
  no upgrades persist, no continues; the game is a one-credit arcade run and the boss ladder is the
  difficulty curve (carrying the Warhead into wave 1 would collapse Act I). What persists instead is
  the RECORD: `Stats.best_wave` (deepest wave ever reached, updated by `record_high_score` and the
  win), shown on the Game Over screen as `REACHED WAVE n — BEST m` (gold `NEW BEST!` on a record),
  plus a **nearest-grind ticker** (`nearest_grind` — the unfinished counter achievement with the
  highest fraction, e.g. `TRUE BLUE 612 / 1000`). Lifetime achievements and decrypted Pilot Log
  entries survive every death by design. **Pillar: flying and shooting stay CLEAN — skilled play is
  the reward loop; difficulty may be merciless but the ship never is.** The release bar: the game
  ships when it can be beaten legitimately (no dev keys).
- **NEW GAME+ (2026-07-30, deliberately SMALL — user: "start small since it's essentially a New
  Game").** Unlocked forever once `stats.phantom` is set; the `NEW GAME+` menu button exists ONLY
  then (button-only, no keyboard shortcut — the second lap is chosen, never stumbled into). It IS a
  new game: same `reset_run`, nothing carried, achievements all count. Three difficulty dials, all
  at the SOURCE (never player nerfs): `NGP_POP_BONUS` (+6 rocks every wave, past the cap; mobs and
  mines scale automatically — they're capped as fractions of the rock count; the finale cap gains
  +3; the Slinger arena stays sparse by fight design), `NGP_BOSS_HP_MULT` (cores ×1.5 at spawn, one
  place: `boss_director`), and a music-corruption FLOOR of tier 1 (the Belt is already wrong when
  you return). The HUD wave line carries a quiet `NG+` tag; restarting keeps the mode, normal PLAY
  clears it. **NG+ Act I (waves 1-5) rolls the FULL rock roster** (the finale's all-types mix via
  `roll_finale_kind`) — the lap assumes mastery, no teaching rosters; act arcs resume at wave 6.
  **Every boss carries the mark: `THE WARDEN+`** (warning banner + HUD, `(name)+` in NG+ only).
  **THE WARDEN+** is the first upgraded fight: old kit at 0.65× cadence, a TWO-rock spread per
  throw, and every hurled rock is PRIMED (`Detonating`, 1.7s fuse — shoot it out of the air or
  clear the blast; reuses `detonate` wholesale). Upgraded mechanics for bosses 2-6 come one at a
  time (small steps). Explicitly deferred: rock speed scaling, per-phase Phantom scaling,
  NG+-only rewards.
- **The JUICE layer (2026-07-30, the AAA-bar feel pass).** Hit-stop (`HitStop`, real-time ticked,
  freezes `Time<Virtual>`: player death 0.12s / boss down 0.14s / Nova absorb 0.07s, capped at
  0.14, never stacking) + trauma screenshake (`Shake`, offset = trauma² × 14px max, smooth layered
  sines — NEVER per-frame randomness, that's strobe-jitter) + kill-pop rings (type-colored
  `Shockwave` in `break_asteroid`, size ≥ 2 only so the field can't wash out). Both driven by
  `juice_director` off the EXISTING SoundFx events — one mapping, zero flags in kill sites, and
  anything that sounds big automatically feels big. Boss kills got their own `SoundFx::BossDown`
  boom (they died silently before). Photosensitivity holds: freezes + smooth motion, no strobes.
- **Corrupted music tiers (2026-07-30).** `main_track_variants(6)`: ONE main-track synthesis, six
  masters at rising grit (drive → tanh, bit depth 15→9, sample-hold decimation 1→4). Cue is
  `MusicCue::Main(tier)`, tier = (wave-1)/5 — each boss down, the field's music returns a tier
  wronger; menu/Briefing/Victory play tier 0 (a win hands the clean track back). The lore speaks
  through the mix: the Belt sounds progressively wronger the deeper the run goes.
- **PRODUCED music (2026-07-30) — the score is now ALL produced tracks.** Generated in
  **Antigravity** (the same tool the whole of Wingman was made on, so licensing is settled by that
  precedent) using the old procedural score as the style reference. Shipped: **MAIN**
  (`assets/main.mp3`, arcade drive — supersaw leads, acid sub-bass, 909s, ~26s), **BOSS**
  (`assets/boss.mp3`, dark industrial — tritone stabs, distorted sub-rumble, ~24s), **GAME OVER**
  (`assets/gameover.mp3`, ambient synthwave — Am–Fmaj7–Dm7–E7 pads + Rhodes arps, ~15s). All
  `include_bytes!`-embedded (exe stays self-contained), decoded by bevy's `mp3` feature. The only
  synthesized music left is the boss BUILDUP riser; the procedural score was **deleted** (in git
  history) rather than left as dead code — user call.
  **Per-track wiring checklist:** (1) verify **no edge silence** (`silencedetect`) or the loop
  gaps; (2) **level-match** via `play_track`'s per-cue `gain` — measure `volumedetect` mean against
  the mix (produced tracks have arrived both quieter *and* hotter: `GAMEOVER_GAIN` 1.2,
  `MAIN_GAIN` 0.61, `BOSS_GAIN` 0.73, the last two also reclaiming headroom from full-scale
  masters).
  ⚠️ **Corruption tiers are DORMANT, not removed:** `MusicCue::Main(tier)` and the NG+ tier-1 floor
  are intact, but the tier index **clamps to `dir.mains.len()`** — with one produced main every
  wave plays tier 0, which is also what stops the track restarting at act boundaries (test-pinned:
  `a_single_main_variant_never_restarts_the_track`). To revive the story beat, add per-act produced
  variants to `mains`; nothing else changes. (The old `corrupt()` DSP is gone — reviving it over
  produced audio would mean decoding the mp3 to PCM first.)
- **Flight model (tuned 2026-07-29, hitbox EXPLICITLY untouched — user rule: clean maneuvering must
  never come from shrinking the ship).** `ship_control` is fully dt-correct: turn 5.2 rad/s (~300°/s,
  raised from 4.6 for gap-weaving), thrust 1000 px/s² against heavy drag (`FRICTION 0.15` retention/s,
  ~0.37s speed half-life → terminal ~527, under the 560 cap), so flying is deliberate, not a glide;
  drift management IS the skill (no brake key by design). Edges slide (only the into-wall velocity
  component is killed). Controller: analog trigger thrust (feathered burns), left-stick turn rescaled
  past the deadzone (no threshold kink). Rejected on principle: aim assist (v0.4.2), velocity-alignment
  "grip", speed-dependent turn rates — hidden assists muddy the feel the skill ceiling lives on.
- **Achievements (23):** the boss ladder (First Blood, Warden Off, Glutton for Punishment, Outgunned, Defused, Lights Out — one per boss, named for it), a grind per rock type (True Blue / Green Thumb / Demolition Derby / Beat It / Seeing Red / Ice Breaker / Keymaster), lifetime grinds (Minesweeper, Gold Rush, Wave Goodbye, Event Horizon), the restart ladder (Back for More / Sisyphus / The Definition of Insanity — 10/25/50 runs started), and the capstones (Edgelord = beat the game, Untouchable = deathless win, Purist = no-powerup win). Thresholds live beside the `Ach` enum (§main.rs); the full list with numbers is in the CHANGELOG.
- **Field population:** the on-screen count targets `POP_BASE + wave` (cap `POP_CAP`), topped up from the edges at `SPAWN_INTERVAL`. Edge spawns are ~80% large; a `BIG_FLOOR` keeps large rocks present even at the cap. Rocks that drift fully off-screen are recycled back in *only if large* — small debris usually despawns for good (mids sometimes), so breaking rocks apart can't silt the arena up with an overwhelming cloud of little ones; the top-up refills with fresh large rocks. Freshly-broken fragments get a short grace window (`FRAGMENT_GRACE`) during which they always recycle rather than being culled, so a rock shattered right at the edge can't lose its pieces off-screen before you get a shot. The Warden grabs large/mid rocks for its shield and only resorts to a small one when nothing bigger is on-screen.
