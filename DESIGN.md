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
| **Green** | ✅ | Dense — takes multiple hits (HP = size, so a large green needs 3). A normal bullet *chips* it; the chain beam, a mine blast, or a **mass shot** shear it in one. Introduced wave 6 (mixed with blue), waves 7–9 are all green. |
| **Orange** | ✅ | Explosive. Instead of splitting, a destroyed orange **detonates** after a brief fuse — a big AOE (`ORANGE_BLAST_R`) that **destroys everything inside outright** (rocks obliterated, not split), pops mines/enemies, kills the ship if caught, and lights other oranges (chain reaction). **Gold is spared**; bullet/chain/mine all *light* it. **Mechanic done; not yet in wave content** — dev **F3** spawns one to test. |
| **Red** | 🔷 | Grows, like the Glutton boss: absorbs nearby asteroids to gain size. A large red that's broken splits into two, and *those* can absorb more and swell back up — an emergent "whack-a-mole" if you don't clear the field around them. |
| **Pulser** | 🔷 | Pulses bright white on a cycle; **invulnerable while lit**. You have to time shots to its dark phase. |
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

| Wave | Boss | Status | Verb → mechanic | Counterplay |
| --- | --- | --- | --- | --- |
| 5 | **The Warden** | ✅ | *Hoard* — shield of captured rocks on rotating arms; hurls the small ones | Strip the shield, shoot the core through the gaps (bullets hurt the core; chain/warp don't) |
| 10 | **The Glutton** | ✅ | *Eat* — red seeker devours free rocks to grow bigger & tankier; gorged to full it **OVERLOADS**: swells huge (flashing white as a tell), detonates a near screen-wide blast (wipes the field, kills you unless you're far — `DEVOURER_BURST_R`), then shrinks to nothing and starts over | Starve it (clear the field) + **shoot it down** — gunfire chips its HP *and* claws its size back (`DEVOURER_SHRINK_PER_HIT`), so active fire holds off the overload; when it's swollen and flashing, **get clear** before it bursts |
| 15 | **The Slinger** | ✅ | *Shoot* — a large gunship that hovers high and TRACTOR-BEAMS a field rock, reels it to its muzzle, then fires it at you like a cannonball; exposed core, no shield. Its wave is green (dense) rocks, so grabbed rounds resist being shot away | Dodge the fast shots (you can't reliably spam-break a dense round); chip the core between barrages |
| 20 | **The Detonator** | 🔷 | *Prime* — turns nearby rocks into live bombs; itself armored | Bait the chain — only explosive blasts crack its shell |
| 25 | **The Pulsar** | 🔷 | *Pulse* — invulnerable while lit; shockwaves fling every rock (and you) outward | Hit only on the dark beat; don't get pinned to a wall |
| 30 | **The Singularity** | 🔷 | *Pull* — gravity drags all rocks + you into a crushing orbit | Thrust against the pull; let it crush on its own haul, or feed it an explosive |
| 35 | **The Hive** | 🔷 | *Split* — the boss **is** an asteroid; every hit mitoses, fragments re-fuse if ignored | Burn all pieces down before they merge |
| 40 | **The Prism** | 🔷 | *Reflect* — facets bounce your shots; spawns crystal rocks that also reflect | Catch an open facet, or shoot it *through* a rock |
| 45 | **Gemini** | 🔷 | *Link* — twin ships tethered by a shared rock core; damage transfers between them | Break the core rock to sever them, then focus one |
| 50 | **The Progenitor** | 🔷 | *All* — the first asteroid; cycles the earlier verbs as phases | Apply each phase's counter in turn — a full-run mastery check |

## Pickups (powerups) ↔ boss mapping

Each boss drops a powerup that echoes its own mechanic.

| Boss (drop) | Powerup | Status | Thematic tie |
| --- | --- | --- | --- |
| Warden (W5) | **Chain Shot** | ✅ | beam arcs/*links* between rocks — the Warden links rocks on its arms |
| Glutton (W10) | **Mass Shot** | ✅ | heavier, high-*mass* rounds — the red Glutton gains mass |
| Slinger (W15) | **Drone** | 🔷 | an ally *ship* that seeks & fires for you — mirrors the enemy ship |
| Detonator (W20) | **Warhead rounds** | 🔷 | your shots detonate & chain — echoes the primed bombs |
| Pulsar (W25) | **Nova pulse** | 🔷 | a shockwave that shoves rocks away — echoes its pulse |
| Singularity (W30) | **Magnet** | 🔷 | pulls pickups/small rocks in — echoes its gravity (base Warp stays core kit) |
| Hive (W35) | **Spread shot** | 🔷 | your shot *splits* into several — echoes mitosis |
| Prism (W40) | **Ricochet rounds** | 🔷 | bullets *reflect* off walls/rocks — echoes the facets |
| Gemini (W45) | **Twin cannons** | 🔷 | two linked fire streams — echoes the twins (kept distinct from the drone) |
| Progenitor (W50) | — | — | final boss; no drop (or a combined ultimate) |

> Resolve at implementation: Drone (W15) vs Twin cannons (W45) must feel distinct, and Magnet (W30)
> must not just re-skin the base Warp weapon.

## Progression — 5 acts

Each new asteroid debuts a few waves *before* the boss that weaponizes it: learn the toy, then fight
the thing made of it. (Waves 1–15 are now bespoke content; the loop resumes at 16, repeating the
1–15 arc until later acts are built.)

| Act | Waves | New asteroid(s) | New enemy | Bosses |
| --- | --- | --- | --- | --- |
| I — The Field | 1–10 | Blue, Green ✅ | Yellow mob ✅ | Warden (5), Glutton (10) |
| II — Volatile | 11–20 | Orange (explosive) ✅ | Limpet | Slinger (15), Detonator (20) |
| III — Unstable | 21–30 | Red (growing), Pulser (invuln-lit) | — | Pulsar (25), Singularity (30) |
| IV — Deep Belt | 31–40 | **Crystal** (reflects) *or* **Ice** (shard-burst) — TBD | — | Hive (35), Prism (40) |
| V — The Core | 41–50 | **Void** (swallows bullets) *or* **Magnetic** (bends fire) — TBD | — | Gemini (45), Progenitor (50) |

### Waves 11–15 — building now (won't extend past 15 until a no-dev-invuln run reaches 16)

Per-wave content plan. `content_wave` is now identity through 15 with a `rem_euclid(15)` loop after;
the rock mix lives in `roll_rock_kind` (orange fraction ~0.25 on waves 11–13, 1.0 on 14 — tunable):

| Wave | Content | Section status |
| --- | --- | --- |
| 11 | green + orange | wired ✅ |
| 12 | Limpet (new mob) + orange | orange wired ✅ · Limpet ✅ (core) |
| 13 | green + orange + Limpets (as 12) | orange/green wired ✅ · Limpet ✅ (core) |
| 14 | orange only | wired ✅ |
| 15 | **The Slinger** (boss) + green only | green wired ✅ · Slinger ✅ (Drone drop TODO) |

Build order (one section at a time): **1. orange mechanic ✅ → 2. wave restructure + orange/green
wiring ✅ (§A) → 3. Limpet mob ✅ core (§B) → 4. Slinger boss ✅ (§C) → 5. Slinger's Drone powerup.**

The Slinger (§C, ✅): boss 3, wave 15 — a large **ice-blue gunship** (its nose/cannon tracks the
player; unique boss colour, apart from the Warden's magenta + Devourer's red). Glides in, then hovers
high mirroring the ship's x. **Tractor beam:** it grabs the nearest field rock (tags it `Cannonball`,
draws a beam), reels it to its muzzle at `SLINGER_REEL_SPEED`, holds `SLINGER_HOLD`s, then launches it
at the ship at `SLINGER_CANNON_SPEED`; grabs every `SLINGER_COOL`s. Because its wave is **green
(dense)** rocks, a grabbed round takes several hits to break — you can't spam it away, you *dodge*.
Grabs refill from the field (`top_up`) so it never runs dry; a launched round despawns off-screen. Its
wave runs a **sparse field** (`SLINGER_WAVE_ROCKS`, the beam's ammo reservoir), cleared when it arrives
(clean green-only slate — the Warden/Devourer keep their rocks). Exposed core (`SLINGER_HP`, no shield).
**TODO:** drop the Drone pickup on death (currently just advances). Rule of thumb going forward: **each
boss gets a unique colour** (Warden magenta · Devourer red · Slinger ice-blue).

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
