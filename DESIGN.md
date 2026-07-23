# VIOLET EDGE — design reference

Living roster of hazards and enemies. **✅ implemented · 🔷 planned.** Behaviour notes are the
intended design; implemented rows describe what the code does today.

## Design rules

- **Asteroids are the theme.** Every other hazard exists to change *how you engage the rocks*, never
  to replace them. Enemies stay a fraction of the live asteroid count and thin themselves out — the
  field is always mostly rocks.
- **Each boss weaponizes one *relationship to asteroids*** (a verb): hoard · eat · shoot · prime ·
  pulse · pull · split · reflect · link. The boss is a lens on the rock field, not a separate spectacle.
- **Each powerup is thematically derived from the boss whose defeat drops it** — the reward echoes the
  mechanic you just beat (chain shot ← the Warden *linking* rocks; mass shot ← the red Glutton gaining
  *mass*; drone ← the enemy *ship*). The base Warp weapon is core kit and exempt.
- **Difficult but fair.** Manageable chaos is welcome; only pull back at unavoidable/instant death.
  Leave skill-gated tricks for players to discover rather than hand-holding them.
- **Each new asteroid escalates.** Every new asteroid type should be genuinely dangerous and harder to
  manage than the last — the player must *learn* each one (spacing, engagement order, timing). A new
  asteroid that feels harmless is a bug; lean into the threat (bounded only by the instant-death line).

## Asteroids

| Colour | Status | Behaviour |
| --- | --- | --- |
| **Blue** | ✅ | The standard rock. Sizes L/M/S (radius 88 / 46 / 22); a hit splits it into two of the next size down, the smallest is destroyed. One bullet per break. |
| **Green** | ✅ | Dense — takes multiple hits (HP = size, so a large green needs 3). A normal bullet *chips* it; the chain beam or a mine blast shear it in one, and a **mass shot** destroys it outright. Introduced wave 6 (mixed with blue), waves 7–9 are all green. |
| **Orange** | ✅ | Explosive. Instead of splitting, a destroyed orange **detonates** after a brief fuse — a big AOE (`ORANGE_BLAST_R`) that **destroys everything inside outright** (rocks obliterated, not split), pops mines/enemies, kills the ship if caught, and lights other oranges (chain reaction). **Gold is spared**; bullet/chain/mine all *light* it. **Mechanic done; not yet in wave content** — dev **F3** spawns one to test. |
| **Red** | 🔷 | Grows, like the Glutton boss: absorbs nearby asteroids to gain size. A large red that's broken splits into two, and *those* can absorb more and swell back up — an emergent "whack-a-mole" if you don't clear the field around them. |
| **Pulser** | ✅ | Pulses bright white ↔ dim on its own **slow** beat (`PULSE_RATE`, ~3.7s, per-rock phase); **invulnerable while LIT** — bullets/chain/mine-blast all no-op on it (a shot fizzles with a white spark). Hit it on the **dark** beat. Breaks into **smaller pulsers** (a sustained timing puzzle, not inert rubble); internally dense so there's never any blue. Debuts wave 16. `pulser_lit()` derives the beat from global time. |
| **Gold (1UP)** | ✅ | Not a hazard — a *reward*. A rare shimmering gold rock that drifts in at random times during play (any wave, boss waves included). Destroy the **whole lineage** (it + every fragment) for +1 life. Its pieces get a **long grace** (`GOLD_GRACE`, they recycle) so they're never lost *immediately* — but after that a piece that drifts off is culled and the life is **forfeit**, so clear them before they scatter. **Only your shots break it** — mines bounce off, the Devourer won't eat it. See Life economy below. |

## Enemies

Minor by design — capped as a fraction of the live asteroid count, and they flee/despawn over a
lifetime so the field stays mostly rocks. Three types total is the ceiling; more starts competing
with the asteroids for attention.

| Enemy | Status | Behaviour |
| --- | --- | --- |
| **Yellow mob** | ✅ | Standard enemy ship. Glides in, hovers and strafes around the player, steers clear of rocks/mines, lobs slow (dodgeable) shots, and flees off-screen after a lifetime. Runs in two windows: waves 3–4 and 8–9. |
| **Darter** | 🔷 | Fast interceptor — telegraphs, then charges in a straight line. Pure dodge, no ranged threat. **Chosen as the Act II mob (waves 12–13); next to build.** |
| **Miner** | 🔷 | Clings to and rides an asteroid as cover, popping out to fire. Kill the rock or catch it exposed. Deferred (Darter took the Act II slot) — a later act if we want a third type. |

## Bosses — the 50-level ladder

Every 5th wave. Each weaponizes a different *relationship to asteroids* (see Design rules). **Every
boss shows a top-center HP bar** (shared `boss_hp_bar`, tinted to the boss) and STARTS full. The
Glutton starts full too (heal cap == starting HP); eating heals the damage you've dealt back toward
full (never past — it grows in *size*, not max HP), so letting it feed visibly refills the bar.

**Run-up telegraph (last 10s, `BOSS_CAMEO_SECS`):** the *actual* incoming boss drifts across the
background as a faint silhouette in its own colour, a music riser builds, and stray mobs clear off. On
top of that the screen names the boss ("WARNING:  THE GLUTTON INCOMING") and a full-screen tint pulses
in that boss's colour, rising in intensity as the wave nears (`boss_warning_update`).

| Wave | Boss | Status | Verb → mechanic | Counterplay |
| --- | --- | --- | --- | --- |
| 5 | **The Warden** | ✅ | *Hoard* — shield of captured rocks on rotating arms; hurls the small ones | Strip the shield, shoot the core through the gaps (bullets hurt the core; chain/warp don't) |
| 10 | **The Glutton** | ✅ | *Eat* — red seeker devours free rocks to grow bigger & tankier; gorged to full it **OVERLOADS**: swells huge (flashing white as a tell), detonates a near screen-wide blast (wipes the field, kills you unless you're far — `DEVOURER_BURST_R`), then shrinks to nothing and starts over | Starve it (clear the field) + **shoot it down** — gunfire chips its HP *and* claws its size back (`DEVOURER_SHRINK_PER_HIT`), so active fire holds off the overload; when it's swollen and flashing, **get clear** before it bursts |
| 15 | **The Slinger** | ✅ | *Shoot* — a large gunship that hovers high and TRACTOR-BEAMS a field rock, reels it to its muzzle, then fires it at you like a cannonball; exposed core, no shield. Its wave is green (dense) rocks, so grabbed rounds resist being shot away | Dodge the fast shots (you can't reliably spam-break a dense round); chip the core between barrages |
| 20 | **The Detonator** | ✅ | *Prime* — primes nearby rocks into live bombs; **armored EXCEPT while priming** (its chartreuse core opens for the channel) | Punish the priming window — unload on the exposed core; dodge the bombs it plants |
| 25 | **The Pulsar** | 🔷 | *Pulse* — invulnerable while lit; shockwaves fling every rock (and you) outward | Hit only on the dark beat; don't get pinned to a wall |
| 30 | **The Phantom** ("The Haunt") | ✅ | *Untouchable* — an INTANGIBLE ghost (shots pass through) that must SURFACE to fire its quadrant-sweeping ray; p2 splits into identical decoys (only the real one attacks, and it shuffles), p3 drops the mask — solid, charging, searing a lethal spectral wake | Bait the ray, punish the surface window; watch the eyes to find the real one; sidestep the locked charge line and don't get walled in by the wake |
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
| Pulsar (W25) | **Nova pulse** | 🔷 | a shockwave that shoves rocks away — echoes its pulse |
| Phantom (W30) | **Magnet** | 🔷 | pulls pickups/small rocks in — echoes the gravity pull the Phantom keeps (base Warp stays core kit) |
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
| II — Volatile | 11–20 | Orange (explosive) ✅, Pulser (invuln-lit) ✅ | Limpet ✅ | Slinger (15) ✅, Detonator (20) ✅ |
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
| 12 | Limpet (new mob) + orange | orange wired ✅ · Limpet ✅ (core) |
| 13 | green + orange + Limpets (as 12) | orange/green wired ✅ · Limpet ✅ (core) |
| 14 | orange only | wired ✅ |
| 15 | **The Slinger** (boss) + green only | green wired ✅ · Slinger ✅ · Drone drop ✅ |

Build order (one section at a time): **1. orange mechanic ✅ → 2. wave restructure + orange/green
wiring ✅ (§A) → 3. Limpet mob ✅ core (§B) → 4. Slinger boss ✅ (§C) → 5. Slinger's Drone powerup ✅.**

### Waves 16–20 — building now

`content_wave` is now identity through **30** (`rem_euclid(30)` loop after 30). No blue past wave 10;
waves 11–20 harden leftovers to green, and from content 21 on (Act III) green retires so leftovers are
orange. The **Pulser** debuts here, and
the **gravity Well** hazard appears on 18–19 (no mobs — Limpets stay in 12–13).

| Wave | Content | Status |
| --- | --- | --- |
| 16 | pulser ONLY (a pure timing wave to learn the beat) | ✅ Pulser mechanic + wiring |
| 17 | green + orange + pulser | ✅ wired |
| 18 | pulser-heavy + orange + **Well** | ✅ wired |
| 19 | green + orange + pulser + **Well** | ✅ wired |
| 20 | **The Detonator** (boss) + orange | orange wired ✅ · Detonator ✅ · Warhead drop ✅ |

The **gravity Well** (`WELL_*`, ✅): an "opposite warp" HAZARD — a small, tight rose-red swirl that
**pops in at random intervals** (`WELL_MIN_GAP`..`WELL_MAX_GAP`), drags the *ship* toward it
(`well_pull`, under `THRUST` so you can always fly out — a compile-time invariant), and **collapses
after ~5s** (`WELL_LIFE`). A fleeting flight-disruptor, not a fixture: it doesn't kill on its own — the
threat is that it yanks your movement while you're dodging. Ship-only pull, ≤2 at a time. A
field-hazard preview of the Phantom's *Pull* (W30).

The Detonator (§D, ✅): boss 4, wave 20 — a hazard-**chartreuse** armored core. Invulnerable while it
drifts; it drifts UNTIL it reaches a rock (within `DETONATOR_ATTACH_R`), then HALTS and PRIMES that rock —
a ~`DETONATOR_PRIME_SECS` channel with a chartreuse **beam** to the rock, its core OPENING (the ONLY
window to damage it, `det.prime > 0`). The primed rock becomes a live bomb (a `Detonating` rock on
`DETONATOR_BOMB_FUSE`) to dodge. It never primes "nothing" — no rock in reach ⇒ keep drifting in. Wave 20
is **all-orange** (its bombs). On death it drops **Warhead rounds** (permanent passive: every primary shot
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
- **§B — Pulsar boss (25) ✅** *(Nova drop pending)*: electric white-cyan; invulnerable while LIT / open
  while DARK (reuses `pulser_lit(phase, t)`); on a beat it emits a `Shockwave` that flings every rock +
  the ship outward (`PULSAR_SHOCK_*`). Counter: shoot it on the dark beat, don't get pinned to a wall.
  Slow drift-chase so it can't be camped; contact kills. Still open: the **Nova-pulse** powerup drop and
  the *W25 two-older-boss variant*.
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
  - **The Sweep Ray** (`PHANTOM_RAY_*`, Idle→Telegraph→Fire; every phase): telegraphs a random 90° quadrant
    (~1.7s tell) then sweeps a lethal beam (swept-arc `angle_in_arc`, frame-rate-robust) that vaporizes rocks
    + kills the ship; faster each phase (4.6 → 3.5 → 2.4s). It roams an unhurried Lissajous
    (`PHANTOM_ROAM_EASE`), holding still while a beam is live or while surfaced.
  - **P1 — HAUNT:** the ghost + the ray. Learn the bait-and-punish rhythm.
  - **P2 — SPLIT:** it fractures into `PHANTOM_DECOYS = 2` **identical apparitions** (`PhantomDecoy`,
    `phantom_decoy_update` roams the same Lissajous seeded apart; drawn by the shared `draw_haunt_skull` —
    pixel-identical, same idle ember). Decoys never attack and can't be hit; **only the real one fires** (its
    eyes blaze on the telegraph), and when its surface window closes it **SWAPS positions with a random
    decoy** (the shell game re-deals every punish). Decoys dispel at the phase break.
  - **P3 — HUNT:** the mask drops — **solid full-time** (always hittable in `collisions`, body kills on
    contact). It **locks the ship's bearing** (`PHANTOM_CHARGE_AIM` telegraph line, eyes blazing), then
    **DASHES** (`PHANTOM_CHARGE_SPEED/SECS`, every `PHANTOM_CHARGE_EVERY`), **searing a wake of spectral
    afterimages** (`SpectralTrail`, `spectral_trail_update`: lethal `PHANTOM_TRAIL_R` for `PHANTOM_TRAIL_TTL`)
    — lingering walls in the no-wrap arena while the ray runs at its fastest. No surface freeze (it no longer
    hides). The wake dies with it on the win.
  - **Look:** the spectral skull (shared `draw_haunt_skull`: domed cranium, angry brow, ember eyes that blaze
    when it attacks or locks on, nasal, clenched teeth) — **ghost-faint + edge-wavering** while intangible;
    when surfaced the **skull CRACKS OPEN — jagged fractures + a molten core burn through, sealing as `vuln`
    runs out** (read the boss's own form, not a UI ring — the old containment ring was cut for being too
    gamey); solid all of P3; hue morphs per phase (spectral → chartreuse → hot rose); phase-break flash; **3
    phase pips** by the bar.
  - **The finale field arrives in SEQUENTIAL mono-type GROUPS of ten** (`FinaleGroup` + `top_up_asteroids`,
    `FINALE_GROUP_SIZE`): 10 blue → (field clear) → 10 green → orange → pulser → red → … `boss_director`
    clears the field + resets the cycle on the Phantom's arrival. (Fixed an over-crowded finale field.)
  - **It's the player's fight:** the ally Drone is deliberately **excluded from targeting the Phantom**
    (`drone_update`'s boss query drops `With<Phantom>`) — no auto-fire at the ghost, no tracer giving away
    the real decoy. Beating it awards `BOSS_SCORE` (parity with the other bosses) and latches Victory
    hard (zeroes `run.respawn` so a same-frame last-life death can't stomp the win with GameOver).
  - **Dev F4 (`dev_face_phantom`, debug only)** — wipes the field (keeps the ship) + jumps to wave 30 (resets
    the group cycle) so the finale can be tested without clearing 29 waves. Dev F2 sets phase 3 + zero → the
    win path in one press. (Dev keys are gated to the Playing state.)
  **The standard run is beatable end-to-end — six bosses, waves 1–30.** Still open: the **Nova** (§B) and
  **Magnet** (§C) powerup drops; an **achievements pass for the 30-wave era** (the "beat the game" achievement
  still triggers at the old wave-10 arc and the real wave-30 win records nothing); then balance tuning.

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

## Life economy (implemented: gold 1UP rock)

50 levels on 3 lives is likely impossible, especially a no-powerup **Purist** run — so lives are
recoverable, but only by earning them, via a rare gold asteroid. ✅ Implemented:

- A gold rock **drifts in at a randomized time during play** (a countdown, not tied to wave starts) —
  a distinct shimmering gold large rock that otherwise behaves normally (splits when shot). One hunt
  at a time; a long random gap measured from when it *appears* (`GOLD_MIN_GAP`..`GOLD_MAX_GAP`, ~4-6
  min) is armed on each spawn, so at most ~1 appears per (3-min) wave and never back-to-back.
  `GOLD_INITIAL_DELAY` graces the run start. Any wave, boss waves included (the Devourer won't eat it;
  a rock the Warden grabs is just shot off its shield).
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
  | Mine | 150 |
  | Boss | 3000 |
  | Warden shield rock (small remnant) | 20 |
  | Rock swallowed by the warp | `WARP_ROCK_SCORE` (25, low flat — no farming) |

  Gold rocks score like normal rocks (their reward is the life, not points). Score is purely for
  ranking — it doesn't grant lives.
- **High scores:** a persisted **top 5** (numeric), saved to `violet-edge.hiscore`. On game over the
  final score slots into the table (`record_high_score`), the screen shows the board with the new
  entry lit and a **NEW BEST!** / **TOP 5!** banner, and the main menu shows a **BEST** line.
- **Achievements:** First Blood, Warden Off, Glutton for Punishment, True Blue (100 blue), Green Thumb (100 green), Edgelord (beat the arc), Purist (beat it with no powerups). New bosses each get one, named for the boss.
- **Field population:** the on-screen count targets `POP_BASE + wave` (cap `POP_CAP`), topped up from the edges at `SPAWN_INTERVAL`. Edge spawns are ~80% large; a `BIG_FLOOR` keeps large rocks present even at the cap. Rocks that drift fully off-screen are recycled back in *only if large* — small debris usually despawns for good (mids sometimes), so breaking rocks apart can't silt the arena up with an overwhelming cloud of little ones; the top-up refills with fresh large rocks. Freshly-broken fragments get a short grace window (`FRAGMENT_GRACE`) during which they always recycle rather than being culled, so a rock shattered right at the edge can't lose its pieces off-screen before you get a shot. The Warden grabs large/mid rocks for its shield and only resorts to a small one when nothing bigger is on-screen.
