//! VIOLET EDGE — Bevy port. (Renamed from NEON EDGE, which was taken.)
//!
//! Vertical slice + juice: window, neon grid (dim, shimmering) over a starfield,
//! a ship you rotate/thrust/fire (Space or left-click) with a thrust flame, blue
//! asteroids with elastic physics that split when shot, particle bursts, bullet
//! trails, ship death → respawn (with invuln), a lives HUD, and Pause + Game-Over
//! screens driven by a Bevy state machine.
//!
//! Rendering: Bevy gizmos (immediate-mode wireframes) on an HDR camera with Bloom
//! for the glow. UI text via bevy_ui (default font). Written against Bevy 0.16.

// Bevy ECS systems idiomatically take many query params, and its query types are
// verbose by nature — clippy's `too_many_arguments`/`type_complexity` are noise here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitWindows;

mod audio;
use rand::Rng;
use std::collections::HashSet;

// ─────────────────────────────── config ───────────────────────────────
const TAU: f32 = std::f32::consts::TAU;

const SHIP_R: f32 = 13.5; // was 15 — trimmed slightly per playtest (hitbox + visuals shrink together, a touch forgiving)
const TURN_RATE: f32 = 5.2; // rad/s (~300°/s) — raised from 4.6 in the flight-feel pass: a 180° flip in ~0.6s makes gap-weaving answer the hands, and taps still land fine aim (~5°/frame at 60fps)
const THRUST: f32 = 1200.0; // px/s^2 — raised in lockstep with the heavier drag below so terminal speed stays ~520 (less drift must never read as a slower ship)
const FRICTION: f32 = 0.10; // velocity kept per second — tightened from 0.15 per playtest ("drift too great"): glide-out is ~20% shorter (~0.30s half-life), still a drift game, not a stop-on-release one
const MAX_SPEED: f32 = 560.0; // px/s (a cap; sustained thrust settles a bit under it)
const FIRE_COOLDOWN: f32 = 0.18; // s

const BULLET_SPEED: f32 = 720.0; // px/s
const BULLET_LIFE: f32 = 1.6; // s — MINIMUM range floor (small windows); real range scales with the arena
const BULLET_RANGE_FRAC: f32 = 1.5; // bullet travels this × the arena half-width, so reach scales with the screen (fixes "too short on a big display")
const BULLET_R: f32 = 3.0;

// Mass shot (pickup after boss 2): a bigger, slower, harder-hitting primary. Toggle standard↔mass.
const MASS_COOLDOWN: f32 = 0.5; // s between mass shots (vs 0.18 standard — much slower)
const MASS_BULLET_R: f32 = 7.0; // fat round (vs 3.0 standard)
// Mass shot damage. Vs a free ASTEROID it's `MASS_POWER` per hit (stronger than standard's 1 — one-shots a
// dense rock, which then SPLITS normally); vs a BOSS/mob it's the smaller `MASS_BOSS_POWER` (a per-hit bump,
// not a boss-melter — its slow rate keeps standard the better boss DPS). The instant-destroy WIPE lives on
// the Warhead now, not the mass shot.
const MASS_POWER: i32 = 3;
const MASS_BOSS_POWER: i32 = 2;

const GRID_CELL: f32 = 52.0;
const WAVE_SECS: f32 = 100.0; // survive the timer to advance. 2026-07-30: 120 → 60 → user settled on 100 — snappier than the original hour-plus run without feeling breathless (~50 min full clear with the heavier bosses)
const POP_BASE: i32 = 5; // asteroids on screen = POP_BASE + wave...
const POP_CAP: i32 = 18; // ...capped so the field never becomes an unavoidable wall
const SLINGER_WAVE_ROCKS: i32 = 6; // sparse field on the Slinger wave — it spawns its own cannonballs and
                                   // doesn't use field rocks, so a full field is just clutter to dodge through
const FINALE_TRICKLE: f32 = 0.45; // wave 30: gap between trickled-in rocks — the RATE is what keeps the finale readable
const FINALE_FIELD_CAP: i32 = 10; // wave 30: max non-gold rocks out at once (matches the old group density — never a wall)

// Cluster (waves 26+): a fractured ice rock that SHATTERS — instead of splitting in two, it bursts
// into a ring of tiny fast shards. The mass shot vaporizes it clean and the warp swallows it whole,
// so tool choice finally matters against a rock. Point-blank shots become a bad habit.
const CLUSTER_SHARDS: usize = 7;
const CLUSTER_SHARD_SPEED: f32 = 210.0; // outward fling of the shard ring (× the caller's chunk_mult)

// Hunter (waves 6+): the first rock that CHASES. It steers at the ship with `HUNTER_ACCEL`, capped at
// HUNTER_MAX_SPEED, and its aggression ramps over HUNTER_RAMP seconds of life — a slow menace that
// becomes a real threat if ignored. Deliberately slower than the ship at full charge (outrunnable, so
// it's pressure rather than a death sentence) and children reset to charge 0 when it splits.
const HUNTER_ACCEL: f32 = 115.0; // px/s² steering toward the ship at FULL charge
const HUNTER_MAX_SPEED: f32 = 205.0; // px/s cap — well under the ship's 560, so you can always disengage
const HUNTER_RAMP: f32 = 14.0; // s of life to reach full aggression (fresh chunks start docile)
// Outrunnability is an invariant, not a preference: a hunter must never be able to catch a ship that
// is flying away. Enforced at compile time — raise it past MAX_SPEED and the build fails.
const _: () = assert!(HUNTER_MAX_SPEED < MAX_SPEED);

// LAPSE (NG+ roster): a rock that goes INTERMITTENT — it dissolves, spends a spell absent, then
// materializes again on a randomized clock. Deliberately NOT a gotcha: while it's gone AND all the
// way through the fade back IN it is intangible and harmless (you can't hit it, it can't hit you),
// and the fade-in is slow enough to read as a warning, so a rock rematerializing on your hull is
// always something you had time to leave. It keeps drifting while absent, so it comes back somewhere
// you weren't watching. Distinct from the Pulser: that one STAYS and toggles invulnerability; this
// one actually leaves. All transitions are ≥0.9s ramps — nowhere near the 3 Hz photosensitivity line.
const LAPSE_SOLID_MIN: f32 = 4.0; // seconds present and dangerous
const LAPSE_SOLID_MAX: f32 = 7.0;
const LAPSE_FADE_OUT: f32 = 0.9; // dissolving (still solid — it's on its way out, not gone)
const LAPSE_GONE_MIN: f32 = 1.5; // absent: invisible bar a faint scar, intangible, harmless
const LAPSE_GONE_MAX: f32 = 3.0;
const LAPSE_REAPPEAR_CLEAR: f32 = 170.0; // it never materializes closer than this to the ship
const LAPSE_FADE_IN: f32 = 1.6; // the TELEGRAPH: a neon tube STRIKING back into being (see `lapse_glow`).
// 1.6s, not 1.3: the strike's peaks worked out at 3.1 Hz over the shorter window, a hair over the
// ≤3 flashes/sec photosensitivity rule — and a slower strike is the better look anyway.
// The fade-in IS the fairness guarantee — a rock may only rematerialize after a window long enough
// to read and fly out of. Enforced at compile time so nobody can tune it into a cheap death.
const _: () = assert!(LAPSE_FADE_IN >= 1.0);

// HUSK (NG+ roster): a rock that isn't a rock. It looks ordinary bar a hollow core, and breaking it
// doesn't produce chunks — the shell cracks and lets out a PAIR OF HUNTERS that were riding inside.
// Deliberately NOT a cascade: the shell releases live rocks instead of splitting, so a husk can never
// contain another husk and one careless shot can't snowball into a screen of chasers. The hollow is
// always drawn, so a player who looks can tell one from a drift rock before firing.
const HUSK_BROOD: usize = 2; // hunters inside
const HUSK_BROOD_SPEED: f32 = 130.0; // how fast they scatter out of the shell

// FACET (NG+ roster): a mirrored rock. Its faces REFLECT your rounds — and a reflected round is
// live, so your own fire can kill you. Exactly ONE face is open, and it rotates with the rock, so the
// counter is to read the gap and time the shot rather than to hold the trigger down. Blasts, the
// beam and the warp ignore the mirror entirely (they aren't rounds), which keeps it from being a
// hard wall. The ricochet is deliberately SLOWER than your shot so it can be dodged on the way back.
const FACET_OPEN_ARC: f32 = 1.15; // radians of vulnerable face (~66°) — wide enough to hit on purpose
const FACET_RICOCHET_SPEED: f32 = 0.62; // reflected rounds keep this fraction of their speed
// A ricochet must always be slower than the shot that made it, or it's an unavoidable counterattack.
const _: () = assert!(FACET_RICOCHET_SPEED < 1.0);
const FACET_RICOCHET_LIFE: f32 = 1.1; // s a ricochet stays lethal before it fizzles out

// Beacon (waves 23+): a teal warden rock projecting an AURA — rocks inside it are immune to gunfire
// and the chain until the beacon falls (blasts, the warp, and red-absorption all bypass it: the
// counterplay). Spawns dense (hp = size), never splits — it dies clean when cracked.
const BEACON_AURA_R: f32 = 270.0; // was 200 — too small to matter; now it genuinely owns a region
const BIG_FLOOR: i32 = 4; // always keep at least this many LARGE (size-3) rocks around: keeps the
                          // field from silting up with small debris, and gives the boss big rocks to grab
const SPAWN_INTERVAL: f32 = 1.6; // seconds between streamed-in replacement rocks (manageable rate)

const WARP_MAX_CHARGES: i32 = 3; // fire all 3, THEN the long cooldown refills them together
const WARP_COOLDOWN: f32 = 35.0; // long refill once all charges are spent — not spammable
const WARP_MISSILE_SPEED: f32 = 550.0;
const WARP_MISSILE_LIFE: f32 = 1.4; // ~770px max — but it detonates the instant it hits a rock, so in a busy field it opens the hole right there rather than sailing across the arena
const WARP_MISSILE_R: f32 = 10.0; // contact radius for detonating on an asteroid
const WARP_HOLE_LIFE: f32 = 2.6;
const WARP_PULL_RADIUS: f32 = 560.0; // a bit bigger than the old 440 (JS 360 read too small
// with our longer missile throw); still well short of arena-spanning (~755 was too far)
const WARP_PULL: f32 = 2600.0; // very aggressive inward yank — the hole should feel greedy (JS was 900)
const WARP_CONSUME_R: f32 = 120.0; // event horizon — anything whose EDGE crosses this is
// instantly destroyed (rocks/enemies/mines), like a real black hole. Big enough that pulled-in
// rocks are eaten on contact instead of clumping + colliding around a tiny mouth.
const WARP_GRID_RADIUS: f32 = 340.0; // the grid bends toward the hole within this
const GRID_SHIMMER_AMP: f32 = 2.8; // eased 4.5 → 2.8 (user, 2026-07-31): the crest was too hot. This is ONLY the moving shimmer wave — the warp's own grid flicker (`warp_flick`) is separate and untouched. Brightness of the shimmer CREST (× base grid color, over a ~0.5 dim floor). Was 1.1 — too dim; this makes the lit shimmer waves read clearly. Tunable.
const WARP_GRID_STRENGTH: f32 = 82.0; // max inward grid displacement (px) at the hole
const WARP_SNAP_DUR: f32 = 0.7; // rubber-band snapback time after the hole closes

// Mines (wave 2+): drifting proximity crimson mines.
const MINE_FIRST_WAVE: i32 = 2;
const MINE_PER_WAVE: i32 = 1; // target = (wave - first + 1) * per... (gentle ramp — was 2)
const MINE_MAX_FRACTION: f32 = 0.3; // ...never more than this fraction of the asteroid count (was 0.5 — mines are a garnish, not half the field)
const MINE_HARD_CAP: i32 = 6; // and never more than this many at once, so mines never become a wall
const MINE_R: f32 = 18.0; // bumped from 13 — a bigger body to shoot (the lethal reach is MINE_BLAST_R, unchanged)
const MINE_SPEED: f32 = 62.0; // px/s drift
const MINE_TRIGGER_R: f32 = 92.0; // ship within → the mine arms (blinks)
const MINE_BLAST_R: f32 = 52.0; // armed + ship within → detonate (kills the ship)
const MINE_SCORE: u32 = 150; // for destroying the MINE itself — rocks its blast breaks score nothing (see blast_asteroids)
const MINE_FUSE: f32 = 0.6; // arming time before it can detonate (time to escape)
const WARP_ROCK_SCORE: u32 = 25; // a rock swallowed by the warp scores a low flat value (no farming)
const MINE_SPAWN_INTERVAL: f32 = 2.6;
const MINE_CHUNK_MULT: f32 = 1.9; // HIDDEN: rocks shattered by a mine blast fling chunks this much faster

// The Well (waves 18+): a gravity well — an "opposite warp" that drags the SHIP toward it. Deliberately
// weaker than the ship's thrust, so you can always fly out; the threat is that it fouls your dodging,
// not a direct kill.
const WELL_R: f32 = 12.0; // small, tight core (a compact swirl — not a screen-filling spiral)
const WELL_PULL_RADIUS: f32 = 300.0; // reach — smaller than the player warp's 560
const WELL_PULL: f32 = 660.0; // inward accel at the core — stronger than before, still under THRUST (escapable)
const WELL_LIFE: f32 = 5.0; // s it lingers — it POPS IN, yanks your flight, then collapses (was 14, lingered too long)
const WELL_MAX: i32 = 2; // hard cap on live wells
const WELL_MIN_GAP: f32 = 4.0; // random gap between pop-ins → sporadic surprises, not a steady stream
const WELL_MAX_GAP: f32 = 9.0;
// Escapability is a hard invariant: the well's pull must stay under the ship's thrust so you can
// always fly out. Enforced at compile time — bump WELL_PULL past THRUST and the build fails.
const _: () = assert!(WELL_PULL < THRUST);

// Enemy ships (wave 3+): drift in, hover-and-strafe while firing at the ship, dodge
// mines/rocks, get sucked into the warp, and bug out if they linger too long.
const ENEMY_MAX_FRACTION: f32 = 0.3; // mob count is capped well below the rock count (a garnish)
const ENEMY_R: f32 = 19.0; // bumped from 14 — a bigger mob to hit (mobs threaten with bullets, not contact)
const ENEMY_MAX_SPEED: f32 = 125.0; // px/s
const ENEMY_ACCEL: f32 = 640.0; // px/s² steering force
const ENEMY_PREF_DIST: f32 = 260.0; // hovers around this range from the ship
const ENEMY_AVOID_R: f32 = 95.0; // steer away from mines/rocks within this
const ENEMY_SEP_R: f32 = ENEMY_R * 4.0; // steer away from EACH OTHER within this (no stacking)
const ENEMY_FIRE_EVERY: f32 = 2.4; // s between shots (deliberately slow, dodgeable)
const ENEMY_FIRE_JITTER: f32 = 0.9;
const ENEMY_BULLET_SPEED: f32 = 205.0; // px/s — eased from 250 (user, 2026-07-31): still a real
// threat, but slow enough that a shot fired across the field can be read and slipped rather than
// reacted to. The ship's 560 top speed means you can always outrun one.
const ENEMY_BULLET_R: f32 = 5.0;
const ENEMY_BULLET_LIFE: f32 = 4.5; // s
const ENEMY_LIFETIME: f32 = 11.0; // s on-screen before it flees (never overstays)
const ENEMY_SCORE: u32 = 300;
const ENEMY_SPAWN_INTERVAL: f32 = 3.0;

// THE TENDER (NG+ only): the thing that maintains the Belt. It doesn't shoot — it REPAIRS the field,
// hauling two of your leftover fragments together with a tractor beam and FUSING them back into a
// whole rock. It undoes your work, which makes it the first mob that's a genuine priority target.
// Fragile on purpose (one hit) and it never touches the player directly: the threat is entirely that
// the field stops shrinking while it lives.
const TENDER_R: f32 = 14.0;
const TENDER_SPEED: f32 = 95.0; // unhurried — it's a maintenance drone, not a hunter
const TENDER_ACCEL: f32 = 420.0;
const TENDER_REACH: f32 = 300.0; // how far it will look for a pair of fragments to salvage
const TENDER_FUSE_SECS: f32 = 2.6; // beam time to drag a pair together — long enough to interrupt
const TENDER_HAUL: f32 = 150.0; // px/s it reels its two targets toward their midpoint
const TENDER_SCORE: u32 = 250; // worth more than a raider: killing one protects your progress
const TENDER_LIFETIME: f32 = 22.0; // longer than a raider's — it has work to do
const TENDER_COOL: f32 = 1.2; // pause between salvage jobs

// (The Limpet parasite mob was REMOVED 2026-07-28 — the asteroids are the star, and the bosses carry
// the spectacle; waves 12-13 are pure rock waves now.)

// Dense (green) asteroids take multiple bullet hits to crack (hp = size); chain/mine still break
// them at once. The per-wave rock mix (blue / green / orange) lives in `roll_rock_kind`.

// Octopus boss (every 5th wave): a magenta core that captures field asteroids into a
// rotating orbital shield (its "arms") and hurls the smallest held rocks at the ship.
const BOSS_WAVE_INTERVAL: i32 = 5; // waves 5, 10, 15, … are boss waves
const BOSS_R: f32 = 38.0;
const BOSS_HP: i32 = 50; // core hits to kill (the shield blocks most shots) — base of the ascending boss-HP ramp (2026-07-30 pass: whole ramp ~doubled, bosses were burning down too fast)
const BOSS_ARMS: usize = 6; // asteroids it can hold at once
const BOSS_ORBIT_R: f32 = 132.0; // arm length — how far the shield orbits
const BOSS_SPIN: f32 = 0.85; // rad/s shield rotation
const BOSS_GRAB_TIME: f32 = 1.8; // s a grabbed rock reels into its slot (slow + telegraphed)
const BOSS_CAPTURE_EVERY: f32 = 1.1; // s between grabs (deliberately unhurried)
const BOSS_ENTER_SPEED: f32 = 320.0; // px/s glide-in from the top
const BOSS_FIRE_EVERY: f32 = 2.0; // s throw cadence
const BOSS_FIRE_JITTER: f32 = 0.7;
const BOSS_THROW_SPEED: f32 = 280.0; // px/s of a hurled rock
const BOSS_CHARGE: f32 = 1.4; // s power-up after entering (invulnerable)
const BOSS_DEATH_SECS: f32 = 2.2; // slow death animation before it despawns
const BOSS_CALM: f32 = 10.0; // s post-kill lull before the next wave (the pickup window)
const BOSS_SCORE: u32 = 3000;
const BOSS_CAMEO_SECS: f32 = 10.0; // boss drifts by in the background this long before its wave

// The Slinger (boss 3, wave 15): a large gunship that hovers high and uses a TRACTOR BEAM — it grabs a
// field rock, reels it to its muzzle, holds a beat, then FIRES it at you. On wave 15 the field is
// green (dense) rocks, so a grabbed round takes several hits to break — you can't just spam it away;
// dodge the fast shots and chip its exposed core. Grabs refill from the field (top_up), so it never
// runs dry. Drops the Drone (wired when the pickup is built).
const SLINGER_HP: i32 = 85; // core hits to kill (no shield — survive the barrage while you chip it). Ascending ramp: > Glutton; biggest bump of the 2026-07-30 pass because its core is exposed the WHOLE fight (it was the most burnable)
const SLINGER_R: f32 = 40.0; // a big ship — a decent target
const SLINGER_SPEED: f32 = 155.0; // hover reposition speed (stays high, mirroring the ship's x)
const SLINGER_ENTER_SPEED: f32 = 340.0; // glide-in from the top
const SLINGER_INTRO: f32 = 1.2; // invulnerable power-up after entering
const SLINGER_COOL: f32 = 0.9; // s between grabs (a steady barrage → you must keep dodging, can't camp)
const SLINGER_REEL_SPEED: f32 = 420.0; // px/s it reels a grabbed rock toward its muzzle
const SLINGER_HOLD: f32 = 0.45; // s it holds the reeled rock at the muzzle (aiming) before firing
const SLINGER_CANNON_SPEED: f32 = 640.0; // px/s of a launched rock — fast; must be dodged
const SLINGER_DEATH_SECS: f32 = 2.2; // slow death animation before it despawns

// The Detonator (boss 4, wave 20): ARMORED except while it primes a rock — that channel is your only
// damage window. It halts, beams a nearby rock, and when the channel completes that rock becomes a live
// bomb (a `Detonating` rock on a fuse). Drops the Warhead-rounds powerup.
const DETONATOR_HP: i32 = 72; // core hits — still the smallest bump of the ramp because it's landable only during priming windows (doubling here would double the whole fight's length)
const DETONATOR_R: f32 = 42.0;
const DETONATOR_ENTER_SPEED: f32 = 320.0; // glide-in from the top
const DETONATOR_INTRO: f32 = 1.2; // invulnerable power-up after entering
const DETONATOR_SPEED: f32 = 120.0; // drift speed while armored (repositioning toward rocks to prime)
const DETONATOR_ATTACH_R: f32 = 150.0; // must be within this of a rock to START priming it (else keep drifting in — never primes "nothing")
const DETONATOR_COOL: f32 = 1.6; // s armored between priming channels
const DETONATOR_PRIME_SECS: f32 = 2.5; // length of the priming channel — the VULNERABLE window (was 1.5:
                                       // too short to land real damage once drift/search time was paid)
const DETONATOR_BOMB_FUSE: f32 = 1.4; // once primed, the rock ticks this long, then detonates (dodge it)
const DETONATOR_DEATH_SECS: f32 = 2.2;

// Red (growing) asteroid — Act III's new type: absorbs a nearby non-red rock to swell (up to large).
const RED_ABSORB_R: f32 = 130.0;   // a red eats a non-red rock within this radius
const RED_ABSORB_EVERY: f32 = 2.6; // s between absorptions (also a fresh/child red's initial cooldown)

// The Pulsar (boss 5, wave 25): invulnerable while LIT (its pulse beat), vulnerable while DARK, and it
// periodically emits a shockwave that FLINGS every rock + the ship outward. Counter: shoot it on the
// dark beat; don't get pinned to a wall by the shove. Reuses `pulser_lit` (the beat) + `Shockwave`.
const PULSAR_HP: i32 = 90; // scaled with the beat in mind — it's landable only on the DARK half, so this plays like ~half the number
const PULSAR_R: f32 = 40.0;
const PULSAR_ENTER_SPEED: f32 = 320.0; // glide-in from the top
const PULSAR_INTRO: f32 = 1.2;         // invulnerable power-up after entering
const PULSAR_SPEED: f32 = 90.0;        // slow drift (repositions, hard to camp)
const PULSAR_SHOCK_EVERY: f32 = 2.4;   // s between fling-shocks (penultimate boss — a relentless beat)
const PULSAR_SHOCK_R: f32 = 420.0;     // fling radius (reaches most of the arena)
const PULSAR_SHOCK_PUSH: f32 = 520.0;  // outward impulse (px/s) — hard shove: flings rocks fast + can pin you to a wall
const PULSAR_DEATH_SECS: f32 = 2.2;

// The Phantom (boss 6, wave 30 — the FINALE): THE HAUNT — a spectral predator too arrogant to be touched.
// It fights across three PHASES gated by a per-phase health pool: bring the current phase's core to zero and
// it RESETS (invulnerable, reforms, repositions) before the next begins. Its signature is the SWEEP RAY
// (every phase, faster each). The twist: it's INTANGIBLE — your shots pass straight through — and only turns
// VULNERABLE for a short window right after it fires the ray (it has to SURFACE to attack; that's your only
// opening). PHASE 2 POSSESSES a homing rock it hides in (break the vessel to rip it out); PHASE 3 it turns solid and CHARGES,
// leaving a lethal trail. It ROAMS, holding still while a beam is live or while surfaced. Clear phase 3 → WIN.
const PHANTOM_PHASE_HP: i32 = 95; // health PER PHASE (refills on each reset) — 3 phases. USER RULE: no phase may be the lowest-HP fight in the game, so each phase tops the whole ramp (max is the Pulsar's 90) — 285 total; the finale outlasts everything before it
const PHANTOM_R: f32 = 46.0;      // a big, imposing core (the finale centrepiece)
const PHANTOM_ENTER_SPEED: f32 = 300.0;
const PHANTOM_INTRO: f32 = 1.4;         // invulnerable power-up after entering
const PHANTOM_RESET_SECS: f32 = 1.8;    // the invulnerable "reset" beat between phases (reforms, repositions)
const PHANTOM_ROAM_EASE: f32 = 1.1;     // how it eases toward its roam target — unhurried; it believes it's untouchable
const PHANTOM_MATERIALIZE: f32 = 1.6;   // after firing the ray it SURFACES (solid + hittable) this long — the ONLY window to damage it
// ── the Sweep Ray (the Phantom's own signature mechanic — present every phase, faster each one) ──
const PHANTOM_RAY_QUADRANT: f32 = std::f32::consts::FRAC_PI_2; // 90° sweep — one telegraphed quadrant of the arena
const PHANTOM_RAY_TELEGRAPH: f32 = 1.7;  // warning-wedge duration before the beam ignites (a clear, readable tell)
const PHANTOM_RAY_FIRE: f32 = 0.8;       // how long the beam takes to sweep across the quadrant
const PHANTOM_RAY_COOLDOWN: f32 = 4.6;   // gap between sweeps in phase 1 (tightens as it escalates)
const PHANTOM_RAY_FIRST: f32 = 2.2;      // grace before the very first sweep (after the intro)
const PHANTOM_RAY_INNER_R: f32 = 48.0;   // the beam ignores the core zone
const PHANTOM_RAY_WIDTH: f32 = 26.0;     // visual thickness of the beam
// ── phase 2 — POSSESSION: it SEEKS an existing field rock, dives in (that rock becomes a haunted vessel
//    that homes + kills on contact) and hides inside; break the vessel to rip it out — its "surface to be
//    hit". Shooting the possessed rock is how you force it out; the exposed ghost is then the punish window ──
const PHANTOM_POSSESS_HP: i32 = 4;        // hits to break a vessel and force the Haunt out
const PHANTOM_POSSESS_SPEED: f32 = 150.0; // how fast a possessed rock homes toward the ship
const PHANTOM_POSSESS_R: f32 = 30.0;      // vessel radius (contact-kill + shot target)
const PHANTOM_SEEK_SPEED: f32 = 260.0;    // how fast the ghost glides to the rock it's about to possess
const PHANTOM_DIVE_FIRST: f32 = 0.7;      // beat after entering phase 2 before the first hunt
const PHANTOM_DIVE_EVERY: f32 = 0.9;      // beat between being ripped out and hunting the next rock
// ── phase 3 — HUNT: the mask drops — solid full-time, charging the arena and leaving a lethal wake ──
const PHANTOM_CHARGE_EVERY: f32 = 3.5;   // gap between charges (P3 is charge-only now — no beam — so they come a bit more often)
const PHANTOM_CHARGE_AIM: f32 = 0.8;     // aim-telegraph before the dash (eyes blaze, it locks your position)
const PHANTOM_CHARGE_SPEED: f32 = 900.0; // dash speed — fast, but locked straight (dodge sideways)
const PHANTOM_CHARGE_SECS: f32 = 0.6;    // dash duration
const PHANTOM_TRAIL_TTL: f32 = 2.2;      // how long each spectral afterimage in its wake stays lethal
const PHANTOM_TRAIL_R: f32 = 16.0;       // kill radius of one afterimage
// ── the win: a death-throes beat + a spectral shard streaking off-screen (a seed for whatever comes next) ──
const PHANTOM_VICTORY_SECS: f32 = 9.0;      // death-scene SAFETY cap — normally it ends when the shard + ship have flown off (event-driven)
const PHANTOM_SHARD_MIN_SPEED: f32 = 60.0;  // the escaping core tears loose slowly…
const PHANTOM_SHARD_MAX_SPEED: f32 = 270.0; // …then accelerates east off-screen (ease-in over PHANTOM_SHARD_RAMP)
const PHANTOM_SHARD_RAMP: f32 = 2.1;        // seconds for the fleeing core to reach full speed (slower → the send-off lingers)
const SHIP_DEPART_SPEED: f32 = 460.0;       // the hero's ship warps off east after the shard has left

// Boss 2 — the devourer (wave 10): a red seeker that eats rocks to grow + heal.
const DEVOURER_HP: i32 = 60; // core HP; it STARTS full and HEALS toward it, so it plays tankier than this raw number (ramp: > Warden)
const DEVOURER_HP_MAX: i32 = DEVOURER_HP; // heal cap == starting HP: eating heals DAMAGE back toward full, never past it (it grows in SIZE, not in max HP)
const DEVOURER_BASE_R: f32 = 42.0; // fully-shrunk floor (was 22 — too small to keep hitting once you clawed it down)
const DEVOURER_MAX_R: f32 = 200.0; // fully gorged — swells huge, then OVERLOADS and bursts (see devourer_update)
const DEVOURER_BURST_R: f32 = 420.0; // overload blast reach — near screen-wide; escapable only by being far
const DEVOURER_GROW_PER_EAT: f32 = 0.09; // grow step per rock (~11 rocks → max size)
const DEVOURER_HEAL_PER_EAT: i32 = 2; // HP regained per rock (was 4 — it out-healed player fire and dragged the fight out)
const DEVOURER_SHRINK_PER_HIT: f32 = 0.03; // each player hit claws its size back (~⅓ of a rock's growth) — keeps it manageable and lets you hold off the overload
const DEVOURER_SPEED: f32 = 95.0; // px/s seek speed (below the ship's, so it's dodgeable)

// Chain shot: a wide lightning BEAM secondary weapon. Unlocked by the pickup that
// appears in the calm after the first boss (wave 5). 3 charges that regenerate.
const CHAIN_MAX_CHARGES: i32 = 3;
const CHAIN_RECHARGE: f32 = 5.5; // s to regenerate one charge
const CHAIN_COOLDOWN: f32 = 0.27; // min s between shots
const CHAIN_SPEED: f32 = 540.0; // px/s
const CHAIN_HALF: f32 = 58.0; // half the beam width (gap between the two chained ends)
const CHAIN_R: f32 = 8.0; // beam hit half-thickness
const CHAIN_LIFE: f32 = 1.5; // s
const PICKUP_R: f32 = 30.0; // reward-orb radius

// Nova Shield (the Pulsar's drop): a regenerating one-hit barrier — see `Nova`.
const NOVA_REGEN: f32 = 9.0; // s the shield stays DOWN after eating a hit (long enough that it can't tank everything)
const NOVA_GRACE: f32 = 1.0; // s of immunity as it pops — the overlap that broke it can't instantly re-kill
const NOVA_RELIGHT: f32 = 0.8; // the regen's final stretch — the shell flickers as it comes back (≤3 Hz, photosafe)
// AEGIS SHARDS — the Warden+'s drop (NG+ boss 1). The Warden PENS rocks on orbital arms; this is
// that trick in the player's hands: SMALL shards orbiting the hull, moving with the ship, that grind
// any rock which would have hit you. Deliberately NOT invincibility: each block consumes a shard and
// they come back one at a time on a slow cooldown, so a careless stretch leaves you bare.
const AEGIS_SHARDS: u8 = 3; // shards at full strength
const AEGIS_ORBIT_R: f32 = 30.0; // how far out they ride (just off the hull)
const AEGIS_SHARD_R: f32 = 4.2; // SMALL, per the user's call — chips, not plates
const AEGIS_SPIN: f32 = 1.5; // rad/s — a slow, readable rotation
const AEGIS_REGEN: f32 = 11.0; // s to grow ONE shard back (the anti-invincibility throttle)
// The regrow cooldown IS the anti-invincibility mechanism — a trivial value would make the ring
// permanent. Enforced at compile time so a careless retune can't quietly hand out immortality.
const _: () = assert!(AEGIS_REGEN > 5.0);
const NOVA_SHELL: f32 = 1.8; // the shield is the SHIP'S OWN silhouette scaled out — a second hull layer, not a separate polygon
const PICKUP_DRIFT: f32 = 32.0; // px/s slow drift
const PICKUP_LIFE: f32 = 20.0; // the orb lingers this long (well past the 10s boss calm) before vanishing

// The Drone (boss-3 reward): an ally that orbits the ship a short distance out and auto-plinks the
// nearest asteroid in range — cleaning up rocks the player left behind. Fires the player's own Bullet.
const DRONE_R: f32 = 9.0;
const DRONE_FOLLOW_DIST: f32 = 64.0; // how far it orbits from the ship (a short leash)
const DRONE_ORBIT_RATE: f32 = 0.9; // rad/s — a slow circle around the ship
const DRONE_FOLLOW_GAIN: f32 = 6.0; // how snappily it chases its orbit point (lerp/s)
const DRONE_RANGE: f32 = 380.0; // only targets asteroids within this of the drone
const DRONE_FIRE_EVERY: f32 = 1.0; // s between shots — an assist, not a firehose

const MAX_SEP: f32 = 6.0; // px/frame cap on overlap push-out
const RESTITUTION: f32 = 1.0; // fully elastic bounce
const MIN_DRIFT: f32 = 30.0; // px/s — rocks never fully stop (elastic hits can zero them → "stuck")
const FRAGMENT_GRACE: f32 = 1.8; // s a freshly-broken fragment is protected from off-screen culling
const GOLD_GRACE: f32 = 6.0; // gold fragments get a longer window (recycle, not culled) — a fair chance to catch them before one can drift off and forfeit the life
const ORANGE_BLAST_R: f32 = 250.0; // explosive-asteroid kill/chain radius (+ the victim's own radius). Was 150 — too small on big screens, so it looked huge (the particle burst throws to ~440) but barely caught neighbours. Now the reach matches the visual.
const WARHEAD_BLAST_R: f32 = 110.0; // the Warhead's blast radius — REAL AoE since the on-impact rework: everything inside dies with the struck rock (the ring Shockwave draws exactly this reach)
// THE GORGE ROUND — the Glutton+'s drop (NG+ boss 2), and its verb in the player's hands: a slow,
// heavy round that EATS each rock it hits and GROWS, ending as a rolling wrecking ball. Distinct from
// what you already carry: the Warhead detonates and stops, Mass is a fat one-shot, this one snowballs.
// Bounded so it can't clear a whole field: it grows to a hard cap and dies at GORGE_BITES.
const GORGE_COOLDOWN: f32 = 1.05; // s between rounds — slow, deliberate
const GORGE_R0: f32 = 7.0; // starting radius
const GORGE_GROW: f32 = 5.2; // radius gained per rock eaten
const GORGE_R_MAX: f32 = 34.0; // hard size cap
const GORGE_BITES: u32 = 6; // rocks it can eat before it breaks up
const GORGE_SPEED: f32 = 430.0; // slower than a standard round (BULLET_SPEED), and it keeps that pace
const _: () = assert!(GORGE_SPEED < BULLET_SPEED); // heavy = slow: it must be readable on the way out
const WARHEAD_COOLDOWN: f32 = 1.3; // s between Warhead rounds — VERY slow on purpose: since the on-impact AoE rework each round clears a 110px disk, so it's a toggled siege weapon (Q-cycle), not a machine gun. Aim, fire, wait.
const ORANGE_FUSE: f32 = 0.09; // brief lit flash after a lethal hit before it detonates (a visible "pop")

// Pulser (waves 16+): a rock that pulses LIT (bright white, invulnerable) ↔ DARK (dim, vulnerable) on
// its own beat. Shots only hurt it on the dark beat — time them. Internally a dense rock (so its bits
// are green, never blue) with a render override; `pulser_lit` derives the beat from global time.
const PULSE_RATE: f32 = 1.7; // rad/s → a slow lit/dark cycle (~3.7s) — long invulnerable windows make it harder
const PULSE_LIT_THRESHOLD: f32 = 0.15; // sin above this = LIT (≈45% lit / 55% dark — a generous dark window)

const RESPAWN_DELAY: f32 = 1.3; // s the ship stays gone after dying
const GAMEOVER_DELAY: f32 = 1.5; // s to let the final death play out before the Game Over screen
const HUD_FLASH_TIME: f32 = 0.7; // s the warp pips / life icons flicker after refilling / gaining a life

// HUD ability-strip slot layout — design px from the LEFT edge. The ui labels (`left: Val::Px(X)`)
// and the gizmo glyphs (world `-h.x + X`) share these, so the two layers can't drift apart (the
// camera scale-to-fits DESIGN_H and UiScale tracks the same factor, so ui px == design-world px).
const HUD_SLOT_WARP: f32 = 32.0;
const HUD_SLOT_CHAIN: f32 = 150.0;
const HUD_SLOT_MODE: f32 = 296.0;
const HUD_SLOT_SHIELD: f32 = 388.0;
const HUD_SLOT_DRONE: f32 = 466.0;
const HUD_STRIP_LABEL_TOP: f32 = 60.0; // the label row (ui `top` px)
const HUD_STRIP_Y: f32 = 92.0; // the glyph row (world y = h.y - this), under its labels
const SHOT_MODE_SHOW: f32 = 1.4; // s the "MASS/STANDARD SHOT" label lingers after a toggle
const SPAWN_INVULN: f32 = 2.0; // s of blink-invulnerability on (re)spawn
const TRAIL_LEN: usize = 10; // bullet trail points kept
const SHIP_TRAIL_LEN: usize = 72; // ship light-ribbon points kept (~1.2s of motion — extended THREE times per playtest; the user wants a real Tron presence)
const STAR_COUNT: usize = 90;
// The game renders at a fixed DESIGN height, scale-to-fit to the window: on ANY monitor the camera
// magnifies so DESIGN_H world-units fill the window height (a bigger screen magnifies — it does NOT reveal
// more empty arena). The arena's half-WIDTH follows the window aspect so it fills the screen edge-to-edge.
const DESIGN_H: f32 = 800.0;
const DESIGN_HALF_H: f32 = DESIGN_H * 0.5;
const START_LIVES: i32 = 3;
const LIFE_CAP: i32 = START_LIVES; // gold restores a LOST life only — never above the starting count
// The gold 1UP rock drifts in at a randomized time during play (a countdown), not at wave starts.
const GOLD_INITIAL_DELAY: f32 = 40.0; // grace before the first gold rock can appear in a run (scaled with the 100s waves)
// Gap from when a gold rock APPEARS to the earliest the next one may. WAVE-DEPENDENT: short (frequent
// life rocks) through the early game — a spare life is most useful then — tapering LONG (rare) by the
// wave-30 finale. A fresh random value in the wave's [min..max] is rolled on each spawn.
const GOLD_GAP_EARLY_MIN: f32 = 100.0; // waves ≤ GOLD_TAPER_START: ~one per wave-and-a-bit (scaled to the 100s waves so lives-per-WAVE stays the original tuning)
const GOLD_GAP_EARLY_MAX: f32 = 155.0;
const GOLD_GAP_LATE_MIN: f32 = 220.0; // by wave 30: rare (~2.2-3 waves between golds — same cadence-per-wave as the original 120s tuning)
const GOLD_GAP_LATE_MAX: f32 = 300.0;
const GOLD_TAPER_START: i32 = 16; // gap stays "early/frequent" through this wave…
const GOLD_TAPER_END: i32 = 30; // …then ramps linearly to "late/rare" by this wave
const WAVE_BANNER_SECS: f32 = 2.4; // how long the big "WAVE n" flash lingers
const WAVE_BANNER_FADE: f32 = 1.2; // of that, the trailing fade-out duration

// Bright (>1.0) colors so the HDR camera's bloom makes them glow.
fn ship_color() -> Color {
    Color::srgb(2.6, 0.55, 5.2)
} // neon violet — the player + its kit (peak dialled back ~20% to ease the bloom)
fn flame_color() -> Color {
    Color::srgb(3.2, 1.7, 5.0)
} // hot purple-white exhaust
fn bullet_color() -> Color {
    Color::srgb(2.4, 1.0, 4.6)
}
fn mass_color() -> Color {
    Color::srgb(5.2, 2.4, 6.5)
} // bright hot violet — the mass shot (player kit)

// A bullet's hit radius and damage depend on whether it's a mass shot.
fn bullet_radius(mass: bool) -> f32 {
    if mass {
        MASS_BULLET_R
    } else {
        BULLET_R
    }
}
// Mass-shot damage to a boss/mob (NOT to free asteroids — those are destroyed outright in `collisions`).
fn bullet_boss_power(mass: bool) -> i32 {
    if mass {
        MASS_BOSS_POWER
    } else {
        1
    }
}
// The player ship's hull — the ORIGINAL dart (two logo-styled redesigns were tried and rejected;
// this classic shape stays, user call 2026-07-28). UNIT coords, nose = +X, closed loop; one shared
// definition for every ship drawing (in play, the HUD lives icons, the finale send-off).
fn ship_hull() -> [Vec2; 5] {
    [
        Vec2::new(1.0, 0.0),
        Vec2::new(-0.7, -0.7),
        Vec2::new(-0.4, 0.0), // rear notch
        Vec2::new(-0.7, 0.7),
        Vec2::new(1.0, 0.0),
    ]
}

// Draw the ship at `c`, facing `rot` (a unit vector), `scale` ≈ SHIP_R (the HUD icons pass a mini scale).
// `fill` paints the hull SOLID in its neon color — the ship, and ONLY the ship, is filled (user call);
// everything else in the game stays wireframe. The fill is ten nested inset copies of the outline
// (~1.5px apart at ship scale) that bloom fuses into a solid body — no mesh pipeline needed. The body
// DARKENS toward the center while the outline stays full-bright (user call): a lit neon rim over a
// deeper violet core.
fn draw_ship(gizmos: &mut Gizmos, c: Vec2, rot: Vec2, scale: f32, color: Color, fill: bool) {
    let rings = if fill { 10 } else { 1 };
    for r in 0..rings {
        let k = 1.0 - r as f32 / rings as f32;
        let shade = if rings > 1 { 1.0 - 0.5 * (r as f32 / (rings - 1) as f32) } else { 1.0 }; // rim 1.0 → center 0.5
        let pts: Vec<Vec2> = ship_hull().iter().map(|v| c + rot.rotate(*v * k * scale)).collect();
        gizmos.linestrip_2d(pts, dim(color, shade));
    }
}

// The Tron light ribbon: a fading band along `pts` (oldest → newest) in `color`. A hot center line
// plus two soft parallel edges give it real WIDTH (gizmo lines are 1px; bloom fuses the three into a
// glowing band), tapering to a point at the tail like a ribbon should.
fn draw_light_ribbon(gizmos: &mut Gizmos, pts: &[Vec2], color: Color) {
    let n = pts.len();
    for i in 1..n {
        let f = i as f32 / n as f32; // 0 = oldest → 1 = at the ship
        let (a, b) = (pts[i - 1], pts[i]);
        let nrm = (b - a).perp().normalize_or_zero();
        let w = 2.2 * f; // half-width: full body at the ship, a point at the tail
        let col = dim(color, 0.05 + 0.75 * f * f);
        gizmos.line_2d(a, b, col);
        gizmos.line_2d(a + nrm * w, b + nrm * w, dim(col, 0.55));
        gizmos.line_2d(a - nrm * w, b - nrm * w, dim(col, 0.55));
    }
}

fn rock_color() -> Color {
    Color::srgb(0.25, 1.9, 4.0)
} // neon blue (peak dialled back ~20% to ease the bloom)
fn dense_color() -> Color {
    Color::srgb(0.4, 4.0, 1.1)
} // neon green — dense (tanky) asteroids
fn grid_color() -> Color {
    Color::srgb(0.02, 0.06, 0.2)
} // faint backdrop
fn star_color() -> Color {
    Color::srgb(0.5, 0.7, 1.15)
}
fn warp_color() -> Color {
    Color::srgb(2.6, 1.2, 5.0)
} // warp purple (player kit)
fn mine_color() -> Color {
    Color::srgb(4.0, 0.55, 1.35)
} // hot crimson = danger (peak dialled back ~20% to ease the bloom)
fn enemy_color() -> Color {
    Color::srgb(4.0, 2.9, 0.4)
} // neon yellow — enemy ships + their shots (peak dialled back ~20%)
fn boss_color() -> Color {
    Color::srgb(5.0, 1.6, 4.1)
} // neon magenta — the boss
fn devourer_color() -> Color {
    Color::srgb(6.0, 0.7, 0.6)
} // hot red — the devourer (boss 2); no blue, so it never reads as the player's purple
fn chain_color() -> Color {
    Color::srgb(3.4, 2.0, 5.6)
} // electric violet lightning — the chain shot (player kit)
fn gold_color() -> Color {
    Color::srgb(6.0, 4.6, 1.6)
} // bright warm gold — the rare 1UP asteroid (lighter/whiter than the enemy yellow)
fn orange_color() -> Color {
    Color::srgb(6.0, 2.0, 0.25)
} // hot orange — explosive asteroids (high R, low B; distinct from the yellow enemy)
fn red_color() -> Color {
    Color::srgb(6.0, 0.15, 0.9)
} // deep CRIMSON (cool, blue-leaning) — the growing asteroid; the blue tint sets it clearly apart from the warm orange rock
fn pulsar_color() -> Color {
    Color::srgb(3.6, 6.0, 6.6)
} // electric white-cyan — the Pulsar (boss 5); brighter/whiter than the Slinger's ice-blue
fn phantom_color() -> Color {
    Color::srgb(2.6, 6.0, 4.4)
} // spectral pale green-cyan — the Phantom (boss 6, finale); a ghostly hue, apart from the player's purple and every other boss
fn phantom_ray_color() -> Color {
    Color::srgb(9.0, 2.0, 1.2)
} // hot hazard red — the Phantom's Sweep Ray telegraph + beam; screams "danger" against its cool spectral body
fn slinger_color() -> Color {
    Color::srgb(0.9, 2.2, 5.2)
} // cold electric ICE-BLUE — the Slinger gunship (boss 3); a unique boss hue, clearly apart from the
  // Warden's magenta + Devourer's red, and no blue rocks exist on its wave to confuse it with
fn detonator_color() -> Color {
    Color::srgb(3.8, 6.0, 0.4)
} // hazard CHARTREUSE (yellow-green) — the Detonator (boss 4); high R+G, ~no blue, apart from every other boss hue
fn warhead_color() -> Color {
    Color::srgb(3.0, 0.9, 6.0)
} // vivid VIOLET — the player's Warhead-round blast (player kit → purple; also signals "your blast, safe")
fn drone_color() -> Color {
    Color::srgb(2.6, 2.2, 5.6)
} // lavender-violet — the ally Drone (player kit, so it reads as yours; distinct from the ship's core purple)
fn nova_color() -> Color {
    Color::srgb(3.4, 2.8, 6.0)
} // glassy pale violet — the Nova Shield bubble (player kit → purple family; whiter/airier than drone or hull)
fn well_color() -> Color {
    Color::srgb(5.0, 0.9, 2.4)
} // hot rose-red — the gravity-well hazard (a "dark" swirl, clearly NOT the player's blue-purple warp)
// A Pulser is LIT (invulnerable) when its beat is in the bright half. Derived from global time + the
// rock's own phase offset, so no per-frame state is needed — collisions/chain/render all agree.
fn pulser_lit(offset: f32, t: f32) -> bool {
    (t * PULSE_RATE + offset).sin() > PULSE_LIT_THRESHOLD
}
fn husk_color() -> Color {
    Color::srgb(2.6, 2.2, 1.5)
} // drab BONE — deliberately dull and rock-like, because it's meant to pass for one. The tell is the
  // hollow core (drawn, always), not the hue.

fn facet_color() -> Color {
    Color::srgb(4.6, 4.9, 5.4)
} // hard SILVER-WHITE — a mirror. Its identity is the flat faces + the open notch (silhouette, not
  // hue: the neon palette is full — see the palette policy in DESIGN.md)

fn lapse_ignite_color() -> Color {
    Color::srgb(6.2, 3.0, 1.2)
} // the warm amber-white flash of a neon tube STRIKING — the lapse cools to its steel-blue from this

// A LAPSE materializing doesn't fade up linearly — this is a neon game, so it comes back the way a
// tube ignites: a few WARM stutters that settle into steady colour. Three strikes across
// LAPSE_FADE_IN (~2.3 Hz — comfortably inside the ≤3 flashes/sec photosensitivity rule), each one
// brief, over a slowly rising baseline so the shape is always legible between them.
fn lapse_glow(l: &Lapse) -> Color {
    match l.phase {
        LapsePhase::Gone => dim(lapse_color(), 0.0),
        LapsePhase::Solid => lapse_color(),
        LapsePhase::FadingOut => dim(lapse_color(), l.presence()), // a dying tube just dims out
        LapsePhase::FadingIn => {
            let f = l.presence();
            let strike = |at: f32, w: f32| (1.0 - ((f - at) / w).abs()).max(0.0).powi(2);
            let b = 0.08 + 0.55 * f * f + 0.8 * strike(0.20, 0.08) + 0.9 * strike(0.50, 0.09) + 1.0 * strike(0.80, 0.11);
            // warm at the strike, cooling to its own colour as the gas settles
            dim(mix(lapse_ignite_color(), lapse_color(), f), b.min(1.3))
        }
    }
}

fn lapse_color() -> Color {
    Color::srgb(1.4, 3.0, 5.6)
} // cold spectral STEEL-BLUE — the intermittent rock. Kept desaturated so its fade reads as absence
  // rather than as another neon type; its identity is the dissolve, not the hue.
fn hunter_color() -> Color {
    Color::srgb(6.0, 1.1, 0.35)
} // hazard VERMILLION — the wave-6 predator. Distinct from orange (yellower, and an Act II type) and
  // from the red rock (pinker, Act III); its tracking EYE and its motion are the real identity.
fn cluster_color() -> Color {
    Color::srgb(2.0, 3.2, 4.4)
} // pale fractured ICE — brighter than a dark pulser, colder/whiter than the blue rocks; waves 26+
fn beacon_color() -> Color {
    Color::srgb(0.5, 4.4, 2.2)
} // deep teal-GREEN — the aura warden (green rocks have retired by its debut, so the hue is free); waves 23+

fn mine_target(level: i32, asteroids: i32) -> i32 {
    // key on content_wave so the 16+ loop resets mine density like rocks/enemies do (raw `level` would
    // keep it pinned at the fraction cap forever). Identical for waves 1-15 (content_wave == level).
    let cw = content_wave(level);
    if cw < MINE_FIRST_WAVE {
        return 0;
    }
    let raw = (cw - MINE_FIRST_WAVE + 1) * MINE_PER_WAVE;
    raw.min((asteroids as f32 * MINE_MAX_FRACTION) as i32).min(MINE_HARD_CAP)
}

// Split economy (2026-07-31, user design): breaking a rock no longer guarantees two children.
// LARGE sheds 1-2 mediums; MEDIUM either shatters into 2 smalls or dies outright. Fewer smalls
// crowd the screen over a lineage (avg ~4.4 entities per large, was always 7), and every break
// has variance. GOLD is exempt (the 1UP hunt's lineage length is tuned economy) and so is RED
// (splitting-and-regrowing IS its identity); CLUSTER has its own shatter rule.
const SPLIT_LARGE_TWO_CHANCE: f64 = 0.6; // large → 2 mediums, else 1
const SPLIT_MEDIUM_CHANCE: f64 = 0.55; // medium → 2 smalls, else destroyed clean

// How many children this break sheds (0 only ever for a medium dying clean).
fn split_children(size: u8, gold: bool, red: bool, rng: &mut impl Rng) -> usize {
    if size <= 1 {
        return 0; // smallest rocks always die clean
    }
    if gold {
        // THE 1UP HUNT MAKES NO SMALL FRAGMENTS (2026-07-31 — user: life rocks were too unforgiving).
        // Smalls were the problem: a large gold used to become 2 mids and then FOUR smalls, and a
        // small that crosses an edge past its grace is gone for good 85% of the time, forfeiting the
        // life. Now a large sheds two MIDS and those die clean: three hittable targets instead of
        // seven with tiny stragglers, and mids only wander off 35% of the time (larges never do).
        return if size >= 3 { 2 } else { 0 };
    }
    if red {
        return 2; // a broken red always begets two reds — splitting-and-regrowing IS its identity
    }
    if size >= 3 {
        if rng.gen_bool(SPLIT_LARGE_TWO_CHANCE) { 2 } else { 1 }
    } else if rng.gen_bool(SPLIT_MEDIUM_CHANCE) {
        2
    } else {
        0
    }
}

fn asteroid_radius(size: u8) -> f32 {
    match size {
        3 => 88.0, // LARGE
        2 => 46.0, // MID
        _ => 26.0, // SMALL — was 22 (unhittable) then 30 (ate too much screen); 26 is the playtested middle: still an easy target, visibly debris again
    }
}
fn body_mass(r: f32) -> f32 {
    r * r
}
fn population_target(level: i32, plus: bool) -> i32 {
    // the Slinger doesn't eat or grab rocks (it makes its own cannonballs), so keep its arena sparse —
    // a full field just clutters the fight (NG+ included: the fight's design, not its difficulty).
    // The Warden (shield) and Devourer (food) DO use rocks, so they keep the normal count.
    if is_slinger_wave(level) {
        return SLINGER_WAVE_ROCKS;
    }
    // NG+ adds its bonus PAST the cap — the whole curve shifts up, wave 1 through the finale
    (POP_BASE + level).min(POP_CAP) + if plus { NGP_POP_BONUS } else { 0 }
}

/// Elastic collision between two circular bodies: separate out of overlap
/// (capped) and exchange momentum along the normal. Ported from JS collideAsteroids.
fn resolve(pa: &mut Vec2, va: &mut Vec2, ma: f32, ra: f32, pb: &mut Vec2, vb: &mut Vec2, mb: f32, rb: f32) {
    let delta = *pb - *pa;
    let d2 = delta.length_squared();
    let min = ra + rb;
    if d2 >= min * min || d2 == 0.0 {
        return;
    }
    let dist = d2.sqrt();
    let n = delta / dist;
    let total = ma + mb;
    let corr = (min - dist).min(MAX_SEP);
    *pa -= n * (corr * mb / total);
    *pb += n * (corr * ma / total);
    let vn = (*vb - *va).dot(n);
    if vn > 0.0 {
        return;
    }
    let j = -(1.0 + RESTITUTION) * vn / (1.0 / ma + 1.0 / mb);
    *va -= n * (j / ma);
    *vb += n * (j / mb);
}

/// Scale a color's brightness (fade). With bloom, dimming kills the glow too.
fn dim(color: Color, f: f32) -> Color {
    let s = color.to_srgba();
    Color::srgb(s.red * f, s.green * f, s.blue * f)
}

// Linear blend between two colors (t: 0 → a, 1 → b). Used for gradient effects
// like the bullet flame (deep purple tip → hot base).
fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    Color::srgb(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
    )
}

// The Haunt's visage — a menacing spectral skull-mask: an elongated angular cranium in two halves, a heavy
// glaring brow, downward eye-slashes with red embers, and a jagged fanged maw, wreathed in a slow broken
// halo (its crown). When `open` > 0 (surfaced) the mask SPLITS — the halves part and a searing core blazes
// in the widening seam, sealing as the window closes. `wispy` shivers the whole form (the intangible ghost).
// `ember` is the eye brightness. Shared by the real Phantom and its phase-2 decoys (pixel-identical).
#[allow(clippy::too_many_arguments)]
fn draw_haunt_skull(gizmos: &mut Gizmos, c: Vec2, hr: f32, body: Color, ember: f32, pulse: f32, wispy: bool, open: f32) {
    let wv = if wispy { (pulse * 1.6).sin() * 0.05 } else { 0.0 }; // spectral shiver at the extremities
    // ── broken HALO / crown: shard-ticks orbiting, slowly counter-rotating ──
    let halo = dim(body, 0.4);
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU - pulse * 0.35;
        let r1 = hr * (1.42 + 0.08 * (pulse * 2.2 + k as f32 * 1.7).sin());
        gizmos.line_2d(c + Vec2::from_angle(a) * hr * 1.24, c + Vec2::from_angle(a) * r1, halo);
    }
    let split = open * hr * 0.42; // how far the two halves have parted
    // ── searing CORE in the seam (blazes brighter as it opens) ──
    if open > 0.02 {
        let core = mix(body, Color::srgb(9.0, 7.5, 8.5), 0.72); // white-hot heart
        let (ch, cw) = (hr * 0.92, hr * 0.05 + split * 0.85);
        gizmos.linestrip_2d(
            [c + Vec2::new(0.0, ch), c + Vec2::new(cw, ch * 0.18), c + Vec2::new(0.0, -ch), c + Vec2::new(-cw, ch * 0.18), c + Vec2::new(0.0, ch)],
            dim(core, 0.55 + 0.45 * open),
        );
        gizmos.circle_2d(Isometry2d::from_translation(c), cw * 1.4 + hr * 0.06, dim(core, 0.5 + 0.5 * open));
    }
    // ── the two face-halves (mirrored), parting by `split` ──
    for side in [-1.0f32, 1.0] {
        let off = Vec2::new(side * split, 0.0);
        let m = |x: f32, y: f32| c + off + Vec2::new(side * x * hr, y * hr); // half-space → world (x mirrored)
        // elongated angular contour: crown → temple → cheekbone → lower cheek → jaw → chin
        gizmos.linestrip_2d(
            [m(0.10, 1.05 + wv), m(0.64, 0.82), m(0.95, 0.24), m(0.80, -0.34), m(0.46, -0.82), m(0.11, -1.05 - wv)],
            body,
        );
        gizmos.line_2d(m(0.70, 0.48), m(0.16, 0.22), body); // heavy angry brow ridge (slants down to the seam)
        // angular EYE-SLASH socket (downward glare) + red ember
        let ec = m(0.42, 0.05);
        gizmos.linestrip_2d(
            [ec + Vec2::new(-side * hr * 0.13, hr * 0.11), ec + Vec2::new(side * hr * 0.15, hr * 0.03), ec + Vec2::new(side * hr * 0.15, -hr * 0.07), ec + Vec2::new(-side * hr * 0.13, -hr * 0.12), ec + Vec2::new(-side * hr * 0.13, hr * 0.11)],
            body,
        );
        gizmos.circle_2d(Isometry2d::from_translation(ec + Vec2::new(side * hr * 0.02, -hr * 0.01)), hr * 0.065, dim(phantom_ray_color(), ember));
        // jagged FANGS on this half of the maw (irregular lengths)
        let fang = [0.42f32, 0.95, 0.6, 1.0];
        for (t, &f) in fang.iter().enumerate() {
            let tx = 0.07 + t as f32 * 0.12;
            gizmos.line_2d(m(tx, -0.46), m(tx, -0.46 - 0.16 * f), dim(body, 0.9));
        }
    }
    // ── tattered CLOAK: wisps trailing beneath the jaw, each swaying on its own beat — the ghost is
    //    never still even when it holds position ──
    for k in -1i32..=1 {
        let x0 = k as f32 * 0.34 * hr;
        let pts: Vec<Vec2> = (0..=5)
            .map(|i| {
                let f = i as f32 / 5.0;
                c + Vec2::new(x0 + (pulse * 1.1 + k as f32 * 1.9 + f * 2.6).sin() * hr * 0.14 * f, -hr * (1.02 + 1.15 * f))
            })
            .collect();
        gizmos.linestrip_2d(pts, dim(body, 0.42 - 0.09 * k.abs() as f32));
    }
}

// How many of a boss's `parts` still remain at this point of its death countdown. The staged deaths
// key on this: when the count DROPS between frames a piece just sheared off (the update fires a burst
// there, and the render — using the same formula — simply stops drawing it).
fn death_parts(dying: f32, total: f32, parts: usize) -> usize {
    ((dying / total).clamp(0.0, 1.0) * parts as f32).ceil() as usize
}

// A curved, tapering tentacle (a quadratic bezier bowed by a sine-driven curl, with a traveling
// RIPPLE running down its length so it visibly writhes) from `from` to `to`. The Warden's arms.
fn draw_tentacle(gizmos: &mut Gizmos, from: Vec2, to: Vec2, curl_phase: f32, color: Color) {
    let d = to - from;
    let dist = d.length().max(1.0);
    let perp = Vec2::new(-d.y, d.x) / dist;
    let mid = from + d * 0.5 + perp * (curl_phase.sin() * dist * 0.22);
    let n = 11;
    let pts: Vec<Vec2> = (0..=n)
        .map(|i| {
            let tt = i as f32 / n as f32;
            let it = 1.0 - tt;
            let base = from * (it * it) + mid * (2.0 * it * tt) + to * (tt * tt); // quadratic bezier
            // the ripple: a wave that travels tip-ward, pinned at both ends
            base + perp * ((curl_phase * 1.6 + tt * 5.2).sin() * dist * 0.05 * (tt * it * 4.0))
        })
        .collect();
    gizmos.linestrip_2d(pts, color);
}


// Inward pull a black hole applies to a body at `pos` this frame (velocity delta).
// Zero outside `pull_r`; a strong floor at the rim, ramping harder toward the core.
// Shared by every hazard the warp drags in (rocks, enemies, mines).
fn warp_pull(pos: Vec2, hole: Vec2, pull_r: f32, dt: f32) -> Vec2 {
    let d = pos.distance(hole);
    if d >= pull_r || d < 1.0 {
        return Vec2::ZERO;
    }
    let dir = (hole - pos) / d;
    let falloff = 1.0 - d / pull_r;
    dir * (WARP_PULL * (0.35 + 0.65 * falloff)) * dt
}

// Inward velocity delta a gravity Well applies to the ship. Like `warp_pull` but weaker (`WELL_PULL`
// < THRUST), so the ship can always thrust free — the well fouls your movement, it doesn't trap you.
fn well_pull(ship: Vec2, well: Vec2, dt: f32) -> Vec2 {
    let d = ship.distance(well);
    if !(1.0..WELL_PULL_RADIUS).contains(&d) {
        return Vec2::ZERO;
    }
    let dir = (well - ship) / d;
    let falloff = 1.0 - d / WELL_PULL_RADIUS;
    dir * (WELL_PULL * (0.3 + 0.7 * falloff)) * dt
}

// Squared distance from point `p` to segment `a`–`b` (for chain-beam vs target hits).
fn seg_dist2(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length_squared()
}

/// Rubber-band ease: 0→1 with a decaying overshoot past 1 (used for the snapback).
fn ease_out_elastic(p: f32) -> f32 {
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let c4 = (2.0 * std::f32::consts::PI) / 3.0;
    2.0_f32.powf(-10.0 * p) * ((p * 10.0 - 0.75) * c4).sin() + 1.0
}

/// Displace a point toward the warp hole (or push it out when `amount` is negative
/// during the snapback overshoot). Falls off to nothing at WARP_GRID_RADIUS.
fn warp_point(p: Vec2, wf: &WarpField) -> Vec2 {
    if wf.amount == 0.0 {
        return p;
    }
    let to = wf.pos - p;
    let d = to.length();
    if !(1.0..WARP_GRID_RADIUS).contains(&d) {
        return p;
    }
    let fall = 1.0 - d / WARP_GRID_RADIUS;
    p + (to / d) * (wf.amount * WARP_GRID_STRENGTH * fall * fall)
}

/// Spawn a spray of fading particles from `pos`.
fn burst(commands: &mut Commands, pos: Vec2, color: Color, count: usize, speed: f32, rng: &mut impl Rng) {
    for _ in 0..count {
        let a = rng.gen_range(0.0..TAU);
        let s = rng.gen_range(speed * 0.35..speed);
        let ttl = rng.gen_range(0.3..0.75);
        commands.spawn((
            Particle { vel: Vec2::from_angle(a) * s, life: ttl, ttl, color },
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
    }
}

/// A radial burst of light whose particles actually REACH the arena edge (and a bit past) before fading —
/// the finale death scene's screen-filling light. `Particle` velocity DECELERATES (`vel *= 0.25^dt`), so a
/// particle's total travel is `v0·(1 − 0.25^ttl)/ln4`; we invert that to launch each one fast enough to
/// carry it all the way out. Emitted from `pos`.
fn light_burst_to_edge(commands: &mut Commands, pos: Vec2, half: Vec2, count: usize, color: Color, rng: &mut impl Rng) {
    let edge = half.length(); // centre → far corner: the farthest a particle might need to travel
    for _ in 0..count {
        let a = rng.gen_range(0.0..TAU);
        let ttl = rng.gen_range(1.9..2.8);            // linger longer — a slower, more majestic expansion
        let target = edge * rng.gen_range(1.15..1.55); // gentler overshoot → a less violent launch (still crosses the edge)
        let reach = (1.0 - 0.25_f32.powf(ttl)) / 4.0_f32.ln(); // fraction of v0 covered over the lifetime
        let v0 = target / reach.max(1e-3);
        commands.spawn((
            Particle { vel: Vec2::from_angle(a) * v0, life: ttl, ttl, color },
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
    }
}

// The shared ship-death sequence: debris burst, despawn, lose a life → respawn or
// (on the last life) Game Over. Used by ship_death (asteroids) and mine_update.
fn kill_ship(
    commands: &mut Commands,
    run: &mut Run,
    _next: &mut NextState<GameState>, // game-over is now triggered by `respawn` after a beat, not here
    sfx: &mut EventWriter<SoundFx>,
    ship_e: Entity,
    pos: Vec2,
    rng: &mut impl Rng,
) {
    // Nova Shield: every death path funnels through here, so the absorb covers them all uniformly.
    // While UP it eats the hit instead of the ship — collapse it, start the regen, and open a brief
    // grace (the overlap that broke it can't also kill before it resolves). While DOWN, the hit falls
    // through and costs the life — exactly the shield's stated deal.
    if run.nova.unlocked {
        if run.nova.grace > 0.0 {
            return; // still phasing through the hit the shield just ate
        }
        if run.nova.down <= 0.0 {
            run.nova.down = NOVA_REGEN;
            run.nova.grace = NOVA_GRACE;
            burst(commands, pos, nova_color(), 26, 340.0, rng);
            sfx.write(SoundFx::NovaPop);
            return; // the ship lives
        }
    }
    burst(commands, pos, ship_color(), 30, 340.0, rng);
    burst(commands, pos, Color::srgb(4.0, 4.0, 5.0), 12, 220.0, rng);
    sfx.write(SoundFx::Death);
    commands.entity(ship_e).despawn();
    run.lives -= 1;
    run.died = true; // a real death (not a Nova absorb) — forfeits Untouchable for this run
    // Even on the last life we DON'T jump straight to Game Over — set a timer so the death
    // explosion plays out; `respawn` makes the transition once it elapses (less abrupt).
    run.respawn = if run.lives <= 0 { GAMEOVER_DELAY } else { RESPAWN_DELAY };
}

// Everything the player can DESTROY, summed. The Pacifist streak diffs this across a wave — every
// kill is already credited in exactly one place, so a sum here beats sprinkling a "broke something"
// flag into every kill site. Warps count: firing one is reaching for the destruction tool.
fn total_breaks(s: &Stats) -> u64 {
    s.blue as u64
        + s.green as u64
        + s.orange as u64
        + s.pulser as u64
        + s.red as u64
        + s.cluster as u64
        + s.beacon as u64
        + s.mines as u64
        + s.enemies as u64
        + s.golds as u64
        + s.warps as u64
}

// Credit a player rock-kill to its type's lifetime counter (the per-type achievements). One source
// of truth for the priority order (beacon/pulser are ALSO dense internally, so they check first).
// A rock's TYPE TAGS in one value. This used to be a run of positional bools threaded through
// `break_asteroid` (15 arguments) and `credit_rock_kill` — where transposing any two of them would
// silently give a rock the wrong behaviour AND the wrong kill credit, with nothing to catch it.
// Built once at each call site from the queried components; add a field here when adding a type.
#[derive(Clone, Copy, Default)]
struct Flavor {
    dense: bool,
    gold: bool,
    pulser: bool,
    red: bool,
    cluster: bool,
    beacon: bool,
    hunter: bool,
    lapse: bool,
    facet: bool,
    husk: bool,
}

// Build it from the optional components every rock query already carries.
#[allow(clippy::too_many_arguments)]
fn flavor(
    dense: bool,
    gold: Option<&Gold>,
    pulser: Option<&Pulser>,
    red: Option<&Red>,
    cluster: Option<&Cluster>,
    beacon: Option<&Beacon>,
    hunter: Option<&Hunter>,
    lapse: Option<&Lapse>,
    facet: Option<&Facet>,
    husk: Option<&Husk>,
) -> Flavor {
    Flavor {
        dense,
        gold: gold.is_some(),
        pulser: pulser.is_some(),
        red: red.is_some(),
        cluster: cluster.is_some(),
        beacon: beacon.is_some(),
        hunter: hunter.is_some(),
        lapse: lapse.is_some(),
        facet: facet.is_some(),
        husk: husk.is_some(),
    }
}

fn credit_rock_kill(stats: &mut Stats, f: Flavor) {
    let (dense, pulser, red, cluster, beacon, hunter, lapse) =
        (f.dense, f.pulser, f.red, f.cluster, f.beacon, f.hunter, f.lapse);
    if f.husk {
        stats.husk += 1;
    } else if f.facet {
        stats.facet += 1;
    } else if lapse {
        stats.lapse += 1;
    } else if hunter {
        stats.hunter += 1;
    } else if beacon {
        stats.beacon += 1;
    } else if pulser {
        stats.pulser += 1;
    } else if red {
        stats.red += 1;
    } else if cluster {
        stats.cluster += 1;
    } else if dense {
        stats.green += 1;
    } else {
        stats.blue += 1;
    }
}

// A combat kill of an enemy mob: award score, splash debris, play the death zap, despawn.
// Shared by the bullet hit and the chain-beam hit so the two can't drift apart.
fn kill_enemy(commands: &mut Commands, score: &mut Score, sfx: &mut EventWriter<SoundFx>, e: Entity, pos: Vec2, rng: &mut impl Rng) {
    score.0 += ENEMY_SCORE;
    burst(commands, pos, enemy_color(), 20, 320.0, rng);
    sfx.write(SoundFx::EnemyDie);
    commands.entity(e).despawn();
}

// ─────────────────────────────── state / components / resources ───────
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum GameState {
    #[default]
    Splash, // Baz Studios boot logo + sting (auto-advances to Menu; any input skips)
    Menu,
    Achievements, // the achievements screen, reached from the main menu
    Lore,         // the lore archive — entries decrypt as bosses fall (reached from the main menu)
    Gallery,      // the BESTIARY — one page per rock/hazard/boss, unlocked as you meet them
    Controls,     // input method + key/button rebinding, reached from the main menu
    Briefing,     // the lore + objectives screen, reached from the main menu
    Playing,
    Paused,
    GameOver,
    Victory, // beat the final boss (the Phantom, wave 30) — the win screen
}

// A run is "active" (grid + HUD drawn) in these states, not on the menu screens.
fn run_active(state: &GameState) -> bool {
    matches!(state, GameState::Playing | GameState::Paused | GameState::GameOver | GameState::Victory)
}

// One filter for "everything spawned during a run" — used to wipe the field when quitting to the
// menu or restarting (the starfield + camera are NOT in here, so the backdrop survives).
type GameplayEntity = Or<(
    With<Ship>,
    With<Asteroid>,
    With<Bullet>,
    With<Tender>,
    With<Particle>,
    With<BlackHole>,
    With<WarpMissile>,
    With<Mine>,
    With<Enemy>,
    With<EnemyBullet>,
    With<Shockwave>,
    With<Boss>,
    With<Devourer>,
    With<Slinger>,
    // (Cannonball entities are also Asteroids, so With<Asteroid> already covers them.)
    // Nested Or keeps this within Bevy's 15-element tuple-filter limit.
    Or<(With<ChainShot>, With<Pickup>, With<Drone>, With<Well>, With<Detonator>, With<Pulsar>, With<Phantom>, With<Possessed>, With<SpectralTrail>, With<EscapeShard>)>,
)>;

#[derive(Component)]
struct Ship {
    angle: f32, // facing, radians (CCW from +X; +Y is up)
    cooldown: f32,
    invuln: f32, // spawn-protection seconds (blinks while > 0)
    flame: f32,  // 0..1 thrust flame intensity
}

#[derive(Component)]
struct Velocity(Vec2);

#[derive(Component)]
struct Asteroid {
    size: u8,
    verts: Vec<Vec2>,
    rot: f32,
    spin: f32,
    dense: bool, // green + tanky: takes `hp` bullet hits before it cracks
    hp: i32,     // bullet hits remaining (1 for normal rocks; = size for dense)
}

#[derive(Component)]
struct Bullet {
    life: f32,
    trail: Vec<Vec2>,
    mass: bool, // a mass shot — bigger, harder-hitting
}

#[derive(Component)]
struct Particle {
    vel: Vec2,
    life: f32,
    ttl: f32,
    color: Color,
}

#[derive(Component)]
struct Star {
    phase: f32,
    bright: f32,
}

// A drifting proximity mine: arms when the ship is near, detonates after a fuse.
#[derive(Component)]
struct Mine {
    armed: bool,
    fuse: f32,
}

// An enemy ship: glides in, then hovers + strafes around the ship firing shots,
// until it's killed, sucked into a warp, or its lifetime runs out (then it flees).
#[derive(Component)]
struct Enemy {
    fire: f32,     // countdown to the next shot
    life: f32,     // time left before it bugs out
    strafe: f32,   // ±1 orbit direction
    entered: bool, // has it finished gliding onto the screen?
    fleeing: bool, // lifetime elapsed → heading for the nearest edge
}

// A TENDER drone. `job` holds the pair of fragments it's currently fusing plus the beam's progress;
// it's dropped the moment either target dies, so shooting one of them interrupts the salvage.
#[derive(Component)]
struct Tender {
    life: f32,
    entered: bool,
    fleeing: bool,
    cool: f32,
    job: Option<(Entity, Entity)>,
    progress: f32,
}

// A slow enemy shot. Distinct from the player's `Bullet` (never purple).
#[derive(Component)]
struct EnemyBullet {
    life: f32,
}

// The octopus boss core.
// Ring speed for the current whirl phase, x BOSS_SPIN. Shared because `boss_update` rotates the ring
// while `boss_shield` positions it — they must agree exactly or the sweep desyncs from its own arms.
fn whirl_spin_mult(w: Whirl, t: f32) -> f32 {
    match w {
        // the TELEGRAPH: the ring visibly STALLS, then creeps BACKWARDS at the end of the wind-up.
        // Nothing else in the fight does that, so it can't be mistaken for normal behaviour.
        Whirl::Wind => {
            let f = 1.0 - (t / NGP_WARDEN_WIND).clamp(0.0, 1.0);
            0.85 - 1.15 * f
        }
        // accelerate in, hold, ease out — it winds up visibly rather than snapping to full speed
        Whirl::Spin => {
            let f = 1.0 - (t / NGP_WARDEN_SPIN).clamp(0.0, 1.0);
            let ramp = (f / 0.3).min(1.0) * (1.0 - ((f - 0.8) / 0.2).clamp(0.0, 1.0) * 0.45);
            1.0 + (NGP_WARDEN_SPIN_MULT - 1.0) * ramp
        }
        Whirl::Recover => 0.35, // spent: the ring barely turns
        Whirl::Idle => 1.0,
    }
}

// How far the arms extend, x BOSS_ORBIT_R — the sweep's actual reach.
fn whirl_reach(w: Whirl, t: f32) -> f32 {
    if w != Whirl::Spin {
        return 1.0;
    }
    let f = 1.0 - (t / NGP_WARDEN_SPIN).clamp(0.0, 1.0);
    let ease = (f / 0.25).min(1.0) * (1.0 - ((f - 0.75) / 0.25).clamp(0.0, 1.0));
    1.0 + (NGP_WARDEN_WHIRL_REACH - 1.0) * ease
}

// The Warden+'s whirl state machine. Idle → Wind (telegraph) → Spin (the sweep) → Recover (spent).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Whirl {
    Idle,
    Wind,
    Spin,
    Recover,
}

#[derive(Component)]
struct Boss {
    hp: i32,
    rot: f32,     // shield rotation angle
    pulse: f32,   // visual throb + bob phase
    entered: bool, // finished gliding in?
    charge: f32,  // > 0 while powering up (invulnerable)
    fire: f32,    // countdown to the next throw
    capture: f32, // countdown to the next grab
    dying: f32,   // > 0 → death animation counting down; despawns at 0
    whirl: Whirl, // NG+ only: the charged spin attack's phase
    whirl_t: f32, // seconds left in the current whirl phase (or until the next one, while Idle)
}

// An asteroid captured onto the boss's shield (orbits slot `slot`; `grab` eases it in).
#[derive(Component)]
struct Shielded {
    slot: usize,
    grab: f32,
}

// Boss 2 (wave 10): a red seeker that hunts free rocks and EATS them to grow bigger (crowds the
// player) and tankier (heals). Starve it by clearing rocks while you chip its HP with gunfire.
#[derive(Component)]
struct Devourer {
    hp: i32,
    grow: f32, // 0..1 — feeds the radius (base → max)
    fed: i32,  // rocks eaten — also arms the NG+ REGURGITATE past NGP_GLUT_SPIT_FED
    dying: f32,
    pulse: f32,
    // NG+ only (NGP_GLUT_*). `inhale` counts down through wind-then-pull; `spit` is a wind-up that
    // fires at zero. Both sit at 0 for the base-game Glutton, which behaves exactly as before.
    inhale: f32,
    inhale_cd: f32,
    spit: f32,
}

impl Devourer {
    // Maw gaping = telegraph only, nothing is being pulled yet.
    fn inhale_winding(&self) -> bool {
        self.inhale > NGP_GLUT_INHALE_DUR
    }
    fn inhaling(&self) -> bool {
        self.inhale > 0.0 && !self.inhale_winding()
    }
}

// A rock the boss just hurled — briefly un-grabbable so it can't be re-captured instantly.
#[derive(Component)]
struct Thrown(f32);

// Boss 3 (wave 15): the Slinger — a gunner that loads a rock and fires it at the ship like a cannonball.
#[derive(Component)]
struct Slinger {
    hp: i32,
    entered: bool,        // finished gliding in
    charge: f32,          // > 0 = intro power-up (invulnerable)
    cool: f32,            // countdown to loading the next round
    load: f32,            // > 0 while a cannonball charges in front of it; launches at 0
    ammo: Option<Entity>, // the loaded cannonball (a `Cannonball`-tagged asteroid)
    pulse: f32,
    recoil: f32,          // > 0 → the hull kicks back after a launch (decays; pure spectacle)
    dying: f32,           // > 0 → death animation counting down; despawns at 0
}

// Boss 4 (wave 20): the Detonator — armored EXCEPT while priming a rock (that channel is the damage
// window). It halts, beams a nearby rock; when the channel completes the rock becomes a live bomb.
#[derive(Component)]
struct Detonator {
    hp: i32,
    entered: bool,
    charge: f32,            // > 0 = intro power-up (invulnerable)
    cool: f32,              // armored countdown to the next priming
    prime: f32,             // > 0 while priming — the VULNERABLE window; arms the bomb at 0
    target: Option<Entity>, // the rock being primed (beam target)
    pulse: f32,
    dying: f32,             // > 0 → death animation counting down; despawns at 0
}

// Boss 5 (wave 25): the Pulsar — invulnerable while LIT (its beat, via `pulser_lit(phase, t)`), open
// while DARK; periodically shockwaves every rock + the ship outward.
#[derive(Component)]
struct Pulsar {
    hp: i32,
    entered: bool,
    charge: f32,     // > 0 = intro power-up (invulnerable)
    phase: f32,      // lit/dark beat offset (fed to pulser_lit)
    shock_cool: f32, // countdown to the next fling-shock
    pulse: f32,
    dying: f32,      // > 0 → death animation counting down; despawns at 0
}

// Which stage the Sweep Ray is in: waiting, warning its quadrant, or actively sweeping the lethal beam.
#[derive(Clone, Copy, PartialEq)]
enum RayPhase {
    Idle,
    Telegraph,
    Fire,
}

// Boss 6 (wave 30, FINALE): the Phantom — THE HAUNT, a spectral predator too arrogant to be touched.
// Fights across three phases gated by a PER-PHASE health pool (`hp` refills each phase): deplete the phase
// → it RESETS (invulnerable `transition` beat, reforms + repositions) → the next phase begins. Every phase
// has the Sweep Ray (faster each). It is INTANGIBLE — shots pass through — EXCEPT during `vuln` (the window
// right after it fires the ray, when it must SURFACE). p2 possesses a homing rock; p3 turns solid + charges.
#[derive(Component)]
struct Phantom {
    hp: i32,          // health of the CURRENT phase (refills to PHANTOM_PHASE_HP on each reset)
    entered: bool,
    charge: f32,      // > 0 = intro power-up (invulnerable)
    pulse: f32,
    phase: u8,        // 1..=3 — advanced only by a completed reset, never mid-phase
    transition: f32,  // > 0 → the invulnerable RESET beat between phases (reposition + reform, no attacks)
    flash: f32,       // > 0 → a phase-start flash is fading (the spectacle: the mind fracturing further)
    victory: f32,     // > 0 → the finale is beaten: the death SCENE plays out before the Victory screen
    erupted: bool,    // the death scene has two beats: gather to CENTRE (false) → then ERUPT (light + core flees)
    vuln: f32,        // > 0 → SURFACED: solid, still, and hittable (set after each ray; the p1/p2 damage window)
    // ── Sweep Ray state (its own signature mechanic) ──
    ray: RayPhase,
    ray_cool: f32, // Idle: time until the next sweep begins
    ray_t: f32,    // elapsed time within the current Telegraph / Fire stage
    ray_from: f32, // beam start angle (radians) — the leading edge of the chosen quadrant
    ray_span: f32, // signed sweep width (± a quadrant): which way the beam rotates and how far
    // ── phase 3 — the HUNT (charge) state ──
    charge_cool: f32, // countdown to the next charge
    aim: f32,         // > 0 → aim-telegraph before a dash (locked on, eyes blazing)
    charging: f32,    // > 0 → mid-DASH, leaving the lethal trail
    charge_dir: Vec2, // locked dash direction (fixed when the aim starts — dodge sideways)
    // ── phase 2 — POSSESSION state ──
    possessed: Option<Entity>, // the vessel it's hiding in (None = hunting a rock, gliding to one, or just ripped out)
    seeking: Option<Entity>,   // the field rock it's currently gliding toward to possess (None = not yet fixed on one)
    dive: f32,                 // countdown before it goes hunting for the next rock
}

// The phase-2 VESSEL: the Haunt pours into a chunk of rock and hides inside it. The vessel homes at the
// ship and kills on contact (`possessed_update`); shots hit the vessel (`collisions`), and breaking it (its
// `hp`) rips the Haunt out into the open. `verts` is its rock silhouette; `pulse` drives the haunted glow.
#[derive(Component)]
struct Possessed {
    hp: i32,
    pulse: f32,
    verts: Vec<Vec2>,
}

// One spectral afterimage of the phase-3 charge: lingers `ttl`, kills the ship on contact while it lasts.
#[derive(Component)]
struct SpectralTrail {
    ttl: f32,
}

// A shard of the beaten Phantom, streaking off-screen after the finale kill — a seed for what comes next.
// Purely cosmetic (no collision); moved by its Velocity and culled at the edge in `escape_shard_update`.
// The Haunt's TRUE FORM — the searing core you glimpse through the mask-split while it's surfaced. When the
// finale kill shatters its shell, THIS is what's left, and it tears free and flees off-screen (the sequel
// seed). Moved along `dir` at an ease-in speed (slow rip-loose → accelerating streak); purely cosmetic.
#[derive(Component)]
struct EscapeShard {
    dir: Vec2,         // flight direction, fixed at birth
    spin: f32,
    age: f32,          // seconds alive → drives the ease-in speed ramp
    verts: Vec<Vec2>,  // its spiky little core silhouette (spun as it flies)
    trail: Vec<Vec2>,  // recent world positions → a fading comet streak so the escape reads
}

// The hero's ship, warping off EAST after the shard — the closing beat of the finale (the player pursues
// what fled). Cosmetic: a plain entity (NOT a `Ship`, so ship_control/ship_bounds leave it alone) flown
// off-screen by `departing_ship_update`, then culled. Spawned once the escaping shard has left the arena.
#[derive(Component)]
struct DepartingShip {
    flame: f32, // thrust flicker
}

// A short LIGHT TRAIL behind the ship — recent world positions drawn as fading segments (the logo's
// comet tail). Replaces the old triangular exhaust flame, which broke into "sparks" at speed (it was
// redrawn somewhere new each frame). Its own component rather than a Ship field so the many test
// `Ship { .. }` literals stay untouched; `spawn_player` (and the finale's DepartingShip) attach it,
// and `ship_trail` records into any entity that carries it. Stationary, the points coincide and the
// trail vanishes on its own.
#[derive(Component, Default)]
struct ShipTrail(Vec<Vec2>);

impl Phantom {
    // Fresh finale core, phase 1 with a full phase pool. Ray starts Idle with the first-sweep grace.
    fn new(hp: i32, entered: bool, charge: f32) -> Self {
        Phantom {
            hp,
            entered,
            charge,
            pulse: 0.0,
            phase: 1,
            transition: 0.0,
            flash: 0.0,
            victory: 0.0,
            erupted: false,
            vuln: 0.0,
            ray: RayPhase::Idle,
            ray_cool: PHANTOM_RAY_FIRST,
            ray_t: 0.0,
            ray_from: 0.0,
            ray_span: 0.0,
            charge_cool: PHANTOM_CHARGE_EVERY,
            aim: 0.0,
            charging: 0.0,
            charge_dir: Vec2::X,
            possessed: None,
            seeking: None,
            dive: 0.0,
        }
    }
}

// The Slinger's loaded/launched projectile — a large asteroid it charges then fires. Reuses the rock
// systems (bullets can shatter it to disarm; it kills the ship on contact) but despawns off-screen
// instead of recycling like a normal rock.
#[derive(Component)]
struct Cannonball {
    launched: bool,
}

// A freshly-broken fragment during its grace window: while this timer runs it recycles at the edges
// instead of being culled, so a rock shattered right at the border can't lose its pieces off-screen
// before the player gets a shot at them. Counts down in `asteroid_bounds`.
#[derive(Component)]
struct Fresh(f32);

// The rare gold 1UP asteroid. Inherited by every fragment it breaks into (see `break_asteroid`), so
// the whole lineage is gold until it's fully cleared. Destroy the entire lineage for +1 life; let a
// piece escape off-screen and the reward is forfeit. See [[neon-edge-design-doc]] "Life economy".
#[derive(Component)]
struct Gold;

// An explosive (orange) asteroid: instead of splitting when destroyed, it detonates — see `detonate`.
#[derive(Component)]
struct Explosive;

// A fractured ice rock (waves 26+): breaking it SHATTERS it into a ring of tiny fast shards instead
// of the usual two chunks. Shards carry the marker too (for the tint) but are size 1, so they just die.
#[derive(Component)]
struct Cluster;

// A teal warden rock (waves 23+): projects an aura (BEACON_AURA_R) — non-beacon rocks inside are
// immune to gunfire + the chain until it falls. Spawns dense (chips like a green), never splits.
#[derive(Component)]
struct Beacon;

// A HUNTER rock (waves 6+): the field's first thing that comes AFTER you. It steers toward the ship
// and `charge` ramps 0→1 over HUNTER_RAMP seconds, so the longer one lives the harder it drives —
// you cannot park and farm. Breaking one resets the hunt: children inherit the marker at charge 0
// (see `break_asteroid`), so a split buys you real breathing room instead of doubling the pressure.
// A HUSK: hollow, with a brood of Hunters inside (see `break_asteroid`).
#[derive(Component)]
struct Husk;

// A FACET rock: mirrored but for one open face. `open` is that face's angle RELATIVE to the rock's
// own rotation, so the vulnerable side sweeps as the rock spins and you have to track it.
#[derive(Component, Clone, Copy)]
struct Facet {
    open: f32,
}

// A player round that has been REFLECTED off a facet. It is now live against the ship — your own
// shot, coming back. Times out so the arena never fills with strays.
#[derive(Component)]
struct Ricochet(f32);

// A LAPSE rock's phase clock. `t` counts down within the current phase; `phase` cycles
// Solid → FadingOut → Gone → FadingIn → Solid. TANGIBLE (hittable AND lethal) only while Solid or
// FadingOut — so the whole return is a free warning, which is the point.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LapsePhase {
    Solid,
    FadingOut,
    Gone,
    FadingIn,
}

#[derive(Component, Clone, Copy)]
struct Lapse {
    phase: LapsePhase,
    t: f32,
}

impl Lapse {
    // Can it be shot, and can it kill? Only when it's really here.
    fn tangible(&self) -> bool {
        matches!(self.phase, LapsePhase::Solid | LapsePhase::FadingOut)
    }
    // 0..1 presence, for drawing the dissolve and the materialize.
    fn presence(&self) -> f32 {
        match self.phase {
            LapsePhase::Solid => 1.0,
            LapsePhase::FadingOut => (self.t / LAPSE_FADE_OUT).clamp(0.0, 1.0),
            LapsePhase::Gone => 0.0,
            LapsePhase::FadingIn => 1.0 - (self.t / LAPSE_FADE_IN).clamp(0.0, 1.0),
        }
    }
}

#[derive(Component)]
#[derive(Clone, Copy)]
struct Hunter {
    charge: f32,
    look: Vec2, // unit heading toward the ship — kept here so `render` can draw the eye without a Velocity join
}

// An orange rock that's been lit and is about to blow. The brief fuse gives a visible flash, then
// `detonate` blasts a radius and chains any other oranges caught in it.
#[derive(Component)]
struct Detonating {
    fuse: f32,
    friendly: bool, // true = the player's Warhead round (purple blast, spares the player); false = a hostile bomb (orange, lethal)
}

// A pulsing rock (waves 16+): invulnerable while LIT, vulnerable while DARK. `offset` phases its beat
// (from global time). Rendered white/dim; internally a dense rock so its fragments are green, not blue.
#[derive(Component)]
struct Pulser {
    offset: f32,
}

// A growing (red) asteroid (Act III): absorbs a nearby non-red rock every `cool` seconds to swell one
// size, up to large. Fragments inherit Red (see `break_asteroid`) so a broken red eats the field back up.
#[derive(Component)]
struct Red {
    cool: f32,
}

// A gravity-well hazard (waves 18+): drags the ship inward (see `well_pull`), then collapses after
// `life`. `spin` drives its swirl visual.
#[derive(Component)]
struct Well {
    life: f32,
    spin: f32,
}

// A brief expanding ring drawn where an explosion went off (the orange blast) — pure visual, no
// gameplay. Expands to `max_r` (the actual kill radius) over `ttl`, brightening the danger zone.
#[derive(Component)]
struct Shockwave {
    age: f32,
    ttl: f32,
    max_r: f32,
    color: Color,
}

// Tracks the current gold-rock hunt. `active` while a gold lineage is in play; `forfeited` latches if
// a gold piece is culled off-screen AFTER its (long) grace, so the life is denied even once the rest
// are cleared. `cooldown` counts down to the next spawn (re-armed to a random gap when a hunt ends),
// so gold appears at organic random times without spawning back-to-back.
#[derive(Resource, Default)]
struct GoldRush {
    active: bool,
    forfeited: bool,
    cooldown: f32,
}

// Gate so the click/keypress that STARTS or RESUMES a run doesn't also fire a shot on the first
// frame. Disarmed on entering Playing; `fire` re-arms it once the fire button is released, so you
// must press fresh to shoot. Avoids the "click PLAY → instant bullet" bleed-through.
#[derive(Resource, Default)]
struct FireArmed(bool);

// True once the player has left the main menu at least once. The neon title warm-up only plays on
// the very first show (app launch); later returns to the menu (from a sub-screen or a run) show the
// title already lit, so it doesn't re-flicker every time.
#[derive(Resource, Default)]
struct TitleIntroPlayed(bool);

// A chain-shot beam: travels along `Velocity`; the damaging lightning spans `perp`·±half.
#[derive(Component)]
struct ChainShot {
    life: f32,
    perp: Vec2,
}

// Which weapon a reward orb unlocks. Chain drops after boss 1, mass shot after boss 2, drone after boss 3.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickupKind {
    Chain,
    Mass,
    Drone,
    Warhead,
    Nova, // the Pulsar's drop (boss 5): the regenerating one-hit Nova Shield
    Aegis, // the Warden+'s drop (NG+ boss 1): orbiting shards that grind rocks off your hull
    Gorge, // the Glutton+'s drop (NG+ boss 2): a round that eats rocks and grows
}

// An ally drone (boss-3 reward): orbits the ship a short distance out and auto-fires at the nearest
// asteroid in range. Spawned when the Drone pickup is collected; one per run.
#[derive(Component)]
struct Drone {
    fire: f32,  // cooldown to the next shot
    angle: f32, // orbit phase around the ship
}

// Tags a player Warhead round (the shot mode): a piercing destroy-shot with a violet blast ring. Only the
// player's own Warhead shots carry it — the ally Drone fires plain standard bullets, so it never gets Warhead.
#[derive(Component)]
struct WarheadShot;

// The reward orb that drifts in the calm after a boss — fly into it (or shoot it) to unlock the
// weapon, or leave it (hardcore).
#[derive(Component)]
struct Pickup {
    rot: f32,
    pulse: f32,
    life: f32, // seconds the orb lingers before it's gone for good (outlives the boss calm)
    kind: PickupKind,
}

// UI markers (each overlay's root; despawned on state exit — despawn is recursive).
#[derive(Component)]
struct PauseUi;
#[derive(Component)]
struct GameOverUi;
#[derive(Component)]
struct VictoryUi;
// A victory-screen line, faded in on a stagger (credits-style). `color` is its final colour (it fades
// from alpha 0 once `VictoryReveal` passes `delay`).
#[derive(Component)]
struct VictoryLine {
    delay: f32,
    color: Color,
}
#[derive(Resource, Default)]
struct VictoryReveal(f32); // seconds since the win, driving the reveal
#[derive(Component)]
struct MenuUi;
#[derive(Component)]
struct AchievementsUi;
#[derive(Component)]
struct ControlsUi;
#[derive(Component)]
struct BriefingUi;
#[derive(Component)]
struct LoreUi;
#[derive(Component)]
struct Hud; // HUD roots — hidden on the menu screens

// Clickable menu buttons (mouse), mirrored by the keyboard shortcuts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Play,
    PlayPlus, // NEW GAME+ — the button exists only once the game has been beaten (stats.phantom)
    Achievements,
    Controls, // main menu → the controls / input-rebinding screen
    Briefing,
    Lore, // main menu → the lore archive
    Gallery, // main menu → the bestiary
    PageNext, // gallery paging
    PagePrev,
    Back,   // return to the main menu from a sub-screen
    Resume, // pause menu → back to the game
    Quit,   // pause menu → abandon the run to the main menu
    SetInput(InputMethod), // controls screen: choose the input method
    ResetBinds,            // controls screen: restore default bindings
}
#[derive(Component)]
struct MenuButton(MenuAction);
#[derive(Component)]
struct MenuTitle {
    age: f32, // seconds since spawn — drives the neon flicker-on then a steady breathe
}
#[derive(Component)]
struct MenuFrame; // the neon border frame — pulses with the title
#[derive(Event)]
struct MenuClick(MenuAction); // fired on click; menu_start / submenu_back / pause_toggle consume it
#[derive(Component)]
struct WaveText; // top-center "WAVE n  M:SS"
#[derive(Component)]
struct ScoreText; // top-left "SCORE n"
#[derive(Component)]
struct WaveBannerText; // big center-screen "WAVE n" flash that fades out

// The boss run-up telegraph (the last BOSS_CAMEO_SECS before a boss wave): a named warning line + a
// pulsing full-screen tint in the incoming boss's colour. Text/alpha driven by `boss_warning_update`.
#[derive(Component)]
struct BossWarnText; // "WARNING:  THE <boss> INCOMING", upper-centre
#[derive(Component)]
struct BossWarnFlash; // full-screen colour pulse behind the rest of the HUD
#[derive(Component)]
struct CalmCountdownText; // "NEXT WAVE IN n" — the visual countdown during the post-boss calm
#[derive(Component)]
struct ShotModeText; // top-center "MASS/STANDARD SHOT" label (under the wave text), fades after a Q toggle

// A named slot on the HUD ability strip. Each slot's LABEL (ui text) reveals when the ability is
// earned — the strip is an actual HUD: every light on it is named. (Not `Hud`-marked: visibility is
// state-driven by `hud_ability_labels`, which also handles the off-run hide.)
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum AbilitySlot {
    Warp,
    Chain,
    Mode,   // the Q-cycled shot modes (mass / Warhead)
    Shield, // the Nova Shield
    Drone,
}

// Warp: a slow missile that tears open a black hole which drags in + consumes rocks.
#[derive(Component)]
struct WarpMissile {
    life: f32,
}
#[derive(Component)]
struct BlackHole {
    life: f32,
    spin: f32,
}

#[derive(Resource, Default)]
struct Score(u32);

// Brief HUD flourishes: warp pips flicker for `pips` seconds after they refill, life icons for
// `life` seconds after a life is gained. Set at the event, ticked down by `hud_flash_tick`.
#[derive(Resource, Default)]
struct HudFlash {
    pips: f32,
    life: f32,
}

// Countdown for the "MASS SHOT / STANDARD SHOT" label after a Q toggle (drives its fade).
#[derive(Resource, Default)]
struct ShotModeFlash(f32);

// The persisted top-5 scores, sorted descending. `just_placed` is the index THIS run's score landed
// at (Some when it made the table, for the game-over highlight); it's transient, not saved.
#[derive(Resource, Default)]
struct HighScores {
    top: [u32; 5],
    just_placed: Option<usize>,
}

// The Nova Shield (the Pulsar's reward, boss 5 / wave 25): a regenerating one-hit barrier the player
// inherits from the boss's lit↔dark identity. While UP it eats one lethal hit and collapses; after
// NOVA_REGEN it flickers back on. A hit while it's DOWN costs a life as normal. Lives inside `Run`
// (per-run vital state) so every `kill_ship` caller already has it — no signature churn.
#[derive(Default, Clone, Copy)]
struct Nova {
    unlocked: bool,
    down: f32,  // >0 → collapsed, counting down to the re-light; 0 while UP
    grace: f32, // brief post-pop immunity so the hit that broke it can't also kill next frame
}

// The Aegis Shards' per-run state (see the AEGIS_* consts). `shards` is how many are live right now;
// `regen` counts down to the next regrowth; `spin` is the ring's rotation.
#[derive(Default, Clone, Copy)]
struct Aegis {
    unlocked: bool,
    shards: u8,
    regen: f32,
    spin: f32,
}

#[derive(Resource, Default, Clone, Copy)]
struct Run {
    lives: i32,
    respawn: f32,
    nova: Nova, // the Nova Shield's per-run state (see `Nova`)
    aegis: Aegis, // the Aegis Shards' per-run state (the Warden+'s NG+-only drop)
    died: bool, // any life lost this run (a Nova absorb doesn't count) — drives the deathless-win achievement
    powerup_fires: u32, // chain/mass/warhead activations this run — the Pacifist streak diffs this per wave (warp counts via stats.warps)
}

#[derive(Resource)]
struct Wave {
    level: i32,
    timer: f32,
    calm: f32, // > 0 during the post-boss calm — pauses spawns + the wave timer
}

// NEW GAME+ — the second lap, for players who've beaten the game (the menu button exists only once
// `stats.phantom` is set). Same 30-wave arc, harder at the SOURCE (never via player nerfs): a denser
// field from wave 1 (mobs and mines scale with it automatically — they're capped as fractions of
// the rock count), boss cores half again as tough, and a music-corruption FLOOR of tier 1 (dormant
// while the produced main ships as a single track — see MAIN_MP3). Selected per-run from the menu;
// restarting a run keeps the mode, launching normal PLAY clears it.
#[derive(Resource, Default, Clone, Copy)]
struct NewGamePlus(bool);
const NGP_POP_BONUS: i32 = 3; // extra rocks over the normal curve, every wave (the density dial).
// Cut 6 → 3 (2026-07-31) when the NG+ roster grew its own teeth: homing/phasing rocks and a mob that
// FUSES your debris back together are difficulty at the SOURCE, so raw volume doesn't have to be —
// and a thinner field leaves headroom for those mechanics without walling the screen.
const NGP_BOSS_HP_MULT: f32 = 1.5; // boss cores half again as tough
// The WARDEN+ (NG+ boss 1): the old kit at a meaner cadence, plus a new trick — every rock it
// hurls is PRIMED (a live bomb on a fuse): shoot it out of the air or clear the blast radius.
const NGP_WARDEN_RATE: f32 = 0.65; // throw + regrab cadence multiplier (lower = faster)
const NGP_WARDEN_VOLLEY: usize = 2; // rocks hurled per throw (a spread, not a single lob)
// THE WHIRL — the Warden+'s charged spin (NG+ only). It weaponizes the one thing the Warden already
// is: a keeper with rocks penned on arms. It winds up, then rips the whole ring around at speed with
// the arms extended, sweeping a lethal circle. STRICTLY TELEGRAPHED (user requirement): the wind-up
// visibly STALLS the ring and lights the core for a full NGP_WARDEN_WIND before anything moves fast,
// and the danger zone is a fixed radius you can simply be outside of — it never chases. Afterwards it
// hangs there spent, which is the player's reward window for reading it correctly.
// THE GLUTTON+ (NG+ boss 2). Both upgrades extend its one verb, EAT:
//   INHALE - the maw gapes and a suction WEDGE drags loose rocks AND THE SHIP toward it. The pull on
//     the ship is capped below its thrust (compile-time asserted), so flying out is always possible;
//     what it costs you is a dodge you had already committed to, and it feeds the boss while it runs.
//   REGURGITATE - once it has eaten enough it spits the mass back: a spread of rocks along its
//     facing. What went in is what comes out, so the count is readable, and the wind-up is the tell.
const NGP_GLUT_INHALE_EVERY: f32 = 8.0; // gap between inhales (from the end of the last)
const NGP_GLUT_INHALE_WIND: f32 = 1.1; // maw gapes - pure telegraph, nothing moves yet
const NGP_GLUT_INHALE_DUR: f32 = 2.2; // how long the suction runs
const NGP_GLUT_INHALE_REACH: f32 = 430.0; // wedge length
const NGP_GLUT_INHALE_ARC: f32 = 1.5; // wedge half-angle (radians) - a cone, not a sphere
const NGP_GLUT_INHALE_PULL: f32 = 520.0; // px/s^2 on the ship at the mouth - MUST stay under THRUST
const _: () = assert!(NGP_GLUT_INHALE_PULL < THRUST); // escapability is not negotiable
// The wedge must stay a cone (side-stepping is the counter) and the gape must be readable before
// anything moves. Both enforced at build time, like the pull cap above.
const _: () = assert!(NGP_GLUT_INHALE_ARC < TAU / 4.0);
const _: () = assert!(NGP_GLUT_INHALE_WIND >= 0.8);
const NGP_GLUT_ROCK_PULL: f32 = 900.0; // rocks are hauled harder than the ship (it is feeding)
const NGP_GLUT_SPIT_FED: i32 = 5; // rocks eaten before it can spit
const NGP_GLUT_SPIT_WIND: f32 = 0.9; // swell + lock on before firing
const NGP_GLUT_SPIT_ROCKS: usize = 5; // the spread
const NGP_GLUT_SPIT_ARC: f32 = 0.62; // total spread angle (radians)
const NGP_GLUT_SPIT_SPEED: f32 = 300.0;

const NGP_WARDEN_WHIRL_EVERY: f32 = 10.0; // gap between whirls (from the END of the last one)
const NGP_WARDEN_WIND: f32 = 1.7; // the telegraph: ring stalls, core charges — LONG on purpose
// The sweep must never arrive un-announced: shortening the wind-up below reaction time fails the BUILD.
const _: () = assert!(NGP_WARDEN_WIND >= 1.5);
const NGP_WARDEN_SPIN: f32 = 2.4; // the sweep itself
const NGP_WARDEN_RECOVER: f32 = 1.5; // spent afterwards: slow ring, no throws, no grabs
const NGP_WARDEN_SPIN_MULT: f32 = 6.5; // ring speed at full tilt, x BOSS_SPIN
const NGP_WARDEN_WHIRL_REACH: f32 = 1.5; // arms extend to this x BOSS_ORBIT_R during the sweep
// The sweep must stay a ZONE you can stand outside of, never an arena-wide hit.
const _: () = assert!(BOSS_ORBIT_R * NGP_WARDEN_WHIRL_REACH < 220.0);
const NGP_WARDEN_FUSE: f32 = 1.7; // the primed throw's fuse — ~475px of flight, then the blast

// A boss core's spawn HP for the current mode.
fn scaled_hp(base: i32, plus: bool) -> i32 {
    if plus { (base as f32 * NGP_BOSS_HP_MULT).round() as i32 } else { base }
}

// The Pacifist streak (clear 2 straight waves breaking nothing — dying is FINE, this is about
// restraint, not survival): snapshots taken at each wave start, diffed at its end. Breaking = any
// kill (total_breaks, warp fires included) or any powerup activation (Run.powerup_fires — chain,
// mass, warhead). `primed_at_level` catches boss waves sneaking into the window — a boss advance
// (defeat_boss) never re-primes, so the level mismatch marks the next check dirty and the streak
// resets (a boss kill is not pacifism, even a collateral-free one).
#[derive(Resource, Default)]
struct PacifistWatch {
    primed_at_level: i32,
    breaks: u64,
    fires: u32,
    streak: u32,
}

// Counts down while the big "WAVE n" flash is on screen (0 = hidden).
#[derive(Resource, Default)]
struct WaveBanner {
    timer: f32,
}

// Throttles the streamed-in replacement asteroids so the field refills gradually.
#[derive(Resource, Default)]
struct SpawnClock(f32);

// Throttles mine spawns.
#[derive(Resource, Default)]
struct MineClock(f32);

// Throttles enemy-ship spawns.
#[derive(Resource, Default)]
struct EnemyClock(f32);

// Throttles Tender spawns (NG+ late waves).
#[derive(Resource, Default)]
struct TenderClock(f32);


// Throttles gravity-well spawns.
#[derive(Resource, Default)]
struct WellClock(f32);

// Tracks the last boss wave a boss was spawned for, so exactly one spawns per wave.
#[derive(Resource, Default)]
struct BossState {
    fought: i32,
}

// Chain-shot state (the secondary weapon). `unlocked` flips when the pickup is grabbed.
#[derive(Resource, Default)]
struct Chain {
    unlocked: bool,
    charges: i32,
    recharge: f32, // countdown to regenerating one charge
    cooldown: f32, // min gap between shots
}

// Mass-shot state (primary-weapon upgrade). `unlocked` flips when its pickup is grabbed;
// `active` toggles standard↔mass with Q.
#[derive(Resource, Default)]
struct MassShot {
    unlocked: bool,
    active: bool,
}

// Warhead-rounds state (the Detonator's drop). A toggle shot mode (cycled with the mass shot via Q): a
// piercing round that DESTROYS each rock it passes through (no chunks, no chain) with a violet blast ring.
#[derive(Resource, Default)]
struct Warhead {
    unlocked: bool,
    active: bool,
}

// The Gorge Round shot mode (NG+ only — dropped by the Glutton+). Fourth entry on the Q cycle.
#[derive(Resource, Default)]
struct Gorge {
    unlocked: bool,
    active: bool,
}

// A live gorge round: `eaten` counts the rocks it has swallowed, which drives both its radius and its
// remaining lifetime. It is deliberately finite — a round that never stopped growing would trivialise
// a whole field.
#[derive(Component)]
struct GorgeShot {
    eaten: u32,
}

impl GorgeShot {
    fn radius(&self) -> f32 {
        (GORGE_R0 + GORGE_GROW * self.eaten as f32).min(GORGE_R_MAX)
    }
}

// ─────────────────────────────── achievements ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ach {
    FirstBlood,
    Warden,
    Glutton,
    Slinger,
    Detonator,
    Pulsar,
    TrueBlue,
    GreenThumb,
    Demolition,
    BeatIt,
    SeeingRed,
    IceBreaker,
    Keymaster,
    Whostheprey,
    ObjectPermanence,
    ThroughTheCracks,
    EmptyNest,
    PlannedObsolescence,
    Minesweeper,
    GoldRush,
    WaveGoodbye,
    EventHorizon,
    Runs10,
    Runs25,
    Runs50,
    Pacifist,
    Edgelord,
    Untouchable,
    Purist,
}
// Order defines the index into `Achievements.unlocked` and the menu list: the boss ladder, the
// rock-type grinds (one per type), the other lifetime grinds, the restart ladder, then the
// beat-the-game capstones.
const ACHIEVEMENTS: [Ach; 29] = [
    Ach::FirstBlood,
    Ach::Warden,
    Ach::Glutton,
    Ach::Slinger,
    Ach::Detonator,
    Ach::Pulsar,
    Ach::TrueBlue,
    Ach::GreenThumb,
    Ach::Demolition,
    Ach::BeatIt,
    Ach::SeeingRed,
    Ach::IceBreaker,
    Ach::Keymaster,
    Ach::Whostheprey,
    Ach::ObjectPermanence,
    Ach::ThroughTheCracks,
    Ach::EmptyNest,
    Ach::PlannedObsolescence,
    Ach::Minesweeper,
    Ach::GoldRush,
    Ach::WaveGoodbye,
    Ach::EventHorizon,
    Ach::Runs10,
    Ach::Runs25,
    Ach::Runs50,
    Ach::Pacifist,
    Ach::Edgelord,
    Ach::Untouchable,
    Ach::Purist,
];

// Lifetime-grind thresholds. Stats accumulate across EVERY run — and runs end in death a lot, by
// design — so these are set high on purpose: they're careers, not errands. Per-type targets scale
// with how much of the game each rock inhabits (blues everywhere; beacons rare and late).
const ACH_BLUE: u32 = 1000;
const ACH_GREEN: u32 = 500; // dense rocks are far rarer than blues
const ACH_ORANGE: u32 = 400;
const ACH_PULSER: u32 = 300; // every one is a timed dark-beat kill
const ACH_RED: u32 = 400;
const ACH_CLUSTER: u32 = 300;
const ACH_BEACON: u32 = 100;
const ACH_HUSK: u32 = 300; // each one hands you two chasers, so they're never cheap
const ACH_FACET: u32 = 400; // each one has to go through a MOVING gap — a real career
const ACH_LAPSE: u32 = 500; // NG+-only and common there, but NG+ itself is earned — a long career
const ACH_TENDERS: u32 = 100; // one at a time, late NG+ waves only: ~8-10 full second laps
const ACH_HUNTER: u32 = 350; // hunters run waves 6-9 (and every NG+ wave past 5), so this accrues fast on lap two // the rarest rock — and each takes deliberate focus
const ACH_MINES: u32 = 250;
const ACH_GOLDS: u32 = 25;
const ACH_WAVES: u32 = 250; // lifetime waves cleared (a full win is 30)
const ACH_WARPS: u32 = 150; // warp holes opened

fn ach_meta(a: Ach) -> (&'static str, &'static str) {
    match a {
        Ach::FirstBlood => ("First Blood", "Destroy an enemy ship"),
        Ach::Warden => ("Warden Off", "Defeat the Warden — boss 1"),
        Ach::Glutton => ("Glutton for Punishment", "Defeat the Glutton — boss 2"),
        Ach::Slinger => ("Outgunned", "Defeat the Slinger — boss 3"),
        Ach::Detonator => ("Defused", "Defeat the Detonator — boss 4"),
        Ach::Pulsar => ("Lights Out", "Defeat the Pulsar — boss 5"),
        Ach::TrueBlue => ("True Blue", "Destroy 1,000 blue asteroids"),
        Ach::GreenThumb => ("Green Thumb", "Destroy 500 dense green asteroids"),
        Ach::Demolition => ("Demolition Derby", "Set off 400 orange asteroids"),
        Ach::BeatIt => ("Beat It", "Crack 300 pulsers on the dark beat"),
        Ach::SeeingRed => ("Seeing Red", "Destroy 400 red asteroids"),
        Ach::IceBreaker => ("Ice Breaker", "Shatter 300 clusters"),
        Ach::Keymaster => ("Keymaster", "Crack 100 beacons"),
        Ach::Whostheprey => ("Who's the Prey Now", "Destroy 350 hunters"),
        Ach::ObjectPermanence => ("Object Permanence", "Destroy 500 lapse rocks — catch them while they exist"),
        Ach::ThroughTheCracks => ("Through the Cracks", "Crack 400 facets — every one through its open face"),
        Ach::EmptyNest => ("Empty Nest", "Crack open 300 husks — and deal with what was inside"),
        Ach::PlannedObsolescence => ("Planned Obsolescence", "Scrap 100 Tender repair drones"),
        Ach::Minesweeper => ("Minesweeper", "Destroy 250 mines"),
        Ach::GoldRush => ("Gold Rush", "Earn 25 extra lives from gold rocks"),
        Ach::WaveGoodbye => ("Wave Goodbye", "Clear 250 waves, lifetime"),
        Ach::EventHorizon => ("Event Horizon", "Open 150 warp holes"),
        Ach::Runs10 => ("Back for More", "Start 10 runs"),
        Ach::Runs25 => ("Sisyphus", "Start 25 runs"),
        Ach::Runs50 => ("The Definition of Insanity", "Start 50 runs"),
        Ach::Pacifist => ("Pacifist", "Clear two straight waves breaking nothing — warp and powerups included"),
        Ach::Edgelord => ("Edgelord", "Beat the game — defeat the Phantom at wave 30"),
        Ach::Untouchable => ("Untouchable", "Beat the game without losing a single life"),
        Ach::Purist => ("Purist", "Beat the game without a single powerup"),
    }
}

fn ach_met(a: Ach, s: &Stats) -> bool {
    match a {
        Ach::FirstBlood => s.enemies >= 1,
        Ach::Warden => s.warden,
        Ach::Glutton => s.glutton,
        Ach::Slinger => s.slinger,
        Ach::Detonator => s.detonator,
        Ach::Pulsar => s.pulsar,
        Ach::TrueBlue => s.blue >= ACH_BLUE,
        Ach::GreenThumb => s.green >= ACH_GREEN,
        Ach::Demolition => s.orange >= ACH_ORANGE,
        Ach::BeatIt => s.pulser >= ACH_PULSER,
        Ach::SeeingRed => s.red >= ACH_RED,
        Ach::IceBreaker => s.cluster >= ACH_CLUSTER,
        Ach::Keymaster => s.beacon >= ACH_BEACON,
        Ach::Whostheprey => s.hunter >= ACH_HUNTER,
        Ach::ObjectPermanence => s.lapse >= ACH_LAPSE,
        Ach::ThroughTheCracks => s.facet >= ACH_FACET,
        Ach::EmptyNest => s.husk >= ACH_HUSK,
        Ach::PlannedObsolescence => s.tenders >= ACH_TENDERS,
        Ach::Minesweeper => s.mines >= ACH_MINES,
        Ach::GoldRush => s.golds >= ACH_GOLDS,
        Ach::WaveGoodbye => s.waves >= ACH_WAVES,
        Ach::EventHorizon => s.warps >= ACH_WARPS,
        Ach::Runs10 => s.runs >= 10,
        Ach::Runs25 => s.runs >= 25,
        Ach::Runs50 => s.runs >= 50,
        Ach::Pacifist => s.pacifist,
        // The REAL win: the wave-30 Phantom kill. (Historically this fired on boss 2 — the old
        // 10-wave arc's "beat the game" — which read as unlocking a third of the way in.)
        Ach::Edgelord => s.phantom,
        Ach::Untouchable => s.deathless,
        Ach::Purist => s.no_powerups,
    }
}

// Counter achievements only: (current, target). Boss flags and the capstones are binary — they have
// no meaningful "progress" — so they return None and never appear in the nearest-grind ticker.
fn ach_progress(a: Ach, s: &Stats) -> Option<(u32, u32)> {
    match a {
        Ach::TrueBlue => Some((s.blue, ACH_BLUE)),
        Ach::GreenThumb => Some((s.green, ACH_GREEN)),
        Ach::Demolition => Some((s.orange, ACH_ORANGE)),
        Ach::BeatIt => Some((s.pulser, ACH_PULSER)),
        Ach::SeeingRed => Some((s.red, ACH_RED)),
        Ach::IceBreaker => Some((s.cluster, ACH_CLUSTER)),
        Ach::Keymaster => Some((s.beacon, ACH_BEACON)),
        Ach::Whostheprey => Some((s.hunter, ACH_HUNTER)),
        Ach::ObjectPermanence => Some((s.lapse, ACH_LAPSE)),
        Ach::ThroughTheCracks => Some((s.facet, ACH_FACET)),
        Ach::EmptyNest => Some((s.husk, ACH_HUSK)),
        Ach::PlannedObsolescence => Some((s.tenders, ACH_TENDERS)),
        Ach::Minesweeper => Some((s.mines, ACH_MINES)),
        Ach::GoldRush => Some((s.golds, ACH_GOLDS)),
        Ach::WaveGoodbye => Some((s.waves, ACH_WAVES)),
        Ach::EventHorizon => Some((s.warps, ACH_WARPS)),
        Ach::Runs10 => Some((s.runs, 10)),
        Ach::Runs25 => Some((s.runs, 25)),
        Ach::Runs50 => Some((s.runs, 50)),
        _ => None,
    }
}

// The unfinished counter achievement closest to done — the game-over screen's "one more run" hook.
// Every death advanced SOMETHING; this line points at the something.
fn nearest_grind(s: &Stats) -> Option<(Ach, u32, u32)> {
    ACHIEVEMENTS
        .iter()
        .filter_map(|&a| ach_progress(a, s).map(|(c, t)| (a, c, t)))
        .filter(|&(_, c, t)| c < t)
        .max_by(|a, b| (a.1 as f32 / a.2 as f32).total_cmp(&(b.1 as f32 / b.2 as f32)))
}

// LIFETIME progress — accumulates across runs and is persisted to disk (see load/save_progress).
// NOT reset by `reset_run`.
#[derive(Resource, Default, Clone, Copy)]
struct Stats {
    blue: u32,         // blue asteroids destroyed (lifetime)
    green: u32,        // dense green asteroids destroyed (lifetime)
    enemies: u32,      // enemy ships destroyed (lifetime)
    warden: bool,      // ever defeated boss 1
    glutton: bool,     // ever defeated boss 2
    no_powerups: bool, // ever beat the GAME (wave 30) having grabbed no powerup that run
    slinger: bool,     // ever defeated boss 3
    detonator: bool,   // ever defeated boss 4
    pulsar: bool,      // ever defeated boss 5
    phantom: bool,     // ever defeated the Phantom (boss 6, wave 30) = beat the game
    mines: u32,        // mines destroyed (lifetime, player-credited)
    golds: u32,        // gold lineages fully cleared (extra lives earned, lifetime)
    orange: u32,       // orange (explosive) rocks lit/destroyed by the player's fire (lifetime)
    pulser: u32,       // pulsers cracked on the dark beat (lifetime)
    red: u32,          // red (growing) rocks destroyed (lifetime)
    cluster: u32,      // clusters shattered (lifetime)
    beacon: u32,       // beacons cracked (lifetime)
    runs: u32,         // runs STARTED (every restart counts — the player dies a lot, by design)
    waves: u32,        // waves cleared, lifetime (advancing past any wave counts one)
    warps: u32,        // warp holes opened (lifetime)
    deathless: bool,   // ever beat the game without losing a single life
    best_wave: u32,    // deepest wave ever REACHED — the game-over screen's "you were close" marker
    pacifist: bool,    // ever survived two straight timer waves breaking nothing (and not dying)
    hunter: u32,       // hunter rocks destroyed (lifetime)
    lapse: u32,        // lapse rocks destroyed (lifetime)
    husk: u32,         // husks cracked open (lifetime)
    facet: u32,        // facet rocks cracked (lifetime) — every one through its open face — caught while SOLID, which is the trick
    tenders: u32,      // Tender repair drones destroyed (lifetime)
    seen: u32,         // GALLERY sightings bitmask — one bit per subject, set the frame it first
                       // appears on your field (see `gallery_bit` / `gallery_sightings`)
}

// Which achievements are unlocked (drives the toast + the menu list). Initialized from the loaded
// Stats at startup; the `achievements` system flips a bool + fires a toast the first time each is met.
#[derive(Resource, Default)]
struct Achievements {
    unlocked: [bool; ACHIEVEMENTS.len()],
}

// Per-RUN flags, cleared each run by `reset_run`.
#[derive(Resource, Default)]
struct RunFlags {
    powerup_used: bool, // grabbed any pickup this run (for the Purist achievement)
}

// Which Pilot Log entries have been SEEN decrypted (drives the "PILOT LOG UPDATED" toast).
// Initialized from the loaded Stats at startup so a returning save never re-toasts old reports;
// `lore_watch` flips a slot + pops a toast the first frame its gate opens.
#[derive(Resource, Default)]
struct LoreSeen([bool; 8]);

#[derive(Component)]
struct ToastRoot; // persistent top-center column that unlock toasts stack into
#[derive(Component)]
struct Toast {
    life: f32,
}
const TOAST_LIFE: f32 = 3.5; // seconds an unlock toast lingers

fn is_boss_wave(level: i32) -> bool {
    level % BOSS_WAVE_INTERVAL == 0
}

// Waves 1-20 are hand-authored; 21+ loop back over that arc (we build the arc out in five-wave acts).
fn content_wave(level: i32) -> i32 {
    (level - 1).rem_euclid(30) + 1
}
// Boss waves alternate: content-10 = the devourer (boss 2); content-5 = the shaman (boss 1).
fn is_devourer_wave(level: i32) -> bool {
    is_boss_wave(level) && content_wave(level) == 10
}
fn is_slinger_wave(level: i32) -> bool {
    is_boss_wave(level) && content_wave(level) == 15
}
fn is_detonator_wave(level: i32) -> bool {
    is_boss_wave(level) && content_wave(level) == 20
}
fn is_pulsar_wave(level: i32) -> bool {
    is_boss_wave(level) && content_wave(level) == 25
}
fn is_phantom_wave(level: i32) -> bool {
    is_boss_wave(level) && content_wave(level) == 30
}
// True during the run-up to a boss wave (the last BOSS_CAMEO_SECS before it): drives the background
// cameo, the music riser, and clearing stray mobs off the field so the boss arrives to a clean arena.
fn boss_incoming(wave: &Wave) -> bool {
    !is_boss_wave(wave.level) && is_boss_wave(wave.level + 1) && wave.calm <= 0.0 && wave.timer <= BOSS_CAMEO_SECS
}
// Which boss the given level's wave is (or, for the run-up, `level + 1`). Used for the cameo + color.
fn boss_kind(level: i32) -> BossKind {
    if is_devourer_wave(level) {
        BossKind::Devourer
    } else if is_slinger_wave(level) {
        BossKind::Slinger
    } else if is_detonator_wave(level) {
        BossKind::Detonator
    } else if is_pulsar_wave(level) {
        BossKind::Pulsar
    } else if is_phantom_wave(level) {
        BossKind::Phantom
    } else {
        BossKind::Warden
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum BossKind {
    Warden,
    Devourer,
    Slinger,
    Detonator,
    Pulsar,
    Phantom,
}
fn boss_kind_color(k: BossKind) -> Color {
    match k {
        BossKind::Warden => boss_color(),
        BossKind::Devourer => devourer_color(),
        BossKind::Slinger => slinger_color(),
        BossKind::Detonator => detonator_color(),
        BossKind::Pulsar => pulsar_color(),
        BossKind::Phantom => phantom_color(),
    }
}
// The player-facing boss name. The Devourer is surfaced as "the Glutton" everywhere the player reads a
// name (achievements, changelog), so match that here rather than the internal enum spelling.
fn boss_kind_name(k: BossKind) -> &'static str {
    match k {
        BossKind::Warden => "THE WARDEN",
        BossKind::Devourer => "THE GLUTTON",
        BossKind::Slinger => "THE SLINGER",
        BossKind::Detonator => "THE DETONATOR",
        BossKind::Pulsar => "THE PULSAR",
        BossKind::Phantom => "THE PHANTOM",
    }
}
fn devourer_radius(grow: f32) -> f32 {
    DEVOURER_BASE_R + grow.clamp(0.0, 1.0) * (DEVOURER_MAX_R - DEVOURER_BASE_R)
}

// Shared "boss defeated" bookkeeping (both bosses use it): reward, then advance into the calm.
fn defeat_boss(score: &mut Score, wave: &mut Wave, banner: &mut WaveBanner, stats: Option<&mut Stats>) {
    score.0 += BOSS_SCORE;
    wave.level += 1;
    wave.timer = WAVE_SECS;
    wave.calm = BOSS_CALM;
    banner.timer = WAVE_BANNER_SECS;
    if let Some(s) = stats {
        s.waves += 1; // a boss wave cleared counts toward the lifetime wave tally
    }
}

#[derive(Resource)]
struct Warp {
    charges: i32,
    cooldown: f32, // > 0 only while refilling after all charges were spent
}

// Drives the grid's pull-toward-hole warp + its elastic snapback. `amount` is the
// warp strength: eases 0→1 while a hole is open, then snaps 1→0 (overshooting
// negative = grid bulges out) over WARP_SNAP_DUR after it closes.
#[derive(Resource, Default)]
struct WarpField {
    pos: Vec2,
    active: bool,
    snap_t: f32,
    amount: f32,
}

#[derive(Resource)]
struct Arena {
    half: Vec2,
}

// ─────────────────────────────── setup / spawners ─────────────────────
fn setup(mut commands: Commands) {
    // HDR + bloom camera → the neon glow. (Global bloom stays at Bevy's default;
    // the warp shot glows harder via its own brighter HDR colors, not more bloom.)
    commands.spawn((
        Camera2d,
        Camera { hdr: true, ..default() },
        Tonemapping::TonyMcMapface,
        Bloom::default(),
        // Scale-to-fit: DESIGN_H world-units of HEIGHT always fill the window height, so the game renders at
        // a consistent apparent size on ANY monitor (magnify, don't reveal-more). Width follows the window
        // aspect — `update_arena` sizes the arena to it, so it fills the screen with no letterbox/distortion.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: DESIGN_H },
            ..OrthographicProjection::default_2d()
        }),
    ));

    // starfield — fixed positions, each with its own twinkle phase
    let mut rng = rand::thread_rng();
    for _ in 0..STAR_COUNT {
        // NORMALIZED [-1,1] — scaled to the live arena at draw time so the field always fills the screen,
        // whatever its size or aspect (a fixed world box left dark starless margins on big/ultrawide monitors).
        let pos = Vec2::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0));
        commands.spawn((
            Star { phase: rng.gen_range(0.0..TAU), bright: rng.gen_range(0.3..1.0) },
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
    }

    // No ship yet — the game boots to the main menu; a run spawns the player on Start.
    // The field then starts EMPTY and `top_up_asteroids` drifts rocks in from the edges.
}

// Persistent HUD. Lives label (top-right; the ship-icon count is drawn per-frame in
// `render`), score (top-left), and wave + timer (top-center).
fn spawn_hud(mut commands: Commands) {
    // Full-screen boss-warning tint — spawned FIRST so every other HUD element renders on top of it.
    // Transparent until `boss_warning_update` pulses it during the 10s boss run-up.
    commands.spawn((
        Hud,
        BossWarnFlash,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    let label = Color::srgb(0.7, 0.85, 1.2);
    commands.spawn((
        Hud,
        Text::new("LIVES"),
        TextFont { font_size: 18.0, ..default() },
        TextColor(label),
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), right: Val::Px(22.0), ..default() },
    ));
    // the ability strip's slot labels (left edge, above the glyph row) — each names its light, and
    // reveals only once that ability is earned (see `hud_ability_labels`)
    for (slot, name, x) in [
        (AbilitySlot::Warp, "WARP", HUD_SLOT_WARP),
        (AbilitySlot::Chain, "CHAIN", HUD_SLOT_CHAIN),
        (AbilitySlot::Mode, "MODE", HUD_SLOT_MODE),
        (AbilitySlot::Shield, "SHIELD", HUD_SLOT_SHIELD),
        (AbilitySlot::Drone, "DRONE", HUD_SLOT_DRONE),
    ] {
        commands.spawn((
            slot,
            Text::new(name),
            TextFont { font_size: 14.0, ..default() },
            TextColor(dim(label, 0.8)),
            Node { position_type: PositionType::Absolute, top: Val::Px(HUD_STRIP_LABEL_TOP), left: Val::Px(x), ..default() },
            Visibility::Hidden, // revealed by hud_ability_labels once earned (Warp: once a run is on)
        ));
    }
    commands.spawn((
        Hud,
        ScoreText,
        Text::new("SCORE 0"),
        TextFont { font_size: 18.0, ..default() },
        TextColor(label),
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), left: Val::Px(22.0), ..default() },
    ));
    // centered wrapper so the wave/timer sits at the top-center
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                WaveText,
                Text::new("WAVE 1    3:00"),
                TextFont { font_size: 22.0, ..default() },
                TextColor(Color::srgb(0.85, 0.9, 1.2)),
            ));
        });
    // big center-screen "WAVE n" flash — alpha driven by wave_banner_update
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                WaveBannerText,
                Text::new(""),
                TextFont { font_size: 66.0, ..default() },
                TextColor(Color::srgba(0.8, 0.9, 1.3, 0.0)),
            ));
        });
    // post-boss countdown to the next wave (shown during the 10s calm, below the WAVE banner)
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(57.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                CalmCountdownText,
                Text::new(""),
                TextFont { font_size: 32.0, ..default() },
                TextColor(Color::srgba(0.72, 0.85, 1.15, 0.0)),
            ));
        });
    // boss run-up warning — names the incoming boss (upper-centre; colour/alpha from boss_warning_update)
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(40.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                BossWarnText,
                Text::new(""),
                TextFont { font_size: 38.0, ..default() },
                TextColor(Color::srgba(1.0, 0.3, 0.3, 0.0)),
            ));
        });
    // shot-mode NAME — part of the MODE slot on the ability strip (sits right of its bracket glyph):
    // names the equipped Q-selection, flaring bright on a toggle
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(HUD_STRIP_LABEL_TOP + 24.0),
                left: Val::Px(HUD_SLOT_MODE + 44.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                ShotModeText,
                Text::new(""),
                TextFont { font_size: 16.0, ..default() },
                TextColor(Color::srgba(0.72, 0.28, 1.0, 0.0)), // violet (player kit), starts hidden
            ));
        });
}

fn spawn_player(commands: &mut Commands) {
    commands.spawn((
        Ship { angle: TAU / 4.0, cooldown: 0.0, invuln: SPAWN_INVULN, flame: 0.0 },
        ShipTrail::default(),
        Velocity(Vec2::ZERO),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

// On entering Playing (start, restart, OR resume-from-pause), disarm the gun so the click/keypress
// that got us here doesn't leak into an instant shot. `fire` re-arms on the first release.
fn disarm_fire(mut armed: ResMut<FireArmed>) {
    armed.0 = false;
}

// The three flavors of edge-spawned rock. A rock is exactly one — never both green and orange.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RockKind {
    Blue,    // plain
    Green,   // dense / tanky (takes `hp` hits)
    Hunter,  // vermillion predator (Act I, w6+) — HOMES on the ship, accelerating the longer it lives
    Lapse,   // NG+ roster — phases OUT of existence and materializes again elsewhere
    Facet,   // NG+ roster — mirrored: reflects your rounds off every face but one
    Husk,    // NG+ roster — hollow: cracking it lets out a pair of hunters
    Orange,  // explosive (detonates instead of splitting)
    Pulser,  // pulses lit (invulnerable) ↔ dark (vulnerable) — hit it on the dark beat
    Red,     // growing (Act III) — absorbs nearby rocks to swell; a plain shot splits it into more reds
    Cluster, // fractured ice (late Act III) — SHATTERS into a ring of tiny fast shards
    Beacon,  // teal warden (late Act III) — aura-shields nearby rocks from gunfire until it falls
}

// Which flavor should a rock spawned for `level` be? One roll shared by every edge-spawn caller,
// so a wave's whole rock mix is defined here. Fractions are the tuning knobs for wave feel.
fn roll_rock_kind(level: i32, plus: bool, rng: &mut impl Rng) -> RockKind {
    // NEW GAME+ runs its own bestiary. Waves 1-5 recap the OLD roster (`roll_ngplus_opener` — the
    // pilot has seen it all, so no teaching rosters, but the NEW types are held back); from wave 6
    // the old roster RETIRES entirely and lap two runs new rock types only (`roll_ngplus_kind`).
    if plus {
        return if content_wave(level) <= 5 { roll_ngplus_opener(rng) } else { roll_ngplus_kind(level, rng) };
    }
    let cw = content_wave(level);
    // Beacon (aura warden) — RARE, rolled first so nothing eats its slice. Debuts w23 (target-order
    // pressure right as green's tankiness retires), a fixture through the late act.
    let beacon = match cw {
        23 | 24 => 0.12,
        26..=29 => 0.1,
        _ => 0.0,
    };
    if rng.gen_bool(beacon) {
        return RockKind::Beacon;
    }
    // Cluster (shatters into shards) — debuts w26, splitting Act III into two eras; heavy through the
    // pre-finale gauntlet.
    let cluster = match cw {
        26 | 27 => 0.35,
        28 | 29 => 0.3,
        _ => 0.0,
    };
    if rng.gen_bool(cluster) {
        return RockKind::Cluster;
    }
    // Red (growing) — ACT III'S CARRIER: it needs no roll of its own, it's the act's FALLBACK below
    // (waves 21-29 are red wherever beacon/cluster don't land — wave 21 is the all-red teaching wave,
    // and the Pulsar's wave 25 is fought over a pure red field its shockwaves keep scattering).
    // Pulser (invuln-when-lit) — an ACT II type: debuts w16, retires with its act at 20 (no rock
    // outlives its act — user rule; the wave-30 finale's all-types roll is the one exception).
    let pulser = match cw {
        16 => 1.0, // wave 16 is pulser-ONLY — a pure timing wave to learn the beat
        17 | 19 => 0.3,
        18 => 0.55,
        _ => 0.0,
    };
    if rng.gen_bool(pulser) {
        return RockKind::Pulser;
    }
    // Orange (explosive) — an ACT II type: debuts w11, peaks at 14, and is GONE by wave 20, its own
    // boss's wave. The Detonator can't prime explosives (they're already bombs), so every orange on
    // its field was a dead slot that left the boss hunting — armored — for something green: wave 20
    // is pure green fodder now, and the boss itself brings the explosions. No orange in Act III.
    let orange = match cw {
        11..=13 => 0.25,
        14 => 1.0,  // the all-orange danger wave
        17..=19 => 0.3,
        _ => 0.0, // wave 30 (the finale) rolls its own all-types mix (`roll_finale_kind`), not this table
    };
    if rng.gen_bool(orange) {
        return RockKind::Orange;
    }
    // Hunter (homing) — an ACT I type: wave 6 is its DEBUT and headline (the first rock that comes
    // after you, landing right after the Warden teaches you that things can chase), it garnishes
    // 7-9 alongside green, and it retires with its act at wave 10. Rolled before green so its
    // teaching wave isn't eaten by the dense baseline.
    let hunter = match cw {
        6 => 0.7, // the teaching wave — mostly hunters, a few blues to break the pressure
        7..=9 => 0.25,
        _ => 0.0,
    };
    if rng.gen_bool(hunter) {
        return RockKind::Hunter;
    }
    // Green (dense) — bridges Act I (now debuts 7, after the hunter's wave) and CARRIES Act II (the
    // 11-19 baseline), then retires with its act at wave 20. No green in Act III.
    let green = match cw {
        7..=9 => 1.0,
        11..=13 | 15..=17 | 19 => 1.0,
        _ => 0.0,
    };
    if rng.gen_bool(green) {
        return RockKind::Green;
    }
    // EACH ACT OWNS ITS ROSTER (user rule): blue lives only in Act I (1-10); Act II (11-20) runs
    // green + orange + pulser; Act III (21-29) runs red + beacon + cluster, with RED as the carrier —
    // so the fallback here is the current act's baseline rock. Keyed on content_wave so a later loop
    // repeats the arc cleanly. (Wave 30, the finale, is the one all-types exception.)
    if cw > 20 {
        RockKind::Red
    } else if cw > 10 {
        RockKind::Green
    } else {
        RockKind::Blue
    }
}

// Spawn one rock of `kind` at `pos` and apply its flavor (Explosive / Pulser / Red tag + dense green).
// Factored out so the kind→component tagging lives in one place; called by `spawn_edge_asteroid`.
fn spawn_kind_rock(commands: &mut Commands, pos: Vec2, size: u8, vel: Vec2, rng: &mut impl Rng, kind: RockKind) -> Entity {
    // Pulsers spawn DENSE (a few dark-beat hits, fragments stay dense = no blue) with a random beat phase;
    // they break into smaller pulsers (see `break_asteroid`). Beacons are dense too: the aura's key should
    // take deliberate focus to crack (they never split — see break_asteroid).
    let dense = matches!(kind, RockKind::Green | RockKind::Pulser | RockKind::Beacon);
    let e = spawn_asteroid(commands, pos, size, vel, rng, dense);
    match kind {
        RockKind::Orange => {
            commands.entity(e).insert(Explosive); // detonates instead of splitting (see `detonate`)
        }
        RockKind::Pulser => {
            commands.entity(e).insert(Pulser { offset: rng.gen_range(0.0..TAU) });
        }
        RockKind::Red => {
            commands.entity(e).insert(Red { cool: RED_ABSORB_EVERY }); // grows by absorbing nearby rocks
        }
        RockKind::Cluster => {
            commands.entity(e).insert(Cluster); // shatters into a shard ring (see break_asteroid)
        }
        RockKind::Beacon => {
            commands.entity(e).insert(Beacon); // aura-shields its neighbours (see collisions/chain)
        }
        RockKind::Hunter => {
            commands.entity(e).insert(Hunter { charge: 0.0, look: Vec2::X }); // starts docile, ramps into a chaser
        }
        RockKind::Husk => {
            commands.entity(e).insert(Husk);
        }
        RockKind::Facet => {
            commands.entity(e).insert(Facet { open: rng.gen_range(0.0..TAU) });
        }
        RockKind::Lapse => {
            // a randomized opening spell, so a field of them phases out of step
            commands.entity(e).insert(Lapse { phase: LapsePhase::Solid, t: rng.gen_range(LAPSE_SOLID_MIN..LAPSE_SOLID_MAX) });
        }
        RockKind::Blue | RockKind::Green => {} // plain / dense — no extra tag
    }
    e
}

// The wave-30 finale field: every type the Belt has shown, rolled at RANDOM — the trickle rate (not a
// mono-group cycle) is what keeps it readable. The beacon stays rare: an aura mid-boss-fight is spice,
// not a wall.
fn roll_finale_kind(rng: &mut impl Rng) -> RockKind {
    if rng.gen_bool(0.06) {
        return RockKind::Beacon;
    }
    match rng.gen_range(0..7) {
        0 => RockKind::Blue,
        1 => RockKind::Green,
        2 => RockKind::Orange,
        3 => RockKind::Pulser,
        4 => RockKind::Red,
        5 => RockKind::Hunter,
        _ => RockKind::Cluster,
    }
}

// NEW GAME+ from wave 6 on: the OLD ROSTER IS RETIRED (user rule, 2026-07-31) — lap two opens on the
// greatest-hits mix through wave 5, then sheds every rock the first lap taught you and runs the NEW
// bestiary instead. ⚠️ Only the Hunter exists so far, so NG+ 6-30 is currently a single-type field;
// each new rock type added here widens it.
// NG+ waves 1-5: a greatest-hits of LAP ONE — the OLD roster only. The new bestiary is held back
// until wave 6 (user rule, 2026-07-31) so the opener is pure recap and the roster switch at 6 lands
// as a distinct event. Deliberately NOT `roll_finale_kind`: that one now includes the Hunter (a
// base-game Act I rock), which would leak the new roster into the opener.
fn roll_ngplus_opener(rng: &mut impl Rng) -> RockKind {
    if rng.gen_bool(0.08) {
        return RockKind::Beacon; // the rare spice, same as the finale mix
    }
    match rng.gen_range(0..6) {
        0 => RockKind::Blue,
        1 => RockKind::Green,
        2 => RockKind::Orange,
        3 => RockKind::Pulser,
        4 => RockKind::Red,
        _ => RockKind::Cluster,
    }
}

fn roll_ngplus_kind(level: i32, rng: &mut impl Rng) -> RockKind {
    // The lap-two bestiary, introduced IN ORDER so the second lap still teaches: Hunter and Lapse
    // carry waves 6-7, the Facet debuts at 8 (wave 8 is mostly mirrors — its teaching wave).
    // The roster rule means everything past wave 5 comes from here; widen as new types land.
    let cw = content_wave(level);
    if cw >= 9 && rng.gen_bool(if cw == 9 { 0.5 } else { 0.28 }) {
        return RockKind::Husk; // wave 9 is its teaching wave — learn to check for the hollow
    }
    if cw >= 8 && rng.gen_bool(if cw == 8 { 0.6 } else { 0.32 }) {
        return RockKind::Facet;
    }
    if rng.gen_bool(0.45) { RockKind::Lapse } else { RockKind::Hunter }
}

fn spawn_edge_asteroid(commands: &mut Commands, half: Vec2, rng: &mut impl Rng, kind: RockKind, force_big: bool) -> Entity {
    // mostly LARGE rocks (break into mid → small), with some MID ones mixed in. `force_big`
    // guarantees a LARGE one (used to refill the big-rock floor).
    let size = if force_big || rng.gen_bool(0.8) { 3 } else { 2 };
    let r = asteroid_radius(size);
    let inward = rng.gen_range(50.0..110.0);
    let jitter = rng.gen_range(-40.0..40.0);
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - r, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + r, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - r), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + r), Vec2::new(jitter, -inward)),
    };
    spawn_kind_rock(commands, pos, size, vel, rng, kind)
}

// Spawn the rare gold 1UP asteroid: a large rock from a random edge, tagged `Gold` so it (and every
// fragment it breaks into) is part of the lineage the player must fully clear for the extra life.
fn spawn_gold_rock(commands: &mut Commands, half: Vec2, rng: &mut impl Rng) {
    let e = spawn_edge_asteroid(commands, half, rng, RockKind::Blue, true); // always large, plain (not dense/explosive)
    commands.entity(e).insert(Gold);
}


// A jagged rock outline sized for `size` (regenerated when a shield rock shrinks).
fn asteroid_verts(size: u8, rng: &mut impl Rng) -> Vec<Vec2> {
    let r = asteroid_radius(size);
    let n = rng.gen_range(9..14);
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            let rr = r * rng.gen_range(0.72..1.12);
            Vec2::new(a.cos() * rr, a.sin() * rr)
        })
        .collect()
}

fn spawn_asteroid(commands: &mut Commands, pos: Vec2, size: u8, vel: Vec2, rng: &mut impl Rng, dense: bool) -> Entity {
    commands
        .spawn((
            Asteroid {
                size,
                verts: asteroid_verts(size, rng),
                rot: rng.gen_range(0.0..TAU),
                spin: rng.gen_range(-0.8..0.8),
                dense,
                hp: if dense { size as i32 } else { 1 },
            },
            Velocity(vel),
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ))
        .id()
}

// Shatter one rock: despawn it, award score, splash debris, and (unless it's the
// smallest) split it into two smaller rocks flung outward. `chunk_mult` scales
// the child fling speed — 1.0 for a normal bullet break; a mine blast passes a
// bigger value so its chunks scatter faster (a discoverable interaction).
#[allow(clippy::too_many_arguments)]
// `chunks`: whether a size>1 rock spawns its two smaller fragments. True for every normal break; the
// mass shot passes false to VAPORIZE a rock outright (its field-clearing identity — no rubble left).
fn break_asteroid(commands: &mut Commands, rng: &mut impl Rng, score: &mut Score, e: Entity, pos: Vec2, size: u8, chunk_mult: f32, f: Flavor, chunks: bool) {
    let (dense, gold, pulser, red, cluster, beacon, hunter, lapse) =
        (f.dense, f.gold, f.pulser, f.red, f.cluster, f.beacon, f.hunter, f.lapse);
    let (facet, husk) = (f.facet, f.husk);
    commands.entity(e).despawn();
    let base = match size {
        3 => 20,
        2 => 50,
        _ => 100,
    };
    score.0 += if dense { base * 2 } else { base }; // dense rocks are worth more
    let splash = if gold {
        gold_color() // gold lineage sprays warm-gold debris — was falling through to blue rock_color()
    } else if pulser {
        Color::srgb(4.5, 4.8, 5.6)
    } else if red {
        red_color()
    } else if cluster {
        cluster_color()
    } else if beacon {
        beacon_color()
    } else if husk {
        husk_color()
    } else if facet {
        facet_color()
    } else if lapse {
        lapse_color()
    } else if hunter {
        hunter_color()
    } else if dense {
        dense_color()
    } else {
        rock_color()
    };
    burst(commands, pos, splash, 10 + size as usize * 5, 260.0, rng);
    // KILL POP: a fast type-colored ring so every meaningful kill reads as an impact, on every kill
    // path (bullet/chain/mine/warhead/blast — they all come through here). Size-1 rocks skip it:
    // they die constantly, and a pop per pebble would wash the field in rings.
    if size >= 2 {
        commands.spawn((
            Shockwave { age: 0.0, ttl: 0.16, max_r: asteroid_radius(size) * 1.5, color: splash },
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
    }
    // ── THE HUSK CRACKS ── no chunks: the shell lets out the pair of Hunters it was carrying. They
    // scatter outward at charge 0, so they start docile and you get a moment to deal with them. A
    // husk never contains another husk, so this can't cascade.
    if husk && chunks && size > 1 {
        let base_a = rng.gen_range(0.0..TAU);
        for i in 0..HUSK_BROOD {
            let a = base_a + i as f32 / HUSK_BROOD as f32 * TAU;
            let out = Vec2::from_angle(a);
            let child = spawn_asteroid(commands, pos + out * (asteroid_radius(1) + 6.0), 1, out * HUSK_BROOD_SPEED * chunk_mult, rng, false);
            commands.entity(child).insert((Hunter { charge: 0.0, look: out }, Fresh(FRAGMENT_GRACE)));
        }
        return;
    }
    // A BEACON never splits: cracking it kills the aura clean (children would re-shield the field).
    let chunks = chunks && !beacon;
    // A CLUSTER above the smallest size SHATTERS: a ring of tiny fast shards instead of two chunks.
    // (Shards are size-1 clusters — the tint carries, and smallest rocks just die, so no re-shatter.)
    if cluster && chunks && size > 1 {
        let base_a = rng.gen_range(0.0..TAU);
        for i in 0..CLUSTER_SHARDS {
            let a = base_a + i as f32 / CLUSTER_SHARDS as f32 * TAU + rng.gen_range(-0.18..0.18);
            let out = Vec2::from_angle(a);
            let spd = rng.gen_range(0.8..1.2) * CLUSTER_SHARD_SPEED * chunk_mult;
            let child = spawn_asteroid(commands, pos + out * (asteroid_radius(1) + 4.0), 1, out * spd, rng, false);
            commands.entity(child).insert((Cluster, Fresh(FRAGMENT_GRACE)));
        }
        return;
    }
    if chunks && size > 1 {
        // Split into chunks that fly APART along a random axis — HOW MANY is the split economy's
        // roll (`split_children`): a large sheds 1-2 mediums, a medium sheds 2 smalls or nothing
        // (gold/red lineages keep the guaranteed pair). Each chunk is spawned already clear of its
        // sibling (offset past their combined radii) so the pair never overlaps — an overlapping
        // spawn lets the collision resolver cancel their motion and leaves them oozing apart at
        // the break point instead of shooting off. Headings get a little jitter so it isn't a
        // rigid mirror. Children inherit density (a dense rock breaks into dense chunks).
        let children = split_children(size, gold, red, rng);
        let axis = rng.gen_range(0.0..TAU);
        let out = Vec2::from_angle(axis);
        let offset = asteroid_radius(size - 1) + 3.0;
        for &side in [1.0f32, -1.0].iter().take(children) {
            let spd = rng.gen_range(60.0..150.0) * chunk_mult;
            let vel = Vec2::from_angle(axis + rng.gen_range(-0.35..0.35)) * (side * spd);
            let child = spawn_asteroid(commands, pos + out * (side * offset), size - 1, vel, rng, dense);
            // grace window: a freshly-broken chunk recycles instead of being culled, so its pieces
            // aren't lost before you can shoot them. Gold gets a longer window (a fair chance to catch
            // the whole lineage before a piece can drift off and forfeit the life).
            commands.entity(child).insert(Fresh(if gold { GOLD_GRACE } else { FRAGMENT_GRACE }));
            if gold {
                commands.entity(child).insert(Gold); // the whole lineage stays gold until fully cleared
            }
            if pulser {
                // a pulser breaks into smaller PULSERS (own beat each) — a sustained timing puzzle, not
                // inert green rubble. They stay dense so there's still no blue.
                commands.entity(child).insert(Pulser { offset: rng.gen_range(0.0..TAU) });
            }
            if red {
                commands.entity(child).insert(Red { cool: RED_ABSORB_EVERY }); // a broken red begets reds (whack-a-mole)
            }
            if hunter {
                // the hunt carries to the chunks, but RESET: fresh pieces start docile and must
                // ramp again, so breaking a charged hunter genuinely relieves the pressure
                commands.entity(child).insert(Hunter { charge: 0.0, look: Vec2::X });
            }
            if facet {
                // chunks stay mirrored, each with its own open face — breaking one doesn't hand you
                // a free kill on the pieces
                commands.entity(child).insert(Facet { open: rng.gen_range(0.0..TAU) });
            }
            if lapse {
                // chunks keep phasing, each on its own fresh clock (they scatter out of step)
                commands.entity(child).insert(Lapse { phase: LapsePhase::Solid, t: rng.gen_range(LAPSE_SOLID_MIN..LAPSE_SOLID_MAX) });
            }
        }
    }
}

// A mine blast: break every rock within the blast radius, flinging chunks fast
// (MINE_CHUNK_MULT). Shared by EVERY way a mine goes off — shot, ship contact, or
// drifting into a rock — so the crowd-clear behaviour stays identical. `broken`
// guards against hitting the same rock twice when blasts overlap in one frame.
fn blast_asteroids(
    commands: &mut Commands,
    rng: &mut impl Rng,
    asteroids: &Query<(Entity, &Transform, &mut Asteroid, Option<&Gold>, Option<&Explosive>, Option<&Pulser>, Option<&Red>, Option<&Cluster>, Option<&Beacon>, Option<&Hunter>, Option<&Lapse>, Option<&Facet>, Option<&Husk>), (Without<Mine>, Without<Shielded>)>,
    broken: &mut HashSet<Entity>,
    center: Vec2,
    t: f32,
) {
    // A mine blast breaks rocks for FREE: the points were paid on the MINE (aimed play); rocks that
    // happen to stand in the blast score nothing, so mine chains can't be farmed for points.
    let mut unscored = Score(0);
    let score = &mut unscored;
    // shared &Query → iterates read-only, so we just read size/dense/gold/explosive here
    for (ae, at, a, gold, explosive, pulser, red, cluster, beacon, hunter, lapse, facet, husk) in asteroids {
        if broken.contains(&ae) {
            continue;
        }
        if gold.is_some() {
            continue; // gold 1UP rocks are immune to mines — only the player's shots may break them
        }
        if pulser.is_some_and(|pl| pulser_lit(pl.offset, t)) {
            continue; // a LIT pulser is invulnerable — the blast can't crack it either
        }
        if lapse.is_some_and(|l| !l.tangible()) {
            continue; // absent / materializing: the blast passes through it
        }
        let ap = at.translation.truncate();
        let br = MINE_BLAST_R + asteroid_radius(a.size);
        if center.distance_squared(ap) < br * br {
            broken.insert(ae);
            if explosive.is_some() {
                commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE, friendly: false }); // a mine lights the orange → it chain-detonates (hostile)
            } else {
                // mine flings chunks (ignores hp + the beacon aura — blasts are the counterplay); never gold;
                // a mined red splits into reds; a mined CLUSTER shatters spectacularly (fast shard ring)
                break_asteroid(commands, rng, score, ae, ap, a.size, MINE_CHUNK_MULT, flavor(a.dense, None, pulser, red, cluster, beacon, hunter, lapse, facet, husk), true);
            }
        }
    }
}

// HUNTER rocks (waves 6+): steer at the ship and get hungrier the longer they live. `charge` ramps
// 0→1 over HUNTER_RAMP seconds, scaling both the steering force and the speed cap, so a fresh chunk
// drifts like any rock and a veteran bears down on you. Capped well under the ship's top speed —
// you can always outrun one; the pressure is that it never stops coming. Rocks the boss is holding
// (Shielded) and thrown cannonballs are left alone: they're the boss's, not the field's.
fn hunter_update(
    time: Res<Time>,
    ships: Query<&Transform, (With<Ship>, Without<Asteroid>)>,
    mut q: Query<(&Transform, &mut Velocity, &mut Hunter), (With<Asteroid>, Without<Shielded>, Without<Cannonball>)>,
) {
    let dt = time.delta_secs();
    let Some(ship) = ships.iter().next() else {
        return; // mid-respawn: nothing to hunt, so they just drift
    };
    let target = ship.translation.truncate();
    for (t, mut v, mut h) in &mut q {
        h.charge = (h.charge + dt / HUNTER_RAMP).min(1.0);
        let to_ship = (target - t.translation.truncate()).normalize_or_zero();
        if to_ship != Vec2::ZERO {
            h.look = to_ship; // the eye tracks whatever it's driving at
        }
        v.0 += to_ship * HUNTER_ACCEL * h.charge * dt;
        let cap = HUNTER_MAX_SPEED * (0.35 + 0.65 * h.charge); // docile at spawn, full pace once charged
        if v.0.length() > cap {
            v.0 = v.0.normalize() * cap;
        }
    }
}

// Drive every LAPSE rock's phase clock. Randomized spells so a field of them never falls into lockstep
// (which would make the whole wave blink as one — both ugly and unfair).
fn lapse_update(
    time: Res<Time>,
    arena: Res<Arena>,
    ships: Query<&Transform, (With<Ship>, Without<Lapse>)>,
    mut q: Query<(&mut Lapse, &mut Transform), Without<Ship>>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (mut l, mut tf) in &mut q {
        l.t -= dt;
        if l.t > 0.0 {
            continue;
        }
        let (phase, t) = match l.phase {
            LapsePhase::Solid => (LapsePhase::FadingOut, LAPSE_FADE_OUT),
            LapsePhase::FadingOut => (LapsePhase::Gone, rng.gen_range(LAPSE_GONE_MIN..LAPSE_GONE_MAX)),
            LapsePhase::Gone => {
                // it comes back SOMEWHERE ELSE (user's design): pick a fresh spot on the field rather
                // than reappearing where it left. Kept clear of the ship by LAPSE_REAPPEAR_CLEAR so
                // the slow materialize is always a warning you can act on, never a spawn on your hull.
                let h = arena.half;
                let mut spot = Vec2::new(rng.gen_range(-h.x * 0.9..h.x * 0.9), rng.gen_range(-h.y * 0.9..h.y * 0.9));
                if let Some(sp) = ship {
                    let away = (spot - sp).normalize_or_zero();
                    let away = if away == Vec2::ZERO { Vec2::X } else { away };
                    if spot.distance(sp) < LAPSE_REAPPEAR_CLEAR {
                        spot = sp + away * LAPSE_REAPPEAR_CLEAR;
                    }
                }
                tf.translation.x = spot.x.clamp(-h.x, h.x);
                tf.translation.y = spot.y.clamp(-h.y, h.y);
                (LapsePhase::FadingIn, LAPSE_FADE_IN)
            }
            LapsePhase::FadingIn => (LapsePhase::Solid, rng.gen_range(LAPSE_SOLID_MIN..LAPSE_SOLID_MAX)),
        };
        l.phase = phase;
        l.t = t;
    }
}

// Red (growing) asteroids (Act III): each absorbs the nearest rock within RED_ABSORB_R every
// RED_ABSORB_EVERY, swelling one size (up to large) — OTHER REDS included, so an all-red pack (the
// wave-30 finale's mono-type red group) still consolidates into fewer, bigger threats instead of just
// drifting inert. Broken into smaller reds, they eat the field back up — a whack-a-mole. It stays soft
// (1 hp): the threat is regrowth, not tankiness. Your plain shot splits a red into more reds;
// mass/warhead/chain/mine destroy one outright (no regrow) — the counters.
// Gold, live bombs, boss-held rocks and cannonballs are never eaten.
fn red_growth(
    time: Res<Time>,
    mut commands: Commands,
    mut reds: Query<(Entity, &mut Asteroid, &mut Red, &Transform)>,
    // absorbable rocks — INCLUDING other reds (a red just excludes ITSELF below), so a mono-type red pack
    // still has prey. Gold, live bombs, shielded/boss-held rocks and cannonballs are off the menu.
    others: Query<(Entity, &Transform), (With<Asteroid>, Without<Shielded>, Without<Cannonball>, Without<Gold>, Without<Detonating>)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let mut eaten: HashSet<Entity> = HashSet::new();
    for (self_e, mut a, mut red, rt) in &mut reds {
        if eaten.contains(&self_e) {
            continue; // already swallowed by another red this frame — it can't also feed (prevents a pair annihilating each other)
        }
        red.cool -= dt;
        if red.cool > 0.0 || a.size >= 3 {
            continue; // still digesting, or already at max size
        }
        let rp = rt.translation.truncate();
        let prey = others
            .iter()
            .filter(|(e, _)| *e != self_e && !eaten.contains(e)) // never itself, never an already-eaten rock
            .map(|(e, t)| (e, t.translation.truncate()))
            .filter(|(_, p)| p.distance_squared(rp) < RED_ABSORB_R * RED_ABSORB_R)
            .min_by(|(_, p), (_, q)| p.distance_squared(rp).total_cmp(&q.distance_squared(rp)));
        if let Some((oe, _)) = prey {
            eaten.insert(oe); // guard: one prey per red per frame (despawn is deferred)
            commands.entity(oe).despawn();
            a.size += 1;
            a.verts = asteroid_verts(a.size, &mut rng);
            a.hp = 1; // stays soft — the regrowth is the threat, not HP
            red.cool = RED_ABSORB_EVERY;
            burst(&mut commands, rp, red_color(), 12, 220.0, &mut rng);
        }
        // no prey in reach → stays "hungry" (cool <= 0), absorbs the instant a rock drifts in
    }
}

// Explosive (orange) asteroids: once lit (`Detonating`), each blasts a radius after a brief fuse —
// shattering rocks, popping mines/enemies, killing the ship if it's caught, and lighting OTHER
// oranges in range (a chain reaction that ripples out over the next frames). Gold is spared, like mines.
#[allow(clippy::too_many_arguments)]
fn detonate(
    time: Res<Time>,
    mut commands: Commands,
    dev: Res<Dev>,
    mut score: ResMut<Score>,
    mut stats: ResMut<Stats>,
    mut sfx: EventWriter<SoundFx>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut lit: Query<(Entity, &Transform, &Asteroid, &mut Detonating)>,
    victims: (
        Query<(Entity, &Transform, &Asteroid, Option<&Explosive>, Option<&Gold>), (Without<Detonating>, Without<Shielded>)>,
        Query<(Entity, &Transform), With<Mine>>,
        Query<(Entity, &Transform), With<Enemy>>,
        Query<(Entity, &Transform, &Ship)>,
    ),
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let (rocks, mines, enemies, ships) = (&victims.0, &victims.1, &victims.2, &victims.3);
    for (oe, ot, oa, mut det) in &mut lit {
        det.fuse -= dt;
        if det.fuse > 0.0 {
            continue; // still flashing — blows when the fuse elapses
        }
        let c = ot.translation.truncate();
        // a big, punchy blast + an expanding shockwave ring out to the kill radius. A FRIENDLY Warhead
        // blast is violet (player kit), spares the player, and is LOCAL (small radius, no chain); a hostile
        // bomb is orange, lethal, full-radius, and chains other oranges.
        let blast_r = if det.friendly { WARHEAD_BLAST_R } else { ORANGE_BLAST_R };
        let (blast_col, spray_col) = if det.friendly {
            (warhead_color(), Color::srgb(4.2, 2.0, 6.0))
        } else {
            (orange_color(), Color::srgb(6.0, 4.2, 1.6))
        };
        burst(&mut commands, c, blast_col, 64, 560.0, &mut rng);
        burst(&mut commands, c, spray_col, 20, 300.0, &mut rng);
        commands.spawn((
            Shockwave { age: 0.0, ttl: 0.32, max_r: blast_r, color: blast_col },
            Transform::from_xyz(c.x, c.y, 0.0),
        ));
        sfx.write(SoundFx::Mine); // reuse the explosion thump
        score.0 += match oa.size { 3 => 20, 2 => 50, _ => 100 }; // scores like a normal rock of its size
        commands.entity(oe).despawn();
        // rocks in range: chain other oranges, shatter the rest (gold is spared, same as mines)
        for (ae, at, a, explosive, gold) in rocks {
            if ae == oe || gold.is_some() {
                continue;
            }
            let rr = blast_r + asteroid_radius(a.size);
            if c.distance_squared(at.translation.truncate()) < rr * rr {
                if explosive.is_some() && !det.friendly {
                    commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE, friendly: false }); // hostile orange CHAINS; a friendly Warhead pop does NOT (no screen-clearing cascade)
                } else {
                    // caught in the AOE → DESTROYED outright (obliterated, not split into chunks)
                    let ap = at.translation.truncate();
                    burst(&mut commands, ap, if a.dense { dense_color() } else { rock_color() }, 8, 240.0, &mut rng);
                    let base = match a.size { 3 => 20, 2 => 50, _ => 100 };
                    score.0 += if a.dense { base * 2 } else { base };
                    commands.entity(ae).despawn();
                }
            }
        }
        for (me, mt) in mines {
            let rr = blast_r + MINE_R;
            if c.distance_squared(mt.translation.truncate()) < rr * rr {
                burst(&mut commands, mt.translation.truncate(), mine_color(), 18, 300.0, &mut rng);
                commands.entity(me).despawn();
                score.0 += MINE_SCORE;
                stats.mines += 1; // your blast chain popped it — player-credited
            }
        }
        for (ee, et) in enemies {
            let rr = blast_r + ENEMY_R;
            if c.distance_squared(et.translation.truncate()) < rr * rr {
                burst(&mut commands, et.translation.truncate(), enemy_color(), 18, 300.0, &mut rng);
                commands.entity(ee).despawn();
                score.0 += ENEMY_SCORE;
                stats.enemies += 1;
                sfx.write(SoundFx::EnemyDie);
            }
        }
        // the player is caught too — but NOT by their own friendly Warhead blast, and not mid-respawn
        // or while blinking/invincible
        if !det.friendly && run.respawn <= 0.0 {
            for (se, st, sh) in ships {
                let sp = st.translation.truncate();
                let rr = blast_r + SHIP_R;
                if c.distance_squared(sp) < rr * rr && !immune(sh, &dev) {
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                }
            }
        }
    }
}

// ─────────────────────────────── input layer ──────────────────────────
// Abstract gameplay actions. Physical keys / mouse buttons / gamepad buttons map to these via
// `Bindings`, so input is rebindable and works on keyboard+mouse OR a controller. `gather_input`
// resolves the raw devices into `ActionState` each frame; gameplay systems read that, not the devices.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    TurnLeft,
    TurnRight,
    Thrust,
    Fire,
    Warp,
    Chain,
    ToggleShot,
    Pause,
    Mute,
}
// A physical input bindable to an action.
#[derive(Clone, Copy, PartialEq)]
enum Bind {
    Key(KeyCode),
    Mouse(MouseButton),
    Pad(GamepadButton),
}

// The player's bindings (rebindable). Separate keyboard/mouse and gamepad lists; an action may have
// several binds. Flat vecs so the settings screen can add / replace them easily.
#[derive(Resource, Clone)]
struct Bindings {
    kbm: Vec<(Action, Bind)>,
    pad: Vec<(Action, Bind)>,
}

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        Bindings {
            kbm: vec![
                (TurnLeft, Bind::Key(KeyCode::ArrowLeft)),
                (TurnLeft, Bind::Key(KeyCode::KeyA)),
                (TurnRight, Bind::Key(KeyCode::ArrowRight)),
                (TurnRight, Bind::Key(KeyCode::KeyD)),
                (Thrust, Bind::Key(KeyCode::ArrowUp)),
                (Thrust, Bind::Key(KeyCode::KeyW)),
                (Fire, Bind::Key(KeyCode::Space)),
                (Fire, Bind::Mouse(MouseButton::Left)),
                (Warp, Bind::Key(KeyCode::ShiftLeft)),
                (Warp, Bind::Key(KeyCode::ShiftRight)),
                (Chain, Bind::Mouse(MouseButton::Right)),
                (ToggleShot, Bind::Key(KeyCode::KeyQ)),
                (Pause, Bind::Key(KeyCode::Escape)),
                (Mute, Bind::Key(KeyCode::KeyM)),
            ],
            pad: vec![
                (TurnLeft, Bind::Pad(GamepadButton::DPadLeft)),
                (TurnRight, Bind::Pad(GamepadButton::DPadRight)),
                (Thrust, Bind::Pad(GamepadButton::RightTrigger2)),
                (Fire, Bind::Pad(GamepadButton::South)),
                (Warp, Bind::Pad(GamepadButton::LeftTrigger2)),
                (Chain, Bind::Pad(GamepadButton::RightTrigger)),
                (ToggleShot, Bind::Pad(GamepadButton::West)),
                (Pause, Bind::Pad(GamepadButton::Start)),
            ],
        }
    }
}

// Every rebindable action, in the order the settings screen lists them.
const ACTIONS: [Action; 9] = [
    Action::TurnLeft,
    Action::TurnRight,
    Action::Thrust,
    Action::Fire,
    Action::Warp,
    Action::Chain,
    Action::ToggleShot,
    Action::Pause,
    Action::Mute,
];

fn action_label(a: Action) -> &'static str {
    match a {
        Action::TurnLeft => "Turn left",
        Action::TurnRight => "Turn right",
        Action::Thrust => "Thrust",
        Action::Fire => "Fire",
        Action::Warp => "Warp",
        Action::Chain => "Chain shot",
        Action::ToggleShot => "Cycle shot mode",
        Action::Pause => "Pause",
        Action::Mute => "Mute music",
    }
}

// A short, readable name for a bound input (for the settings rows).
fn bind_label(b: &Bind) -> String {
    match b {
        Bind::Key(k) => format!("{k:?}").trim_start_matches("Key").to_string(),
        Bind::Mouse(MouseButton::Left) => "Mouse L".into(),
        Bind::Mouse(MouseButton::Right) => "Mouse R".into(),
        Bind::Mouse(MouseButton::Middle) => "Mouse M".into(),
        Bind::Mouse(m) => format!("Mouse {m:?}"),
        Bind::Pad(b) => match b {
            GamepadButton::South => "A".into(),
            GamepadButton::East => "B".into(),
            GamepadButton::West => "X".into(),
            GamepadButton::North => "Y".into(),
            GamepadButton::LeftTrigger => "LB".into(),
            GamepadButton::RightTrigger => "RB".into(),
            GamepadButton::LeftTrigger2 => "LT".into(),
            GamepadButton::RightTrigger2 => "RT".into(),
            GamepadButton::Select => "Select".into(),
            GamepadButton::Start => "Start".into(),
            GamepadButton::DPadUp => "D-Up".into(),
            GamepadButton::DPadDown => "D-Down".into(),
            GamepadButton::DPadLeft => "D-Left".into(),
            GamepadButton::DPadRight => "D-Right".into(),
            other => format!("{other:?}"),
        },
    }
}

// Join the binds for one action + device into a display string like "A / D" (or "—" if none).
fn binds_label(binds: &[(Action, Bind)], a: Action) -> String {
    let s: Vec<String> = binds.iter().filter(|(act, _)| *act == a).map(|(_, b)| bind_label(b)).collect();
    if s.is_empty() {
        "—".into()
    } else {
        s.join(" / ")
    }
}

// How the player drives the game. Auto = use a controller if one's connected, else keyboard+mouse.
// Both device types are always read regardless (nothing breaks if you switch mid-run); this mainly
// drives which control prompts the settings/controls screens show.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
enum InputMethod {
    #[default]
    Auto,
    KeyboardMouse,
    Controller,
}

impl InputMethod {
    fn label(self) -> &'static str {
        match self {
            InputMethod::Auto => "Auto",
            InputMethod::KeyboardMouse => "Keyboard + Mouse",
            InputMethod::Controller => "Controller",
        }
    }
    // The device actually in use: under Auto, a controller if one is connected, else keyboard+mouse.
    fn active(self, gamepad_connected: bool) -> InputMethod {
        match self {
            InputMethod::Auto if gamepad_connected => InputMethod::Controller,
            InputMethod::Auto => InputMethod::KeyboardMouse,
            other => other,
        }
    }
}

// Resolved input for the current frame (built by `gather_input`).
#[derive(Resource, Default)]
struct ActionState {
    turn: f32,   // +1 = counter-clockwise (left), -1 = clockwise (right); analog on a stick
    thrust: f32, // 0..1
    fire_held: bool,
    warp: bool,
    chain: bool,
    toggle: bool,
    pause: bool,
    mute: bool,
}

const STICK_DEADZONE: f32 = 0.2; // ignore small left-stick drift before it counts as turning

// Resolve raw device state (keyboard, mouse, any connected gamepad) into ActionState each frame
// (PreUpdate, all states). Digital binds OR together; the left stick adds analog turn on top.
fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    bindings: Res<Bindings>,
    mut state: ResMut<ActionState>,
) {
    let mut turn = 0.0f32;
    let mut thrust = 0.0f32;
    let mut fire_held = false;
    let (mut warp, mut chain, mut toggle, mut pause, mut mute) = (false, false, false, false, false);
    for (act, b) in bindings.kbm.iter().chain(bindings.pad.iter()) {
        let (h, j) = match b {
            Bind::Key(k) => (keys.pressed(*k), keys.just_pressed(*k)),
            Bind::Mouse(m) => (mouse.pressed(*m), mouse.just_pressed(*m)),
            Bind::Pad(btn) => (gamepads.iter().any(|g| g.pressed(*btn)), gamepads.iter().any(|g| g.just_pressed(*btn))),
        };
        match act {
            Action::TurnLeft => {
                if h {
                    turn += 1.0;
                }
            }
            Action::TurnRight => {
                if h {
                    turn -= 1.0;
                }
            }
            Action::Thrust => {
                // keyboard/mouse thrust is full-on; PAD thrust is read as ANALOG below instead,
                // so a trigger half-pull doesn't snap to 1.0 the frame it crosses press-threshold
                if h && !matches!(b, Bind::Pad(_)) {
                    thrust = 1.0;
                }
            }
            Action::Fire => fire_held |= h,
            Action::Warp => warp |= j,
            Action::Chain => chain |= j,
            Action::ToggleShot => toggle |= j,
            Action::Pause => pause |= j,
            Action::Mute => mute |= j,
        }
    }
    // left stick adds analog turn (finer than the d-pad): stick right (+x) = clockwise = negative
    // turn. Rescaled past the deadzone so the response ramps smoothly from zero — the old raw pass
    // jumped straight to 0.2 turn at the threshold, a kink you could feel when easing into a bank.
    for g in &gamepads {
        if let Some(x) = g.get(GamepadAxis::LeftStickX) {
            if x.abs() > STICK_DEADZONE {
                turn -= x.signum() * ((x.abs() - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).min(1.0);
            }
        }
    }
    // ANALOG thrust from whatever the pad's thrust is bound to: a trigger half-pull is a half burn
    // (feathering is skill expression). Digital buttons report 0/1 here so a face-button bind still
    // reads full-on; keyboard already set 1.0 above.
    for (act, b) in bindings.pad.iter() {
        if matches!(act, Action::Thrust) {
            if let Bind::Pad(btn) = b {
                for g in &gamepads {
                    if let Some(v) = g.get(*btn) {
                        if v > 0.05 {
                            thrust = thrust.max(v.min(1.0));
                        }
                    }
                }
            }
        }
    }
    *state = ActionState { turn: turn.clamp(-1.0, 1.0), thrust: thrust.clamp(0.0, 1.0), fire_held, warp, chain, toggle, pause, mute };
}

// ─────────────────────────────── gameplay systems (Playing only) ──────
fn ship_control(
    time: Res<Time>,
    input: Res<ActionState>,
    mut q: Query<(&mut Ship, &mut Velocity)>,
) {
    let dt = time.delta_secs();
    for (mut ship, mut vel) in &mut q {
        ship.angle += input.turn * TURN_RATE * dt;
        let thrusting = input.thrust > 0.05;
        if thrusting {
            vel.0 += Vec2::from_angle(ship.angle) * THRUST * input.thrust * dt;
            ship.flame = (ship.flame + dt * 5.0).min(1.0);
            // (no exhaust particles — the persistent sparks read as broken dashes behind the ship.
            // The engine visual is the fading LIGHT TRAIL, drawn in `render` off ShipTrail.)
        } else {
            ship.flame = (ship.flame - dt * 6.0).max(0.0);
        }
        vel.0 *= FRICTION.powf(dt);
        if vel.0.length() > MAX_SPEED {
            vel.0 = vel.0.normalize() * MAX_SPEED;
        }
        if ship.invuln > 0.0 {
            ship.invuln -= dt;
        }
    }
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    input: Res<ActionState>,
    mut mass: ResMut<MassShot>,
    mut warhead: ResMut<Warhead>,
    mut gorge: ResMut<Gorge>,
    mut armed: ResMut<FireArmed>,
    mut mode: ResMut<ShotModeFlash>,
    arena: Res<Arena>,
    mut sfx: EventWriter<SoundFx>,
    mut run: ResMut<Run>,
    mut q: Query<(&mut Ship, &Transform)>,
) {
    let dt = time.delta_secs();
    // bullet lifetime scales with the arena so its reach is a consistent fraction of the screen,
    // not a fixed distance that looks tiny on a big display (floored at BULLET_LIFE for small windows)
    let bullet_life = (BULLET_RANGE_FRAC * arena.half.x / BULLET_SPEED).max(BULLET_LIFE);
    // Q CYCLES the shot mode through the unlocked options: Standard → Mass → Warhead → Standard.
    if input.toggle && (mass.unlocked || warhead.unlocked || gorge.unlocked) {
        let cur = if gorge.active { 3u8 } else if warhead.active { 2 } else if mass.active { 1 } else { 0 };
        let mut avail = vec![0u8];
        if mass.unlocked { avail.push(1); }
        if warhead.unlocked { avail.push(2); }
        if gorge.unlocked { avail.push(3); }
        let i = avail.iter().position(|&m| m == cur).unwrap_or(0);
        let next = avail[(i + 1) % avail.len()];
        mass.active = next == 1;
        warhead.active = next == 2;
        gorge.active = next == 3;
        mode.0 = SHOT_MODE_SHOW;
        sfx.write(SoundFx::Toggle);
    }
    let is_gorge = gorge.unlocked && gorge.active;
    let is_warhead = !is_gorge && warhead.unlocked && warhead.active;
    let is_mass = !is_gorge && !is_warhead && mass.unlocked && mass.active;
    let want_fire = input.fire_held;
    if !want_fire {
        armed.0 = true; // released → the next press is a genuine fire, not the start/resume click
    }
    for (mut ship, t) in &mut q {
        if ship.cooldown > 0.0 {
            ship.cooldown -= dt;
        }
        if want_fire && armed.0 && ship.cooldown <= 0.0 {
            ship.cooldown = if is_gorge { GORGE_COOLDOWN } else if is_warhead { WARHEAD_COOLDOWN } else if is_mass { MASS_COOLDOWN } else { FIRE_COOLDOWN };
            let aim = Vec2::from_angle(ship.angle);
            let ship_pos = t.translation.truncate();
            let dir = aim; // fire exactly where the ship points — no assist (small targets are handled by size instead)
            let pos = ship_pos + dir * SHIP_R;
            let speed = if is_gorge { GORGE_SPEED } else { BULLET_SPEED };
            let mut b = commands.spawn((
                Bullet { life: bullet_life, trail: Vec::new(), mass: is_mass },
                Velocity(dir * speed),
                Transform::from_xyz(pos.x, pos.y, 0.0),
            ));
            if is_gorge {
                b.insert(GorgeShot { eaten: 0 });
            }
            if is_warhead {
                b.insert(WarheadShot); // piercing destroy-round (see collisions)
            }
            if is_gorge || is_warhead || is_mass {
                run.powerup_fires += 1; // a powerup round left the barrel — the Pacifist streak is over
            }
            sfx.write(SoundFx::Fire);
        }
    }
}

fn integrate(time: Res<Time>, mut q: Query<(&mut Transform, &Velocity)>) {
    let dt = time.delta_secs();
    for (mut t, v) in &mut q {
        t.translation.x += v.0.x * dt;
        t.translation.y += v.0.y * dt;
    }
}

fn bullet_trail(mut q: Query<(&Transform, &mut Bullet)>) {
    for (t, mut b) in &mut q {
        b.trail.push(t.translation.truncate());
        if b.trail.len() > TRAIL_LEN {
            let extra = b.trail.len() - TRAIL_LEN;
            b.trail.drain(0..extra);
        }
    }
}

// Lay down the ship's light trail (same recording pattern as bullet_trail). Records the FLAME ROOT —
// just behind the tail along the facing — so the ribbon streams from the exhaust, not out of the
// hull's middle. Component-driven, so it also feeds the finale's DepartingShip (no Ship → faces +X).
fn ship_trail(mut q: Query<(&Transform, Option<&Ship>, &mut ShipTrail)>) {
    for (t, ship, mut tr) in &mut q {
        let back = Vec2::from_angle(ship.map_or(0.0, |s| s.angle));
        tr.0.push(t.translation.truncate() - back * SHIP_R * 0.55);
        if tr.0.len() > SHIP_TRAIL_LEN {
            let extra = tr.0.len() - SHIP_TRAIL_LEN;
            tr.0.drain(0..extra);
        }
    }
}

// Asteroids bounce off each other (elastic), never interpenetrate.
fn asteroid_collisions(mut q: Query<(&mut Transform, &mut Velocity, &Asteroid), Without<Shielded>>) {
    let mut it = q.iter_combinations_mut::<2>();
    while let Some([(mut ta, mut va, aa), (mut tb, mut vb, ab)]) = it.fetch_next() {
        let (ra, rb) = (asteroid_radius(aa.size), asteroid_radius(ab.size));
        let mut pa = ta.translation.truncate();
        let mut pb = tb.translation.truncate();
        resolve(&mut pa, &mut va.0, body_mass(ra), ra, &mut pb, &mut vb.0, body_mass(rb), rb);
        ta.translation.x = pa.x;
        ta.translation.y = pa.y;
        tb.translation.x = pb.x;
        tb.translation.y = pb.y;
    }
}

// Dev-only invincibility (toggled with F1 — see `dev_toggle`). The resource always
// exists so the death checks can read it cheaply; only the TOGGLE is compiled into
// debug builds, so a release build can never flip it on.
#[derive(Resource, Default)]
struct Dev {
    invincible: bool,
}

// A ship shrugs off lethal hits while blinking after (re)spawn, OR while dev
// invincibility is on. One place so ship_death + mine_update stay in agreement.
fn immune(ship: &Ship, dev: &Dev) -> bool {
    ship.invuln > 0.0 || dev.invincible
}

// Ship DIES on contact with an asteroid (unless invulnerable): burst + despawn,
// then either schedule a respawn or — on the last life — go to Game Over.
fn ship_death(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship)>,
    // Include the boss's rocks, but a rock still being REELED IN (grab in progress) is exempt — it
    // mustn't kill the player as the boss drags it across the field. A SETTLED shield rock (orbiting
    // the boss) still hurts, and free / thrown rocks always do (a thrown rock drops Shielded).
    asteroids: Query<(Entity, &Transform, &Asteroid, Option<&Shielded>, Option<&Lapse>)>,
) {
    if run.respawn > 0.0 {
        return; // already dead/respawning
    }
    for (e, t, ship) in &ships {
        if immune(ship, &dev) {
            continue;
        }
        let sp = t.translation.truncate();
        for (ae, at, a, shielded, lapse) in &asteroids {
            if shielded.is_some_and(|sh| sh.grab < BOSS_GRAB_TIME) {
                continue; // mid-grab: harmless while it reels across the field
            }
            // A LAPSE rock only kills while it's really here. It is harmless the entire time it's
            // absent AND all the way through materializing, so a rock coming back on top of you is
            // always something you were shown and had time to leave — never an unavoidable death.
            if lapse.is_some_and(|l| !l.tangible()) {
                continue;
            }
            let rr = asteroid_radius(a.size) + SHIP_R * 0.6;
            if sp.distance_squared(at.translation.truncate()) < rr * rr {
                let mut rng = rand::thread_rng();
                // AEGIS SHARDS: a live shard GRINDS the rock instead of you dying — it vaporizes
                // (no chunks, no score, no kill credit: this is a save, not a kill you earned) and
                // one shard is spent. When the ring is empty the rock kills you as usual, so the
                // shard count + the slow regrowth are what keep this from being invincibility.
                if run.aegis.unlocked && run.aegis.shards > 0 && shielded.is_none() {
                    run.aegis.shards -= 1;
                    if run.aegis.shards == AEGIS_SHARDS - 1 {
                        run.aegis.regen = AEGIS_REGEN; // first loss starts the clock
                    }
                    let rp = at.translation.truncate();
                    commands.entity(ae).despawn();
                    burst(&mut commands, rp, ship_color(), 12, 260.0, &mut rng);
                    sfx.write(SoundFx::Break(a.size));
                    continue; // that rock is gone — keep checking the rest of the field
                }
                kill_ship(&mut commands, &mut run, &mut next, &mut sfx, e, sp, &mut rng);
                break;
            }
        }
    }
}

fn respawn(mut commands: Commands, time: Res<Time>, mut run: ResMut<Run>, mut next: ResMut<NextState<GameState>>, ships: Query<&Ship>) {
    if run.respawn <= 0.0 {
        return;
    }
    run.respawn -= time.delta_secs();
    if run.respawn <= 0.0 {
        if run.lives <= 0 {
            next.set(GameState::GameOver); // a beat after the final death → then the screen
        } else if ships.is_empty() {
            spawn_player(&mut commands);
        }
    }
}

// Tick the Nova Shield's regen + pop-grace (Playing only, so it doesn't heal while paused). When the
// regen elapses the shield flickers back ON with its own soft cue.
fn aegis_tick(time: Res<Time>, mut run: ResMut<Run>) {
    if !run.aegis.unlocked {
        return;
    }
    let dt = time.delta_secs();
    run.aegis.spin += AEGIS_SPIN * dt;
    if run.aegis.shards < AEGIS_SHARDS {
        run.aegis.regen -= dt;
        if run.aegis.regen <= 0.0 {
            run.aegis.shards += 1; // one back — never the whole ring at once
            run.aegis.regen = AEGIS_REGEN;
        }
    }
}

fn nova_tick(time: Res<Time>, mut run: ResMut<Run>, mut sfx: EventWriter<SoundFx>) {
    if !run.nova.unlocked {
        return;
    }
    let dt = time.delta_secs();
    if run.nova.grace > 0.0 {
        run.nova.grace -= dt;
    }
    if run.nova.down > 0.0 {
        run.nova.down -= dt;
        if run.nova.down <= 0.0 {
            sfx.write(SoundFx::NovaUp); // back online
        }
    }
}

fn particle_update(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Particle)>,
) {
    let dt = time.delta_secs();
    for (e, mut t, mut p) in &mut q {
        t.translation.x += p.vel.x * dt;
        t.translation.y += p.vel.y * dt;
        p.vel *= 0.25_f32.powf(dt);
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

// Age out the brief explosion rings.
fn shockwave_update(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Shockwave)>) {
    let dt = time.delta_secs();
    for (e, mut sw) in &mut q {
        sw.age += dt;
        if sw.age >= sw.ttl {
            commands.entity(e).despawn();
        }
    }
}

// Draw each shockwave as a bright ring expanding (ease-out) to its kill radius, fading as it goes.
// Its own Gizmos system so `render`'s params stay under Bevy's limit.
fn render_shockwaves(mut gizmos: Gizmos, q: Query<(&Shockwave, &Transform)>) {
    for (sw, t) in &q {
        let f = (sw.age / sw.ttl).clamp(0.0, 1.0);
        let r = (sw.max_r * (1.0 - (1.0 - f) * (1.0 - f))).max(1.0); // ease-out toward max_r
        let fade = (1.0 - f) * (1.0 - f); // brightness falls off as it expands
        let c = t.translation.truncate();
        gizmos.circle_2d(Isometry2d::from_translation(c), r, dim(sw.color, 2.4 * fade)); // bright leading edge
        gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.86, dim(sw.color, 1.0 * fade)); // trailing thickness
    }
}

fn spin_asteroids(time: Res<Time>, mut q: Query<&mut Asteroid>) {
    let dt = time.delta_secs();
    for mut a in &mut q {
        a.rot += a.spin * dt;
    }
}

fn ship_bounds(arena: Res<Arena>, mut q: Query<(&mut Transform, &mut Velocity), With<Ship>>) {
    let h = arena.half;
    for (mut t, mut v) in &mut q {
        // no wrap — the edge is the bound; clamp position AND kill the into-wall
        // velocity so the ship can't push past the edge (which caused border ghosting)
        if t.translation.x < -h.x + SHIP_R {
            t.translation.x = -h.x + SHIP_R;
            v.0.x = v.0.x.max(0.0);
        } else if t.translation.x > h.x - SHIP_R {
            t.translation.x = h.x - SHIP_R;
            v.0.x = v.0.x.min(0.0);
        }
        if t.translation.y < -h.y + SHIP_R {
            t.translation.y = -h.y + SHIP_R;
            v.0.y = v.0.y.max(0.0);
        } else if t.translation.y > h.y - SHIP_R {
            t.translation.y = h.y - SHIP_R;
            v.0.y = v.0.y.min(0.0);
        }
    }
}

fn asteroid_bounds(mut commands: Commands, time: Res<Time>, arena: Res<Arena>, mut rush: ResMut<GoldRush>, mut q: Query<(Entity, &mut Transform, &mut Velocity, &Asteroid, Option<&mut Fresh>, Option<&Gold>, Option<&Cannonball>), Without<Shielded>>) {
    let h = arena.half;
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    for (e, mut t, mut v, a, fresh, gold, cannon) in &mut q {
        // tick the post-break grace; while it runs the fragment is protected from culling below
        let mut grace = false;
        if let Some(mut f) = fresh {
            f.0 -= dt;
            if f.0 <= 0.0 {
                commands.entity(e).remove::<Fresh>();
            } else {
                grace = true;
            }
        }
        // never let a rock sit dead-still — elastic hits (or the boss shield) can zero
        // its velocity, which reads as "stuck". Keep a slow drift going.
        let sp = v.0.length();
        if sp < MIN_DRIFT {
            v.0 = if sp > 1.0 {
                v.0 / sp * MIN_DRIFT
            } else {
                Vec2::from_angle(rng.gen_range(0.0..TAU)) * MIN_DRIFT
            };
        }
        let r = asteroid_radius(a.size);
        let p = t.translation.truncate();
        if !(p.x < -h.x - r || p.x > h.x + r || p.y < -h.y - r || p.y > h.y + r) {
            continue;
        }
        // a launched cannonball that misses just leaves for good — it must NOT recycle as a
        // normal large rock (that would litter the arena with the Slinger's spent shots).
        if cannon.is_some() {
            commands.entity(e).despawn();
            continue;
        }
        // A rock that's fully drifted off-screen either leaves for good or recycles back in.
        // Small debris usually leaves — otherwise broken-up rocks pile into an overwhelming
        // cloud of little ones that never clears. The population top-up then streams in fresh
        // LARGE rocks to replace them. Large rocks always recycle, keeping a healthy backbone
        // of big targets (and food for the bosses). A fragment still in its grace window always
        // recycles, so a rock shattered at the edge can't lose its pieces before you engage them.
        let leaves = !grace
            && match a.size {
                1 => rng.gen_bool(0.85), // small: usually gone for good
                2 => rng.gen_bool(0.35), // mid: now and then
                _ => false,              // large: always kept in play
            };
        if leaves {
            if gold.is_some() {
                rush.forfeited = true; // a gold piece drifted off (past its long grace) — the 1UP is forfeit
            }
            commands.entity(e).despawn();
            continue;
        }
        let inward = rng.gen_range(50.0..130.0);
        let jitter = rng.gen_range(-40.0..40.0);
        match rng.gen_range(0..4) {
            0 => {
                t.translation = Vec3::new(-h.x - r, rng.gen_range(-h.y..h.y), 0.0);
                v.0 = Vec2::new(inward, jitter);
            }
            1 => {
                t.translation = Vec3::new(h.x + r, rng.gen_range(-h.y..h.y), 0.0);
                v.0 = Vec2::new(-inward, jitter);
            }
            2 => {
                t.translation = Vec3::new(rng.gen_range(-h.x..h.x), -h.y - r, 0.0);
                v.0 = Vec2::new(jitter, inward);
            }
            _ => {
                t.translation = Vec3::new(rng.gen_range(-h.x..h.x), h.y + r, 0.0);
                v.0 = Vec2::new(jitter, -inward);
            }
        }
    }
}

fn bullet_bounds(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut q: Query<(Entity, &Transform, &mut Bullet)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    for (e, t, mut b) in &mut q {
        b.life -= dt;
        let p = t.translation.truncate();
        if b.life <= 0.0 || p.x.abs() > h.x || p.y.abs() > h.y {
            commands.entity(e).despawn();
        }
    }
}

fn collisions(
    mut commands: Commands,
    time: Res<Time>,
    mut bullets: Query<(Entity, &Transform, &Bullet, &Velocity, Has<WarheadShot>, Option<&mut GorgeShot>)>,
    mut asteroids: Query<(Entity, &Transform, &mut Asteroid, Option<&Gold>, Option<&Explosive>, Option<&Pulser>, Option<&Red>, Option<&Cluster>, Option<&Beacon>, Option<&Hunter>, Option<&Lapse>, Option<&Facet>, Option<&Husk>), (Without<Mine>, Without<Shielded>)>,
    mines: Query<(Entity, &Transform), With<Mine>>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    tenders: Query<(Entity, &Transform), With<Tender>>,
    mut shield_rocks: Query<(Entity, &Transform, &mut Asteroid), With<Shielded>>,
    // the five boss cores bundled into one param — five separate queries would exceed Bevy's 16-param system limit
    boss_cores: (
        Query<(&Transform, &mut Boss)>,
        Query<(&Transform, &mut Devourer)>,
        Query<(&Transform, &mut Slinger)>,
        Query<(&Transform, &mut Detonator)>,
        Query<(&Transform, &mut Pulsar)>,
        Query<(&Transform, &mut Phantom)>,
        Query<(Entity, &Transform, &mut Possessed)>, // p2 vessels: gunfire chips their hp; break one → the Haunt is ripped out
    ),
    mut score: ResMut<Score>,
    mut sfx: EventWriter<SoundFx>,
    mut stats: ResMut<Stats>,
) {
    let mut rng = rand::thread_rng();
    let (mut bosses, mut devourers, mut slingers, mut detonators, mut pulsars, mut phantoms, mut vessels) = boss_cores;
    let mut dead_b: HashSet<Entity> = HashSet::new();
    let mut dead_a: HashSet<Entity> = HashSet::new();
    let mut dead_m: HashSet<Entity> = HashSet::new();
    let mut dead_e: HashSet<Entity> = HashSet::new();
    let mut dead_t: HashSet<Entity> = HashSet::new(); // tenders shot this pass
    let mut dead_s: HashSet<Entity> = HashSet::new();
    let mut dead_v: HashSet<Entity> = HashSet::new(); // p2 vessels broken this pass
    // BEACON auras: any non-beacon rock inside a beacon's aura is immune to gunfire until the beacon
    // falls. Zones are collected in a read-only pass so the mutable bullet loop below can test them.
    let beacon_zones: Vec<Vec2> = asteroids
        .iter()
        .filter(|(.., beacon, _, _, _, _)| beacon.is_some())
        .map(|(_, at, ..)| at.translation.truncate())
        .collect();
    let beacon_shielded =
        |p: Vec2, is_beacon: bool| !is_beacon && beacon_zones.iter().any(|z| z.distance_squared(p) < BEACON_AURA_R * BEACON_AURA_R);
    for (be, bt, b, bvel, is_warhead, mut gorge_shot) in &mut bullets {
        if dead_b.contains(&be) {
            continue;
        }
        let bp = bt.translation.truncate();
        // A GORGE round's hitbox is its DRAWN size — the maw is up to GORGE_R_MAX across, and a mouth
        // that visibly swallowed a rock without eating it would be a lie. Sampled once per frame, so
        // it can't snowball its own reach inside a single tick.
        let br = gorge_shot.as_deref().map(|g| g.radius()).unwrap_or_else(|| bullet_radius(b.mass)); // mass shots are fatter…
        let power = bullet_boss_power(b.mass); // …and (vs a boss/mob) hit a bit harder; vs free rocks, see below
        let mut warhead_blast_at: Option<Vec2> = None; // set when a warhead round detonates on a rock
        for (ae, at, mut a, gold, explosive, pulser, red, cluster, beacon, hunter, lapse, facet, husk) in &mut asteroids {
            if dead_a.contains(&ae) {
                continue;
            }
            let ap = at.translation.truncate();
            let rr = asteroid_radius(a.size) + br;
            if bp.distance_squared(ap) < rr * rr {
                // ── THE MIRROR ── a FACET reflects any round that lands on a closed face, and the
                // reflection is LIVE: it can come back and kill you. Only the one open face (which
                // sweeps with the rock's spin) takes damage. Blasts/beam/warp are unaffected — they
                // aren't rounds, and they're the answer when you can't get the angle.
                if let Some(fc) = facet {
                    let hit_ang = (bp - ap).to_angle();
                    let open_ang = a.rot + fc.open;
                    let off = (hit_ang - open_ang + TAU * 0.5).rem_euclid(TAU) - TAU * 0.5; // signed angle to the gap
                    if off.abs() > FACET_OPEN_ARC * 0.5 {
                        // closed face: bounce the round off the surface normal and hand it back
                        let n = (bp - ap).normalize_or_zero();
                        let v = bvel.0;
                        let refl = (v - n * 2.0 * v.dot(n)) * FACET_RICOCHET_SPEED;
                        commands
                            .entity(be)
                            .insert((Velocity(refl), Ricochet(FACET_RICOCHET_LIFE)))
                            .remove::<WarheadShot>(); // a bounced warhead is just a stray round
                        // shove it clear of the surface so it can't re-collide on the next frame
                        commands.entity(be).insert(Transform::from_xyz(
                            ap.x + n.x * (asteroid_radius(a.size) + br + 2.0),
                            ap.y + n.y * (asteroid_radius(a.size) + br + 2.0),
                            0.0,
                        ));
                        burst(&mut commands, bp, facet_color(), 4, 150.0, &mut rng);
                        break; // this round is spent on the mirror — it belongs to the field now
                    }
                }
                // an ABSENT / still-materializing lapse rock isn't physically here: rounds pass
                // straight through it, and it can't be damaged until it's solid again
                if lapse.is_some_and(|l| !l.tangible()) {
                    continue;
                }
                // a LIT pulser is invulnerable — every shot fizzles on its shield (a WARHEAD round
                // doesn't detonate on an invulnerable shield either; it flies on past)
                if pulser.is_some_and(|pl| pulser_lit(pl.offset, time.elapsed_secs())) {
                    burst(&mut commands, bp, Color::srgb(6.0, 6.0, 7.0), 4, 130.0, &mut rng); // white spark
                    if is_warhead {
                        continue; // no detonation on a shield — the round carries on
                    }
                    dead_b.insert(be);
                    commands.entity(be).despawn();
                    break;
                }
                // inside a beacon's aura → EVERY round fizzles, warhead included. Kill the beacon —
                // or answer with a blast or the warp, which bypass the aura.
                if beacon_shielded(ap, beacon.is_some()) {
                    burst(&mut commands, bp, dim(beacon_color(), 0.9), 4, 130.0, &mut rng); // teal fizzle
                    dead_b.insert(be);
                    commands.entity(be).despawn();
                    break;
                }
                // ── THE GORGE ROUND ── it EATS the rock and swells, then carries on. Bounded: it
                // dies once it has taken GORGE_BITES, so it can never sweep an entire field. It sits
                // AFTER the mirror/lapse/pulser/beacon guards on purpose — it's still a ROUND, so
                // every defence that answers a round answers this too. Gold is claimed whole (no
                // chunks → the lineage clears → the 1UP lands), same as a warhead impact or the warp:
                // aimed shots may take the gold the fast way; only blasts and mines may not.
                if let Some(g) = gorge_shot.as_deref_mut() {
                    dead_a.insert(ae);
                    break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, a.size, 1.0, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk), false);
                    sfx.write(SoundFx::Break(a.size));
                    credit_rock_kill(&mut stats, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk));
                    g.eaten += 1;
                    burst(&mut commands, ap, devourer_color(), 8, 200.0, &mut rng);
                    if g.eaten >= GORGE_BITES {
                        // full: it breaks up in a last spray rather than sailing on forever
                        dead_b.insert(be);
                        commands.entity(be).despawn();
                        burst(&mut commands, ap, devourer_color(), 22, 320.0, &mut rng);
                        break;
                    }
                    continue; // still hungry — keep flying
                }
                if is_warhead {
                    // WARHEAD: detonate ON IMPACT — the round is spent here (it's a warhead, not a
                    // drill; the old infinite pierce read as the shot "just keeping going"). The rock
                    // it struck dies outright, and the violet ring is now a REAL blast: everything
                    // within WARHEAD_BLAST_R is destroyed in the after-pass below.
                    dead_a.insert(ae);
                    break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, a.size, 1.0, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk), false);
                    commands.spawn((
                        Shockwave { age: 0.0, ttl: 0.28, max_r: WARHEAD_BLAST_R, color: warhead_color() },
                        Transform::from_xyz(ap.x, ap.y, 0.0),
                    ));
                    sfx.write(SoundFx::Break(a.size));
                    if explosive.is_some() {
                        stats.orange += 1; // the round deletes an orange whole — still a player-credited kill
                    } else {
                        credit_rock_kill(&mut stats, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk));
                    }
                    warhead_blast_at = Some(ap); // AoE applied after this loop (can't nest a second &mut pass)
                    dead_b.insert(be);
                    commands.entity(be).despawn();
                    break;
                }
                // MASS / STANDARD: consume the round, chip hp (mass hits harder), split on the killing hit
                dead_b.insert(be);
                commands.entity(be).despawn();
                a.hp -= if b.mass { MASS_POWER } else { 1 };
                if a.hp > 0 {
                    // survived: white sparks off a pulser, green off a dense rock
                    let spark = if pulser.is_some() { Color::srgb(4.5, 4.8, 5.6) } else { dense_color() };
                    burst(&mut commands, ap, spark, 6, 160.0, &mut rng);
                } else {
                    dead_a.insert(ae);
                    if explosive.is_some() {
                        commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE, friendly: false }); // orange detonates + chains
                        stats.orange += 1; // you lit it — the kill is yours
                    } else {
                        break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, a.size, 1.0, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk), true);
                        sfx.write(SoundFx::Break(a.size));
                        credit_rock_kill(&mut stats, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk));
                    }
                }
                break;
            }
        }
        // ── the warhead's AoE: the ring means it now — every rock inside the blast dies, credited.
        // Blast rules apply, same as a mine's: gold is spared (only aimed shots may break the 1UP),
        // a LIT pulser shrugs it off, and the beacon aura does NOT protect (blasts are its counter).
        if let Some(c) = warhead_blast_at {
            for (ae, at, a, gold, explosive, pulser, red, cluster, beacon, hunter, lapse, facet, husk) in &mut asteroids {
                if dead_a.contains(&ae) || gold.is_some() {
                    continue;
                }
                if pulser.is_some_and(|pl| pulser_lit(pl.offset, time.elapsed_secs())) {
                    continue;
                }
                let ap = at.translation.truncate();
                let rr = WARHEAD_BLAST_R + asteroid_radius(a.size);
                if c.distance_squared(ap) < rr * rr {
                    dead_a.insert(ae);
                    break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, a.size, 1.0, flavor(a.dense, None, pulser, red, cluster, beacon, hunter, lapse, facet, husk), false);
                    if explosive.is_some() {
                        stats.orange += 1; // the blast deletes an orange whole — yours
                    } else {
                        credit_rock_kill(&mut stats, flavor(a.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk));
                    }
                }
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on an asteroid
        }
        for (me, mt) in &mines {
            if dead_m.contains(&me) {
                continue;
            }
            let rr = MINE_R + br;
            if bp.distance_squared(mt.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                dead_m.insert(me);
                commands.entity(be).despawn();
                commands.entity(me).despawn();
                score.0 += MINE_SCORE;
                stats.mines += 1; // shot it down
                let mp = mt.translation.truncate();
                burst(&mut commands, mp, mine_color(), 24, 320.0, &mut rng);
                // shooting a mine detonates it: the blast shatters rocks in range
                // with fast chunks, same as any other detonation.
                blast_asteroids(&mut commands, &mut rng, &asteroids, &mut dead_a, mp, time.elapsed_secs());
                sfx.write(SoundFx::Mine);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on a mine
        }
        // TENDERS: one hit ends the salvage crew, and any fusion it was holding dies with it.
        // Worth more than a raider — killing one protects work you've already done.
        for (tde, tdt) in &tenders {
            if dead_t.contains(&tde) {
                continue;
            }
            let tp = tdt.translation.truncate();
            let rr = TENDER_R + br;
            if bp.distance_squared(tp) < rr * rr {
                dead_b.insert(be);
                dead_t.insert(tde);
                commands.entity(be).despawn();
                commands.entity(tde).despawn();
                score.0 += TENDER_SCORE;
                stats.tenders += 1; // its own tally — a Tender isn't a raider, so it doesn't muddy that count
                burst(&mut commands, tp, enemy_color(), 22, 300.0, &mut rng);
                sfx.write(SoundFx::EnemyDie);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // round spent on a tender
        }
        for (ene, ent) in &enemies {
            if dead_e.contains(&ene) {
                continue;
            }
            let ep = ent.translation.truncate();
            let rr = ENEMY_R + br;
            if bp.distance_squared(ep) < rr * rr {
                dead_b.insert(be);
                dead_e.insert(ene);
                commands.entity(be).despawn();
                kill_enemy(&mut commands, &mut score, &mut sfx, ene, ep, &mut rng); // dies in one shot
                stats.enemies += 1;
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on an enemy
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent
        }
        // the boss's held shield rocks intercept shots — a hit shrinks the rock one
        // size IN PLACE (it stays on the arm); the smallest one shatters + frees the arm.
        for (se, st, mut sa) in &mut shield_rocks {
            if dead_s.contains(&se) {
                continue;
            }
            let sp = st.translation.truncate();
            let rr = asteroid_radius(sa.size) + br;
            if bp.distance_squared(sp) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                burst(&mut commands, sp, rock_color(), 8, 200.0, &mut rng);
                if sa.size > 1 {
                    sa.size -= 1;
                    sa.verts = asteroid_verts(sa.size, &mut rng); // shrink, still held on the arm
                } else {
                    dead_s.insert(se);
                    commands.entity(se).despawn(); // smallest shatters, freeing the arm
                    score.0 += 20;
                }
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on a shield rock
        }
        // the boss core takes a hit — bullets that slip through a gap in the spinning shield.
        for (bpos, mut boss) in &mut bosses {
            if boss.charge > 0.0 || boss.dying > 0.0 {
                continue; // invulnerable while charging up / already dying
            }
            let rr = BOSS_R + br;
            if bp.distance_squared(bpos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                boss.hp -= power;
                burst(&mut commands, bp, boss_color(), 6, 180.0, &mut rng);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue;
        }
        // the devourer (boss 2) takes gunfire directly — no shield; chip its HP while you starve it
        for (dpos, mut dv) in &mut devourers {
            if dv.dying > 0.0 {
                continue;
            }
            let rr = devourer_radius(dv.grow) + br;
            if bp.distance_squared(dpos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                dv.hp -= power;
                dv.grow = (dv.grow - DEVOURER_SHRINK_PER_HIT).max(0.0); // gunfire shrinks it too, not just its HP
                burst(&mut commands, bp, devourer_color(), 6, 180.0, &mut rng);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue;
        }
        // the Slinger (boss 3) takes gunfire directly — no shield; chip its core while dodging its shots
        for (spos, mut sl) in &mut slingers {
            if sl.charge > 0.0 || sl.dying > 0.0 {
                continue; // invulnerable while entering / already dying
            }
            let rr = SLINGER_R + br;
            if bp.distance_squared(spos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                sl.hp -= power;
                burst(&mut commands, bp, slinger_color(), 6, 180.0, &mut rng);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue;
        }
        // the Detonator (boss 4) is ARMORED — gunfire only lands while it's PRIMING a rock (core exposed).
        // Otherwise the shot clanks off its shell for no damage.
        for (dpos, mut det) in &mut detonators {
            if det.dying > 0.0 {
                continue;
            }
            let rr = DETONATOR_R + br;
            if bp.distance_squared(dpos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                if det.prime > 0.0 && det.charge <= 0.0 {
                    det.hp -= power; // exposed during the priming channel — the only damage window
                    burst(&mut commands, bp, detonator_color(), 6, 180.0, &mut rng);
                } else {
                    burst(&mut commands, bp, Color::srgb(5.0, 5.6, 2.0), 4, 130.0, &mut rng); // clank — armored
                }
                break;
            }
        }
        if dead_b.contains(&be) {
            continue;
        }
        // the Pulsar (boss 5) is invulnerable while LIT — gunfire only lands during its DARK beat.
        for (ppos, mut pl) in &mut pulsars {
            if pl.charge > 0.0 || pl.dying > 0.0 {
                continue;
            }
            let rr = PULSAR_R + br;
            if bp.distance_squared(ppos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                if pulser_lit(pl.phase, time.elapsed_secs()) {
                    burst(&mut commands, bp, Color::srgb(6.0, 6.2, 7.0), 4, 130.0, &mut rng); // clank — lit shield up
                } else {
                    pl.hp -= power;
                    burst(&mut commands, bp, pulsar_color(), 6, 180.0, &mut rng);
                }
                break;
            }
        }
        if dead_b.contains(&be) {
            continue;
        }
        // the Phantom (boss 6, finale) is a GHOST — shots pass straight through — except while SURFACED
        // (`vuln > 0`, the recovery right after its ray) or in PHASE 3 (the mask is off: solid full-time).
        for (spos, mut sg) in &mut phantoms {
            let ghost = sg.vuln <= 0.0 && sg.phase < 3;
            if ghost || sg.charge > 0.0 || sg.transition > 0.0 || sg.victory > 0.0 {
                continue; // intangible — the round sails through the apparition (or it's already beaten, mid-throes)
            }
            let rr = PHANTOM_R + br;
            if bp.distance_squared(spos.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                sg.hp -= power;
                burst(&mut commands, bp, phantom_color(), 6, 180.0, &mut rng);
                break;
            }
        }
        // ── phase-2 VESSELS: a possessed rock takes gunfire; breaking it (hp ≤ 0) rips the Haunt out ──
        for (pve, pvt, mut pv) in &mut vessels {
            if dead_v.contains(&pve) {
                continue;
            }
            let rr = PHANTOM_POSSESS_R + br;
            if bp.distance_squared(pvt.translation.truncate()) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn();
                pv.hp -= power; // mass shots (higher `power`) crack it faster
                burst(&mut commands, bp, phantom_color(), 6, 180.0, &mut rng);
                if pv.hp <= 0 {
                    dead_v.insert(pve); // broken — `possessed_update` shatters + despawns it, and the Haunt surfaces
                }
                break;
            }
        }
    }
}

// Non-boss waves: survive the timer to advance; each new wave streams in more rocks (up to the cap).
// Boss waves end on the kill instead (handled by each boss's update).
fn wave_timer(
    time: Res<Time>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut commands: Commands,
    arena: Res<Arena>,
    plus: Res<NewGamePlus>,
    asteroids: Query<(), With<Asteroid>>,
    mut stats: Option<ResMut<Stats>>, // optional so headless tests needn't insert it
    run: Option<Res<Run>>,
    mut watch: Option<ResMut<PacifistWatch>>,
) {
    if wave.calm > 0.0 {
        wave.calm -= time.delta_secs(); // during the post-boss calm the timer is paused
        return;
    }
    if is_boss_wave(wave.level) {
        return; // a boss wave ends when the boss dies, not when the timer runs out
    }
    wave.timer -= time.delta_secs();
    if wave.timer > 0.0 {
        return;
    }
    wave.level += 1;
    wave.timer = WAVE_SECS;
    banner.timer = WAVE_BANNER_SECS; // flash the new wave number
    if let Some(s) = stats.as_mut() {
        s.waves += 1; // lifetime wave tally (saved on run end / progress saves)
        // PACIFIST: the wave that just ended is clean if nothing was broken, no powerup was fired,
        // and no boss advance slipped into the window since it was primed. Dying does NOT break the
        // streak — restraint is the test, not survival. Two clean in a row = the unlock.
        if let (Some(w), Some(r)) = (watch.as_deref_mut(), run.as_deref()) {
            let clean = total_breaks(s) == w.breaks && r.powerup_fires == w.fires && wave.level - 1 == w.primed_at_level;
            w.streak = if clean { w.streak + 1 } else { 0 };
            if w.streak >= 2 {
                s.pacifist = true; // persisted by the `achievements` system's unlock save
            }
            w.breaks = total_breaks(s);
            w.fires = r.powerup_fires;
            w.primed_at_level = wave.level;
        }
    }
    let target = population_target(wave.level, plus.0);
    let have = asteroids.iter().count() as i32;
    let mut rng = rand::thread_rng();
    for _ in 0..(target - have).max(0) {
        let kind = roll_rock_kind(wave.level, plus.0, &mut rng);
        spawn_edge_asteroid(&mut commands, arena.half, &mut rng, kind, false);
    }
}

// The rare gold 1UP rock drifts in at a randomized time DURING play (not tied to wave starts). Only
// one hunt runs at a time, and a cooldown after each hunt keeps them from spawning back-to-back. It
// may appear on any wave (boss waves included — the Devourer won't eat it and a rock the Warden grabs
// is just a shoot-it-off-the-shield target).
fn gold_spawn(time: Res<Time>, wave: Res<Wave>, arena: Res<Arena>, mut rush: ResMut<GoldRush>, mut commands: Commands) {
    rush.cooldown -= time.delta_secs(); // counts down from the last APPEARANCE (keeps ticking during a hunt)
    // No gold in wave 1 — a spare life that early is wasted (nothing threatening yet), so it felt
    // pointless. The first life rock now arrives in wave 2.
    if rush.active || rush.cooldown > 0.0 || wave.calm > 0.0 || wave.level < 2 {
        return; // a hunt's running, the gap hasn't elapsed, the post-boss field is kept clear, or it's wave 1
    }
    let mut rng = rand::thread_rng();
    spawn_gold_rock(&mut commands, arena.half, &mut rng);
    rush.active = true;
    rush.forfeited = false;
    // Wave-tapered gap: frequent early (life matters most then), easing back to the old rare cadence by wave 30.
    let taper = ((wave.level - GOLD_TAPER_START).max(0) as f32 / (GOLD_TAPER_END - GOLD_TAPER_START) as f32).clamp(0.0, 1.0);
    let gmin = GOLD_GAP_EARLY_MIN + (GOLD_GAP_LATE_MIN - GOLD_GAP_EARLY_MIN) * taper;
    let gmax = GOLD_GAP_EARLY_MAX + (GOLD_GAP_LATE_MAX - GOLD_GAP_EARLY_MAX) * taper;
    rush.cooldown = rng.gen_range(gmin..gmax);
}

// Stream replacement rocks in gradually so the field stays populated as you clear
// it — but NOT during the post-boss calm (kept clear for the reward).
fn top_up_asteroids(
    time: Res<Time>,
    mut clock: ResMut<SpawnClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    plus: Res<NewGamePlus>,
    mut commands: Commands,
    asteroids: Query<&Asteroid, Without<Gold>>, // EXCLUDE the gold 1UP — a lingering gold must not eat
) {                                             // into the finale's field cap (or stall the trickle)
    if wave.calm > 0.0 {
        return;
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let count = asteroids.iter().count() as i32; // non-gold rocks on the field

    // ── FINALE (wave 30): fully RANDOM across every rock type the Belt has shown (see
    //    `roll_finale_kind`), TRICKLED in one at a time at the same gentle rate as before, against a
    //    modest field cap — variety without ever becoming a wall of rocks. ──
    if content_wave(wave.level) == 30 {
        let cap = FINALE_FIELD_CAP + if plus.0 { NGP_POP_BONUS / 2 } else { 0 }; // NG+ finale: denser, still a trickle
        if count < cap {
            let mut rng = rand::thread_rng();
            // NG+ honours its roster rule even here: past wave 5 lap two runs the new bestiary, so
            // its finale is NOT the all-types mix. (Without this the wave-30 field was inconsistent
            // with itself — `wave_timer`'s opening fill goes through `roll_rock_kind`, which already
            // respects the rule, while this trickle didn't.)
            let kind = if plus.0 { roll_ngplus_kind(wave.level, &mut rng) } else { roll_finale_kind(&mut rng) };
            spawn_edge_asteroid(&mut commands, arena.half, &mut rng, kind, false);
            clock.0 = FINALE_TRICKLE;
        } else {
            clock.0 = 0.5; // field's at its cap — re-check shortly
        }
        return;
    }

    let bigs = asteroids.iter().filter(|a| a.size == 3).count() as i32;
    // refill toward the count target, AND separately keep big rocks above the floor even at the
    // cap — otherwise breaking large rocks leaves the field as nothing but small debris.
    if count < population_target(wave.level, plus.0) || bigs < BIG_FLOOR {
        let mut rng = rand::thread_rng();
        let kind = roll_rock_kind(wave.level, plus.0, &mut rng);
        spawn_edge_asteroid(&mut commands, arena.half, &mut rng, kind, bigs < BIG_FLOOR);
        clock.0 = SPAWN_INTERVAL;
    } else {
        clock.0 = 0.5; // at target — recheck shortly
    }
}

// The post-boss calm is a clean breather (and the pickup window): keep the field empty by
// despawning any leftover asteroids/mines — including the boss's scattered shield — for its whole
// duration. New spawns are already gated (top-ups bail while `calm > 0`).
fn clear_calm_field(wave: Res<Wave>, mut commands: Commands, junk: Query<(Entity, &Transform), (Or<(With<Asteroid>, With<Mine>)>, Without<Gold>)>) {
    if wave.calm > 0.0 {
        let mut rng = rand::thread_rng();
        for (e, t) in &junk {
            // dissolve with a soft puff instead of a silent vanish (the arena "resets" for the reward)
            burst(&mut commands, t.translation.truncate(), Color::srgb(2.2, 2.8, 4.2), 9, 175.0, &mut rng);
            commands.entity(e).despawn();
        }
    }
}

// A mine entering from a random edge.
fn spawn_edge_mine(commands: &mut Commands, half: Vec2, rng: &mut impl Rng) {
    let inward = rng.gen_range(0.6..1.0) * MINE_SPEED;
    let jitter = rng.gen_range(-0.4..0.4) * MINE_SPEED;
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - MINE_R, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + MINE_R, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - MINE_R), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + MINE_R), Vec2::new(jitter, -inward)),
    };
    commands.spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(vel), Transform::from_xyz(pos.x, pos.y, 0.0)));
}

// Stream mines in (wave 2+), capped as a fraction of the asteroids; not during calm.
fn top_up_mines(
    time: Res<Time>,
    mut clock: ResMut<MineClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    mines: Query<(), With<Mine>>,
    asteroids: Query<(), With<Asteroid>>,
) {
    if wave.calm > 0.0 || is_boss_wave(wave.level) {
        return; // no new mines during the calm or a boss wave
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let target = mine_target(wave.level, asteroids.iter().count() as i32);
    if (mines.iter().count() as i32) < target {
        let mut rng = rand::thread_rng();
        spawn_edge_mine(&mut commands, arena.half, &mut rng);
        clock.0 = MINE_SPAWN_INTERVAL;
    } else {
        clock.0 = 1.0;
    }
}

// Mines drift + recycle at the edges. Three ways one goes off, each blasting the
// rocks in range: it drifts into an asteroid (no life lost), the ship contacts it,
// or it's armed (ship was near) and the fuse elapses with the ship inside the blast.
fn mine_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    dev: Res<Dev>,
    wave: Res<Wave>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<(Entity, &Transform, &Ship), Without<Mine>>,
    mut mines: Query<(Entity, &mut Transform, &mut Velocity, &mut Mine)>,
    // &mut to match blast_asteroids' type; only read here (iter + shared borrow)
    asteroids: Query<(Entity, &Transform, &mut Asteroid, Option<&Gold>, Option<&Explosive>, Option<&Pulser>, Option<&Red>, Option<&Cluster>, Option<&Beacon>, Option<&Hunter>, Option<&Lapse>, Option<&Facet>, Option<&Husk>), (Without<Mine>, Without<Shielded>)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    let mut broken: HashSet<Entity> = HashSet::new();
    for (me, mut mt, mut mv, mut mine) in &mut mines {
        // recycle at the edges (reposition heading inward)
        let mut p = mt.translation.truncate();
        if p.x < -h.x - MINE_R || p.x > h.x + MINE_R || p.y < -h.y - MINE_R || p.y > h.y + MINE_R {
            if is_boss_wave(wave.level) {
                commands.entity(me).despawn(); // boss wave: mines drift off for good, no recycle
                continue;
            }
            let inward = rng.gen_range(0.6..1.0) * MINE_SPEED;
            let jitter = rng.gen_range(-0.4..0.4) * MINE_SPEED;
            match rng.gen_range(0..4) {
                0 => { mt.translation = Vec3::new(-h.x - MINE_R, rng.gen_range(-h.y..h.y), 0.0); mv.0 = Vec2::new(inward, jitter); }
                1 => { mt.translation = Vec3::new(h.x + MINE_R, rng.gen_range(-h.y..h.y), 0.0); mv.0 = Vec2::new(-inward, jitter); }
                2 => { mt.translation = Vec3::new(rng.gen_range(-h.x..h.x), -h.y - MINE_R, 0.0); mv.0 = Vec2::new(jitter, inward); }
                _ => { mt.translation = Vec3::new(rng.gen_range(-h.x..h.x), h.y + MINE_R, 0.0); mv.0 = Vec2::new(jitter, -inward); }
            }
            continue;
        }

        // Gold 1UP rocks are immune to mines: a drifting mine bounces off them instead of
        // detonating, so a mine can never clear the gold lineage for you (only your shots may).
        for (_, at, a, gold, ..) in &asteroids {
            if gold.is_none() {
                continue;
            }
            let gp = at.translation.truncate();
            let rr = MINE_R + asteroid_radius(a.size);
            let d = p.distance(gp);
            if d < rr && d > 0.01 {
                let n = (p - gp) / d;
                let vn = mv.0.dot(n);
                if vn < 0.0 {
                    mv.0 -= 2.0 * vn * n; // elastic reflection off the gold rock
                }
                let np = gp + n * rr; // nudge clear so the mine doesn't stick inside the rock
                mt.translation = Vec3::new(np.x, np.y, 0.0);
                p = np;
            }
        }

        // A mine that has drifted into the field detonates the instant it touches a
        // rock — clearing it and its neighbours with fast chunks. No life is lost
        // here (that's only ship contact); this is the JS "asteroid-management" mine.
        // Gold rocks are excluded (handled above) so a mine never detonates on one.
        let inside = p.x.abs() < h.x && p.y.abs() < h.y;
        if inside
            && asteroids.iter().any(|(_, at, a, gold, ..)| {
                gold.is_none() && {
                    let rr = MINE_R + asteroid_radius(a.size);
                    p.distance_squared(at.translation.truncate()) < rr * rr
                }
            })
        {
            burst(&mut commands, p, mine_color(), 26, 300.0, &mut rng);
            blast_asteroids(&mut commands, &mut rng, &asteroids, &mut broken, p, time.elapsed_secs());
            sfx.write(SoundFx::Mine);
            commands.entity(me).despawn();
            continue;
        }

        if run.respawn > 0.0 {
            continue; // ship already died this cycle
        }
        if let Some((se, st, sh)) = ship {
            if immune(sh, &dev) {
                continue;
            }
            let sp = st.translation.truncate();
            let d = p.distance(sp);
            if !mine.armed && d < MINE_TRIGGER_R {
                mine.armed = true;
                mine.fuse = MINE_FUSE;
            }
            if mine.armed {
                mine.fuse -= dt;
                let contact = d < MINE_R + SHIP_R;
                if contact || (mine.fuse <= 0.0 && d < MINE_BLAST_R) {
                    burst(&mut commands, p, mine_color(), 26, 300.0, &mut rng);
                    blast_asteroids(&mut commands, &mut rng, &asteroids, &mut broken, p, time.elapsed_secs());
                    sfx.write(SoundFx::Mine);
                    commands.entity(me).despawn();
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                }
            }
        }
    }
}

// Warp (Shift): fire a slow missile — on a very long cooldown — that tears open a
// black hole (see below). Ports the JS vortex/warp shot.
fn warp_fire(
    time: Res<Time>,
    input: Res<ActionState>,
    mut commands: Commands,
    mut warp: ResMut<Warp>,
    mut sfx: EventWriter<SoundFx>,
    mut flash: ResMut<HudFlash>,
    ships: Query<(&Ship, &Transform)>,
    mut stats: Option<ResMut<Stats>>, // optional so headless tests needn't insert it
) {
    // While refilling (all charges were spent), tick the long cooldown; when it
    // ends, restore all charges. No firing during the refill.
    if warp.cooldown > 0.0 {
        warp.cooldown -= time.delta_secs();
        if warp.cooldown <= 0.0 {
            warp.cooldown = 0.0;
            warp.charges = WARP_MAX_CHARGES;
            flash.pips = HUD_FLASH_TIME; // charges just came back — flicker the pips
        }
        return;
    }
    if !input.warp || warp.charges <= 0 {
        return;
    }
    if let Some((ship, t)) = ships.iter().next() {
        let dir = Vec2::from_angle(ship.angle);
        let pos = t.translation.truncate() + dir * SHIP_R;
        commands.spawn((
            WarpMissile { life: WARP_MISSILE_LIFE },
            Velocity(dir * WARP_MISSILE_SPEED),
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
        sfx.write(SoundFx::Warp);
        warp.charges -= 1;
        if let Some(s) = stats.as_mut() {
            s.warps += 1; // lifetime holes opened (achievement: Event Horizon)
        }
        if warp.charges <= 0 {
            warp.cooldown = WARP_COOLDOWN; // last charge spent → start the long refill
        }
    }
}

// The missile flies for WARP_MISSILE_LIFE, then becomes a black hole in place — but it detonates
// early if it reaches the arena edge, and the hole is always clamped fully on-screen. Firing at
// the edge therefore opens a usable hole just inside the boundary instead of sailing off-screen
// where it could pull nothing in.
fn warp_missile_update(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut sfx: EventWriter<SoundFx>,
    mut q: Query<(Entity, &Transform, &Velocity, &mut WarpMissile)>,
    rocks: Query<(&Transform, &Asteroid)>, // gold INCLUDED: warping the 1UP is a valid player action — the
    // missile detonates on it and the hole consumes the lineage, so gold_rush_update grants the life
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let margin = WARP_CONSUME_R; // keep the whole event horizon inside the arena
    for (e, t, v, mut m) in &mut q {
        m.life -= dt;
        let p = t.translation.truncate();
        // detonate only at the wall it's HEADING TOWARD — so a shot launched from near an edge flies
        // inward instead of popping at the launch edge.
        let into_x = (p.x > h.x - margin && v.0.x > 0.0) || (p.x < -h.x + margin && v.0.x < 0.0);
        let into_y = (p.y > h.y - margin && v.0.y > 0.0) || (p.y < -h.y + margin && v.0.y < 0.0);
        // …and go off the instant it hits a rock, so it opens the hole on contact instead of passing through
        let hit_rock = rocks.iter().any(|(rt, a)| {
            let rr = asteroid_radius(a.size) + WARP_MISSILE_R;
            p.distance_squared(rt.translation.truncate()) < rr * rr
        });
        if m.life <= 0.0 || into_x || into_y || hit_rock {
            let c = Vec2::new(p.x.clamp(-h.x + margin, h.x - margin), p.y.clamp(-h.y + margin, h.y - margin));
            commands.entity(e).despawn();
            commands.spawn((BlackHole { life: WARP_HOLE_LIFE, spin: 0.0 }, Transform::from_xyz(c.x, c.y, 0.0)));
            sfx.write(SoundFx::Vortex); // the hole's own voice — a 2.6s churn matched to its life, ending in the collapse thump
        }
    }
}

// ─────────────────────────────── enemy ships (wave 3+) ────────────────
fn enemy_target(level: i32, asteroids: i32) -> i32 {
    // yellow mobs run in two windows: waves 3-4 (before boss 1), then 8-9 (after the green intro,
    // before boss 2). None on 6-7 (green rocks are the focus) or on the boss waves (5, 10). Content
    // waves 11-15 also return 0 here — Act II belongs to the rocks (no mobs).
    let raw = match content_wave(level) {
        3 => 2,
        4 => 4,
        8 => 4,
        9 => 6,
        _ => return 0,
    };
    raw.min((asteroids as f32 * ENEMY_MAX_FRACTION) as i32)
}

// An enemy gliding in from a random edge.
fn spawn_edge_enemy(commands: &mut Commands, half: Vec2, rng: &mut impl Rng) {
    let inward = ENEMY_MAX_SPEED * 1.5;
    let jitter = rng.gen_range(-0.3..0.3) * ENEMY_MAX_SPEED;
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - ENEMY_R, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + ENEMY_R, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - ENEMY_R), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + ENEMY_R), Vec2::new(jitter, -inward)),
    };
    commands.spawn((
        Enemy {
            fire: ENEMY_FIRE_EVERY + rng.gen_range(0.0..ENEMY_FIRE_JITTER),
            life: ENEMY_LIFETIME,
            strafe: if rng.gen_bool(0.5) { 1.0 } else { -1.0 },
            entered: false,
            fleeing: false,
        },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
}

// Stream enemies in (wave 3+), capped as a fraction of the asteroids; not during calm.
// TENDERS only exist on lap two, and only late (content wave 26+): by then you've learned to break
// rocks efficiently, so something that puts them back together is a real counter to your habits.
// Hard-capped at ONE at a time — two would be a chore, not a threat.
fn top_up_tenders(
    time: Res<Time>,
    mut clock: ResMut<TenderClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    plus: Res<NewGamePlus>,
    mut commands: Commands,
    tenders: Query<(), With<Tender>>,
) {
    if !plus.0 || wave.calm > 0.0 || is_boss_wave(wave.level) || content_wave(wave.level) < 26 {
        return;
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 || !tenders.is_empty() {
        return;
    }
    let mut rng = rand::thread_rng();
    let inward = TENDER_SPEED * 1.6;
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-arena.half.x - TENDER_R, rng.gen_range(-arena.half.y..arena.half.y)), Vec2::new(inward, 0.0)),
        1 => (Vec2::new(arena.half.x + TENDER_R, rng.gen_range(-arena.half.y..arena.half.y)), Vec2::new(-inward, 0.0)),
        2 => (Vec2::new(rng.gen_range(-arena.half.x..arena.half.x), -arena.half.y - TENDER_R), Vec2::new(0.0, inward)),
        _ => (Vec2::new(rng.gen_range(-arena.half.x..arena.half.x), arena.half.y + TENDER_R), Vec2::new(0.0, -inward)),
    };
    commands.spawn((
        Tender { life: TENDER_LIFETIME, entered: false, fleeing: false, cool: 0.0, job: None, progress: 0.0 },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
    clock.0 = 9.0; // a long gap — one shift at a time
}

fn top_up_enemies(
    time: Res<Time>,
    mut clock: ResMut<EnemyClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    enemies: Query<(), With<Enemy>>,
    asteroids: Query<(), With<Asteroid>>,
) {
    if wave.calm > 0.0 || is_boss_wave(wave.level) {
        return; // no new enemies during the calm or a boss wave
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let target = enemy_target(wave.level, asteroids.iter().count() as i32);
    if (enemies.iter().count() as i32) < target {
        let mut rng = rand::thread_rng();
        spawn_edge_enemy(&mut commands, arena.half, &mut rng);
        clock.0 = ENEMY_SPAWN_INTERVAL;
    } else {
        clock.0 = 1.0;
    }
}

// THE TENDER'S SALVAGE LOOP. It glides in, hunts for two SMALL (size-1) fragments within reach,
// then holds a tractor beam on both and reels them toward their midpoint. When they meet it fuses
// them into one MID rock — the exact inverse of a split, so an unattended Tender slowly rebuilds
// everything you broke. The job aborts the instant either fragment dies (shoot one to interrupt),
// and the drone itself dies to a single hit. It never harms the ship directly.
fn tender_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut score: ResMut<Score>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<&Transform, With<Ship>>,
    mut rocks: Query<(Entity, &mut Transform, &Asteroid), (Without<Tender>, Without<Ship>, Without<Shielded>, Without<Gold>, Without<Cannonball>)>,
    mut tenders: Query<(Entity, &mut Transform, &mut Velocity, &mut Tender), (Without<Asteroid>, Without<Ship>)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (te, mut tt, mut tv, mut tender) in &mut tenders {
        let p = tt.translation.truncate();
        // ── glide in, then work; bug out when its shift ends ──
        if !tender.entered {
            if p.x.abs() < h.x - TENDER_R && p.y.abs() < h.y - TENDER_R {
                tender.entered = true;
            }
            continue; // `integrate` carries it in
        }
        tender.life -= dt;
        if tender.life <= 0.0 && !tender.fleeing {
            tender.fleeing = true;
            tender.job = None;
        }
        if tender.fleeing {
            // head for the nearest edge and leave
            let out = if p.x.abs() > p.y.abs() { Vec2::new(p.x.signum(), 0.0) } else { Vec2::new(0.0, p.y.signum()) };
            tv.0 += out * TENDER_ACCEL * dt;
            if p.x.abs() > h.x + TENDER_R * 3.0 || p.y.abs() > h.y + TENDER_R * 3.0 {
                commands.entity(te).despawn();
            }
            continue;
        }

        // ── an active salvage job: verify both fragments still exist, then reel them together ──
        if let Some((a, b)) = tender.job {
            let both = rocks.get(a).is_ok() && rocks.get(b).is_ok();
            if !both {
                tender.job = None; // interrupted — one of them was destroyed
                tender.progress = 0.0;
                tender.cool = TENDER_COOL;
                continue;
            }
            let pa = rocks.get(a).map(|(_, t, _)| t.translation.truncate()).unwrap_or_default();
            let pb = rocks.get(b).map(|(_, t, _)| t.translation.truncate()).unwrap_or_default();
            let mid = (pa + pb) * 0.5;
            tender.progress += dt;
            // haul both inward; the drone hangs back at the midpoint so the beam reads clearly
            for (e, from) in [(a, pa), (b, pb)] {
                if let Ok((_, mut rt, _)) = rocks.get_mut(e) {
                    let step = (mid - from).clamp_length_max(TENDER_HAUL * dt);
                    rt.translation.x += step.x;
                    rt.translation.y += step.y;
                }
            }
            tv.0 += (mid - p).clamp_length_max(TENDER_ACCEL * dt);
            if tv.0.length() > TENDER_SPEED {
                tv.0 = tv.0.normalize() * TENDER_SPEED;
            }
            // fused: the two fragments become one MID rock — a split, run backwards
            if tender.progress >= TENDER_FUSE_SECS || pa.distance(pb) < asteroid_radius(1) * 1.6 {
                commands.entity(a).despawn();
                commands.entity(b).despawn();
                let vel = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(30.0..70.0);
                spawn_asteroid(&mut commands, mid, 2, vel, &mut rng, false);
                burst(&mut commands, mid, enemy_color(), 14, 200.0, &mut rng);
                sfx.write(SoundFx::Mine); // a heavy mechanical clunk as it welds
                tender.job = None;
                tender.progress = 0.0;
                tender.cool = TENDER_COOL;
            }
            continue;
        }

        // ── idle: find the closest PAIR of small fragments in reach ──
        tender.cool -= dt;
        if tender.cool <= 0.0 {
            let mut smalls: Vec<(Entity, Vec2)> = rocks
                .iter()
                .filter(|(_, _, a)| a.size == 1)
                .map(|(e, t, _)| (e, t.translation.truncate()))
                .filter(|(_, rp)| rp.distance(p) < TENDER_REACH)
                .collect();
            if smalls.len() >= 2 {
                smalls.sort_by(|x, y| x.1.distance_squared(p).total_cmp(&y.1.distance_squared(p)));
                tender.job = Some((smalls[0].0, smalls[1].0));
                tender.progress = 0.0;
            } else if let Some(sp) = ship {
                // nothing to salvage: drift wide of the player rather than engaging (it isn't a fighter)
                let away = (p - sp).normalize_or_zero();
                tv.0 += away * TENDER_ACCEL * 0.25 * dt;
            }
        }
        if tv.0.length() > TENDER_SPEED {
            tv.0 = tv.0.normalize() * TENDER_SPEED;
        }
        // keep it on the field
        tt.translation.x = p.x.clamp(-h.x + TENDER_R, h.x - TENDER_R);
        tt.translation.y = p.y.clamp(-h.y + TENDER_R, h.y - TENDER_R);
        let _ = &mut score; // scoring happens where it's shot (collisions), not here
    }
}

// Enemy movement + firing. `integrate` moves them (they carry a Velocity); here we
// only steer that velocity. Glide in, then hover + strafe around the ship, steering
// clear of mines and rocks and lobbing slow shots. A live warp overrides all of it
// (they get dragged in — handled in black_hole_update). After ENEMY_LIFETIME they
// flee the nearest edge and despawn, so they never overstay.
fn enemy_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    wave: Res<Wave>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<&Transform, With<Ship>>,
    mines: Query<&Transform, With<Mine>>,
    rocks: Query<(&Transform, &Asteroid)>,
    holes: Query<&Transform, With<BlackHole>>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity, &mut Enemy)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    // snapshot every enemy's position up front for mutual separation below — we can't
    // re-borrow the enemies query while iterating it mutably.
    let others: Vec<(Entity, Vec2)> = enemies.iter().map(|(e, t, _, _)| (e, t.translation.truncate())).collect();
    for (e, t, mut v, mut en) in &mut enemies {
        let p = t.translation.truncate();

        // caught in a warp → yield control (black_hole_update drags + consumes it)
        if holes.iter().any(|ht| ht.translation.truncate().distance(p) < WARP_PULL_RADIUS) {
            continue;
        }

        // glide in until fully on-screen, then settle
        if !en.entered {
            if p.x.abs() < h.x - ENEMY_R && p.y.abs() < h.y - ENEMY_R {
                en.entered = true;
                v.0 *= 0.4;
            }
            continue;
        }

        // lifetime → flee straight out and despawn once gone. A boss also clears the field: any mob
        // still around in the run-up flees so the boss arrives to a clean arena.
        en.life -= dt;
        if en.life <= 0.0 || boss_incoming(&wave) {
            en.fleeing = true;
        }
        if en.fleeing {
            v.0 += p.normalize_or_zero() * ENEMY_ACCEL * dt;
            let cap = ENEMY_MAX_SPEED * 2.0;
            if v.0.length() > cap {
                v.0 = v.0.normalize() * cap;
            }
            if p.x.abs() > h.x + ENEMY_R * 3.0 || p.y.abs() > h.y + ENEMY_R * 3.0 {
                commands.entity(e).despawn();
            }
            continue;
        }

        // hover + strafe around the ship
        let mut acc = Vec2::ZERO;
        if let Some(sp) = ship {
            let to = sp - p;
            let d = to.length().max(1.0);
            let n = to / d;
            if d > ENEMY_PREF_DIST + 40.0 {
                acc += n;
            } else if d < ENEMY_PREF_DIST - 40.0 {
                acc -= n;
            }
            acc += Vec2::new(-n.y, n.x) * en.strafe * 0.6; // orbit
        }
        // avoid mines + rocks: push away from anything close (rock size widens reach)
        for mt in &mines {
            let away = p - mt.translation.truncate();
            let d = away.length();
            if d > 0.01 && d < ENEMY_AVOID_R {
                acc += away / d * (1.0 - d / ENEMY_AVOID_R) * 2.2;
            }
        }
        for (rt, a) in &rocks {
            let away = p - rt.translation.truncate();
            let d = away.length();
            let reach = ENEMY_AVOID_R + asteroid_radius(a.size);
            if d > 0.01 && d < reach {
                acc += away / d * (1.0 - d / reach) * 2.6;
            }
        }
        // keep clear of EACH OTHER — enemies spread into a loose formation, never a stack
        for &(oe, op) in &others {
            if oe == e {
                continue;
            }
            let away = p - op;
            let d = away.length();
            if d > 0.01 && d < ENEMY_SEP_R {
                acc += away / d * (1.0 - d / ENEMY_SEP_R) * 2.4;
            }
        }
        v.0 += acc * ENEMY_ACCEL * dt;
        v.0 *= 0.985_f32.powf(dt * 60.0); // damping (frame-rate independent)
        if v.0.length() > ENEMY_MAX_SPEED {
            v.0 = v.0.normalize() * ENEMY_MAX_SPEED;
        }
        // bounce off the arena edges so they stay in play
        if (p.x < -h.x + ENEMY_R && v.0.x < 0.0) || (p.x > h.x - ENEMY_R && v.0.x > 0.0) {
            v.0.x = -v.0.x;
        }
        if (p.y < -h.y + ENEMY_R && v.0.y < 0.0) || (p.y > h.y - ENEMY_R && v.0.y > 0.0) {
            v.0.y = -v.0.y;
        }

        // lob a slow shot at the ship
        en.fire -= dt;
        if en.fire <= 0.0 {
            en.fire = ENEMY_FIRE_EVERY + rng.gen_range(0.0..ENEMY_FIRE_JITTER);
            if let Some(sp) = ship {
                let dir = (sp - p).normalize_or_zero();
                if dir != Vec2::ZERO {
                    commands.spawn((
                        EnemyBullet { life: ENEMY_BULLET_LIFE },
                        Velocity(dir * ENEMY_BULLET_SPEED),
                        Transform::from_xyz(p.x, p.y, 0.0),
                    ));
                    sfx.write(SoundFx::EnemyShot);
                }
            }
        }
    }
}

// Stream gravity Wells in on their content waves (18-19), capped at WELL_MAX; not during the calm or a
// boss wave. Each spawns away from the ship so it never appears right on top of you.
fn top_up_wells(
    time: Res<Time>,
    mut clock: ResMut<WellClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    wells: Query<(), With<Well>>,
    ships: Query<&Transform, With<Ship>>,
) {
    if wave.calm > 0.0 || is_boss_wave(wave.level) || !matches!(content_wave(wave.level), 18..=19) {
        return;
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    if (wells.iter().count() as i32) < WELL_MAX {
        let mut rng = rand::thread_rng();
        let h = arena.half;
        let ship = ships.iter().next().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO);
        let mut pos = Vec2::ZERO;
        for _ in 0..8 {
            pos = Vec2::new(rng.gen_range(-h.x * 0.78..h.x * 0.78), rng.gen_range(-h.y * 0.78..h.y * 0.78));
            if pos.distance(ship) > WELL_PULL_RADIUS * 0.8 {
                break; // not right on top of the player
            }
        }
        commands.spawn((Well { life: WELL_LIFE, spin: 0.0 }, Transform::from_xyz(pos.x, pos.y, 0.0)));
        clock.0 = rng.gen_range(WELL_MIN_GAP..WELL_MAX_GAP); // sporadic — next one pops in at a random time
    } else {
        clock.0 = 1.0;
    }
}

// Gravity Wells: tick each one's life (collapse at 0) and drag the ship toward every live one. The
// pull is weaker than thrust (`well_pull`), so the player can always fly out.
fn well_update(
    time: Res<Time>,
    mut commands: Commands,
    mut wells: Query<(Entity, &Transform, &mut Well)>,
    mut ships: Query<(&Transform, &mut Velocity), With<Ship>>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    for (we, wt, mut w) in &mut wells {
        w.spin += dt * 2.2;
        w.life -= dt;
        if w.life <= 0.0 {
            burst(&mut commands, wt.translation.truncate(), well_color(), 18, 260.0, &mut rng); // collapse
            commands.entity(we).despawn();
        }
    }
    if let Some((st, mut sv)) = ships.iter_mut().next() {
        let sp = st.translation.truncate();
        for (_we, wt, w) in &wells {
            if w.life > 0.0 {
                sv.0 += well_pull(sp, wt.translation.truncate(), dt);
            }
        }
    }
}

// Enemy shots: `integrate` carries them; here we expire them (time / off-screen) and
// kill the ship on contact (respecting invuln + dev invincibility).
fn enemy_bullets(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    mut bullets: Query<(Entity, &Transform, &mut EnemyBullet)>,
    ships: Query<(Entity, &Transform, &Ship)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    for (be, bt, mut b) in &mut bullets {
        b.life -= dt;
        let p = bt.translation.truncate();
        if b.life <= 0.0 || p.x.abs() > h.x + 30.0 || p.y.abs() > h.y + 30.0 {
            commands.entity(be).despawn();
            continue;
        }
        if run.respawn > 0.0 {
            continue;
        }
        if let Some((se, st, sh)) = ship {
            if immune(sh, &dev) {
                continue;
            }
            let sp = st.translation.truncate();
            if p.distance(sp) < ENEMY_BULLET_R + SHIP_R {
                commands.entity(be).despawn();
                kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
            }
        }
    }
}

// ─────────────────────────────── octopus boss (waves 5, 10, …) ────────
// On entering a boss wave, spawn the boss (once) and clear the field of mines +
// enemy ships so only asteroids remain for it to grab.
fn boss_director(
    mut commands: Commands,
    wave: Res<Wave>,
    arena: Res<Arena>,
    plus: Res<NewGamePlus>,
    mut stats: Option<ResMut<Stats>>, // Option so headless tests needn't insert it
    mut state: ResMut<BossState>,
    mut enemies: Query<&mut Enemy>,
    field: Query<(Entity, &Transform), (With<Asteroid>, Without<Cannonball>, Without<Gold>)>, // slate-wipe spares the gold 1UP (else its lineage vanishing reads as "cleared" → a free life)
) {
    if !is_boss_wave(wave.level) || state.fought == wave.level {
        return;
    }
    let mut rng = rand::thread_rng();
    state.fought = wave.level;
    // GALLERY: this is the only place that knows WHICH boss just arrived, so it marks the sighting
    if let Some(s) = stats.as_mut() {
        if mark_seen(s, GalleryArt::Boss(boss_kind(wave.level))) {
            save_progress(s);
        }
    }
    let hp = |base: i32| scaled_hp(base, plus.0); // NG+: every core spawns half again as tough
    if is_devourer_wave(wave.level) {
        // Boss 2: the devourer starts small in the upper arena and hunts free rocks to grow.
        commands.spawn((
            Devourer { hp: hp(DEVOURER_HP), grow: 0.0, fed: 0, dying: 0.0, pulse: 0.0, inhale: 0.0, inhale_cd: NGP_GLUT_INHALE_EVERY, spit: 0.0 },
            Transform::from_xyz(0.0, arena.half.y * 0.55, 0.0),
        ));
    } else if is_slinger_wave(wave.level) {
        // Boss 3: the Slinger glides in from the top, then hovers across from the ship and fires rocks.
        // Clear the field for a clean "boss + green only" slate — the Slinger makes its own cannonballs
        // and doesn't use field rocks, so leftovers from wave 14 would just clutter the fight (and
        // top_up only ADDS, never trims). top_up then streams the sparse SLINGER_WAVE_ROCKS back in.
        for (a, at) in &field {
            burst(&mut commands, at.translation.truncate(), Color::srgb(2.2, 2.8, 4.2), 9, 175.0, &mut rng); // dissolve as the Slinger sweeps in
            commands.entity(a).despawn();
        }
        commands.spawn((
            Slinger { hp: hp(SLINGER_HP), entered: false, charge: SLINGER_INTRO, cool: SLINGER_COOL, load: 0.0, ammo: None, pulse: 0.0, recoil: 0.0, dying: 0.0 },
            Transform::from_xyz(0.0, arena.half.y + SLINGER_R, 0.0),
        ));
    } else if is_detonator_wave(wave.level) {
        // Boss 4: the Detonator glides in from the top. The field is left INTACT — its orange rocks are
        // the bombs it primes, and shooting those (near it, during a priming window) is how you hurt it.
        commands.spawn((
            Detonator { hp: hp(DETONATOR_HP), entered: false, charge: DETONATOR_INTRO, cool: DETONATOR_COOL, prime: 0.0, target: None, pulse: 0.0, dying: 0.0 },
            Transform::from_xyz(0.0, arena.half.y + DETONATOR_R, 0.0),
        ));
    } else if is_pulsar_wave(wave.level) {
        // Boss 5: the Pulsar glides in, pulses lit (invulnerable) / dark (open), and shockwaves the field
        // outward on a beat. Its wave (25) is pulser-heavy, so the field is already a timing gauntlet.
        commands.spawn((
            Pulsar { hp: hp(PULSAR_HP), entered: false, charge: PULSAR_INTRO, phase: 0.0, shock_cool: PULSAR_SHOCK_EVERY, pulse: 0.0, dying: 0.0 },
            Transform::from_xyz(0.0, arena.half.y + PULSAR_R, 0.0),
        ));
    } else if is_phantom_wave(wave.level) {
        // Boss 6 (FINALE): CLEAR the field for a clean slate, then the belt returns in mono-type groups of
        // ten (see `top_up_asteroids`). Reset the group cycle so it opens on blue, trickling from the start.
        for (a, at) in &field {
            burst(&mut commands, at.translation.truncate(), Color::srgb(2.2, 2.8, 4.2), 9, 175.0, &mut rng);
            commands.entity(a).despawn();
        }
        commands.spawn((
            Phantom::new(hp(PHANTOM_PHASE_HP), false, PHANTOM_INTRO),
            Transform::from_xyz(0.0, arena.half.y + PHANTOM_R, 0.0),
        ));
    } else {
        // Boss 1: the shield-shaman glides in from the top.
        commands.spawn((
            Boss {
                hp: hp(BOSS_HP),
                rot: 0.0,
                pulse: 0.0,
                entered: false,
                charge: BOSS_CHARGE,
                fire: BOSS_FIRE_EVERY,
                capture: 0.4,
                dying: 0.0,
                whirl: Whirl::Idle,
                whirl_t: NGP_WARDEN_WHIRL_EVERY,
            },
            Transform::from_xyz(0.0, arena.half.y + BOSS_R + BOSS_ORBIT_R, 0.0),
        ));
    }
    // Enemy ships bug out (flee). Existing mines are LEFT ALONE — they keep behaving
    // normally (drift/detonate/shootable) and drift off the edges (mine_update despawns
    // them at the edge during a boss wave instead of recycling). No new mines or enemies
    // spawn (top_up_mines / top_up_enemies are gated off on boss waves).
    for mut en in &mut enemies {
        en.fleeing = true;
    }
}

// A boss's wandering destination: a Lissajous on two INCOMMENSURATE rates, so the path never settles
// into a loop you can memorise and camp. Amplitudes span the whole playfield minus `margin` (which
// must clear the boss's own hazard geometry, e.g. the Warden's shield ring) — bosses roam ANYWHERE
// on screen, they do not patrol a band (user rule, 2026-07-31).
fn boss_roam_target(pulse: f32, h: Vec2, margin: f32) -> Vec2 {
    Vec2::new(
        (pulse * 0.16).sin() * (h.x - margin) * 0.92,
        (pulse * 0.11 + 1.3).sin() * (h.y - margin) * 0.86,
    )
}

// Boss movement (glide in → roam the arena), charge-up (invulnerable), ship-contact
// kill, and death → big burst, release the shield, reward calm, advance the wave.
fn boss_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    plus: Res<NewGamePlus>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut sfx: EventWriter<SoundFx>,
    mut stats: ResMut<Stats>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship), Without<Boss>>,
    mut bosses: Query<(Entity, &mut Transform, &mut Boss)>,
    mut shielded: Query<(Entity, &mut Velocity), With<Shielded>>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    for (be, mut bt, mut boss) in &mut bosses {
        boss.pulse += dt * 5.0;
        let p = bt.translation.truncate();

        // ── DYING: a slow death animation, then despawn → reward calm → advance ──
        if boss.dying > 0.0 {
            let before = death_parts(boss.dying, BOSS_DEATH_SECS, BOSS_ARMS);
            boss.dying -= dt;
            boss.rot += BOSS_SPIN * 2.5 * dt; // spins up as it comes apart
            // STAGED: the tentacles shear off one by one (the render stops drawing each as it goes)
            let after = death_parts(boss.dying.max(0.0), BOSS_DEATH_SECS, BOSS_ARMS);
            if after < before {
                let a = boss.rot + after as f32 / BOSS_ARMS as f32 * TAU;
                burst(&mut commands, p + Vec2::from_angle(a) * BOSS_ORBIT_R * 0.5, boss_color(), 16, 340.0, &mut rng);
            }
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..BOSS_R);
                burst(&mut commands, p + off, boss_color(), 3, 240.0, &mut rng); // crackle
            }
            if boss.dying <= 0.0 {
                burst(&mut commands, p, boss_color(), 50, 460.0, &mut rng); // final blast
                burst(&mut commands, p, Color::srgb(5.0, 4.0, 5.0), 24, 300.0, &mut rng);
                commands.entity(be).despawn();
                // The chain-shot orb is offered after the shaman (content wave 5). Grab it in the
                // calm or lose it until the next cycle. (Checked before the level-up.)
                if content_wave(wave.level) == BOSS_WAVE_INTERVAL {
                    let dir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                    // ONE orb per boss, always. Lap one gives the chain beam; NEW GAME+ gives the
                    // AEGIS SHARDS *instead* (user call — two orbs on the field read as clutter).
                    // Losing the beam costs NG+ nothing: its roster retires after wave 5, so there
                    // are no beacons for the beam to answer on lap two.
                    let kind = if plus.0 { PickupKind::Aegis } else { PickupKind::Chain };
                    commands.spawn((
                        Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind },
                        Velocity(dir * PICKUP_DRIFT),
                        Transform::from_xyz(0.0, 0.0, 0.0),
                    ));
                }
                stats.warden = true; // achievement: defeated the Warden
                sfx.write(SoundFx::BossDown);
                defeat_boss(&mut score, &mut wave, &mut banner, Some(&mut stats));
            }
            continue; // no movement / contact / damage while it dies
        }

        // ── core destroyed → begin dying: scatter the shield, then animate ──
        if boss.hp <= 0 {
            boss.dying = BOSS_DEATH_SECS;
            burst(&mut commands, p, boss_color(), 30, 320.0, &mut rng);
            for (se, mut sv) in &mut shielded {
                commands.entity(se).remove::<Shielded>();
                sv.0 = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(80.0..160.0);
            }
            continue;
        }

        // ── ALIVE: glide in → bob near the top, charge-up, ship-contact kill ──
        boss.rot += BOSS_SPIN * whirl_spin_mult(boss.whirl, boss.whirl_t) * dt;
        // the margin has to clear the SHIELD RING, and in NG+ the whirl extends the arms — otherwise
        // a sweeping ring would hang off-screen where it can't be read or dodged
        let reach = if plus.0 { NGP_WARDEN_WHIRL_REACH } else { 1.0 };
        let margin = BOSS_R + BOSS_ORBIT_R * reach + 6.0;
        let mut p = p;
        let rest_y = h.y - margin;
        if !boss.entered {
            p.y -= BOSS_ENTER_SPEED * dt;
            if p.y <= rest_y {
                p.y = rest_y;
                boss.entered = true;
            }
        } else {
            if boss.charge > 0.0 {
                boss.charge -= dt;
            }
            // ROAM THE WHOLE ARENA (2026-07-31 — user: bosses must move anywhere on screen, not
            // side-to-side across the top). A Lissajous on two incommensurate rates, so the path
            // never settles into a loop you can memorise and camp: it comes down at you, crosses,
            // and drifts back up. Amplitudes are the full playfield minus the ring's margin.
            let target = boss_roam_target(boss.pulse, h, margin);
            let np = p + (target - p) * (1.0 - (-dt * 2.6).exp());
            p.x = np.x.clamp(-h.x + margin, h.x - margin);
            p.y = np.y.clamp(-h.y + margin, h.y - margin);
        }
        bt.translation = Vec3::new(p.x, p.y, 0.0);

        if run.respawn <= 0.0 {
            if let Some((se, st, sh)) = ship {
                let sp = st.translation.truncate();
                if !immune(sh, &dev) && p.distance(sp) < BOSS_R + SHIP_R {
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                }
            }
        }
    }
}

// Boss 2: the devourer HUNTS free rocks and EATS any within reach (growing bigger + healing),
// and hunts the ship when the field is clear. Chip its HP with gunfire (see `collisions`) while
// CLEARING rocks to starve it — feed it and it snowballs. Death → the shared post-boss calm.
fn devourer_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut stats: ResMut<Stats>,
    dev: Res<Dev>,
    mut sfx: EventWriter<SoundFx>,
    plus: Res<NewGamePlus>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Velocity), (Without<Devourer>, Without<Asteroid>)>,
    mut devourers: Query<(Entity, &mut Transform, &mut Devourer), Without<Asteroid>>,
    mut rocks: Query<(Entity, &Transform, &mut Velocity, &Asteroid), (Without<Shielded>, Without<Devourer>, Without<Gold>, Without<Ship>)>, // never eats gold (would grant a false 1UP)
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    // Snapshot the ship by VALUE: the inhale needs `&mut ships` below, so we must not hold a
    // reference-borrow of the query across the boss loop.
    let ship: Option<(Entity, Vec2, bool)> = ships.iter().next().map(|(e, t, sh, _)| (e, t.translation.truncate(), immune(sh, &dev)));
    for (de, mut tf, mut dv) in &mut devourers {
        dv.pulse += dt * 5.0;
        let p = tf.translation.truncate();
        let r = devourer_radius(dv.grow);

        // ── DYING: crackle, then a big blast → despawn → advance ──
        if dv.dying > 0.0 {
            let before = death_parts(dv.dying, BOSS_DEATH_SECS, 3);
            dv.dying -= dt;
            // STAGED: three deflate-SPASMS on the way down (the shrink is continuous; these punctuate it)
            if death_parts(dv.dying.max(0.0), BOSS_DEATH_SECS, 3) < before {
                burst(&mut commands, p, devourer_color(), 24, 360.0, &mut rng);
            }
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..r);
                burst(&mut commands, p + off, devourer_color(), 3, 260.0, &mut rng);
            }
            if dv.dying <= 0.0 {
                burst(&mut commands, p, devourer_color(), 60, 500.0, &mut rng);
                burst(&mut commands, p, Color::srgb(6.0, 4.0, 4.0), 26, 320.0, &mut rng);
                commands.entity(de).despawn();
                // boss-2 reward (content wave 10): the mass shot normally, the GORGE ROUND in NG+ —
                // one orb either way, so the choice on Q never gets diluted.
                let kind = if plus.0 { PickupKind::Gorge } else { PickupKind::Mass };
                let pdir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                commands.spawn((
                    Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind },
                    Velocity(pdir * PICKUP_DRIFT),
                    Transform::from_xyz(0.0, 0.0, 0.0),
                ));
                stats.glutton = true; // achievement: defeated the Glutton
                sfx.write(SoundFx::BossDown);
                defeat_boss(&mut score, &mut wave, &mut banner, Some(&mut stats));
            }

            continue;
        }
        if dv.hp <= 0 {
            dv.dying = BOSS_DEATH_SECS;
            burst(&mut commands, p, devourer_color(), 34, 340.0, &mut rng);
            continue;
        }

        // ── NG+ INHALE ── phase clock first. The wind-up is PURE TELEGRAPH: the maw gapes and the
        // wedge is drawn (see render_boss), but nothing is pulled until it flips to inhaling.
        let face = ship.map(|(_, sp, _)| (sp - p).normalize_or_zero()).unwrap_or(Vec2::NEG_Y);
        if plus.0 && dv.dying <= 0.0 {
            if dv.inhale > 0.0 {
                dv.inhale -= dt;
                if dv.inhale <= 0.0 {
                    dv.inhale_cd = NGP_GLUT_INHALE_EVERY;
                }
            } else {
                dv.inhale_cd -= dt;
                if dv.inhale_cd <= 0.0 {
                    dv.inhale = NGP_GLUT_INHALE_DUR + NGP_GLUT_INHALE_WIND;
                }
            }
        }
        // is `q` inside the suction wedge, and how strongly does it bite there? (0 = outside)
        let bite = |q: Vec2| -> f32 {
            let to = q - p;
            let d = to.length();
            if !(1.0..=NGP_GLUT_INHALE_REACH).contains(&d) {
                return 0.0;
            }
            let out = to / d;
            if out.dot(face) < NGP_GLUT_INHALE_ARC.cos() {
                return 0.0; // outside the cone — standing off to the side is the counter
            }
            1.0 - d / NGP_GLUT_INHALE_REACH // falls off with distance
        };
        if dv.inhaling() {
            // haul loose rocks in — it is feeding itself, which is what arms the spit below
            for (_re, rt, mut rv, _ra) in &mut rocks {
                let rp = rt.translation.truncate();
                let g = bite(rp);
                if g > 0.0 {
                    rv.0 += (p - rp).normalize_or_zero() * NGP_GLUT_ROCK_PULL * g * dt;
                }
            }
            // …and haul the SHIP. Capped under THRUST and falling off with distance, so flying out is
            // always possible; what it really costs is a dodge you had already committed to.
            for (_se, st, _sh, mut sv) in &mut ships {
                let sp = st.translation.truncate();
                let g = bite(sp);
                if g > 0.0 {
                    sv.0 += (p - sp).normalize_or_zero() * NGP_GLUT_INHALE_PULL * g * dt;
                }
            }
        }

        // ── NG+ REGURGITATE ── gorged enough → a short swell (the tell), then it spits the mass back
        // along its facing as a spread of rocks. What went in is what comes out.
        if plus.0 && dv.spit > 0.0 {
            dv.spit -= dt;
            if dv.spit <= 0.0 {
                let base = face.to_angle();
                for i in 0..NGP_GLUT_SPIT_ROCKS {
                    let f = if NGP_GLUT_SPIT_ROCKS > 1 { i as f32 / (NGP_GLUT_SPIT_ROCKS - 1) as f32 - 0.5 } else { 0.0 };
                    let dir = Vec2::from_angle(base + f * NGP_GLUT_SPIT_ARC);
                    let e = spawn_asteroid(&mut commands, p + dir * (r + asteroid_radius(1) + 6.0), 1, dir * NGP_GLUT_SPIT_SPEED, &mut rng, false);
                    commands.entity(e).insert((Thrown(2.0), Fresh(FRAGMENT_GRACE)));
                }
                burst(&mut commands, p + face * r, devourer_color(), 26, 380.0, &mut rng);
                sfx.write(SoundFx::Mine);
                dv.fed -= NGP_GLUT_SPIT_FED; // it spent what it ate…
                dv.grow = (dv.grow - DEVOURER_GROW_PER_EAT * NGP_GLUT_SPIT_FED as f32).max(0.0); // …and shrinks for it
            }
        } else if plus.0 && dv.dying <= 0.0 && dv.fed >= NGP_GLUT_SPIT_FED && dv.inhale <= 0.0 {
            dv.spit = NGP_GLUT_SPIT_WIND; // arm it — one attack at a time, never mid-inhale
        }

        // ── eat any rock within reach (grow + heal), and note the nearest to chase ──
        let mut nearest: Option<Vec2> = None;
        let mut nd2 = f32::MAX;
        for (re, rt, _rv, ra) in &rocks {
            let rp = rt.translation.truncate();
            let reach = r + asteroid_radius(ra.size);
            let d2 = p.distance_squared(rp);
            if d2 < reach * reach {
                commands.entity(re).despawn();
                dv.grow = (dv.grow + DEVOURER_GROW_PER_EAT).min(1.0);
                dv.hp = (dv.hp + DEVOURER_HEAL_PER_EAT).min(DEVOURER_HP_MAX);
                dv.fed += 1;
                burst(&mut commands, rp, devourer_color(), 12, 240.0, &mut rng);
                sfx.write(SoundFx::Break(ra.size));
            } else if d2 < nd2 {
                nd2 = d2;
                nearest = Some(rp);
            }
        }

        // ── OVERLOAD: gorged to full → a screen-wide detonation, then it shrinks to nothing and
        //    starts feeding again. Starve it (clear the rocks) to keep it from ever filling up. ──
        if dv.grow >= 1.0 {
            for (re, rt, _, _) in &rocks {
                burst(&mut commands, rt.translation.truncate(), devourer_color(), 5, 240.0, &mut rng);
                commands.entity(re).despawn(); // wipe the field (gold isn't in `rocks`, so it's spared)
            }
            burst(&mut commands, p, Color::srgb(7.0, 3.0, 3.0), 90, 760.0, &mut rng); // shockwave
            burst(&mut commands, p, devourer_color(), 50, 520.0, &mut rng);
            sfx.write(SoundFx::Mine);
            // caught in the blast → dead (unless mid-respawn or invincible); escapable only by distance
            if run.respawn <= 0.0 {
                if let Some((se, sp, imm)) = ship {
                    if !imm && p.distance(sp) < DEVOURER_BURST_R {
                        kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                    }
                }
            }
            dv.grow = 0.0; // shrink back to starting size and gorge again
            continue;
        }

        // ── move toward the nearest rock, or hunt the ship when the field is clear ──
        let goal = nearest.or_else(|| ship.map(|(_, sp, _)| sp));
        if let Some(g) = goal {
            let dir = (g - p).normalize_or_zero();
            // it LUNGES at its prey — surging bites of speed instead of a flat glide (alive, and each
            // surge telegraphs; the average pace stays close to the old constant DEVOURER_SPEED)
            let lunge = 0.55 + 0.9 * (dv.pulse * 0.35).sin().max(0.0).powi(2);
            let np = p + dir * DEVOURER_SPEED * lunge * dt;
            tf.translation = Vec3::new(np.x.clamp(-h.x + r, h.x - r), np.y.clamp(-h.y + r, h.y - r), 0.0);
        }

        // ── contact kills the ship ──
        if run.respawn <= 0.0 {
            if let Some((se, sp, imm)) = ship {
                if !imm && p.distance(sp) < r + SHIP_R {
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                }
            }
        }
    }
}

// The shield: reel captured rocks into their rotating orbit slots, grab more field
// rocks to fill empty arms, and hurl any held rock whittled to its smallest size at
// the ship. Also ages out just-thrown rocks so they can't be re-grabbed instantly.
fn boss_shield(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    plus: Res<NewGamePlus>,
    ships: Query<&Transform, (With<Ship>, Without<Boss>, Without<Asteroid>)>,
    mut bosses: Query<(&Transform, &mut Boss)>,
    mut shielded: Query<(Entity, &mut Transform, &mut Velocity, &Asteroid, &mut Shielded), Without<Boss>>,
    free: Query<(Entity, &Transform, &Asteroid), (Without<Shielded>, Without<Boss>, Without<Thrown>, Without<Gold>)>, // never captures the gold 1UP onto the shield
    mut thrown: Query<(Entity, &mut Thrown)>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();

    for (te, mut th) in &mut thrown {
        th.0 -= dt;
        if th.0 <= 0.0 {
            commands.entity(te).remove::<Thrown>();
        }
    }

    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (bt, mut boss) in &mut bosses {
        if boss.hp <= 0 || !boss.entered {
            continue;
        }
        let bp = bt.translation.truncate();

        // ── THE WHIRL (NG+): advance the phase clock; the curves themselves are shared helpers ──
        if plus.0 {
            boss.whirl_t -= dt;
            if boss.whirl_t <= 0.0 {
                let (next, t) = match boss.whirl {
                    Whirl::Idle => (Whirl::Wind, NGP_WARDEN_WIND),
                    Whirl::Wind => (Whirl::Spin, NGP_WARDEN_SPIN),
                    Whirl::Spin => (Whirl::Recover, NGP_WARDEN_RECOVER),
                    Whirl::Recover => (Whirl::Idle, NGP_WARDEN_WHIRL_EVERY),
                };
                boss.whirl = next;
                boss.whirl_t = t;
            }
        }
        let ring_reach = whirl_reach(boss.whirl, boss.whirl_t);
        // hold: reel in / pin each shield rock to its rotating slot (mark used arms)
        let mut used = [false; BOSS_ARMS];
        for (_se, mut st, mut sv, _a, mut sh) in &mut shielded {
            if sh.slot < BOSS_ARMS {
                used[sh.slot] = true;
            }
            let ang = (sh.slot as f32 / BOSS_ARMS as f32) * TAU + boss.rot;
            let target = bp + Vec2::from_angle(ang) * BOSS_ORBIT_R * ring_reach;
            let cur = st.translation.truncate();
            let np = if sh.grab < BOSS_GRAB_TIME {
                sh.grab += dt;
                cur + (target - cur) * (1.0 - (-dt * 1.6).exp()) // slow, readable reel-in
            } else {
                target
            };
            st.translation = Vec3::new(np.x, np.y, 0.0);
            sv.0 = Vec2::ZERO;
        }

        // throw FIRST: fling a held smallest-size rock at the ship (frees an arm)…
        // THE WARDEN+ (NG+): meaner cadence, a TWO-rock spread per throw, and every hurled rock is
        // PRIMED — a live bomb on a short fuse. Shoot it out of the air or clear the blast radius.
        let (fire_every, capture_every) = if plus.0 {
            (BOSS_FIRE_EVERY * NGP_WARDEN_RATE, BOSS_CAPTURE_EVERY * NGP_WARDEN_RATE)
        } else {
            (BOSS_FIRE_EVERY, BOSS_CAPTURE_EVERY)
        };
        let volley = if plus.0 { NGP_WARDEN_VOLLEY } else { 1 };
        // the whirl is a whole attack on its own: it doesn't also throw or grab during it, which
        // keeps the telegraph readable and gives the recovery real value as a punish window
        let busy = !matches!(boss.whirl, Whirl::Idle);
        boss.fire -= dt;
        if boss.fire <= 0.0 && !busy {
            boss.fire = fire_every + rng.gen_range(0.0..BOSS_FIRE_JITTER);
            if let Some(sp) = ship {
                let mut hurled = 0usize;
                for (se, st, mut sv, a, sh) in &mut shielded {
                    if a.size == 1 {
                        // spread the volley: the 2nd rock leads/trails the aim a touch so a NG+ pair
                        // can't be dodged as one object
                        let jink = (hurled as f32 - (volley as f32 - 1.0) * 0.5) * 0.22;
                        let dir = Vec2::from_angle(jink).rotate((sp - st.translation.truncate()).normalize_or_zero());
                        if dir != Vec2::ZERO {
                            sv.0 = dir * BOSS_THROW_SPEED;
                            commands.entity(se).remove::<Shielded>();
                            commands.entity(se).insert(Thrown(2.0));
                            if plus.0 {
                                commands.entity(se).insert(Detonating { fuse: NGP_WARDEN_FUSE, friendly: false });
                            }
                            if sh.slot < BOSS_ARMS {
                                used[sh.slot] = false; // that arm is now free to refill
                            }
                            hurled += 1;
                        }
                        if hurled >= volley {
                            break;
                        }
                    }
                }
            }
        }

        // …THEN grab another rock into an empty arm, biggest first (better shield)
        boss.capture -= dt;
        if boss.capture <= 0.0 {
            boss.capture = capture_every;
            let held = used.iter().filter(|u| **u).count();
            if held < BOSS_ARMS {
                let mut best: Option<(Entity, u8, f32)> = None; // biggest reachable rock (size >= 2)
                let mut small: Option<(Entity, f32)> = None; // nearest small rock — last resort only
                for (fe, ft, fa) in &free {
                    let fp = ft.translation.truncate();
                    // only grab rocks that are ON-SCREEN and in the TOP half (where it lives) —
                    // no cross-screen yanks and nothing dragged in from off the edges
                    if fp.y <= 0.0 || fp.y >= h.y || fp.x.abs() >= h.x {
                        continue;
                    }
                    let d = fp.distance_squared(bp);
                    if fa.size >= 2 {
                        // biggest first, nearest to break ties → a shield of large rocks
                        if best.is_none_or(|(_, bs, bd)| fa.size > bs || (fa.size == bs && d < bd)) {
                            best = Some((fe, fa.size, d));
                        }
                    } else if small.is_none_or(|(_, sd)| d < sd) {
                        small = Some((fe, d));
                    }
                }
                // grab a large/mid rock if one's reachable; only fall back to small debris when
                // nothing bigger is on-screen up top (the Warden rarely bothers with little rocks)
                if let Some(fe) = best.map(|(fe, _, _)| fe).or(small.map(|(fe, _)| fe)) {
                    let slot = (0..BOSS_ARMS).find(|s| !used[*s]).unwrap_or(0);
                    commands.entity(fe).insert(Shielded { slot, grab: 0.0 });
                }
            }
        }
    }
}

// Free asteroids bounce off the boss's held shield rocks (treated as immovable), so
// they clatter around the shield instead of drifting straight through it.
fn shield_deflect(
    mut free: Query<(&mut Transform, &mut Velocity, &Asteroid), (Without<Shielded>, Without<Boss>)>,
    held: Query<(&Transform, &Asteroid), With<Shielded>>,
    bosses: Query<&Transform, With<Boss>>,
) {
    let boss_pos = bosses.iter().next().map(|t| t.translation.truncate());
    for (mut ft, mut fv, fa) in &mut free {
        let fr = asteroid_radius(fa.size);
        let mut fp = ft.translation.truncate();
        let mut hit = false;
        for (ht, ha) in &held {
            let hp = ht.translation.truncate();
            let delta = fp - hp;
            let d = delta.length();
            let min = fr + asteroid_radius(ha.size);
            if d < min && d > 0.01 {
                // Eject OUTWARD from the boss centre (not just away from the rock) so a
                // free rock can never get trapped inside the spinning shield ring — it's
                // always pushed out through it. Falls back to away-from-rock if needed.
                let n = match boss_pos {
                    Some(bp) if fp.distance(bp) > 1.0 => (fp - bp).normalize(),
                    _ => delta / d,
                };
                fp += n * (min - d);
                let vn = fv.0.dot(n);
                if vn < 0.0 {
                    fv.0 -= n * (2.0 * vn); // reflect so it heads back outward
                }
                hit = true;
            }
        }
        if hit {
            ft.translation.x = fp.x;
            ft.translation.y = fp.y;
        }
    }
}

// ─────────────────────────────── chain shot (secondary weapon) ────────
// Right-click fires a wide lightning BEAM — 3 charges that regenerate on a timer.
// Unlocked by the post-boss pickup. (Shift is the warp; primary fire is Space/LMB.)
fn chain_fire(
    time: Res<Time>,
    input: Res<ActionState>,
    mut commands: Commands,
    mut chain: ResMut<Chain>,
    mut run: ResMut<Run>,
    ships: Query<(&Ship, &Transform, &Velocity)>,
) {
    if !chain.unlocked {
        return;
    }
    let dt = time.delta_secs();
    if chain.cooldown > 0.0 {
        chain.cooldown -= dt;
    }
    if chain.charges < CHAIN_MAX_CHARGES {
        chain.recharge -= dt;
        if chain.recharge <= 0.0 {
            chain.charges += 1;
            chain.recharge = CHAIN_RECHARGE;
        }
    } else {
        chain.recharge = CHAIN_RECHARGE; // primed for the next spend
    }
    if !input.chain || chain.charges <= 0 || chain.cooldown > 0.0 {
        return;
    }
    if let Some((ship, t, sv)) = ships.iter().next() {
        let dir = Vec2::from_angle(ship.angle);
        let pos = t.translation.truncate() + dir * SHIP_R;
        commands.spawn((
            ChainShot { life: CHAIN_LIFE, perp: Vec2::new(-dir.y, dir.x) },
            Velocity(dir * CHAIN_SPEED + sv.0 * 0.3),
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ));
        chain.charges -= 1;
        chain.cooldown = CHAIN_COOLDOWN;
        run.powerup_fires += 1; // the beam is a powerup — firing it ends a Pacifist streak
    }
}

// The beam travels (via `integrate`); here it expires and mows through everything its
// segment (centre ± perp·half) touches — rocks, enemies, mines, and the boss core.
fn chain_update(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut score: ResMut<Score>,
    mut sfx: EventWriter<SoundFx>,
    mut stats: ResMut<Stats>,
    mut chains: Query<(Entity, &Transform, &mut ChainShot)>,
    asteroids: Query<(Entity, &Transform, &Asteroid, Option<&Gold>, Option<&Explosive>, Option<&Pulser>, Option<&Red>, Option<&Cluster>, Option<&Beacon>, Option<&Hunter>, Option<&Lapse>, Option<&Facet>, Option<&Husk>), (Without<Mine>, Without<Shielded>)>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    mines: Query<(Entity, &Transform), With<Mine>>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let mut dead: HashSet<Entity> = HashSet::new(); // consumed this frame
    for (ce, ct, mut cs) in &mut chains {
        cs.life -= dt;
        let c = ct.translation.truncate();
        if cs.life <= 0.0 || c.x.abs() > h.x + CHAIN_HALF || c.y.abs() > h.y + CHAIN_HALF {
            commands.entity(ce).despawn();
            continue;
        }
        let a = c + cs.perp * CHAIN_HALF;
        let b = c - cs.perp * CHAIN_HALF;
        // beacon auras block the beam too — collect the zones once per beam sweep
        let beacon_zones: Vec<Vec2> = asteroids
            .iter()
            .filter(|(.., beacon, _, _, _, _)| beacon.is_some())
            .map(|(_, at, ..)| at.translation.truncate())
            .collect();
        for (ae, at, ast, gold, explosive, pulser, red, cluster, beacon, hunter, lapse, facet, husk) in &asteroids {
            if dead.contains(&ae) {
                continue;
            }
            let ap = at.translation.truncate();
            let rr = asteroid_radius(ast.size) + CHAIN_R;
            if seg_dist2(ap, a, b) < rr * rr {
                // an absent / materializing lapse rock isn't there for the beam to shear
                if lapse.is_some_and(|l| !l.tangible()) {
                    continue;
                }
                // a LIT pulser is invulnerable — the beam passes over it (don't mark it dead)
                if pulser.is_some_and(|pl| pulser_lit(pl.offset, time.elapsed_secs())) {
                    continue;
                }
                // aura-shielded (and not itself a beacon) → the beam washes over it
                if beacon.is_none() && beacon_zones.iter().any(|z| z.distance_squared(ap) < BEACON_AURA_R * BEACON_AURA_R) {
                    continue;
                }
                dead.insert(ae);
                if explosive.is_some() {
                    commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE, friendly: false }); // the beam lights the orange (hostile blast)
                    stats.orange += 1; // your beam lit it — the kill is yours
                    continue;
                }
                // chain beam shears dense rocks outright — the beam ignores hp, like a mine (a BEACON dies
                // in one sweep: the beam is a clean answer to an aura)
                break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, ast.size, 1.0, flavor(ast.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk), true); // chain shears rocks; a red splits into reds (they stay red + regrow)
                sfx.write(SoundFx::Break(ast.size));
                credit_rock_kill(&mut stats, flavor(ast.dense, gold, pulser, red, cluster, beacon, hunter, lapse, facet, husk));
            }
        }
        for (ee, et) in &enemies {
            if dead.contains(&ee) {
                continue;
            }
            let ep = et.translation.truncate();
            let rr = ENEMY_R + CHAIN_R;
            if seg_dist2(ep, a, b) < rr * rr {
                dead.insert(ee);
                kill_enemy(&mut commands, &mut score, &mut sfx, ee, ep, &mut rng);
                stats.enemies += 1;
            }
        }
        for (me, mt) in &mines {
            if dead.contains(&me) {
                continue;
            }
            let mp = mt.translation.truncate();
            let rr = MINE_R + CHAIN_R;
            if seg_dist2(mp, a, b) < rr * rr {
                dead.insert(me);
                commands.entity(me).despawn();
                score.0 += MINE_SCORE;
                stats.mines += 1; // the chain beam sheared it
                burst(&mut commands, mp, mine_color(), 20, 320.0, &mut rng);
                sfx.write(SoundFx::Mine);
            }
        }
        // NOTE: the beam deliberately does NOT damage the boss core. Bosses are beaten
        // through their asteroid mechanic (the chain's job here is clearing the rocks),
        // so it can't be used to brute-force the core. See [[neon-edge-difficulty]].
    }
}

// The reward orb (post-boss calm): drifts + bounces on screen; fly into it to unlock
// the chain shot, or leave it (hardcore). It leaves when the calm window closes.
fn pickup_update(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut chain: ResMut<Chain>,
    mut mass: ResMut<MassShot>,
    mut warhead: Option<ResMut<Warhead>>, // Option so headless tests needn't insert it
    mut gorge: Option<ResMut<Gorge>>,     // (same)
    mut run: Option<ResMut<Run>>,         // (same) — carries the Nova Shield state
    mut flags: ResMut<RunFlags>,
    ships: Query<&Transform, With<Ship>>,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
    mut pickups: Query<(Entity, &Transform, &mut Velocity, &mut Pickup)>,
    drones: Query<(), With<Drone>>,
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (pe, pt, mut pv, mut pk) in &mut pickups {
        pk.life -= dt;
        if pk.life <= 0.0 {
            commands.entity(pe).despawn(); // window elapsed → gone for good (a single offer)
            continue;
        }
        pk.rot += dt * 1.8;
        pk.pulse += dt * 5.0;
        let p = pt.translation.truncate(); // integrate moves it; we just bounce + check grab
        if (p.x < -h.x + 60.0 && pv.0.x < 0.0) || (p.x > h.x - 60.0 && pv.0.x > 0.0) {
            pv.0.x = -pv.0.x;
        }
        if (p.y < -h.y + 60.0 && pv.0.y < 0.0) || (p.y > h.y - 60.0 && pv.0.y > 0.0) {
            pv.0.y = -pv.0.y;
        }
        // collect it by flying into it OR by shooting it
        let mut collected = ship.is_some_and(|sp| p.distance(sp) < PICKUP_R + SHIP_R);
        for (be, bt) in &bullets {
            if p.distance(bt.translation.truncate()) < PICKUP_R + BULLET_R {
                collected = true;
                commands.entity(be).despawn(); // the shot is spent grabbing it
                break;
            }
        }
        if collected {
            flags.powerup_used = true; // used a powerup this run → blocks the Purist achievement
            let col = match pk.kind {
                PickupKind::Chain => {
                    chain.unlocked = true;
                    chain.charges = CHAIN_MAX_CHARGES;
                    chain.recharge = CHAIN_RECHARGE;
                    chain_color()
                }
                PickupKind::Mass => {
                    mass.unlocked = true;
                    mass.active = true; // switch to the mass shot on grab (Q cycles among unlocked modes)
                    if let Some(w) = warhead.as_mut() {
                        w.active = false;
                    }
                    mass_color()
                }
                PickupKind::Drone => {
                    // spawn the ally drone once — it starts at the ship (or the orb) and follows from there
                    if drones.is_empty() {
                        let at = ship.unwrap_or(p);
                        commands.spawn((Drone { fire: DRONE_FIRE_EVERY, angle: 0.0 }, Transform::from_xyz(at.x, at.y, 0.0)));
                    }
                    drone_color()
                }
                PickupKind::Warhead => {
                    if let Some(w) = warhead.as_mut() {
                        w.unlocked = true;
                        w.active = true; // switch to the Warhead round on grab (Q cycles among unlocked modes)
                    }
                    mass.active = false;
                    warhead_color()
                }
                PickupKind::Nova => {
                    if let Some(r) = run.as_mut() {
                        r.nova = Nova { unlocked: true, down: 0.0, grace: 0.0 }; // raised, and UP right away
                    }
                    nova_color()
                }
                PickupKind::Gorge => {
                    if let Some(g) = gorge.as_mut() {
                        g.unlocked = true;
                        g.active = true; // handed over ready to fire
                    }
                    mass.active = false;
                    if let Some(w) = warhead.as_mut() {
                        w.active = false;
                    }
                    devourer_color()
                }
                PickupKind::Aegis => {
                    if let Some(r) = run.as_mut() {
                        // arrives at FULL strength; from here it's spend-and-regrow
                        r.aegis = Aegis { unlocked: true, shards: AEGIS_SHARDS, regen: AEGIS_REGEN, spin: 0.0 };
                    }
                    ship_color() // the granted kit is player-violet, like every other effect
                }
            };
            burst(&mut commands, p, col, 30, 300.0, &mut rng);
            commands.entity(pe).despawn();
        }
    }
}

// The ally Drone: orbits the ship a short distance out and auto-fires the player's Bullet at the
// nearest asteroid in range — mopping up rocks left behind. Its position is driven directly (it
// doesn't need physics); if the ship is gone (mid-respawn) it just idles in place.
fn drone_update(
    time: Res<Time>,
    mut commands: Commands,
    ships: Query<&Transform, (With<Ship>, Without<Drone>)>,
    rocks: Query<&Transform, (With<Asteroid>, Without<Drone>)>,
    // NB: the Phantom (the Haunt) is deliberately EXCLUDED — the drone must not auto-fire at an intangible
    // ghost (wasted shots) nor reveal the real one among the phase-2 decoys. The finale is the player's fight.
    bosses: Query<&Transform, (Or<(With<Boss>, With<Devourer>, With<Slinger>, With<Detonator>, With<Pulsar>)>, Without<Drone>)>,
    mut drones: Query<(&mut Transform, &mut Drone)>,
) {
    let dt = time.delta_secs();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (mut dtf, mut dr) in &mut drones {
        let dc = dtf.translation.truncate();
        dr.angle += dt * DRONE_ORBIT_RATE;
        // follow: ease toward an orbit point a short distance from the ship (a trailing wingman)
        if let Some(sp) = ship {
            let target = sp + Vec2::from_angle(dr.angle) * DRONE_FOLLOW_DIST;
            let k = (DRONE_FOLLOW_GAIN * dt).min(1.0);
            dtf.translation.x += (target.x - dc.x) * k;
            dtf.translation.y += (target.y - dc.y) * k;
        }
        // fire at the nearest asteroid OR boss in range on a cooldown (it helps against bosses too)
        dr.fire -= dt;
        if dr.fire <= 0.0 {
            let mut best: Option<(Vec2, f32)> = None;
            for rt in rocks.iter().chain(bosses.iter()) {
                let rp = rt.translation.truncate();
                let d = rp.distance_squared(dc);
                if d < DRONE_RANGE * DRONE_RANGE && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((rp, d));
                }
            }
            if let Some((rp, _)) = best {
                let dir = (rp - dc).normalize_or_zero();
                if dir != Vec2::ZERO {
                    commands.spawn((
                        Bullet { life: BULLET_LIFE, trail: Vec::new(), mass: false },
                        Velocity(dir * BULLET_SPEED),
                        Transform::from_xyz(dc.x, dc.y, 0.0),
                    ));
                    dr.fire = DRONE_FIRE_EVERY;
                }
            }
        }
    }
}

// The black hole drags every nearby asteroid, enemy AND mine inward and consumes
// those that reach its core (the ship is immune — not in these queries).
fn black_hole_update(
    mut commands: Commands,
    time: Res<Time>,
    mut score: ResMut<Score>,
    mut stats: Option<ResMut<Stats>>, // Option so headless tests needn't insert it

    mut holes: Query<(Entity, &Transform, &mut BlackHole)>,
    // boss-HELD rocks (Shielded) are exempt — you can't warp a boss's shield away; the
    // boss itself carries neither Asteroid nor Enemy, so it's exempt automatically.
    mut asteroids: Query<(Entity, &Transform, &mut Velocity, &Asteroid), Without<Shielded>>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity), (With<Enemy>, Without<Asteroid>)>,
    mut mines: Query<(Entity, &Transform, &mut Velocity), (With<Mine>, Without<Asteroid>, Without<Enemy>)>,
) {
    let dt = time.delta_secs();
    let pull_r = WARP_PULL_RADIUS;
    let mut rng = rand::thread_rng();
    for (he, ht, mut hole) in &mut holes {
        hole.spin += dt * 3.5;
        hole.life -= dt;
        let hp = ht.translation.truncate();
        // consume on EDGE contact (center within horizon + own radius) so a big rock is
        // eaten the instant it touches the hole — it never survives to clump at the mouth.
        for (ae, at, mut av, a) in &mut asteroids {
            let ap = at.translation.truncate();
            if ap.distance(hp) < WARP_CONSUME_R + asteroid_radius(a.size) {
                score.0 += WARP_ROCK_SCORE;
                burst(&mut commands, ap, rock_color(), 14, 280.0, &mut rng);
                commands.entity(ae).despawn();
            } else {
                av.0 += warp_pull(ap, hp, pull_r, dt);
            }
        }
        // enemies get sucked in and consumed just like rocks
        for (ee, et, mut ev) in &mut enemies {
            let ep = et.translation.truncate();
            if ep.distance(hp) < WARP_CONSUME_R + ENEMY_R {
                score.0 += ENEMY_SCORE;
                burst(&mut commands, ep, enemy_color(), 18, 300.0, &mut rng);
                commands.entity(ee).despawn();
            } else {
                ev.0 += warp_pull(ep, hp, pull_r, dt);
            }
        }
        // mines are dragged in and consumed too
        for (me, mt, mut mv) in &mut mines {
            let mp = mt.translation.truncate();
            if mp.distance(hp) < WARP_CONSUME_R + MINE_R {
                score.0 += MINE_SCORE;
                if let Some(s) = stats.as_mut() {
                    s.mines += 1; // your warp swallowed it — player-credited
                }
                burst(&mut commands, mp, mine_color(), 16, 300.0, &mut rng);
                commands.entity(me).despawn();
            } else {
                mv.0 += warp_pull(mp, hp, pull_r, dt);
            }
        }
        // suck-in sparks streaking toward the core (extra juice)
        for _ in 0..2 {
            let a = rng.gen_range(0.0..TAU);
            let sp = hp + Vec2::from_angle(a) * rng.gen_range(70.0..150.0);
            let inv = (hp - sp).normalize_or_zero() * rng.gen_range(220.0..360.0);
            commands.spawn((
                Particle { vel: inv, life: 0.3, ttl: 0.3, color: warp_color() },
                Transform::from_xyz(sp.x, sp.y, 0.0),
            ));
        }
        if hole.life <= 0.0 {
            commands.entity(he).despawn();
        }
    }
}

// Drive the grid warp: ease toward 1 while a hole is open (pulling the grid in),
// then rubber-snap 1→0 (overshooting negative) once it closes.
fn update_warp_field(time: Res<Time>, mut wf: ResMut<WarpField>, holes: Query<&Transform, With<BlackHole>>) {
    let dt = time.delta_secs();
    if let Some(ht) = holes.iter().next() {
        wf.pos = ht.translation.truncate();
        wf.active = true;
        wf.snap_t = 0.0;
        wf.amount = (wf.amount + dt * 3.5).min(1.0);
    } else {
        if wf.active {
            wf.active = false;
            wf.snap_t = 0.0;
        }
        if wf.amount != 0.0 {
            wf.snap_t += dt;
            let k = (wf.snap_t / WARP_SNAP_DUR).min(1.0);
            wf.amount = 1.0 - ease_out_elastic(k);
            if k >= 1.0 {
                wf.amount = 0.0;
            }
        }
    }
}

// ─────────────────────────────── always-on systems ────────────────────
fn update_arena(mut arena: ResMut<Arena>, windows: Query<&Window>) {
    if let Some(w) = windows.iter().next() {
        // The camera scale-to-fits DESIGN_H world-units to the window HEIGHT (see `setup`), so the visible
        // half-height is always DESIGN_HALF_H and the half-width follows the window's aspect. Sizing the arena
        // to match makes the edge exactly the visible bound, at a consistent scale on every monitor.
        let aspect = if w.height() > 0.0 { w.width() / w.height() } else { 1.6 };
        arena.half = Vec2::new(DESIGN_HALF_H * aspect, DESIGN_HALF_H);
    }
}

// Scale the UI (HUD + menus) with the window so text/pips track the world's scale-to-fit — a window
// DESIGN_H tall = scale 1.0 (the laptop baseline), bigger monitors scale up so nothing looks tiny.
fn update_ui_scale(mut ui: ResMut<UiScale>, windows: Query<&Window>) {
    if let Some(w) = windows.iter().next() {
        ui.0 = (w.height() / DESIGN_H).max(0.1);
    }
}

fn update_wave_text(wave: Res<Wave>, plus: Res<NewGamePlus>, mut q: Query<&mut Text, With<WaveText>>) {
    let secs = wave.timer.max(0.0) as i32;
    let tag = if plus.0 { "NG+  " } else { "" }; // the second lap says so, quietly, all run long
    let name_plus = if plus.0 { "+" } else { "" }; // …and its bosses carry the mark: THE WARDEN+
    for mut t in &mut q {
        t.0 = if is_boss_wave(wave.level) {
            format!("{tag}WAVE {}    {}{name_plus}", wave.level, boss_kind_name(boss_kind(wave.level)))
        } else {
            format!("{tag}WAVE {}    {}:{:02}", wave.level, secs / 60, secs % 60)
        };
    }
}

fn update_score_text(score: Res<Score>, mut q: Query<&mut Text, With<ScoreText>>) {
    for mut t in &mut q {
        t.0 = format!("SCORE {}", score.0);
    }
}

// The big "WAVE n" flash: quick fade-in, hold, then fade out over WAVE_BANNER_FADE.
fn wave_banner_update(
    time: Res<Time>,
    wave: Res<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut q: Query<(&mut Text, &mut TextColor), With<WaveBannerText>>,
) {
    // During the post-boss calm the "NEXT WAVE IN n" countdown owns the screen — hold the WAVE banner
    // hidden and FROZEN (don't tick it down) so it flashes only when the calm ends and the wave begins.
    if wave.calm > 0.0 {
        for (_, mut color) in &mut q {
            color.0 = color.0.with_alpha(0.0);
        }
        return;
    }
    if banner.timer > 0.0 {
        banner.timer -= time.delta_secs();
    }
    let t = banner.timer.max(0.0);
    let fade_in = ((WAVE_BANNER_SECS - t) / 0.2).clamp(0.0, 1.0);
    let fade_out = (t / WAVE_BANNER_FADE).clamp(0.0, 1.0);
    let alpha = fade_in.min(fade_out);
    for (mut text, mut color) in &mut q {
        if alpha > 0.0 {
            text.0 = format!("WAVE {}", wave.level);
        }
        color.0 = color.0.with_alpha(alpha);
    }
}

// The post-boss calm's visual countdown: "NEXT WAVE IN n", ticking while wave.calm > 0 (the next
// wave's number is already in wave.level). Pulses brighter within each second so it reads as a ticking
// clock. Only bosses set `calm`, so this only appears in the 10s lull after a boss.
fn calm_countdown_update(wave: Res<Wave>, mut q: Query<(&mut Text, &mut TextColor), With<CalmCountdownText>>) {
    for (mut text, mut color) in &mut q {
        // wave 30 is the FINALE — there is no next wave, so never show the countdown there (its `calm` is
        // set only to pause the field for the death scene, not to herald a next wave)
        if wave.calm > 0.0 && content_wave(wave.level) != 30 {
            let secs = wave.calm.ceil().max(1.0) as i32;
            text.0 = format!("NEXT WAVE IN {secs}");
            let pulse = 0.7 + 0.3 * wave.calm.fract(); // brightest just after each tick, easing down
            color.0 = Color::srgb(0.72, 0.85, 1.15).with_alpha(pulse);
        } else {
            color.0 = color.0.with_alpha(0.0);
        }
    }
}

// The boss run-up warning: while a boss is imminent (`boss_incoming`), name it and pulse a full-screen
// tint in its colour — a louder telegraph than the faint background cameo alone. Intensity rises as the
// wave nears (`prog`); the name eases in and stays readable while the flash strobes 0→peak.
fn boss_warning_update(
    wave: Res<Wave>,
    plus: Res<NewGamePlus>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<BossWarnText>>,
    mut flash_q: Query<&mut BackgroundColor, With<BossWarnFlash>>,
) {
    let on = boss_incoming(&wave);
    let kind = boss_kind(wave.level + 1);
    let col = boss_kind_color(kind);
    let prog = if on { ((BOSS_CAMEO_SECS - wave.timer) / BOSS_CAMEO_SECS).clamp(0.0, 1.0) } else { 0.0 };
    // wave.timer drives the pulse phase — it counts BOSS_CAMEO_SECS→0 across the run-up. A SLOW, gentle
    // breath (~0.7 Hz) that never fully vanishes (0.2..1.0) — this is a full-screen tint, so it must not
    // strobe or fully flash on/off (photosensitivity).
    let pulse = 0.6 + 0.4 * (wave.timer * 4.5).sin();
    let fade_in = (prog / 0.04).clamp(0.0, 1.0); // ease the name in over the first ~0.4s
    for (mut text, mut color) in &mut text_q {
        if on {
            // in NG+ every boss carries the mark — THE WARDEN+ is a different fight and says so
            let name_plus = if plus.0 { "+" } else { "" };
            text.0 = format!("WARNING:  {}{} INCOMING", boss_kind_name(kind), name_plus);
        }
        // dim() tones the HDR boss colour into UI range (else it clamps to white); readable, gentle pulse
        let a = if on { fade_in * (0.78 + 0.22 * pulse) } else { 0.0 };
        color.0 = dim(col, 0.28).with_alpha(a);
    }
    for mut bg in &mut flash_q {
        let a = if on { (0.05 + 0.11 * prog) * pulse } else { 0.0 };
        bg.0 = dim(col, 0.16).with_alpha(a);
    }
}

// Tick the HUD flash timers (pips/lives) set at their events.
fn hud_flash_tick(time: Res<Time>, mut flash: ResMut<HudFlash>) {
    let dt = time.delta_secs();
    flash.pips = (flash.pips - dt).max(0.0);
    flash.life = (flash.life - dt).max(0.0);
}

// The "MASS SHOT / STANDARD SHOT" label: shown on a toggle, held, then fades over its last stretch.
// Reveal each ability-strip label as its ability is earned (and hide the lot off-run). The strip is
// an ACTUAL HUD — every light on it is named — so a fresh run shows just WARP, and CHAIN / MODE /
// SHIELD / DRONE announce themselves as their pickups land.
fn hud_ability_labels(
    state: Res<State<GameState>>,
    chain: Res<Chain>,
    mass: Res<MassShot>,
    warhead: Res<Warhead>,
    gorge: Res<Gorge>,
    run: Res<Run>,
    drones: Query<(), With<Drone>>,
    mut labels: Query<(&mut Visibility, &AbilitySlot)>,
) {
    let on = run_active(state.get());
    for (mut vis, slot) in &mut labels {
        let show = on
            && match slot {
                AbilitySlot::Warp => true,
                AbilitySlot::Chain => chain.unlocked,
                AbilitySlot::Mode => mass.unlocked || warhead.unlocked || gorge.unlocked,
                AbilitySlot::Shield => run.nova.unlocked,
                AbilitySlot::Drone => !drones.is_empty(),
            };
        *vis = if show { Visibility::Visible } else { Visibility::Hidden };
    }
}

fn shot_mode_update(time: Res<Time>, mut flash: ResMut<ShotModeFlash>, mass: Res<MassShot>, warhead: Res<Warhead>, gorge: Res<Gorge>, mut q: Query<(&mut Text, &mut TextColor), With<ShotModeText>>) {
    if flash.0 > 0.0 {
        flash.0 -= time.delta_secs();
    }
    // Persistent once any shot mode is unlocked (there's a real choice then): a dim baseline that reads at
    // a glance, flaring bright right after a cycle. Hidden before any unlock. Colour-coded per mode.
    let unlocked = mass.unlocked || warhead.unlocked || gorge.unlocked;
    let base: f32 = if unlocked { 0.5 } else { 0.0 };
    let alpha = base.max((flash.0 / 0.3).clamp(0.0, 1.0));
    // short names — the strip's MODE label already says what the slot is
    let (label, rgb) = if gorge.unlocked && gorge.active {
        ("GORGE", Color::srgb(1.0, 0.42, 0.38)) // the Glutton's red — the boss it was taken from
    } else if warhead.unlocked && warhead.active {
        ("WARHEAD", Color::srgb(0.9, 0.45, 1.0)) // violet
    } else if mass.unlocked && mass.active {
        ("MASS", Color::srgb(0.72, 0.28, 1.0)) // violet
    } else {
        ("STANDARD", Color::srgb(0.58, 0.72, 0.9)) // cool steel
    };
    for (mut text, mut color) in &mut q {
        if unlocked {
            text.0 = label.to_string();
        }
        color.0 = rgb.with_alpha(alpha);
    }
}

fn render(
    mut gizmos: Gizmos,
    time: Res<Time>,
    arena: Res<Arena>,
    run: Res<Run>,
    dev: Res<Dev>,
    // warp + chain + state + hud-flash + shot modes grouped into one tuple param to stay within Bevy's 16-param limit
    abilities: (Res<Warp>, Res<Chain>, Res<State<GameState>>, Res<HudFlash>, Res<MassShot>, Res<Warhead>, Res<Gorge>),
    wf: Res<WarpField>,
    stars: Query<(&Star, &Transform)>,
    ships: Query<(&Ship, &Transform, Option<&ShipTrail>)>,
    asteroids: Query<(&Asteroid, &Transform, Option<&Gold>, Option<&Explosive>, Option<&Detonating>, Option<&Pulser>, Option<&Red>, Option<&Cluster>, Option<&Beacon>, Option<&Hunter>, Option<&Lapse>, Option<&Facet>, Option<&Husk>)>,
    bullets: Query<(&Bullet, &Transform, Has<WarheadShot>, &Velocity, Option<&GorgeShot>)>,
    particles: Query<(&Particle, &Transform)>,
    holes: Query<(&BlackHole, &Transform)>,
    missiles: Query<&Transform, With<WarpMissile>>,
    mines_q: Query<(&Mine, &Transform)>,
    // grouped into one tuple param to stay within Bevy's 16-param system limit (+ the drone, for its HUD icon)
    foes: (Query<(&Enemy, &Transform)>, Query<&Transform, With<EnemyBullet>>, Query<(), With<Drone>>),
) {
    let h = arena.half;
    let t = time.elapsed_secs();
    let (warp_res, chain) = (&abilities.0, &abilities.1);
    let show_run = run_active(abilities.2.get()); // grid + HUD icons only while a run is on
    let hud_flash = &abilities.3;
    let (mass, warhead, gorge) = (&abilities.4, &abilities.5, &abilities.6); // shot modes — drive the HUD's Q-slot
    let nova = &run.nova; // the Nova Shield's state (ship bubble + HUD icon)
    let aegis = &run.aegis; // the Aegis Shards' state (the orbiting chips drawn around the hull)
    let has_drone = !foes.2.is_empty();
    // a rapid bright shimmer applied to pips/lives right after they refill / a life is gained
    let flick = |active: bool| if active { 1.1 + 0.8 * (t * 18.0).sin() } else { 1.0 }; // ~2.9 Hz (was ~6.4) — no strobe

    // stars (backmost). Subtle during a run so they never distract; on the menu they're a feature —
    // bigger, brighter, with a soft glow and diagonal sparkle rays on the brightest ones.
    let star = star_color();
    let menu = !show_run;
    for (s, st) in &stars {
        let tw = 0.35 + 0.65 * (t * 1.6 + s.phase).sin().max(0.0);
        let c = st.translation.truncate() * h; // star pos is NORMALIZED [-1,1] → scale to the live arena so the field fills any screen
        let bright = s.bright * tw * if menu { 2.0 } else { 1.0 };
        let col = dim(star, bright);
        let arm = if menu { 2.3 } else { 1.3 };
        gizmos.line_2d(c - Vec2::X * arm, c + Vec2::X * arm, col);
        gizmos.line_2d(c - Vec2::Y * arm, c + Vec2::Y * arm, col);
        if menu {
            // a soft core dot the bloom smears into a twinkle
            gizmos.circle_2d(Isometry2d::from_translation(c), 0.8 + 0.7 * tw, dim(star, bright * 0.6));
            // the brightest stars get diagonal sparkle rays
            if s.bright > 0.8 {
                let d = arm * 0.62;
                gizmos.line_2d(c - Vec2::new(d, d), c + Vec2::new(d, d), dim(star, bright * 0.55));
                gizmos.line_2d(c - Vec2::new(d, -d), c + Vec2::new(d, -d), dim(star, bright * 0.55));
            }
        }
    }

    // grid — faint, brighter per-line shimmer; bends toward an active warp hole (and rubber-snaps
    // back). Only while a run is on — off-run the color is zeroed so the menu shows no grid.
    let warping = wf.amount.abs() > 0.001;
    let wamt = wf.amount.abs().clamp(0.0, 1.0); // warp-field envelope: 0 → 1 as the hole opens, eases back (elastic bounce) on snapback
    // while a warp bends the grid it brightens just a touch — NO purple tint (that blew the whole
    // screen out); the drama lives in the flicker below, which crackles hardest as the field collapses.
    let grid = if show_run {
        if warping { dim(grid_color(), 1.0 + 2.3 * wamt) } else { grid_color() }
    } else {
        dim(grid_color(), 0.0)
    };
    // per-line electric flicker: two out-of-phase crackles, scaled by the field. The elastic snapback makes
    // `wamt` bounce, so the lines crackle as the hole collapses. Both rates kept ≤~2.9 Hz — the grid is a
    // large-area element, so it must never strobe (photosensitivity).
    let warp_flick = |k: f32| {
        if warping {
            let amp = 0.6 + 0.7 * wamt; // a bolder crackle (bumped so the post-warp flicker really reads) — AMPLITUDE only; the ≤2.9 Hz rates below are untouched (photosensitivity)
            (1.0 + amp * (0.7 * (t * 14.0 + k * 2.1).sin() + 0.5 * (t * 18.0 + k * 3.7).sin())).max(0.1)
        } else {
            1.0
        }
    };
    const SUBDIV: usize = 14;
    // OFF-RUN the grid is invisible (its colour dims to black) — so SKIP it entirely rather than
    // stroking black lines across the screen. Those strokes were painting over the GALLERY's artwork
    // (this system runs in PostUpdate, after the gallery's art layer) and reading as black gashes
    // through the rocks. Drawing nothing is both correct and cheaper.
    let mut i = 0;
    let mut x = if show_run { -(h.x / GRID_CELL).floor() * GRID_CELL } else { h.x + 1.0 };
    while x <= h.x {
        let sh = 0.5 + GRID_SHIMMER_AMP * (0.5 + 0.5 * (i as f32 * 0.7 + t * 1.2).sin());
        let col = dim(grid, sh * warp_flick(i as f32));
        if warping {
            let pts: Vec<Vec2> = (0..=SUBDIV)
                .map(|s| warp_point(Vec2::new(x, -h.y + 2.0 * h.y * (s as f32 / SUBDIV as f32)), &wf))
                .collect();
            gizmos.linestrip_2d(pts, col);
        } else {
            gizmos.line_2d(Vec2::new(x, -h.y), Vec2::new(x, h.y), col);
        }
        x += GRID_CELL;
        i += 1;
    }
    let mut j = 0;
    let mut y = if show_run { -(h.y / GRID_CELL).floor() * GRID_CELL } else { h.y + 1.0 };
    while y <= h.y {
        let sh = 0.5 + GRID_SHIMMER_AMP * (0.5 + 0.5 * (j as f32 * 0.7 + t * 1.2 + 1.7).sin());
        let col = dim(grid, sh * warp_flick(j as f32 + 5.0));
        if warping {
            let pts: Vec<Vec2> = (0..=SUBDIV)
                .map(|s| warp_point(Vec2::new(-h.x + 2.0 * h.x * (s as f32 / SUBDIV as f32), y), &wf))
                .collect();
            gizmos.linestrip_2d(pts, col);
        } else {
            gizmos.line_2d(Vec2::new(-h.x, y), Vec2::new(h.x, y), col);
        }
        y += GRID_CELL;
        j += 1;
    }

    // asteroids — dense (green) rocks carry a concentric inner ring that shrinks as
    // they're chipped, so their tanky state reads at a glance.
    let rock = rock_color();
    let dense = dense_color();
    for (a, at, gold, explosive, det, pulser, red, cluster, beacon, hunter, lapse, facet, husk) in &asteroids {
        let c = at.translation.truncate();
        let rot = Vec2::from_angle(a.rot);
        // colour by type: a lit orange flashes white-hot as its fuse burns; a live orange pulses; gold
        // shimmers; a pulser is bright-white when LIT (invulnerable) / dim when DARK; a beacon is teal
        // (aura warden); a cluster is fractured ice; green=dense; blue=standard.
        let lit = pulser.map(|pl| pulser_lit(pl.offset, t));
        let col = if let Some(d) = det {
            let f = 1.0 - (d.fuse / ORANGE_FUSE).clamp(0.0, 1.0); // ramps up as it's about to blow
            dim(Color::srgb(8.0, 1.5, 1.0), 0.7 + 0.9 * f) // hot RED→white (a live bomb) — deliberately NOT gold, so it never reads like the 1UP
        } else if explosive.is_some() {
            dim(orange_color(), 0.75 + 0.25 * (t * 5.0).sin())
        } else if gold.is_some() {
            dim(gold_color(), 0.7 + 0.3 * (t * 6.0).sin())
        } else if let Some(lit) = lit {
            // bright white shield when lit, dim steel-blue when it's open to fire
            if lit { Color::srgb(6.0, 6.2, 7.0) } else { Color::srgb(0.9, 1.1, 1.7) }
        } else if red.is_some() {
            dim(red_color(), 0.7 + 0.3 * (t * 4.0).sin()) // a slow, menacing throb — it's alive and hungry
        } else if beacon.is_some() {
            dim(beacon_color(), 0.8 + 0.2 * (t * 2.0).sin()) // steady teal breathe — the aura's living key
        } else if husk.is_some() {
            husk_color()
        } else if facet.is_some() {
            facet_color()
        } else if let Some(l) = lapse {
            // presence IS the readout: full when solid, guttering out as it dissolves, a faint ghost
            // as it comes back. Smooth ramps only — no blinking (photosensitivity rule).
            lapse_glow(l) // neon strike on the way in, smooth dim on the way out, nothing while gone
        } else if let Some(h) = hunter {
            dim(hunter_color(), 0.62 + 0.38 * h.charge) // dull when it spawns, burning once it's locked on
        } else if cluster.is_some() {
            cluster_color()
        } else if a.dense {
            dense
        } else {
            rock
        };
        let ring = |scale: f32| {
            let mut pts: Vec<Vec2> = a.verts.iter().map(|v| c + rot.rotate(*v * scale)).collect();
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            pts
        };
        gizmos.linestrip_2d(ring(1.0), col);
        if beacon.is_some() {
            // the AURA: a soft reach circle (what it's protecting) + the dense hp core ring. Slightly
            // brighter than the old faint ring — a zone this size must read as a boundary, not a smudge.
            gizmos.circle_2d(Isometry2d::from_translation(c), BEACON_AURA_R, dim(col, 0.22 + 0.06 * (t * 2.0).sin()));
            let frac = a.hp.max(1) as f32 / a.size.max(1) as f32;
            gizmos.linestrip_2d(ring(0.35 + 0.3 * frac), col);
        } else if husk.is_some() {
            // THE HOLLOW: the honest tell. A husk is drab and rock-like on purpose, so the inner
            // void is what separates it from a drift rock BEFORE you shoot it.
            gizmos.linestrip_2d(ring(0.48), dim(husk_color(), 0.75));
            gizmos.linestrip_2d(ring(0.3), dim(husk_color(), 0.4));
        } else if let Some(fc) = facet {
            // THE OPEN FACE: a bright inward wedge marking the one angle that takes damage. It
            // sweeps with the rock's own spin, so tracking the gap IS the fight.
            let ang = a.rot + fc.open;
            let rad = asteroid_radius(a.size);
            let e0 = Vec2::from_angle(ang - FACET_OPEN_ARC * 0.5);
            let e1 = Vec2::from_angle(ang + FACET_OPEN_ARC * 0.5);
            gizmos.line_2d(c + e0 * rad, c + e0 * rad * 0.55, dim(facet_color(), 0.9));
            gizmos.line_2d(c + e1 * rad, c + e1 * rad * 0.55, dim(facet_color(), 0.9));
            gizmos.line_2d(c + e0 * rad * 0.72, c + e1 * rad * 0.72, dim(Color::srgb(6.0, 5.4, 3.0), 0.85));
        } else if let Some(l) = lapse {
            match l.phase {
                // GONE means gone (user's call): nothing is drawn at all — no scar, no hint. It
                // reappears somewhere else entirely, and the slow materialize is the only warning
                // you get (and the only one you need, since it's harmless until it finishes).
                LapsePhase::Gone => {}
                LapsePhase::FadingIn => {
                    // MATERIALIZING: an inner ring closing in as it solidifies — the countdown you
                    // read to decide whether to leave. It rides the SAME strike envelope as the body
                    // (`lapse_glow`) so the two read as one tube lighting, not two effects.
                    let f = l.presence();
                    gizmos.linestrip_2d(ring(0.25 + 0.7 * f), dim(lapse_glow(l), 0.75));
                }
                _ => {}
            }
        } else if let Some(h) = hunter {
            // THE EYE — the hunter's signature, and the one visual nothing else in the game has: a
            // bright iris that sits toward whatever it's chasing and opens wider as its charge builds.
            // Identity by silhouette, not by hue (the neon palette is crowded — see DESIGN.md).
            let look = h.look;
            let eye = c + look * asteroid_radius(a.size) * 0.34;
            let r = asteroid_radius(a.size) * (0.1 + 0.07 * h.charge);
            gizmos.circle_2d(Isometry2d::from_translation(eye), r, Color::srgb(7.0, 5.6, 4.6));
            gizmos.circle_2d(Isometry2d::from_translation(eye), r * 0.45, Color::srgb(9.0, 2.0, 1.0));
            // a lock-on tick pointing the way it's driving, so its heading reads at a glance
            gizmos.line_2d(eye + look * r * 1.4, eye + look * r * 2.6, dim(col, 0.85));
        } else if let Some(lit) = lit {
            // a Pulser: an inner ring that snaps bright when the shield is up (a clear "don't shoot" tell)
            gizmos.linestrip_2d(ring(0.55), if lit { col } else { dim(col, 0.6) });
        } else if cluster.is_some() {
            // a Cluster: FRACTURE LINES across the face — it's visibly ready to shatter
            let n = a.verts.len();
            for k in [0usize, 2, 4] {
                let (v1, v2) = (a.verts[k % n], a.verts[(k + n / 2) % n]);
                gizmos.line_2d(c + rot.rotate(v1 * 0.85), c + rot.rotate(v2 * 0.85), dim(col, 0.55));
            }
        } else if a.dense {
            let frac = a.hp.max(1) as f32 / a.size.max(1) as f32; // full shell → shrinks to a small core
            gizmos.linestrip_2d(ring(0.35 + 0.3 * frac), col);
        } else if red.is_some() {
            gizmos.linestrip_2d(ring(0.5), dim(col, 0.7)); // a second ring sets the growing red apart
        }
        // orange + gold rocks get NO extra ring — a single outline like any rock; their pulsing
        // colour is what sets them apart, so broken debris looks the same "chunkiness" as normal
    }

    // mines — crimson diamonds; blink faster once armed (the ship is near)
    let mc = mine_color();
    for (mine, mt) in &mines_q {
        let c = mt.translation.truncate();
        if !mine.armed || ((t * 6.0) as i32) % 2 == 0 {
            // armed-mine blink at ~3 Hz (was 6) — several arm in sync, so their combined flash area matters
            let r = MINE_R;
            let pts = [
                c + Vec2::new(0.0, r),
                c + Vec2::new(r, 0.0),
                c + Vec2::new(0.0, -r),
                c + Vec2::new(-r, 0.0),
                c + Vec2::new(0.0, r),
            ];
            gizmos.linestrip_2d(pts, mc);
            gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.4, mc);
        }
    }

    // enemy ships — neon-yellow orbs with a pulsing core (dim while fleeing out)
    let ec = enemy_color();
    for (en, et) in &foes.0 {
        let c = et.translation.truncate();
        let throb = 1.0 + 0.1 * (t * 6.0 + en.life).sin();
        let body = if en.fleeing { dim(ec, 0.55) } else { ec };
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_R * throb, body);
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_R * 0.45 * throb, body);
    }
    // enemy shots — yellow dots with a white-hot core
    for et in &foes.1 {
        let c = et.translation.truncate();
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_BULLET_R, ec);
        gizmos.circle_2d(Isometry2d::from_translation(c), ENEMY_BULLET_R * 0.5, Color::srgb(5.0, 5.0, 4.0));
    }
    // warp: a big black-hole DRAIN spiral (streams corkscrew inward, like water
    // down a drain) with layered glow + comet heads + a pulsing core.
    // The warp shot glows harder than the rest of the scene via brighter HDR colors
    // (NOT more global bloom, which would light up everything else too).
    let glow = 4.2; // the vortex glows much harder than the rest of the scene (brighter bloom)
    let warp = dim(warp_color(), glow);
    let comet = dim(Color::srgb(3.6, 2.2, 5.2), glow); // stream comet heads
    let corec = dim(Color::srgb(4.0, 2.6, 5.2), glow); // pulsing hot core
    let arms = 7;
    let segs = 14;
    let r_out = 112.0; // arms stay INSIDE the event-horizon ring — no spiral spilling past it
    let r_in = 22.0; // fatter throat/core — a bigger, hungrier center
    let wind = 2.4; // looser wrap — arms spiral in as spokes, don't close into rings
    for (hole, ht) in &holes {
        let c = ht.translation.truncate();
        let f = (hole.life / WARP_HOLE_LIFE).clamp(0.0, 1.0);
        let pulse = 1.0 + 0.12 * (hole.spin * 2.0).sin();
        // funnel arms — a clean spiral drawn segment-by-segment, fading to nothing at the rim
        // and brightening toward the core. Contained inside the event-horizon ring, so there's
        // no separate outer circle — it reads as a drain converging inward.
        for a in 0..arms {
            let a0 = a as f32 / arms as f32 * TAU;
            let pt = |p: f32| {
                let rad = r_out - (r_out - r_in) * p;
                c + Vec2::from_angle(a0 + wind * p + hole.spin) * rad
            };
            for k in 0..segs {
                let p0 = k as f32 / segs as f32;
                let p1 = (k + 1) as f32 / segs as f32;
                // p1: 0 at the (near-invisible) outer rim → 1 at the bright inner throat
                gizmos.line_2d(pt(p0), pt(p1), dim(warp, 0.62 * f * (0.05 + 0.95 * p1)));
            }
        }
        // bright streams travelling INWARD (comet: tail streak + bright head), brightening and
        // growing as they fall toward the core (tiny and dim at the rim → no dots on any circle).
        // Two offset streams per arm make the drain busier and sparklier.
        for (offset, headscale) in [(0.0f32, 1.0f32), (0.5, 0.72)] {
            for a in 0..arms {
                let a0 = a as f32 / arms as f32 * TAU;
                let hp = (hole.spin * 0.18 + a as f32 / arms as f32 + offset).rem_euclid(1.0);
                let tp = (hp - 0.16).max(0.0);
                let head = c + Vec2::from_angle(a0 + wind * hp + hole.spin) * (r_out - (r_out - r_in) * hp);
                let tail = c + Vec2::from_angle(a0 + wind * tp + hole.spin) * (r_out - (r_out - r_in) * tp);
                let b = f * hp * hp; // brightens sharply as it accelerates inward (dark at the rim)
                gizmos.line_2d(tail, head, dim(warp, 1.1 * b));
                gizmos.circle_2d(Isometry2d::from_translation(head), headscale * (2.0 + 3.0 * hp), dim(comet, b));
            }
        }
        // No drawn event-horizon ring: the spiral arms (fading to nothing at the rim) ARE the edge,
        // so the vortex reads as a pure drain converging out of the dark. The kill boundary
        // (WARP_CONSUME_R) is still enforced in logic — it just isn't outlined.
        // pulsing hot throat + a white-hot center for a searing bloom
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 30.0 * f) * pulse, dim(warp, 1.0 * f));
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 14.0 * f) * pulse, dim(corec, 1.1 * f));
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 7.0 * f) * pulse, dim(Color::srgb(7.0, 6.0, 7.0), f));
        gizmos.circle_2d(Isometry2d::from_translation(c), (r_in + 2.0 * f) * pulse, dim(Color::srgb(8.0, 7.5, 8.0), f)); // searing white throat
    }
    for mt in &missiles {
        let c = mt.translation.truncate();
        gizmos.circle_2d(Isometry2d::from_translation(c), 9.0, dim(warp, 0.55)); // outer glow
        gizmos.circle_2d(Isometry2d::from_translation(c), 5.0, warp);
        gizmos.circle_2d(Isometry2d::from_translation(c), 2.2, Color::srgb(6.0, 5.0, 7.0)); // hot core
    }

    // particles
    for (p, pt) in &particles {
        let f = (p.life / p.ttl).clamp(0.0, 1.0);
        let c = pt.translation.truncate();
        let dir = p.vel.normalize_or_zero();
        gizmos.line_2d(c, c - dir * 6.0, dim(p.color, f));
    }

    // bullets — a small bright head trailing a tapering purple flame. The flame
    // blobs shrink to a fine point at the tail and heat up toward the head (deep
    // purple tip → hot lavender base); the head itself is kept compact.
    let core = Color::srgb(5.0, 4.2, 5.6); // white-hot center
    for (b, bt, is_warhead, vel, gorge_shot) in &bullets {
        let c = bt.translation.truncate();
        let br = bullet_radius(b.mass);
        if let Some(g) = gorge_shot {
            // GORGE round: the Glutton's maw, thrown. A rolling ring of gnashing teeth that visibly
            // SWELLS with every rock it swallows — the growth IS the readout, so you can see how much
            // is left in it without a HUD number. Rolls as it travels; the jaws chew at ~1.4 Hz.
            let gc = devourer_color();
            let r = g.radius();
            let roll = t * 3.4; // it tumbles forward — a wrecking ball, not a bullet
            let chew = 0.5 + 0.5 * (t * 8.8).sin(); // jaws open/close, ~1.4 Hz
            // body: a broken ring, drawn as arcs between the teeth so the gaps read as a mouth
            const TEETH: usize = 7;
            for k in 0..TEETH {
                let a0 = roll + k as f32 / TEETH as f32 * TAU;
                let a1 = a0 + TAU / TEETH as f32 * 0.55;
                let (p0, p1) = (c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a1) * r);
                gizmos.line_2d(p0, p1, gc); // gum line
                // tooth: a wedge biting INWARD, its length driven by the chew
                let mid = (a0 + a1) * 0.5;
                let d = Vec2::from_angle(mid);
                let bite = r * (0.34 + 0.30 * chew);
                let base = c + d * r;
                let sideways = d.perp() * (r * 0.20);
                gizmos.line_2d(base + sideways, base - d * bite, dim(gc, 0.9));
                gizmos.line_2d(base - sideways, base - d * bite, dim(gc, 0.9));
            }
            // throat: a hot core that brightens as it fills, so a nearly-full round looks dangerous
            let fill = g.eaten as f32 / GORGE_BITES as f32;
            gizmos.circle_2d(Isometry2d::from_translation(c), r * (0.18 + 0.10 * chew), dim(mix(gc, core, 0.5), 0.7 + 1.3 * fill));
        } else if is_warhead {
            // WARHEAD round: an ARMED violet shell — a dart body along its flight plus a slow-spinning
            // ring of detonation ticks (the same language as its HUD glyph and blast ring). Reads
            // instantly as "the one that deletes rocks" against the standard orb / fat mass round.
            let wc = warhead_color();
            let dir = vel.0.normalize_or_zero();
            let perp = dir.perp();
            let tip = c + dir * (br * 2.8);
            let butt = c - dir * (br * 1.6);
            gizmos.line_2d(butt, tip, wc); // spine
            gizmos.line_2d(butt + perp * (br * 0.9), tip, dim(wc, 0.85)); // swept casing sides
            gizmos.line_2d(butt - perp * (br * 0.9), tip, dim(wc, 0.85));
            gizmos.circle_2d(Isometry2d::from_translation(c), br * 0.5, core);
            for k in 0..3 {
                let a = t * 7.0 + k as f32 / 3.0 * TAU; // continuous spin (~1.1 rev/s) — motion, not flash
                let d = Vec2::from_angle(a);
                gizmos.line_2d(c + d * (br * 1.5), c + d * (br * 2.2), dim(wc, 0.7));
            }
        } else if b.mass {
            // mass shot: a fat hot-violet round with a tapering trail
            let base = mass_color();
            let flame_tip = dim(base, 0.5); // deep (tail)
            let flame_base = mix(base, core, 0.35); // hot (near the head)
            let n = b.trail.len();
            for k in 0..n {
                let f = if n > 1 { k as f32 / (n - 1) as f32 } else { 1.0 }; // 0 tail → 1 head
                let r = br * (0.12 + 0.85 * f); // taper to a point at the tail
                gizmos.circle_2d(Isometry2d::from_translation(b.trail[k]), r, mix(flame_tip, flame_base, f * f));
            }
            gizmos.circle_2d(Isometry2d::from_translation(c), br * 0.75, flame_base);
            gizmos.circle_2d(Isometry2d::from_translation(c), br * 0.38, core);
        } else {
            // standard shot: a single clean purple orb (a trail-flame read wrong on a big screen)
            gizmos.circle_2d(Isometry2d::from_translation(c), br, bullet_color());
            gizmos.circle_2d(Isometry2d::from_translation(c), br * 0.5, core);
        }
    }

    // ship — light trail + hull (blinks while invulnerable)
    let sc = ship_color();
    for (s, st, trail) in &ships {
        let c = st.translation.truncate();
        // DEV invincibility indicator. Deliberately a BROKEN reticle (four short arcs) well outside
        // the hull, not a solid ring: the old solid circle sat at SHIP_R*2.2 = 29.7px, which is
        // essentially AEGIS_ORBIT_R (30) — so with the shards up it looked like a track joining them
        // and read as part of the ship's kit. A dashed ring at a clearly different radius can never
        // be mistaken for gameplay. Still obvious on sight, because god-mode must never be subtle.
        // Drawn before the blink skip so it stays visible through respawn flicker.
        if dev.invincible {
            let pulse = 1.0 + 0.06 * (t * 4.0).sin();
            let r = SHIP_R * 3.4 * pulse;
            for k in 0..4 {
                let base = t * 0.5 + k as f32 / 4.0 * TAU;
                let arc: Vec<Vec2> = (0..7).map(|i| c + Vec2::from_angle(base + i as f32 / 6.0 * 0.9) * r).collect();
                gizmos.linestrip_2d(arc, dim(sc, 0.55));
            }
        }
        // Nova Shield shell — the SHIP'S OWN silhouette scaled out (a second hull layer that turns with
        // you), in glassy pale violet. UP: a gentle amplitude-only breathe. In the regen's final stretch
        // it FLICKERS back on at ≤3 Hz (photosafe — same cadence as the spawn blink). Down: no shell.
        if nova.unlocked {
            let show = if nova.down <= 0.0 { true } else { nova.down < NOVA_RELIGHT && (nova.down * 6.0) as i32 % 2 == 0 };
            if show {
                let breathe = 1.0 + 0.04 * (t * 2.2).sin();
                let glow = if nova.down <= 0.0 { 0.8 + 0.2 * (t * 2.2).sin() } else { 0.55 };
                draw_ship(&mut gizmos, c, Vec2::from_angle(s.angle), SHIP_R * NOVA_SHELL * breathe, dim(nova_color(), glow), false);
            }
        }
        // AEGIS SHARDS — small chips riding a slow orbit around the hull, drawn from the ship's own
        // position so they MOVE WITH IT (user's call). One diamond per live shard, evenly spaced, in
        // player violet; the ring visibly thins as they're spent, so the shard count IS the readout
        // (no HUD slot needed). The next one regrowing shows as a faint ghost at its slot.
        if aegis.unlocked {
            for i in 0..AEGIS_SHARDS {
                let a = aegis.spin + i as f32 / AEGIS_SHARDS as f32 * TAU;
                let p = c + Vec2::from_angle(a) * AEGIS_ORBIT_R;
                let live = i < aegis.shards;
                // the slot that's currently regrowing fades in as its timer runs down
                let ghost = i == aegis.shards && aegis.shards < AEGIS_SHARDS;
                if !live && !ghost {
                    continue;
                }
                let grow = if live { 1.0 } else { 1.0 - (aegis.regen / AEGIS_REGEN).clamp(0.0, 1.0) };
                let r = AEGIS_SHARD_R * (0.4 + 0.6 * grow);
                let col = dim(sc, if live { 0.95 } else { 0.3 + 0.4 * grow });
                // a little diamond, tipped along its orbit — reads as a shard, not a dot
                let out = Vec2::from_angle(a);
                let side = out.perp();
                gizmos.linestrip_2d(
                    [p + out * r * 1.6, p + side * r, p - out * r * 1.6, p - side * r, p + out * r * 1.6],
                    col,
                );
            }
        }
        if s.invuln > 0.0 && (s.invuln * 6.0) as i32 % 2 == 0 {
            continue; // spawn-protection blink at ~3 Hz (was 6 — kept ≤3/sec so it doesn't strobe)
        }
        let rot = Vec2::from_angle(s.angle);
        // TRON-style light ribbon (user direction): the ship's recent path in its own violet, short
        // and fading — the light-cycle wall, minus the length and the lethality. Rooted at the flame
        // (recorded there by `ship_trail`), with real width (see draw_light_ribbon). Purely cosmetic;
        // it replaces the old exhaust SPARK particles. Stationary, it vanishes on its own.
        if let Some(tr) = trail {
            draw_light_ribbon(&mut gizmos, &tr.0, sc);
        }
        // thrust flame — the original triangular exhaust, flickering off the tail while burning
        if s.flame > 0.02 {
            let f = s.flame * (0.6 + 0.4 * (t * 40.0).sin().abs());
            let flame = [
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5, -5.0)),
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5 - 17.0 * f, 0.0)),
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5, 5.0)),
            ];
            gizmos.linestrip_2d(flame, dim(flame_color(), f));
        }
        draw_ship(&mut gizmos, c, rot, SHIP_R, sc, true); // filled — the solid neon hull
    }

    // lives HUD icons (top-right, under the "LIVES" label) — only while a run is on
    if show_run {
        let life_col = dim(sc, flick(hud_flash.life > 0.0)); // flickers briefly on a new life
        for k in 0..run.lives.max(0) {
            let p = Vec2::new(h.x - 32.0 - k as f32 * 24.0, h.y - 48.0);
            // the same solid dart as the ship, mini + nose-up (rot = +90°: Vec2::Y rotates (x,y) → (-y,x))
            draw_ship(&mut gizmos, p, Vec2::Y, 9.0, life_col, true);
        }
    }

    // ── the ABILITY STRIP (top-left, under the score): an ACTUAL HUD — fixed, NAMED slots (the ui
    // labels above each, see spawn_hud/hud_ability_labels) that light up as abilities are earned.
    // WARP always; CHAIN / MODE / SHIELD / DRONE appear with their pickups. The MODE slot always
    // shows whichever shot Q has equipped. Slot x's are the shared HUD_SLOT_* constants. ──
    if show_run {
        let gap = 22.0;
        let py = h.y - HUD_STRIP_Y; // the glyph row, just under its label row
        let sx = |slot: f32, off: f32| Vec2::new(-h.x + slot + off, py);

        // WARP (core kit): pips + refill bar
        let pip_lit = dim(warp, flick(hud_flash.pips > 0.0)); // flickers briefly when charges refill
        for k in 0..WARP_MAX_CHARGES {
            let col = if k < warp_res.charges { pip_lit } else { dim(warp, 0.14) };
            gizmos.circle_2d(Isometry2d::from_translation(sx(HUD_SLOT_WARP, 6.0 + k as f32 * gap)), 5.0, col);
        }
        if warp_res.cooldown > 0.0 {
            let prog = 1.0 - warp_res.cooldown / WARP_COOLDOWN;
            let w = gap * (WARP_MAX_CHARGES as f32 - 1.0);
            gizmos.line_2d(sx(HUD_SLOT_WARP, 6.0) - Vec2::Y * 11.0, sx(HUD_SLOT_WARP, 6.0 + w * prog) - Vec2::Y * 11.0, dim(warp, 0.7));
        }

        // CHAIN (the Warden's drop): bolt glyph + pips + refill bar
        if chain.unlocked {
            let cc = chain_color();
            let b = sx(HUD_SLOT_CHAIN, 4.0);
            gizmos.linestrip_2d(
                vec![b + Vec2::new(2.0, 8.0), b + Vec2::new(-3.0, 1.0), b + Vec2::new(1.0, 1.0), b + Vec2::new(-2.0, -8.0)],
                cc,
            );
            for k in 0..CHAIN_MAX_CHARGES {
                let col = if k < chain.charges { cc } else { dim(cc, 0.14) };
                gizmos.circle_2d(Isometry2d::from_translation(sx(HUD_SLOT_CHAIN, 26.0 + k as f32 * gap)), 5.0, col);
            }
            if chain.charges < CHAIN_MAX_CHARGES {
                let prog = 1.0 - chain.recharge / CHAIN_RECHARGE;
                let w = gap * (CHAIN_MAX_CHARGES as f32 - 1.0);
                gizmos.line_2d(sx(HUD_SLOT_CHAIN, 26.0) - Vec2::Y * 11.0, sx(HUD_SLOT_CHAIN, 26.0 + w * prog) - Vec2::Y * 11.0, dim(cc, 0.7));
            }
        }

        // MODE (the Q-cycled shot slot — appears once a second mode exists): a bracketed slot always
        // showing the ACTIVE selection — standard round, fat mass round, ticked Warhead, or the
        // toothed Gorge round. Each glyph is the field object in miniature, so the slot teaches itself.
        if mass.unlocked || warhead.unlocked || gorge.unlocked {
            let cx = sx(HUD_SLOT_MODE, 20.0);
            // slot bracket: four corner ticks, quietly framing whatever's equipped
            let bc = Color::srgb(0.45, 0.5, 0.68);
            let (bw, tick) = (13.0, 5.0);
            for (kx, ky) in [(-1.0f32, 1.0f32), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let corner = cx + Vec2::new(kx * bw, ky * bw);
                gizmos.line_2d(corner, corner - Vec2::new(kx * tick, 0.0), bc);
                gizmos.line_2d(corner, corner - Vec2::new(0.0, ky * tick), bc);
            }
            if gorge.unlocked && gorge.active {
                // Gorge round: the little maw — a ring of inward teeth in the Glutton's red
                let gc = devourer_color();
                gizmos.circle_2d(Isometry2d::from_translation(cx), 7.5, gc);
                for k in 0..6 {
                    let d = Vec2::from_angle(k as f32 / 6.0 * TAU + t * 0.9);
                    gizmos.line_2d(cx + d * 7.5, cx + d * 4.0, dim(gc, 0.85));
                }
                gizmos.circle_2d(Isometry2d::from_translation(cx), 2.0, dim(mix(gc, Color::WHITE, 0.5), 1.2));
            } else if warhead.unlocked && warhead.active {
                // Warhead round: a violet shell with detonation ticks
                gizmos.circle_2d(Isometry2d::from_translation(cx), 4.5, warhead_color());
                for k in 0..4 {
                    let d = Vec2::from_angle(k as f32 / 4.0 * TAU + TAU / 8.0);
                    gizmos.line_2d(cx + d * 6.5, cx + d * 9.5, dim(warhead_color(), 0.8));
                }
            } else if mass.unlocked && mass.active {
                // Mass round: the fat shell
                gizmos.circle_2d(Isometry2d::from_translation(cx), 7.0, mass_color());
                gizmos.circle_2d(Isometry2d::from_translation(cx), 3.5, dim(mass_color(), 0.6));
            } else {
                // Standard round: the small clean shot
                gizmos.circle_2d(Isometry2d::from_translation(cx), 3.5, bullet_color());
            }
        }

        // SHIELD (the Nova — the Pulsar's drop): a mini ghost-ship outline (the shell IS the ship's
        // shape) — bright while UP; while regenerating, dim with a progress underbar, flickering in
        // the final stretch (≤3 Hz, same as the shell itself)
        if nova.unlocked {
            let cx = sx(HUD_SLOT_SHIELD, 16.0);
            if nova.down <= 0.0 {
                draw_ship(&mut gizmos, cx, Vec2::Y, 9.0, dim(nova_color(), 0.85 + 0.15 * (t * 2.2).sin()), false);
            } else {
                let relight = nova.down < NOVA_RELIGHT && (nova.down * 6.0) as i32 % 2 == 0;
                draw_ship(&mut gizmos, cx, Vec2::Y, 9.0, dim(nova_color(), if relight { 0.8 } else { 0.22 }), false);
                let prog = 1.0 - nova.down / NOVA_REGEN;
                gizmos.line_2d(cx + Vec2::new(-8.0, -13.0), cx + Vec2::new(-8.0 + 16.0 * prog, -13.0), dim(nova_color(), 0.7));
            }
        }

        // DRONE (the Slinger's drop): a mini wingman — a core with its orbit dot slowly circling
        if has_drone {
            let cx = sx(HUD_SLOT_DRONE, 16.0);
            let dc = drone_color();
            gizmos.circle_2d(Isometry2d::from_translation(cx), 3.5, dc);
            gizmos.circle_2d(Isometry2d::from_translation(cx + Vec2::from_angle(t * 2.0) * 8.0), 1.6, dim(dc, 0.85));
        }
    }
}

// The Slinger (boss 3, wave 15): glide in → hover up high, mirroring the ship's x → LOAD a cannonball
// in front of itself (a charging telegraph you can shoot to disarm) → LAUNCH it fast at the ship. Its
// core is always exposed: chip it down while dodging the barrage. Death → burst → reward calm → advance.
#[allow(clippy::too_many_arguments)]
fn slinger_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    // bundled to stay under the 16-param limit; Stats is an Option so headless tests needn't insert it
    reward: (ResMut<Score>, ResMut<Wave>, ResMut<WaveBanner>, Option<ResMut<Stats>>),
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship), Without<Slinger>>,
    mut slingers: Query<(Entity, &mut Transform, &mut Slinger)>,
    mut ammo_q: Query<(&mut Transform, &mut Velocity), (With<Cannonball>, Without<Slinger>, Without<Ship>)>,
    grabbable: Query<(Entity, &Transform), (With<Asteroid>, Without<Cannonball>, Without<Slinger>, Without<Shielded>, Without<Gold>)>, // never tractor-beams the gold 1UP
) {
    let dt = time.delta_secs();
    let (mut score, mut wave, mut banner, mut stats) = reward;
    let mut rng = rand::thread_rng();
    let h = arena.half;
    let ship = ships.iter().next();
    let ship_pos = ship.map(|(_, t, _)| t.translation.truncate());
    let aim_from = |from: Vec2| ship_pos.map(|s| (s - from).normalize_or_zero()).filter(|d| *d != Vec2::ZERO).unwrap_or(Vec2::NEG_Y);
    for (se, mut st, mut sl) in &mut slingers {
        let mut p = st.translation.truncate();
        sl.pulse += dt * 4.0;
        sl.recoil = (sl.recoil - dt * 2.5).max(0.0); // the launch kick eases back out

        // ── DYING: crackle apart, then despawn → reward calm → advance the wave ──
        if sl.dying > 0.0 {
            // STAGED: wings shear, then the pods pop (the render hides each part as its threshold falls)
            let before = death_parts(sl.dying, SLINGER_DEATH_SECS, 4);
            sl.dying -= dt;
            if death_parts(sl.dying.max(0.0), SLINGER_DEATH_SECS, 4) < before {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * SLINGER_R * rng.gen_range(0.6..1.1);
                burst(&mut commands, p + off, slinger_color(), 18, 340.0, &mut rng);
            }
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..SLINGER_R);
                burst(&mut commands, p + off, slinger_color(), 3, 240.0, &mut rng);
            }
            if sl.dying <= 0.0 {
                burst(&mut commands, p, slinger_color(), 50, 460.0, &mut rng);
                burst(&mut commands, p, Color::srgb(6.0, 4.0, 3.0), 24, 300.0, &mut rng);
                if let Some(a) = sl.ammo.take() {
                    commands.entity(a).despawn(); // its loaded round goes with it
                }
                commands.entity(se).despawn();
                if let Some(s) = stats.as_mut() {
                    s.slinger = true; // achievement: defeated the Slinger
                }
                // drop the Drone orb (the boss-3 reward, content wave 15)
                let pdir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                commands.spawn((
                    Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Drone },
                    Velocity(pdir * PICKUP_DRIFT),
                    Transform::from_xyz(0.0, 0.0, 0.0),
                ));
                sfx.write(SoundFx::BossDown);
                defeat_boss(&mut score, &mut wave, &mut banner, stats.as_deref_mut());
            }
            continue;
        }
        // ── core destroyed → begin dying ──
        if sl.hp <= 0 {
            sl.dying = SLINGER_DEATH_SECS;
            burst(&mut commands, p, slinger_color(), 30, 320.0, &mut rng);
            continue;
        }

        // ── ENTER: glide down into its hover band (invulnerable) ──
        if !sl.entered {
            p.y -= SLINGER_ENTER_SPEED * dt;
            if p.y <= h.y * 0.55 {
                p.y = h.y * 0.55;
                sl.entered = true;
            }
            st.translation.x = p.x;
            st.translation.y = p.y;
            continue;
        }
        if sl.charge > 0.0 {
            sl.charge -= dt; // intro power-up: invulnerable, not yet firing
        }

        // ── PROWL: mirror the ship's x (so its shots come across at an angle) while WANDERING the
        // full height of the arena — it used to sit in a hover band up top, which made it a fixed
        // firing line you could learn once. Now it takes the low ground too and you have to keep
        // re-solving the angle. (User: bosses move anywhere on screen.)
        let margin = SLINGER_R + 24.0;
        let want = Vec2::new(
            ship_pos.map(|s| -s.x).unwrap_or(0.0).clamp(-h.x + margin, h.x - margin),
            ((sl.pulse * 0.13).sin() * (h.y - margin) * 0.85).clamp(-h.y + margin, h.y - margin),
        );
        p += (want - p).clamp_length_max(SLINGER_SPEED * dt);
        st.translation.x = p.x;
        st.translation.y = p.y;

        if sl.charge > 0.0 {
            continue; // no loading / firing during the intro power-up
        }

        // the grabbed round got shot out from under it (or otherwise vanished) → grab another after cooldown
        if sl.ammo.is_some_and(|a| ammo_q.get(a).is_err()) {
            sl.ammo = None;
            sl.cool = SLINGER_COOL;
        }
        let muzzle = p + aim_from(p) * (SLINGER_R + asteroid_radius(3) + 12.0); // where a round is held, between it and the ship
        if sl.ammo.is_none() {
            // COOL, then TRACTOR-GRAB the nearest free field rock (green on wave 15 → hard to shoot away)
            sl.cool -= dt;
            if sl.cool <= 0.0 {
                if let Some((re, _)) = grabbable.iter().min_by(|(_, a), (_, b)| {
                    let (da, db) = (a.translation.truncate().distance_squared(p), b.translation.truncate().distance_squared(p));
                    da.total_cmp(&db)
                }) {
                    commands.entity(re).insert(Cannonball { launched: false });
                    sl.ammo = Some(re);
                    sl.load = SLINGER_HOLD;
                    sfx.write(SoundFx::Warp); // beam-on cue
                }
                // no rock in reach → stay ready (cool <= 0), grab the instant one drifts in
            }
        } else if let Some(a) = sl.ammo {
            if let Ok((mut at, mut av)) = ammo_q.get_mut(a) {
                let rp = at.translation.truncate();
                let to = muzzle - rp;
                if to.length() > 10.0 {
                    // REEL: haul the rock along the beam toward the muzzle (dwell only starts on arrival)
                    let np = rp + to.clamp_length_max(SLINGER_REEL_SPEED * dt);
                    at.translation.x = np.x;
                    at.translation.y = np.y;
                    av.0 = Vec2::ZERO;
                    sl.load = SLINGER_HOLD;
                } else {
                    // HOLD at the muzzle (aiming), then LAUNCH at the ship's current spot
                    at.translation.x = muzzle.x;
                    at.translation.y = muzzle.y;
                    av.0 = Vec2::ZERO;
                    sl.load -= dt;
                    if sl.load <= 0.0 {
                        av.0 = aim_from(muzzle) * SLINGER_CANNON_SPEED;
                        commands.entity(a).insert(Cannonball { launched: true });
                        sl.ammo = None;
                        sl.cool = SLINGER_COOL;
                        sl.recoil = 1.0; // the hull kicks back off the shot (render eases it out)
                        sfx.write(SoundFx::Mine); // launch thump
                    }
                }
            }
        }

        // ── its body is solid: ship contact kills (unless mid-respawn / invincible) ──
        if run.respawn <= 0.0 {
            if let Some((se2, stf, sh)) = ship {
                if !immune(sh, &dev) {
                    let spp = stf.translation.truncate();
                    if p.distance(spp) < SLINGER_R + SHIP_R {
                        kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se2, spp, &mut rng);
                    }
                }
            }
        }
    }
}

// True if angle `pa` falls in the swept arc [lo, hi] (radians), tolerating ±TAU wrap. The Sweep Ray uses
// this to catch every rock/ship whose bearing the beam crossed this frame, no matter how the arc wraps.
fn angle_in_arc(pa: f32, lo: f32, hi: f32) -> bool {
    [-TAU, 0.0, TAU].iter().any(|k| {
        let a = pa + k;
        a >= lo && a <= hi
    })
}

// The Phantom (boss 6, FINALE) — THE HAUNT: a spectral predator too arrogant to be touched. Glides to
// centre, then fights on three per-phase-pool phases. It is INTANGIBLE (shots pass through, rocks drift
// through it, contact is harmless) EXCEPT while SURFACED: firing the Sweep Ray forces it to materialize for
// a short `vuln` window (solid, still, hittable — and its body kills on contact). Bait the ray, punish the
// window. Deplete a phase → RESET → next. Phase 2 possesses a homing rock (break it to expose it); phase 3 turns it solid and
// charging (later stages). Clear phase 3 → VICTORY.
fn phantom_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>, // the death scene holds the arena calm (pauses the finale spawner)
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    mut phantoms: Query<(Entity, &mut Transform, &mut Phantom)>,
    mut ships: Query<(Entity, &Transform, &mut Ship), Without<Phantom>>, // &mut: the finale kill shields the ship for the send-off
    rocks: Query<(Entity, &Transform), (With<Asteroid>, Without<Phantom>, Without<Gold>)>, // the beam vaporizes rocks it crosses (never the gold 1UP)
    vessels: Query<(Entity, &Transform), (With<Possessed>, Without<Phantom>, Without<Ship>, Without<Asteroid>)>, // p2 vessels: existence tells if the current one's been broken (→ rip out); position lets the ghost ride it
    trails: Query<Entity, With<SpectralTrail>>, // p3 afterimages: cleaned up on the win
    // death scene: are the escaping shard / the departing ship still on screen? (drives the send-off's beats)
    finale_fx: (Query<Entity, With<EscapeShard>>, Query<Entity, With<DepartingShip>>),
    // beating the Haunt IS beating the game — record it (+ Purist if no powerup was grabbed this run).
    // Bundled + optional so headless tests needn't insert them (16-param limit).
    mut progress: (Option<ResMut<Stats>>, Option<Res<RunFlags>>),
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let ray_len = arena.half.length() * 2.0 + 40.0; // FULL arena diagonal — so from any position the beam still reaches the far edge
    let ship_info = ships.iter().next().map(|(e, t, sh)| (e, t.translation.truncate(), immune(sh, &dev)));
    for (sge, mut stf, mut sg) in &mut phantoms {
        let mut p = stf.translation.truncate();
        let h = arena.half;
        sg.pulse += dt * 3.0;
        if sg.flash > 0.0 {
            sg.flash -= dt;
        }

        // ── ENTER: glide to centre (invulnerable) ──
        if !sg.entered {
            let to = Vec2::ZERO - p;
            let step = PHANTOM_ENTER_SPEED * dt;
            if to.length() <= step {
                p = Vec2::ZERO;
                sg.entered = true;
            } else {
                p += to.normalize() * step;
            }
            stf.translation.x = p.x;
            stf.translation.y = p.y;
            continue;
        }
        if sg.charge > 0.0 {
            sg.charge -= dt;
            continue; // intro power-up: inert — no ray, no contact yet
        }

        // ── RESET beat between phases: drifts to centre + reforms, then the next phase begins ──
        if sg.transition > 0.0 {
            sg.transition -= dt;
            p += (Vec2::ZERO - p) * (1.0 - (-dt * 2.4).exp());
            stf.translation.x = p.x;
            stf.translation.y = p.y;
            // the reset's two beats: first the spent form DIES like any boss (crackle-apart bursts at
            // random offsets, same language as the Warden/Glutton/… deaths)…
            if sg.transition > PHANTOM_RESET_SECS * 0.55 {
                for _ in 0..3 {
                    let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..PHANTOM_R);
                    burst(&mut commands, p + off, phantom_color(), 3, 240.0, &mut rng);
                }
            } else if (sg.pulse * 6.0).sin() > 0.7 {
                // …then what's left REFORMS into the new shape
                burst(&mut commands, p, phantom_color(), 3, 220.0, &mut rng);
            }
            if sg.transition <= 0.0 {
                sg.phase += 1; // the reset completes → the next phase begins with a fresh pool
                sg.hp = PHANTOM_PHASE_HP;
                sg.flash = 0.8;
                sg.vuln = 0.0;
                sg.ray = RayPhase::Idle;
                sg.ray_cool = PHANTOM_RAY_FIRST;
                sg.charge_cool = PHANTOM_CHARGE_EVERY;
                if sg.phase == 2 {
                    // POSSESSION begins: after a short beat it hunts a field rock to dive into (no ray in p2)
                    sg.possessed = None;
                    sg.seeking = None;
                    sg.dive = PHANTOM_DIVE_FIRST;
                }
                burst(&mut commands, p, phantom_color(), 46, 380.0, &mut rng);
                sfx.write(SoundFx::Haunt);
            }
            continue; // no attacks / contact / damage during the reset
        }

        // ── DEATH SCENE: it's beaten. Set up on the kill frame (below), it plays out here beat by beat — the
        //    core flees EAST among the light, THEN the hero's ship warps off after it, and ONLY once
        //    everything's cleared does the Victory screen come. ──
        if sg.victory > 0.0 {
            sg.victory -= dt; // safety cap only — the beats below normally end the scene first
            for (ve, _vtf) in &vessels {
                commands.entity(ve).despawn(); // clear any p2 leftover
            }
            for te in &trails {
                commands.entity(te).try_despawn(); // clear the p3 wake
            }
            if !sg.erupted {
                // BEAT 1 — GATHER: the dying boss is drawn back to the MIDDLE, crackling apart like
                // any other boss death on the way, then it ERUPTS
                for _ in 0..3 {
                    let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..PHANTOM_R);
                    burst(&mut commands, p + off, phantom_color(), 3, 240.0, &mut rng);
                }
                p += (Vec2::ZERO - p) * (1.0 - (-dt * 6.0).exp());
                stf.translation.x = p.x;
                stf.translation.y = p.y;
                let since = PHANTOM_VICTORY_SECS - sg.victory;
                if p.length() < 12.0 || since > 0.7 {
                    sg.erupted = true;
                    // The Haunt is destroyed — the game is WON. Record it here (the erupt), not at the
                    // Victory screen: the send-off scene that follows is pure theatre and can't be lost.
                    if let Some(s) = progress.0.as_mut() {
                        s.phantom = true; // achievement: Edgelord (beat the game)
                        s.waves += 1; // the finale counts toward the lifetime wave tally too
                        s.best_wave = s.best_wave.max(30); // a win IS reaching the bottom of the run
                        if progress.1.as_ref().is_some_and(|f| !f.powerup_used) {
                            s.no_powerups = true; // achievement: Purist (won it clean)
                        }
                        if !run.died {
                            s.deathless = true; // achievement: Untouchable (won without losing a life)
                        }
                    }
                    p = Vec2::ZERO;
                    stf.translation = Vec3::ZERO;
                    // POP every remaining asteroid
                    for (re, rt) in &rocks {
                        burst(&mut commands, rt.translation.truncate(), phantom_color(), 8, 260.0, &mut rng);
                        commands.entity(re).despawn();
                    }
                    // the final BANG — the one grand blast (a regular boss death, writ finale-sized):
                    // a single burst of light from the middle, carried all the way to the edge
                    light_burst_to_edge(&mut commands, Vec2::ZERO, h, 220, Color::srgb(7.0, 7.6, 8.2), &mut rng);
                    // the true-form CORE tears free and flees EAST — small + subtle, lost among the light
                    let verts: Vec<Vec2> = (0..5).map(|i| Vec2::from_angle(i as f32 / 5.0 * TAU + 0.3) * (5.0 + rng.gen_range(-1.5..3.0))).collect();
                    commands.spawn((
                        EscapeShard { dir: Vec2::X, spin: rng.gen_range(-4.0..4.0), age: 0.0, verts, trail: Vec::new() },
                        Transform::from_xyz(0.0, 0.0, 0.0),
                    ));
                    sfx.write(SoundFx::Haunt);
                }
                continue;
            }
            // BEAT 2 — SEND-OFF: keep the stage clear; once the core's off-screen the ship follows; then WIN
            for (re, _rt) in &rocks {
                commands.entity(re).despawn();
            }
            let (shards, departing) = &finale_fx;
            let shard_gone = shards.is_empty();
            if !shard_gone {
                // constant dying CRACKLE around the middle while the core flees — steady light in the
                // same language as every other boss death (no rhythmic pulses: aperiodic, never a strobe)
                for _ in 0..4 {
                    let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..PHANTOM_R * 2.4);
                    burst(&mut commands, off, phantom_color(), 3, 280.0, &mut rng);
                }
            }
            let mut launched_ship = false;
            if shard_gone {
                // the core has fled the arena → the hero's ship warps off EAST after it (once — then it's gone)
                for (se, stf2, _) in &ships {
                    launched_ship = true;
                    let sp = stf2.translation.truncate();
                    commands.spawn((DepartingShip { flame: 0.0 }, ShipTrail::default(), Transform::from_xyz(sp.x, sp.y, 0.0)));
                    burst(&mut commands, sp, ship_color(), 30, 320.0, &mut rng);
                    commands.entity(se).despawn();
                    sfx.write(SoundFx::Haunt);
                }
            }
            // EVERYTHING cleared (core gone, ship launched AND flown off) — or the safety cap elapsed → WIN
            if (shard_gone && !launched_ship && departing.is_empty()) || sg.victory <= 0.0 {
                commands.entity(sge).try_despawn();
                next.set(GameState::Victory);
            }
            continue;
        }

        // ── PHASE CLEARED (this phase's pool is gone) → RESET into the next phase, or WIN on the last ──
        if sg.hp <= 0 {
            // the possessed vessel dispels with the phase that made it (and on the win)
            for (ve, vtf) in &vessels {
                burst(&mut commands, vtf.translation.truncate(), phantom_color(), 14, 260.0, &mut rng);
                commands.entity(ve).despawn();
            }
            sg.possessed = None;
            sg.seeking = None;
            if sg.phase >= 3 {
                // FINALE KILL → SET UP the death scene (it plays out in the victory branch above: gather to
                // the middle → erupt in light → the core flees → the ship follows → Victory). Latch the win:
                // zero run.respawn so `respawn` can't stomp it with a same-frame GameOver, bank the score, and
                // hold the arena CALM (pauses the finale spawner). `erupted=false` → it gathers to centre first.
                run.respawn = 0.0;
                score.0 += BOSS_SCORE; // the hardest kill in the game — worth as much as any other boss
                sg.victory = PHANTOM_VICTORY_SECS; // safety cap (the scene ends when the core + ship have flown off)
                sg.erupted = false;
                wave.calm = PHANTOM_VICTORY_SECS + 0.5; // no fresh finale rocks drift in during the scene
                // shield the ship for the whole send-off: the win often lands on your LAST life, and a stray
                // hit during the scene must NOT flip the victory into a Game Over.
                for (_, _, mut sh) in &mut ships {
                    sh.invuln = sh.invuln.max(PHANTOM_VICTORY_SECS + 0.5);
                }
                sfx.write(SoundFx::Haunt);
            } else {
                sg.transition = PHANTOM_RESET_SECS; // "death" throes → reform → next phase
                sg.flash = 0.8;
                // each phase ENDS like a regular boss kill: the big double blast every other boss gets…
                burst(&mut commands, p, phantom_color(), 50, 460.0, &mut rng);
                burst(&mut commands, p, Color::srgb(5.0, 4.0, 5.0), 24, 300.0, &mut rng);
                sfx.write(SoundFx::Haunt);
            }
            continue;
        }

        // ── SURFACED: after firing the ray it's SOLID — still, hittable (collisions), and lethal to touch ──
        if sg.vuln > 0.0 {
            sg.vuln -= dt;
            if run.respawn <= 0.0 {
                if let Some((se, spos, imm)) = ship_info {
                    if !imm && spos.distance(p) < PHANTOM_R + SHIP_R {
                        kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, spos, &mut rng);
                    }
                }
            }
            continue; // it holds still while surfaced — the punish window (every phase)
        }

        // ── phase 2 — POSSESSION: it hunts a real field rock, glides to it, and DIVES IN — that rock becomes
        //    a haunted vessel (homes + kills on contact, `possessed_update`) with the ghost hidden inside,
        //    unhittable. Shooting the vessel breaks it and RIPS the Haunt out (surfaces, above — the punish
        //    window). Then it hunts the next rock. ──
        if sg.phase == 2 {
            if let Some(v) = sg.possessed {
                if let Ok((_, vtf)) = vessels.get(v) {
                    // riding the vessel — the ghost stays where the vessel is, so the rip-out lands there
                    p = vtf.translation.truncate();
                    stf.translation.x = p.x;
                    stf.translation.y = p.y;
                } else {
                    // the vessel was broken → the Haunt is torn out: SURFACE for the punish window
                    sg.possessed = None;
                    sg.seeking = None;
                    sg.vuln = PHANTOM_MATERIALIZE;
                    sg.dive = PHANTOM_DIVE_EVERY;
                    burst(&mut commands, p, phantom_color(), 26, 320.0, &mut rng);
                    sfx.write(SoundFx::Haunt);
                }
            } else if let Some(t) = sg.seeking {
                // gliding to the rock it fixed on — dive in on arrival (the rock BECOMES the vessel)
                if let Ok((_, ttf)) = rocks.get(t) {
                    let tp = ttf.translation.truncate();
                    if p.distance(tp) <= PHANTOM_POSSESS_R + PHANTOM_SEEK_SPEED * dt {
                        commands.entity(t).despawn(); // the field rock is consumed…
                        let verts = asteroid_verts(2, &mut rng);
                        let e = commands
                            .spawn((Possessed { hp: PHANTOM_POSSESS_HP, pulse: rng.gen_range(0.0..TAU), verts }, Transform::from_xyz(tp.x, tp.y, 0.0)))
                            .id(); // …and reborn as the haunted vessel the Haunt now hides in
                        sg.possessed = Some(e);
                        sg.seeking = None;
                        p = tp;
                        stf.translation.x = p.x;
                        stf.translation.y = p.y;
                        burst(&mut commands, tp, phantom_color(), 26, 300.0, &mut rng);
                        sfx.write(SoundFx::Haunt);
                    } else {
                        p += (tp - p).normalize_or_zero() * PHANTOM_SEEK_SPEED * dt; // chase the drifting rock
                        stf.translation.x = p.x;
                        stf.translation.y = p.y;
                    }
                } else {
                    sg.seeking = None; // the rock it was after is gone (shot away) → fix on another next tick
                }
            } else {
                // between vessels → after the beat, FIX on the nearest field rock and lunge for it
                sg.dive -= dt;
                if sg.dive <= 0.0 {
                    if let Some((t, _)) = rocks.iter().min_by(|(_, a), (_, b)| {
                        p.distance_squared(a.translation.truncate()).total_cmp(&p.distance_squared(b.translation.truncate()))
                    }) {
                        sg.seeking = Some(t); // it fixes on a rock (no cue — kept quiet; the dive-in sounds)
                    }
                    // no rock in reach → stay ready, re-check next tick (the finale field keeps trickling in)
                }
            }
            continue; // possession replaces the roam + sweep ray in phase 2
        }

        // ── phase 3 — the HUNT: the mask drops. It periodically locks your position (aim-telegraph, eyes
        //    blazing), then DASHES along that line, leaving a wake of lethal spectral afterimages. ──
        let hunting = sg.aim > 0.0 || sg.charging > 0.0; // mid-charge-sequence: no roam, no ray
        if sg.phase >= 3 {
            if sg.aim > 0.0 {
                sg.aim -= dt; // locked and winding up — the dodge window (direction is already fixed)
                if sg.aim <= 0.0 {
                    sg.charging = PHANTOM_CHARGE_SECS;
                    sfx.write(SoundFx::Haunt); // it lunges with a howl
                }
            } else if sg.charging > 0.0 {
                sg.charging -= dt;
                p += sg.charge_dir * PHANTOM_CHARGE_SPEED * dt;
                let margin = PHANTOM_R * 0.6;
                p.x = p.x.clamp(-h.x + margin, h.x - margin);
                p.y = p.y.clamp(-h.y + margin, h.y - margin);
                stf.translation.x = p.x;
                stf.translation.y = p.y;
                // its wake: a lethal afterimage seared onto the arena each frame of the dash
                commands.spawn((SpectralTrail { ttl: PHANTOM_TRAIL_TTL }, Transform::from_xyz(p.x, p.y, 0.0)));
            } else {
                sg.charge_cool -= dt;
                if sg.charge_cool <= 0.0 && sg.ray == RayPhase::Idle {
                    if let Some((_, spos, _)) = ship_info {
                        // DESPERATION: the more of its final pool you've stripped, the less it waits between
                        // lunges — from ~PHANTOM_CHARGE_EVERY at full health down to a frantic ~40% of that at
                        // death's door. The aim/dodge window (PHANTOM_CHARGE_AIM) never shrinks — it just comes
                        // at you more often, cornered and relentless.
                        let hpf = (sg.hp as f32 / PHANTOM_PHASE_HP as f32).clamp(0.0, 1.0);
                        sg.charge_cool = PHANTOM_CHARGE_EVERY * (0.4 + 0.6 * hpf);
                        sg.aim = PHANTOM_CHARGE_AIM;
                        sg.charge_dir = (spos - p).normalize_or(Vec2::X); // locks NOW — sidestep the line
                    }
                }
            }
        }

        // ── ROAM: an unhurried Lissajous drift — but in phase 3 it STALKS, biasing that drift toward the
        //    ship (harder the more of its final pool you've stripped) so it's always closing in between lunges ──
        if sg.ray == RayPhase::Idle && !hunting {
            let margin = PHANTOM_R + 30.0;
            let mut target = Vec2::new(
                (sg.pulse * 0.17).sin() * (h.x - margin) * 0.62,
                (sg.pulse * 0.11 + 1.3).sin() * (h.y - margin) * 0.42,
            );
            if sg.phase >= 3 {
                if let Some((_, spos, _)) = ship_info {
                    let hpf = (sg.hp as f32 / PHANTOM_PHASE_HP as f32).clamp(0.0, 1.0);
                    target = target.lerp(spos, 0.35 + 0.4 * (1.0 - hpf)); // 35% → 75% pull toward the ship as it nears death
                }
            }
            p += (target - p) * (1.0 - (-dt * PHANTOM_ROAM_EASE).exp());
            p.x = p.x.clamp(-h.x + margin, h.x - margin);
            p.y = p.y.clamp(-h.y + margin, h.y - margin);
            stf.translation.x = p.x;
            stf.translation.y = p.y;
        }

        // ── SWEEP RAY (all phases; faster each): Idle → Telegraph a random quadrant → Fire → SURFACE ──
        // `ray_arc` is Some((lo, hi)) on frames the beam is live: the arc of bearings it crossed THIS frame.
        let mut ray_arc: Option<(f32, f32)> = None;
        if !hunting && sg.phase == 1 {
        // the Sweep Ray is PHASE 1's signature only — p2 possesses, p3 just charges (no beam)
        match sg.ray {
            RayPhase::Idle => {
                sg.ray_cool -= dt;
                if sg.ray_cool <= 0.0 {
                    // AIM at the player — centre the swept quadrant on the ship's bearing so the beam crosses
                    // the corner it's nearest (a little jitter so it isn't pixel-perfect; the ~1.7s telegraph
                    // is the dodge window). Falls back to a random bearing if there's somehow no ship.
                    let bearing = ship_info.map(|(_, sp, _)| (sp - p).to_angle()).unwrap_or_else(|| rng.gen_range(0.0..TAU));
                    sg.ray_from = bearing - PHANTOM_RAY_QUADRANT * 0.5 + rng.gen_range(-0.15f32..0.15);
                    sg.ray_span = PHANTOM_RAY_QUADRANT;
                    sg.ray_t = 0.0;
                    sg.ray = RayPhase::Telegraph;
                }
            }
            RayPhase::Telegraph => {
                sg.ray_t += dt;
                if sg.ray_t >= PHANTOM_RAY_TELEGRAPH {
                    sg.ray_t = 0.0;
                    sg.ray = RayPhase::Fire;
                    sfx.write(SoundFx::Haunt); // the beam ignites with a rising howl
                }
            }
            RayPhase::Fire => {
                let prev = (sg.ray_t / PHANTOM_RAY_FIRE).clamp(0.0, 1.0);
                sg.ray_t += dt;
                let cur = (sg.ray_t / PHANTOM_RAY_FIRE).clamp(0.0, 1.0);
                let a0 = sg.ray_from + sg.ray_span * prev;
                let a1 = sg.ray_from + sg.ray_span * cur;
                ray_arc = Some((a0.min(a1), a0.max(a1)));
                if sg.ray_t >= PHANTOM_RAY_FIRE {
                    sg.ray = RayPhase::Idle;
                    // cadence tightens as it escalates: 4.6s (p1) → 3.5s (p2) → 2.4s (p3), floored at 2.3s
                    sg.ray_cool = (PHANTOM_RAY_COOLDOWN - (sg.phase as f32 - 1.0) * 1.1).max(2.3);
                    if sg.phase < 3 {
                        // firing DRAINED it — it must SURFACE to recover: solid, still, hittable. The window.
                        // (phase 3 doesn't bother hiding: it's solid full-time, so no freeze.)
                        sg.vuln = PHANTOM_MATERIALIZE;
                    }
                }
            }
        }
        } // !hunting — the ray pauses while it aims/charges

        // ── the live beam vaporizes every rock whose bearing it crossed this frame ──
        if let Some((lo, hi)) = ray_arc {
            for (re, rt) in &rocks {
                let rel = rt.translation.truncate() - p;
                let d = rel.length();
                if d > PHANTOM_RAY_INNER_R && d < ray_len && angle_in_arc(rel.to_angle(), lo, hi) {
                    burst(&mut commands, rel + p, phantom_ray_color(), 10, 260.0, &mut rng);
                    commands.entity(re).despawn();
                }
            }
        }

        // ── ship kills: the live beam (any phase), and body contact in phase 3 (the mask is off — solid) ──
        if run.respawn <= 0.0 {
            if let Some((se, spos, imm)) = ship_info {
                if !imm {
                    let rel = spos - p;
                    let d = rel.length();
                    let beamed = ray_arc.is_some_and(|(lo, hi)| d > PHANTOM_RAY_INNER_R && d < ray_len && angle_in_arc(rel.to_angle(), lo, hi));
                    let rammed = sg.phase >= 3 && d < PHANTOM_R + SHIP_R;
                    if beamed || rammed {
                        kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, spos, &mut rng);
                    }
                }
            }
        }
    }
}

// Phase-2 VESSELS: each possessed rock homes at the ship and kills on contact while the Haunt hides inside.
// Its HP is chipped by gunfire in `collisions`; here it just hunts, and self-destructs once broken (which
// `phantom_update` sees as the vessel vanishing → it rips the Haunt out into the open).
fn possessed_update(
    time: Res<Time>,
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    mut vessels: Query<(Entity, &mut Transform, &mut Possessed)>,
    ships: Query<(Entity, &Transform, &Ship), Without<Possessed>>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|(e, t, sh)| (e, t.translation.truncate(), immune(sh, &dev)));
    for (e, mut tf, mut pv) in &mut vessels {
        pv.pulse += dt * 4.0;
        if pv.hp <= 0 {
            burst(&mut commands, tf.translation.truncate(), phantom_color(), 28, 340.0, &mut rng); // vessel shatters
            commands.entity(e).despawn();
            continue;
        }
        let vp = tf.translation.truncate();
        if let Some((se, spos, imm)) = ship {
            let np = vp + (spos - vp).clamp_length_max(PHANTOM_POSSESS_SPEED * dt); // HOME toward the ship
            tf.translation.x = np.x;
            tf.translation.y = np.y;
            if run.respawn <= 0.0 && !imm && np.distance(spos) < PHANTOM_POSSESS_R + SHIP_R {
                kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, spos, &mut rng);
            }
        }
    }
}

// The phase-3 charge's wake: each afterimage burns for its ttl, killing the ship on contact, then fades.
fn spectral_trail_update(
    time: Res<Time>,
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    mut trails: Query<(Entity, &Transform, &mut SpectralTrail)>,
    ships: Query<(Entity, &Transform, &Ship)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|(e, t, sh)| (e, t.translation.truncate(), immune(sh, &dev)));
    for (te, tt, mut tr) in &mut trails {
        tr.ttl -= dt;
        if tr.ttl <= 0.0 {
            commands.entity(te).try_despawn(); // try_ — phantom_update's victory wipe may despawn the same trail this frame
            continue;
        }
        if run.respawn <= 0.0 {
            if let Some((se, spos, imm)) = ship {
                if !imm && spos.distance(tt.translation.truncate()) < PHANTOM_TRAIL_R + SHIP_R {
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, spos, &mut rng);
                }
            }
        }
    }
}

// The Haunt's true-form core, having torn free of the shattered shell, flees off-screen after the finale
// kill — slow at first, then accelerating (ease-in) so the escape reads as a deliberate scene, not a blip.
// Culled once it clears the arena. Purely cosmetic; no collision — the sequel seed.
fn escape_shard_update(time: Res<Time>, mut commands: Commands, arena: Res<Arena>, mut shards: Query<(Entity, &mut Transform, &mut EscapeShard)>) {
    let dt = time.delta_secs();
    let h = arena.half;
    for (e, mut tf, mut sh) in &mut shards {
        sh.age += dt;
        sh.spin += dt * 3.0;
        // ease-IN: it rips loose slowly, then streaks away as it gathers speed
        let ramp = (sh.age / PHANTOM_SHARD_RAMP).clamp(0.0, 1.0);
        let speed = PHANTOM_SHARD_MIN_SPEED + (PHANTOM_SHARD_MAX_SPEED - PHANTOM_SHARD_MIN_SPEED) * ramp * ramp;
        let p = tf.translation.truncate() + sh.dir * speed * dt;
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        sh.trail.push(p); // lay down the comet streak
        if sh.trail.len() > 24 {
            sh.trail.remove(0);
        }
        if p.x.abs() > h.x + 90.0 || p.y.abs() > h.y + 90.0 {
            commands.entity(e).despawn(); // gone — off into the dark
        }
    }
}

// The hero's ship warping off EAST after the finale — a cosmetic entity (no player control, no bounds
// clamp) flown off-screen trailing thrust sparks, then culled. Spawned once the escaping shard has left.
fn departing_ship_update(time: Res<Time>, mut commands: Commands, arena: Res<Arena>, mut ships: Query<(Entity, &mut Transform, &mut DepartingShip)>) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let h = arena.half;
    for (e, mut tf, mut ds) in &mut ships {
        tf.translation.x += SHIP_DEPART_SPEED * dt; // straight east, off the edge
        ds.flame += dt * 40.0;
        if rng.gen_bool(0.6) {
            let tail = tf.translation.truncate() - Vec2::new(SHIP_R * 0.5, 0.0);
            commands.spawn((
                Particle { vel: Vec2::new(-rng.gen_range(140.0..240.0), rng.gen_range(-30.0..30.0)), life: 0.3, ttl: 0.3, color: flame_color() },
                Transform::from_xyz(tail.x, tail.y, 0.0),
            ));
        }
        if tf.translation.x > h.x + 80.0 {
            commands.entity(e).despawn(); // gone — off after the shard
        }
    }
}

// The Haunt's spectral body UNMAKES any asteroid it drifts through, so it never just clips a rock (it IS a
// ghost — matter dissolves in its wake). Spares the rock it's currently hunting to possess (p2); idle while
// it glides in, powers up, or is dying.
fn phantom_dissolve(
    mut commands: Commands,
    phantoms: Query<(&Transform, &Phantom)>,
    rocks: Query<(Entity, &Transform), (With<Asteroid>, Without<Phantom>, Without<Gold>)>,
) {
    let mut rng = rand::thread_rng();
    for (ptf, ph) in &phantoms {
        if !ph.entered || ph.charge > 0.0 || ph.victory > 0.0 {
            continue;
        }
        let p = ptf.translation.truncate();
        for (re, rt) in &rocks {
            if Some(re) == ph.seeking {
                continue; // spare the rock it's flying to possess
            }
            if p.distance(rt.translation.truncate()) < PHANTOM_R {
                burst(&mut commands, rt.translation.truncate(), phantom_color(), 6, 200.0, &mut rng);
                commands.entity(re).despawn();
            }
        }
    }
}

// The Pulsar (boss 5): pulses lit (invulnerable) / dark (open); on a beat it shockwaves every rock and
// the ship outward. Drifts slowly toward the ship so it can't be camped; contact kills. Gunfire lands
// only during its DARK beat (see `collisions`). On death it drops the Nova Shield orb — the player
// inherits its lit↔dark identity as a regenerating one-hit barrier (see `Nova`) — then advances the wave.
fn pulsar_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    reward: (ResMut<Score>, ResMut<Wave>, ResMut<WaveBanner>, Option<ResMut<Stats>>), // Stats optional: headless tests needn't insert it
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    mut pulsars: Query<(Entity, &mut Transform, &mut Pulsar)>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Velocity), Without<Pulsar>>,
    mut rocks: Query<(&Transform, &mut Velocity), (With<Asteroid>, Without<Pulsar>, Without<Ship>, Without<Gold>)>, // never flings the gold 1UP (could knock it off-screen → forfeit)
) {
    let dt = time.delta_secs();
    let (mut score, mut wave, mut banner, mut stats) = reward;
    let mut rng = rand::thread_rng();
    let h = arena.half;
    let sp = ships.iter().next().map(|(_, t, _, _)| t.translation.truncate());
    for (pe, mut ptf, mut pl) in &mut pulsars {
        let mut p = ptf.translation.truncate();
        pl.pulse += dt * 4.0;

        // ── DYING: crackle apart, despawn, advance the wave ──
        if pl.dying > 0.0 {
            // STAGED: the star's spikes shear off one per beat of the countdown
            let before = death_parts(pl.dying, PULSAR_DEATH_SECS, 8);
            pl.dying -= dt;
            let after = death_parts(pl.dying.max(0.0), PULSAR_DEATH_SECS, 8);
            if after < before {
                let a = after as f32 / 8.0 * TAU;
                burst(&mut commands, p + Vec2::from_angle(a) * PULSAR_R * 0.8, pulsar_color(), 12, 320.0, &mut rng);
            }
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..PULSAR_R);
                burst(&mut commands, p + off, pulsar_color(), 3, 240.0, &mut rng);
            }
            if pl.dying <= 0.0 {
                // the NOVA: the collapsed core lets go — a blast plus one clean expanding ring
                commands.spawn((
                    Shockwave { age: 0.0, ttl: 0.6, max_r: PULSAR_SHOCK_R, color: pulsar_color() },
                    Transform::from_xyz(p.x, p.y, 0.0),
                ));
                burst(&mut commands, p, pulsar_color(), 50, 480.0, &mut rng);
                commands.entity(pe).despawn();
                // drop the Nova Shield orb (the boss-5 reward, content wave 25)
                let pdir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                commands.spawn((
                    Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Nova },
                    Velocity(pdir * PICKUP_DRIFT),
                    Transform::from_xyz(0.0, 0.0, 0.0),
                ));
                if let Some(s) = stats.as_mut() {
                    s.pulsar = true; // achievement: defeated the Pulsar
                }
                sfx.write(SoundFx::BossDown);
                defeat_boss(&mut score, &mut wave, &mut banner, stats.as_deref_mut());
            }
            continue;
        }
        if pl.hp <= 0 {
            pl.dying = PULSAR_DEATH_SECS;
            burst(&mut commands, p, pulsar_color(), 30, 320.0, &mut rng);
            continue;
        }

        // ── ENTER: glide down into its band (invulnerable) ──
        if !pl.entered {
            p.y -= PULSAR_ENTER_SPEED * dt;
            if p.y <= h.y * 0.45 {
                p.y = h.y * 0.45;
                pl.entered = true;
            }
            ptf.translation.x = p.x;
            ptf.translation.y = p.y;
            continue;
        }
        if pl.charge > 0.0 {
            pl.charge -= dt;
        }

        // ── DRIFT: a slow chase across the WHOLE arena. It used to hold the upper third and the
        // middle 60% of the width, which let you camp a corner it could never reach; now it follows
        // you anywhere and only the shockwave cadence limits it. (User: bosses move anywhere.)
        if let Some(s) = sp {
            // a slow orbital SWAY rides on the chase — the star never just hangs there
            let sway = Vec2::new((pl.pulse * 0.17).sin() * 46.0, (pl.pulse * 0.13).sin() * 34.0);
            let m = PULSAR_R + 20.0;
            let want = Vec2::new(s.x.clamp(-h.x + m, h.x - m), s.y.clamp(-h.y + m, h.y - m)) + sway;
            p += (want - p).clamp_length_max(PULSAR_SPEED * dt);
        }
        ptf.translation.x = p.x.clamp(-h.x + PULSAR_R, h.x - PULSAR_R);
        ptf.translation.y = p.y.clamp(-h.y + PULSAR_R, h.y - PULSAR_R);
        if pl.charge > 0.0 {
            continue; // no shockwaves during the intro power-up
        }

        // ── SHOCK: on a beat, fling every rock + the ship outward from the Pulsar ──
        pl.shock_cool -= dt;
        if pl.shock_cool <= 0.0 {
            pl.shock_cool = PULSAR_SHOCK_EVERY;
            commands.spawn((
                Shockwave { age: 0.0, ttl: 0.5, max_r: PULSAR_SHOCK_R, color: pulsar_color() },
                Transform::from_xyz(p.x, p.y, 0.0),
            ));
            sfx.write(SoundFx::Mine); // reuse the whump
            for (rt, mut rv) in &mut rocks {
                let d = rt.translation.truncate() - p;
                let dist = d.length();
                if dist < PULSAR_SHOCK_R && dist > 0.01 {
                    rv.0 += d / dist * PULSAR_SHOCK_PUSH;
                }
            }
            if let Some((_, t, _, mut sv)) = ships.iter_mut().next() {
                let d = t.translation.truncate() - p;
                let dist = d.length();
                if dist < PULSAR_SHOCK_R && dist > 0.01 {
                    sv.0 += d / dist * PULSAR_SHOCK_PUSH; // shoved, not killed — the danger is losing control near a wall
                }
            }
        }

        // ── its body is solid: ship contact kills (unless mid-respawn / invincible) ──
        if run.respawn <= 0.0 {
            if let Some((se, t, sh, _)) = ships.iter().next() {
                let spp = t.translation.truncate();
                if !immune(sh, &dev) && spp.distance(p) < PULSAR_R + SHIP_R {
                    kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, spp, &mut rng);
                }
            }
        }
    }
}

// The Detonator (boss 4): ARMORED while it drifts, then it HALTS to PRIME the nearest rock — a
// telegraphed channel during which its core is exposed (your only damage window). At the channel's end
// the primed rock becomes a live bomb (a `Detonating` rock on a fuse) you must dodge. Then it repeats.
fn detonator_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    reward: (ResMut<Score>, ResMut<Wave>, ResMut<WaveBanner>, Option<ResMut<Stats>>), // bundled (16-param limit); Stats optional for headless tests
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship), Without<Detonator>>,
    mut dets: Query<(Entity, &mut Transform, &mut Detonator)>,
    // never primes the gold 1UP — and never an ORANGE: those are already bombs, so charging one was
    // redundant (its primed rocks are the boss's own red-white munitions, distinct from the orange type)
    rocks: Query<(Entity, &Transform), (With<Asteroid>, Without<Detonating>, Without<Shielded>, Without<Detonator>, Without<Gold>, Without<Explosive>)>,
) {
    let dt = time.delta_secs();
    let (mut score, mut wave, mut banner, mut stats) = reward;
    let mut rng = rand::thread_rng();
    let h = arena.half;
    let ship = ships.iter().next();
    for (de, mut dtf, mut det) in &mut dets {
        let mut p = dtf.translation.truncate();
        det.pulse += dt * 4.0;

        // ── DYING: crackle apart, despawn, drop the Warhead orb, then advance the wave ──
        if det.dying > 0.0 {
            // STAGED: the armor petals blow off one by one, baring the failing core
            let before = death_parts(det.dying, DETONATOR_DEATH_SECS, 6);
            det.dying -= dt;
            let after = death_parts(det.dying.max(0.0), DETONATOR_DEATH_SECS, 6);
            if after < before {
                let a = after as f32 / 6.0 * TAU;
                burst(&mut commands, p + Vec2::from_angle(a) * DETONATOR_R * 1.0, detonator_color(), 14, 330.0, &mut rng);
            }
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..DETONATOR_R);
                burst(&mut commands, p + off, detonator_color(), 3, 240.0, &mut rng);
            }
            if det.dying <= 0.0 {
                burst(&mut commands, p, detonator_color(), 50, 480.0, &mut rng);
                burst(&mut commands, p, orange_color(), 24, 320.0, &mut rng);
                commands.entity(de).despawn();
                if let Some(s) = stats.as_mut() {
                    s.detonator = true; // achievement: defeated the Detonator
                }
                let pdir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                commands.spawn((
                    Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Warhead },
                    Velocity(pdir * PICKUP_DRIFT),
                    Transform::from_xyz(0.0, 0.0, 0.0),
                ));
                sfx.write(SoundFx::BossDown);
                defeat_boss(&mut score, &mut wave, &mut banner, stats.as_deref_mut());
            }
            continue;
        }
        if det.hp <= 0 {
            det.dying = DETONATOR_DEATH_SECS;
            burst(&mut commands, p, detonator_color(), 30, 320.0, &mut rng);
            continue;
        }

        // ── ENTER: glide down into the arena (invulnerable) ──
        if !det.entered {
            p.y -= DETONATOR_ENTER_SPEED * dt;
            if p.y <= h.y * 0.5 {
                p.y = h.y * 0.5;
                det.entered = true;
            }
            dtf.translation.x = p.x;
            dtf.translation.y = p.y;
            continue;
        }
        if det.charge > 0.0 {
            det.charge -= dt; // intro power-up: invulnerable, not yet priming
        }

        if det.prime > 0.0 {
            // ── PRIMING: it HALTS (exposed core = your damage window). At the end, arm the bomb. ──
            det.prime -= dt;
            if !det.target.is_some_and(|t| rocks.get(t).is_ok()) {
                // the target rock got shot away mid-channel → cancel, go back armored
                det.target = None;
                det.prime = 0.0;
                det.cool = DETONATOR_COOL;
            } else if det.prime <= 0.0 {
                if let Some(t) = det.target.take() {
                    commands.entity(t).insert(Detonating { fuse: DETONATOR_BOMB_FUSE, friendly: false }); // → live bomb, LETHAL (reuses `detonate`)
                    sfx.write(SoundFx::Mine);
                }
                det.cool = DETONATOR_COOL;
            }
        } else if det.charge <= 0.0 {
            // ── ARMED: drift toward the nearest rock, then start a priming channel on cooldown ──
            det.cool -= dt;
            let nearest = rocks.iter().min_by(|(_, a), (_, b)| {
                let (da, db) = (a.translation.truncate().distance_squared(p), b.translation.truncate().distance_squared(p));
                da.total_cmp(&db)
            });
            if let Some((_, rt)) = nearest {
                p += (rt.translation.truncate() - p).clamp_length_max(DETONATOR_SPEED * dt);
            }
            if det.cool <= 0.0 {
                // only START priming once we've actually reached a rock (attach range). If none is close
                // enough yet (or the field is momentarily empty), keep drifting in — never prime "nothing".
                if let Some((re, rt)) = nearest {
                    if rt.translation.truncate().distance(p) <= DETONATOR_ATTACH_R {
                        det.target = Some(re);
                        det.prime = DETONATOR_PRIME_SECS;
                        sfx.write(SoundFx::Warp); // channel-on cue
                    }
                }
            }
            dtf.translation.x = p.x.clamp(-h.x + DETONATOR_R, h.x - DETONATOR_R);
            dtf.translation.y = p.y.clamp(-h.y + DETONATOR_R, h.y - DETONATOR_R);
        }

        // ── its body is solid: ship contact kills (unless mid-respawn / invincible) ──
        if run.respawn <= 0.0 {
            if let Some((se2, stf, sh)) = ship {
                if !immune(sh, &dev) {
                    let spp = stf.translation.truncate();
                    if p.distance(spp) < DETONATOR_R + SHIP_R {
                        kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se2, spp, &mut rng);
                    }
                }
            }
        }
    }
}

// ─────────────────────────────── boss bodies (the spectacle) ──────────
// ONE canonical draw per boss, shared by the fight, the run-up WARNING BANNER, and the background
// cameo — the silhouette can never drift between the three. Every body carries an IDLE-MOTION layer
// (breathing shells, gnashing teeth, spinning drums, waving feelers): a boss is never a static
// object. Flash rates stay ≤3 Hz (photosensitivity); continuous motion is unrestricted.

// The Warden — an armored VAULT: two counter-rotating octagon shells around a single EYE that tracks
// the player. Its tentacles draw separately (one per held rock + idle stubs on empty slots).
fn draw_warden_body(gizmos: &mut Gizmos, c: Vec2, r: f32, t: f32, eye_to: Vec2, color: Color) {
    let breathe = 1.0 + 0.03 * (t * 1.9).sin();
    let oct = |radius: f32, rot: f32| -> Vec<Vec2> { (0..=8).map(|k| c + Vec2::from_angle(k as f32 / 8.0 * TAU + rot) * radius).collect() };
    gizmos.linestrip_2d(oct(r * breathe, t * 0.22), color);
    gizmos.linestrip_2d(oct(r * 0.74 * breathe, -t * 0.16), dim(color, 0.55));
    let look = eye_to.normalize_or_zero() * r * 0.12; // the eye leans toward its prey
    gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.30, dim(color, 1.1));
    gizmos.circle_2d(Isometry2d::from_translation(c + look), r * 0.11, Color::srgb(6.0, 3.2, 5.6));
}

// The Glutton — a living MAW: a lumpy hide ringed with waving feeler-spines, two COUNTER-ROTATING
// rings of teeth gnashing around a gullet that glows brighter the more it has gorged.
fn draw_glutton_body(gizmos: &mut Gizmos, c: Vec2, r: f32, t: f32, wob: f32, gorge: f32, color: Color) {
    let body: Vec<Vec2> = (0..=18)
        .map(|k| {
            let a = k as f32 / 18.0 * TAU + wob * 0.2;
            let jag = 0.82 + 0.18 * (a * 3.0 + wob * 0.5).sin();
            c + Vec2::from_angle(a) * r * jag
        })
        .collect();
    gizmos.linestrip_2d(body, color);
    for k in 0..8 {
        // feeler-spines, each waving on its own beat
        let a = k as f32 / 8.0 * TAU + 0.12 * (t * 1.4 + k as f32 * 1.3).sin();
        let base = c + Vec2::from_angle(a) * r * 0.94;
        gizmos.line_2d(base, base + Vec2::from_angle(a) * r * (0.16 + 0.05 * (t * 1.8 + k as f32).sin()), dim(color, 0.8));
    }
    for (n, ring_r, dir, tooth) in [(8i32, 0.58f32, 0.5f32, 0.22f32), (6, 0.32, -0.7, 0.16)] {
        // the GNASH: outer teeth ring turns one way, inner the other. Each tooth is a CLOSED fang
        // (outline + a bright center rib, so bloom reads it as solid) — not an open V.
        for k in 0..n {
            let a = k as f32 / n as f32 * TAU + t * dir;
            let out = Vec2::from_angle(a);
            let side = out.perp() * r * tooth * 0.42;
            let base = c + out * r * ring_r;
            let tip = c + out * r * (ring_r - tooth);
            gizmos.linestrip_2d(vec![base + side, tip, base - side, base + side], dim(color, 0.95));
            gizmos.line_2d(c + out * r * (ring_r - tooth * 0.15), tip, color); // the rib — fills the fang
        }
    }
    // the gullet — its glow IS the gorge meter
    gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.13, mix(Color::srgb(2.0, 0.5, 0.4), Color::srgb(7.5, 5.5, 4.0), gorge));
    gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.20, dim(color, 0.4 + 0.5 * gorge));
}

// The Slinger — an ice-blue RAILGUN gunship: dart hull (kicking back on `recoil`), twin rail prongs
// framing the loading round, swept wings, a spinning slotted ammo drum, and engine pods. `stage`
// stages the death: wings shear and pods pop as it falls (1.0 = intact).
fn draw_slinger_body(gizmos: &mut Gizmos, c: Vec2, rot: Vec2, s: f32, t: f32, recoil: f32, stage: f32, color: Color) {
    let kick = rot * (-s * 0.20 * recoil); // the whole hull jolts back as it fires
    let tf = |x: f32, y: f32| c + kick + rot.rotate(Vec2::new(x, y));
    gizmos.linestrip_2d(
        [tf(1.35 * s, 0.0), tf(0.25 * s, 0.62 * s), tf(-0.9 * s, 0.5 * s), tf(-0.55 * s, 0.0), tf(-0.9 * s, -0.5 * s), tf(0.25 * s, -0.62 * s), tf(1.35 * s, 0.0)],
        color,
    );
    if stage > 0.62 {
        gizmos.linestrip_2d([tf(-0.2 * s, 0.55 * s), tf(-1.15 * s, 1.05 * s), tf(-0.95 * s, 0.35 * s)], dim(color, 0.85)); // upper wing
    }
    if stage > 0.45 {
        gizmos.linestrip_2d([tf(-0.2 * s, -0.55 * s), tf(-1.15 * s, -1.05 * s), tf(-0.95 * s, -0.35 * s)], dim(color, 0.85)); // lower wing
    }
    // twin RAIL PRONGS — the round charges in the open between them
    for side in [-1.0f32, 1.0] {
        gizmos.line_2d(tf(0.9 * s, side * 0.16 * s), tf(1.95 * s, side * 0.24 * s), color);
        gizmos.line_2d(tf(1.95 * s, side * 0.24 * s), tf(1.95 * s, side * 0.38 * s), color);
    }
    // ammo drum: a slotted ring, always spinning
    let dc = tf(-0.30 * s, 0.0);
    gizmos.circle_2d(Isometry2d::from_translation(dc), 0.30 * s, dim(color, 0.9));
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU + t * 1.6;
        gizmos.circle_2d(Isometry2d::from_translation(dc + rot.rotate(Vec2::from_angle(a) * 0.18 * s)), 0.045 * s, Color::srgb(4.5, 6.0, 8.0));
    }
    gizmos.circle_2d(Isometry2d::from_translation(dc), 0.12 * s, Color::srgb(4.5, 6.0, 8.0));
    // engine pods + a flickering burn (amplitude-only shimmer)
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        if stage < 0.30 - i as f32 * 0.12 {
            continue; // popped during the death
        }
        let p0 = tf(-0.72 * s, side * 0.72 * s);
        let p1 = tf(-1.02 * s, side * 0.78 * s);
        gizmos.line_2d(p0, p1, dim(color, 0.75));
        let burn = 0.6 + 0.4 * (t * 2.6 + side).sin();
        gizmos.line_2d(p1, p1 + rot.rotate(Vec2::new(-0.28 * s * burn, 0.0)), dim(Color::srgb(2.4, 4.2, 7.0), burn));
    }
}

// The Detonator — an armored BLOOM: six petal plates sealed into a shell that HINGE OPEN as it
// primes (`open` 0..1) — the vulnerability window is literally visible. The caged core brightens
// when exposed; `alive_petals` stages the death (plates blow off one by one).
fn draw_detonator_body(gizmos: &mut Gizmos, c: Vec2, r: f32, t: f32, open: f32, alive_petals: usize, color: Color) {
    for k in 0..alive_petals.min(6) {
        let a = k as f32 / 6.0 * TAU + t * 0.12; // the whole array creeps — never static
        let tilt = 0.34 * open;
        let hinge = c + Vec2::from_angle(a) * r * 0.36;
        let left = c + Vec2::from_angle(a - 0.32 - tilt * 0.4) * r * (0.72 + 0.10 * open);
        let tip = c + Vec2::from_angle(a + tilt) * r * (0.98 + 0.42 * open);
        let right = c + Vec2::from_angle(a + 0.32 + tilt) * r * (0.72 + 0.16 * open);
        gizmos.linestrip_2d(vec![hinge, left, tip, right, hinge], color);
    }
    let expose = 0.3 + 0.7 * open;
    gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.15, dim(Color::srgb(6.0, 7.0, 2.0), expose));
    for k in 0..3 {
        // the cage: three slow-orbiting arcs around the core
        let a0 = k as f32 / 3.0 * TAU + t * 0.8;
        let arc: Vec<Vec2> = (0..=6).map(|i| c + Vec2::from_angle(a0 + i as f32 / 6.0 * 1.6) * r * 0.26).collect();
        gizmos.linestrip_2d(arc, dim(color, 0.5 + 0.4 * open));
    }
}

// The Pulsar — a living STAR: eight spikes that EXTEND blazing toward the lit beat and retract to a
// dim skeleton in the dark window (`lit_ease` runs ahead of the beat, so the extension telegraphs
// the invulnerable window before it lands), wrapped in two counter-rotating gyro arcs.
fn draw_pulsar_body(gizmos: &mut Gizmos, c: Vec2, r: f32, t: f32, lit_ease: f32, alive_spikes: usize, color_dark: Color) {
    let col = mix(dim(color_dark, 0.7), Color::srgb(6.0, 6.3, 7.0), lit_ease);
    for k in 0..alive_spikes.min(8) {
        let a = k as f32 / 8.0 * TAU + t * 0.10;
        let out = Vec2::from_angle(a);
        let base = c + out * r * 0.34;
        let tip = c + out * r * (0.42 + 0.68 * lit_ease);
        let side = out.perp() * r * 0.05;
        gizmos.linestrip_2d(vec![base + side, tip, base - side], col);
    }
    for (rr, spin, span) in [(0.72f32, 0.9f32, 3.6f32), (0.92, -0.65, 3.2)] {
        // gyroscope arcs, counter-rotating
        let a0 = t * spin;
        let arc: Vec<Vec2> = (0..=14).map(|i| c + Vec2::from_angle(a0 + i as f32 / 14.0 * span) * r * rr).collect();
        gizmos.linestrip_2d(arc, dim(color_dark, 0.45));
    }
    gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.30, col);
    gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.13, Color::srgb(6.5, 6.5, 7.0));
}

// One boss body at idle, by kind — shared by the background CAMEO and the warning BANNER so both
// always show the true silhouette (the same draw fns the fight uses).
fn draw_boss_idle(gizmos: &mut Gizmos, kind: BossKind, c: Vec2, r: f32, t: f32, col: Color) {
    match kind {
        BossKind::Warden => {
            draw_warden_body(gizmos, c, r * 0.8, t, Vec2::X, col);
            for k in 0..6 {
                // idle tentacles, waving
                let a = k as f32 / 6.0 * TAU + t * 0.3;
                let tip = c + Vec2::from_angle(a + 0.18 * (t * 1.5 + k as f32).sin()) * r * 1.5;
                draw_tentacle(gizmos, c, tip, t * 1.4 + k as f32 * 1.7, dim(col, 0.6));
            }
        }
        BossKind::Devourer => draw_glutton_body(gizmos, c, r, t, t * 2.0, 0.55, col),
        BossKind::Slinger => draw_slinger_body(gizmos, c, Vec2::X, r * 0.8, t, 0.0, 1.0, col),
        BossKind::Detonator => draw_detonator_body(gizmos, c, r, t, 0.25 + 0.25 * (t * 0.9).sin(), 6, col),
        BossKind::Pulsar => {
            let ease = (((t * PULSE_RATE).sin() + 0.30) / 0.45).clamp(0.0, 1.0);
            draw_pulsar_body(gizmos, c, r, t, ease, 8, col);
        }
        BossKind::Phantom => draw_haunt_skull(gizmos, c, r * 0.9, col, 0.5, t * 1.6, true, 0.0),
    }
}

// Boss rendering, split out of `render` (it was at the 16-param system limit): the warning banner +
// background cameo telegraph, and each boss's canonical body (see the draw fns above) plus its
// fight-specific dressing (tentacles, beams, prongs, rays).
// The shield rocks themselves draw as normal asteroids in `render`.
fn render_boss(
    mut gizmos: Gizmos,
    time: Res<Time>,
    arena: Res<Arena>,
    wave: Res<Wave>,
    bosses: Query<(&Boss, &Transform)>,
    shielded: Query<(&Transform, &Shielded)>,
    devourers: Query<(&Devourer, &Transform)>,
    slingers: Query<(&Slinger, &Transform)>,
    detonators: Query<(&Detonator, &Transform)>,
    pulsars: Query<(&Pulsar, &Transform)>,
    phantoms: Query<(&Phantom, &Transform)>,
    // the Haunt's extra visuals bundled into one param (three separate queries would exceed Bevy's 16-param limit)
    haunt: (Query<(&Possessed, &Transform)>, Query<(&SpectralTrail, &Transform)>, Query<(&EscapeShard, &Transform)>, Query<(&DepartingShip, &Transform, Option<&ShipTrail>)>),
    prime_targets: Query<&Transform, With<Asteroid>>,
    cannonballs: Query<(&Cannonball, &Transform)>,
    players: Query<&Transform, (With<Ship>, Without<Slinger>)>,
) {
    let (vessels, trails, shards, departing) = haunt;
    let h = arena.half;
    let t = time.elapsed_secs();
    let mc = boss_color();

    // ── the devourer (boss 2): a jagged red maw that swells as it feeds; a white-hot HP core ──
    for (dv, dt) in &devourers {
        let c = dt.translation.truncate();
        let scale = if dv.dying > 0.0 { (dv.dying / BOSS_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let r = devourer_radius(dv.grow) * scale;
        let throb = 1.0 + 0.06 * dv.pulse.sin();
        // overload telegraph: as it nears full it flashes white-hot (about to burst — get clear!). The
        // URGENCY ramps via the white-hot MIX (charge·flash below), NOT the flash rate — the rate stays
        // ≤~2.8 Hz (pulse advances at 5/s) so this big boss never strobes (photosensitivity).
        let charge = ((dv.grow - 0.7) / 0.3).clamp(0.0, 1.0);
        let flash = 0.5 + 0.5 * (dv.pulse * (2.0 + 1.5 * charge)).sin();
        let dc = if dv.dying <= 0.0 { mix(devourer_color(), Color::srgb(8.0, 7.5, 7.0), charge * flash) } else { devourer_color() };
        // the living maw: gnashing teeth rings + a gullet that glows with its gorge
        draw_glutton_body(&mut gizmos, c, r * throb, t, dv.pulse, dv.grow, dc);
        // ── NG+ TELEGRAPHS ── the fairness half of both upgrades. The INHALE's wedge is drawn from
        // the first frame of the gape (before anything is pulled) so you can see its reach and step
        // out of the cone; the REGURGITATE's wind-up draws the firing line it's about to spit along.
        // Brightness ramps only — no strobing.
        if dv.dying <= 0.0 {
            let facing = players.iter().next().map(|t| (t.translation.truncate() - c).normalize_or_zero()).unwrap_or(Vec2::NEG_Y);
            if dv.inhale > 0.0 {
                // gaping = telegraph, inhaling = live. Both draw the cone; live is brighter.
                let live = dv.inhaling();
                let g = if live { 0.8 } else { 0.3 + 0.4 * (1.0 - (dv.inhale - NGP_GLUT_INHALE_DUR) / NGP_GLUT_INHALE_WIND) };
                let base = facing.to_angle();
                for side in [-1.0f32, 1.0] {
                    let e = Vec2::from_angle(base + side * NGP_GLUT_INHALE_ARC);
                    gizmos.line_2d(c + e * r, c + e * NGP_GLUT_INHALE_REACH, dim(dc, g));
                }
                // the mouth's reach, as an arc of chords across the wedge
                let steps = 9;
                let pts: Vec<Vec2> = (0..=steps)
                    .map(|k| {
                        let f = k as f32 / steps as f32 * 2.0 - 1.0;
                        c + Vec2::from_angle(base + f * NGP_GLUT_INHALE_ARC) * NGP_GLUT_INHALE_REACH
                    })
                    .collect();
                gizmos.linestrip_2d(pts, dim(dc, g * 0.7));
                // inward chevrons, so the direction of the pull is unmistakable
                if live {
                    for k in 0..5 {
                        let f = k as f32 / 4.0 * 2.0 - 1.0;
                        let d = Vec2::from_angle(base + f * NGP_GLUT_INHALE_ARC * 0.8);
                        let phase = ((t * 1.4 + k as f32 * 0.2) % 1.0).clamp(0.0, 1.0);
                        let far = NGP_GLUT_INHALE_REACH * (1.0 - phase * 0.75);
                        gizmos.line_2d(c + d * far, c + d * (far - 26.0), dim(dc, 0.75));
                    }
                }
            }
            if dv.spit > 0.0 {
                // the FIRING LINE it's about to spit along, plus a swelling gullet
                let f = 1.0 - (dv.spit / NGP_GLUT_SPIT_WIND).clamp(0.0, 1.0);
                let base = facing.to_angle();
                for i in 0..NGP_GLUT_SPIT_ROCKS {
                    let k = if NGP_GLUT_SPIT_ROCKS > 1 { i as f32 / (NGP_GLUT_SPIT_ROCKS - 1) as f32 - 0.5 } else { 0.0 };
                    let d = Vec2::from_angle(base + k * NGP_GLUT_SPIT_ARC);
                    gizmos.line_2d(c + d * r, c + d * (r + 150.0 * f), dim(Color::srgb(6.0, 2.6, 1.4), 0.35 + 0.55 * f));
                }
                gizmos.circle_2d(Isometry2d::from_translation(c), r * (0.3 + 0.25 * f), dim(Color::srgb(6.0, 2.2, 1.2), 0.5 + 0.5 * f));
            }
        }
        // HP bar (top-center) — tracks its heal-toward-max; hidden once dying
        if dv.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, dv.hp as f32 / DEVOURER_HP_MAX as f32, devourer_color());
        }
    }

    // ── the Detonator (boss 4): an armored chartreuse hex that OPENS (bright core) while priming ──
    for (det, dtf) in &detonators {
        let c = dtf.translation.truncate();
        let scale = if det.dying > 0.0 { (det.dying / DETONATOR_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let r = DETONATOR_R * scale;
        let dc = detonator_color();
        let priming = det.prime > 0.0 && det.charge <= 0.0;
        // the BLOOM: petals hinge open through the priming channel (open = the vulnerability, visibly);
        // while DYING they've been blowing off one by one (staged in detonator_update) and what's left
        // gapes fully open around the failing core
        let open = if det.dying > 0.0 {
            1.0 - scale
        } else if priming {
            ((DETONATOR_PRIME_SECS - det.prime) * 5.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let alive_petals = if det.dying > 0.0 { (scale * 6.0).ceil() as usize } else { 6 };
        draw_detonator_body(&mut gizmos, c, r, t, open, alive_petals, dc);
        if priming {
            // the priming beam: MARCHING dashes crawling toward the rock it's cooking
            if let Some(rt) = det.target.and_then(|t| prime_targets.get(t).ok()) {
                let rp = rt.translation.truncate();
                let to = rp - c;
                let len = to.length().max(1.0);
                let d = to / len;
                let dash = 26.0;
                let mut x = (t * 160.0) % dash;
                while x < len {
                    let e = (x + dash * 0.55).min(len);
                    gizmos.line_2d(c + d * x, c + d * e, dc);
                    x += dash;
                }
                gizmos.circle_2d(Isometry2d::from_translation(rp), 5.0 + 3.0 * (det.pulse * 3.0).sin().abs(), Color::srgb(6.0, 7.0, 2.0));
            }
        }
        if det.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, det.hp as f32 / DETONATOR_HP as f32, dc);
        }
    }

    // ── the Pulsar (boss 5): a ring, bright-white when LIT (invulnerable) / dim when DARK (open to fire) ──
    for (pl, ptf) in &pulsars {
        let c = ptf.translation.truncate();
        let scale = if pl.dying > 0.0 { (pl.dying / PULSAR_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let r = PULSAR_R * scale * (1.0 + 0.05 * pl.pulse.sin());
        // the living star: spikes ease out ahead of the lit beat (the extension telegraphs the
        // invulnerable window) and retract to a dim skeleton while it's dark and open to fire.
        // Spikes shear off one by one while dying (staged in pulsar_update).
        let s = (t * PULSE_RATE + pl.phase).sin();
        let lit_ease = ((s + 0.30) / 0.45).clamp(0.0, 1.0);
        let alive_spikes = if pl.dying > 0.0 { (scale * 8.0).ceil() as usize } else { 8 };
        draw_pulsar_body(&mut gizmos, c, r, t, lit_ease, alive_spikes, pulsar_color());
        if pl.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, pl.hp as f32 / PULSAR_HP as f32, pulsar_color());
        }
    }

    // ── the Phantom (boss 6, FINALE) — THE HAUNT: a spectral skull, GHOSTLY-FAINT while intangible and
    //    blazing SOLID while surfaced (the post-ray punish window). Eye-embers flare as the ray charges,
    //    it dims + reforms during a reset, flashes on each phase break, and fires its Sweep Ray. ──
    for (sg, stf) in &phantoms {
        let c = stf.translation.truncate();
        // ── DEATH SCENE render: two beats. GATHER — the boss is drawn back to the middle (its form dimming);
        //    ERUPT — it's gone, replaced by a bright flash-glow at centre that blows outward + fades (the big
        //    light is particles). The fleeing core + departing ship draw separately below. ──
        if sg.victory > 0.0 {
            if !sg.erupted {
                let sc = mix(phantom_color(), Color::srgb(6.0, 1.6, 3.4), 0.5); // its phase-3 hue, dimming
                draw_haunt_skull(&mut gizmos, c, PHANTOM_R, dim(sc, 0.7), 1.0, sg.pulse, false, 0.4);
            } else {
                let f = (1.0 - (PHANTOM_VICTORY_SECS - sg.victory - 0.7) / 0.7).clamp(0.0, 1.0); // bloom + fade over ~0.7s
                if f > 0.0 {
                    let glow = Color::srgb(7.0, 7.6, 8.2);
                    gizmos.circle_2d(Isometry2d::from_translation(Vec2::ZERO), PHANTOM_R * (0.5 + 4.0 * (1.0 - f)), dim(glow, f * f));
                    gizmos.circle_2d(Isometry2d::from_translation(Vec2::ZERO), PHANTOM_R * 0.6 * f, dim(glow, f));
                }
            }
            continue; // custom death visuals — skip the normal skull/ray render
        }
        let hr = PHANTOM_R;
        // per-phase morph: spectral base → chartreuse (p2) → hot rose (p3)
        let sc = match sg.phase {
            1 => phantom_color(),
            2 => mix(phantom_color(), detonator_color(), 0.4),
            _ => mix(phantom_color(), Color::srgb(6.0, 1.6, 3.4), 0.5),
        };
        let surfaced = sg.vuln > 0.0;
        let resetting = sg.transition > 0.0;
        // the body READS its state: a faint apparition (ghost), a dim reforming wisp (reset), or SOLID
        // (the punish window — and all of phase 3, where the mask is off)
        let body = if resetting {
            dim(sc, 0.35)
        } else if surfaced {
            mix(sc, Color::srgb(7.0, 7.5, 7.0), 0.35) // blazing solid — SHOOT IT NOW
        } else if sg.phase >= 3 {
            sc // solid full-time — the mask is off
        } else {
            dim(sc, 0.5 + 0.12 * (sg.pulse * 1.7).sin()) // ghostly, shimmering faint
        };
        let ember = if surfaced || sg.aim > 0.0 || sg.charging > 0.0 {
            1.0 // blazing — exposed, or locked on for a charge
        } else {
            match sg.ray {
                RayPhase::Telegraph => 1.0, // eyes blaze as the beam charges — part of the tell
                RayPhase::Fire => 0.7,
                RayPhase::Idle => 0.25,
            }
        };
        let solid = surfaced || sg.phase >= 3; // phase 3 is solid full-time — no ghost waver
        // `open`: the mask SPLITS around its searing core through the surface window, sealing as it closes
        let open = if surfaced { (sg.vuln / PHANTOM_MATERIALIZE).clamp(0.0, 1.0) } else { 0.0 };
        // while it's hiding INSIDE a possessed vessel (p2) the ghost isn't drawn — only the vessel is
        if !(sg.phase == 2 && sg.possessed.is_some()) {
            draw_haunt_skull(&mut gizmos, c, hr, body, ember, sg.pulse, !solid, open);
        }
        // phase-3 AIM telegraph: the locked charge line flashes ahead of the dash — sidestep it
        if sg.aim > 0.0 {
            let frac = 1.0 - (sg.aim / PHANTOM_CHARGE_AIM).clamp(0.0, 1.0);
            let flash = 0.25 + 0.6 * frac * (sg.pulse * 3.0).sin().abs(); // ≤~2.9 Hz (pulse at 3/s) — no strobe
            gizmos.line_2d(c, c + sg.charge_dir * (h.length() * 1.2), dim(phantom_ray_color(), flash));
        }
        // phase-start flash: a bright ring blows outward as it fractures into the next phase
        if sg.flash > 0.0 {
            let f = (sg.flash / 0.8).clamp(0.0, 1.0);
            gizmos.circle_2d(Isometry2d::from_translation(c), hr * (1.2 + 5.0 * (1.0 - f)), dim(Color::srgb(7.0, 8.0, 8.0), f));
        }
        // ── Sweep Ray: a pulsing warning wedge while telegraphing, then the lethal beam mid-sweep ──
        let ray_len = h.length() * 2.0 + 40.0; // FULL arena diagonal (matches phantom_update) — reaches the far edge from anywhere
        let rc = phantom_ray_color();
        match sg.ray {
            RayPhase::Telegraph => {
                let frac = (sg.ray_t / PHANTOM_RAY_TELEGRAPH).clamp(0.0, 1.0);
                let flash = 0.18 + 0.5 * frac * (sg.pulse * 3.0).sin().abs(); // pulses harder as ignition nears (rate ≤~2.9 Hz — the quadrant must not strobe)
                for k in 0..=8 {
                    let a = sg.ray_from + sg.ray_span * k as f32 / 8.0; // fill lines across the doomed quadrant
                    gizmos.line_2d(c + Vec2::from_angle(a) * PHANTOM_RAY_INNER_R, c + Vec2::from_angle(a) * ray_len, dim(rc, flash * 0.5));
                }
                for edge in [sg.ray_from, sg.ray_from + sg.ray_span] {
                    gizmos.line_2d(c + Vec2::from_angle(edge) * PHANTOM_RAY_INNER_R, c + Vec2::from_angle(edge) * ray_len, dim(rc, flash + 0.3));
                }
            }
            RayPhase::Fire => {
                let cur = (sg.ray_t / PHANTOM_RAY_FIRE).clamp(0.0, 1.0);
                let dir = Vec2::from_angle(sg.ray_from + sg.ray_span * cur);
                let perp = dir.perp();
                for (off, bright) in [(-1.0, 0.35), (-0.5, 0.6), (0.0, 1.0), (0.5, 0.6), (1.0, 0.35)] {
                    let o = perp * off * PHANTOM_RAY_WIDTH * 0.5; // a few parallel strokes → a thick, bright beam
                    gizmos.line_2d(c + dir * PHANTOM_RAY_INNER_R + o, c + dir * ray_len + o, dim(rc, bright));
                }
            }
            RayPhase::Idle => {}
        }
        // per-phase HP bar (refills on each phase reset — its resets already read the phase progress)
        boss_hp_bar(&mut gizmos, h.y - 42.0, sg.hp as f32 / PHANTOM_PHASE_HP as f32, sc);
    }

    // ── phase-2 VESSELS: the haunted rock the Haunt is hiding in — a jagged chunk lit with a pulsing
    //    possessed glow, two ember eyes peering out (so you know which rock to break) ──
    for (pv, pvt) in &vessels {
        let c = pvt.translation.truncate();
        let glow = 0.6 + 0.4 * pv.pulse.sin();
        let hue = mix(phantom_color(), Color::srgb(6.0, 1.6, 3.4), 0.4); // hot-spectral — clearly not a normal rock
        let pts: Vec<Vec2> = pv.verts.iter().map(|v| c + *v).collect();
        let mut loop_pts = pts.clone();
        if let Some(&first) = loop_pts.first() {
            loop_pts.push(first);
        }
        gizmos.linestrip_2d(loop_pts, dim(hue, 0.65 + 0.35 * glow));
        gizmos.circle_2d(Isometry2d::from_translation(c), PHANTOM_POSSESS_R * (0.5 + 0.15 * glow), dim(hue, 0.3 * glow));
        for side in [-1.0f32, 1.0] {
            gizmos.circle_2d(Isometry2d::from_translation(c + Vec2::new(side * PHANTOM_POSSESS_R * 0.28, PHANTOM_POSSESS_R * 0.12)), PHANTOM_POSSESS_R * 0.09, dim(phantom_ray_color(), glow));
        }
    }

    // ── phase-3 spectral wake: each afterimage burns bright then gutters out as its ttl fades ──
    for (tr, tt) in &trails {
        let f = (tr.ttl / PHANTOM_TRAIL_TTL).clamp(0.0, 1.0);
        let c = tt.translation.truncate();
        gizmos.circle_2d(Isometry2d::from_translation(c), PHANTOM_TRAIL_R * (0.5 + 0.5 * f), dim(phantom_ray_color(), 0.15 + 0.55 * f));
    }

    // ── the fleeing TRUE-FORM CORE (the sequel seed): deliberately SUBTLE — a small pale chunk streaking
    //    east, easy to lose among the burst of light unless you're really watching for it ──
    for (sh, stf) in &shards {
        let c = stf.translation.truncate();
        let core = mix(phantom_color(), Color::srgb(6.0, 6.0, 7.0), 0.5); // pale, not blazing
        // a faint short comet streak
        let n = sh.trail.len();
        for i in 1..n {
            let f = i as f32 / n as f32;
            gizmos.line_2d(sh.trail[i - 1], sh.trail[i], dim(core, 0.03 + 0.22 * f));
        }
        // the little chunk (spinning) — no bright halo, just a small silhouette + a dim pinpoint
        let pts: Vec<Vec2> = sh.verts.iter().map(|v| c + Vec2::from_angle(sh.spin).rotate(*v)).collect();
        let mut loop_pts = pts.clone();
        if let Some(&first) = loop_pts.first() {
            loop_pts.push(first);
        }
        gizmos.linestrip_2d(loop_pts, dim(core, 0.8));
        gizmos.circle_2d(Isometry2d::from_translation(c), 1.6, dim(core, 0.9));
    }

    // ── the hero's DEPARTING ship: warps off east after the shard — a ship silhouette (nose east) with a
    //    bright light trail streaming behind it (full-burn for the send-off) ──
    for (ds, dtf, dtrail) in &departing {
        let c = dtf.translation.truncate();
        let sc = ship_color();
        // the same Tron light ribbon as in play, streaming behind the send-off
        if let Some(tr) = dtrail {
            draw_light_ribbon(&mut gizmos, &tr.0, sc);
        }
        // thrust flame out the back (west), flickering — full-burn for the send-off
        let fl = 0.7 + 0.3 * ds.flame.sin();
        gizmos.linestrip_2d(
            [c + Vec2::new(-SHIP_R * 0.5, -5.0), c + Vec2::new(-SHIP_R * 0.5 - 22.0 * fl, 0.0), c + Vec2::new(-SHIP_R * 0.5, 5.0)],
            dim(flame_color(), fl),
        );
        // hull (nose = +X = east) — the same filled dart as in play
        draw_ship(&mut gizmos, c, Vec2::X, SHIP_R, sc, true);
    }

    // ── the run-up telegraph: the WARNING BANNER + the background cameo, both drawn with the boss's
    //    CANONICAL body (draw_boss_idle) — the silhouette you're warned about is the one you fight. ──
    if boss_incoming(&wave) {
        let prog = ((BOSS_CAMEO_SECS - wave.timer) / BOSS_CAMEO_SECS).clamp(0.0, 1.0);
        let kind = boss_kind(wave.level + 1);
        // background cameo: the boss drifts by, faint, alive
        let cam = Vec2::new(-h.x - 150.0 + (2.0 * h.x + 300.0) * prog, h.y * 0.45);
        draw_boss_idle(&mut gizmos, kind, cam, BOSS_R * 1.5, t, dim(boss_kind_color(kind), 0.22));
        // the BANNER: a hazard band framing the ui warning line ("WARNING: THE X INCOMING" sits at
        // ui top:40% = world y ≈ h.y*0.2), its edges ticked and MARCHING, the boss's true body beside
        // the name. Alpha tracks the same ramp + ≤3 Hz pulse as the warning text.
        let by = h.y * 0.2;
        let a = prog * (0.6 + 0.4 * (wave.timer * 4.5).sin());
        let bcol = dim(boss_kind_color(kind), a);
        for sy in [-1.0f32, 1.0] {
            let y = by + sy * 54.0;
            gizmos.line_2d(Vec2::new(-h.x * 0.72, y), Vec2::new(h.x * 0.72, y), dim(bcol, 0.85));
            // hazard ticks marching along the band edge
            let tick = 34.0;
            let mut x = -h.x * 0.72 + (t * 55.0) % tick;
            while x < h.x * 0.72 - 14.0 {
                gizmos.line_2d(Vec2::new(x, y - sy * 5.0), Vec2::new(x + 14.0, y + sy * 5.0), dim(bcol, 0.45));
                x += tick;
            }
        }
        // the boss itself, mini + alive, left of the name
        draw_boss_idle(&mut gizmos, kind, Vec2::new(-h.x * 0.52, by), 30.0, t, bcol);
    }

    let ship_eye = players.iter().next().map(|t| t.translation.truncate());
    for (boss, bt) in &bosses {
        let c = bt.translation.truncate();
        let scale = if boss.dying > 0.0 { (boss.dying / BOSS_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        // DYING, the arms shear off one by one (the bursts fire in boss_update; here the countdown
        // simply stops drawing each sheared arm)
        let arms_alive = if boss.dying > 0.0 { (scale * BOSS_ARMS as f32).ceil() as usize } else { BOSS_ARMS };
        // arms: a rippling tentacle to each held rock…
        let mut held: [bool; BOSS_ARMS] = [false; BOSS_ARMS];
        for (st, sh) in &shielded {
            let slot = sh.slot.min(BOSS_ARMS - 1);
            held[slot] = true;
            if slot < arms_alive {
                draw_tentacle(&mut gizmos, c, st.translation.truncate(), boss.pulse * 1.4 + slot as f32 * 1.7, dim(mc, 0.7));
            }
        }
        // …and idle stubs WAVING on the empty slots (the octopus is never still)
        for (k, taken) in held.iter().enumerate().take(arms_alive) {
            if !taken {
                let a = boss.rot + k as f32 / BOSS_ARMS as f32 * TAU;
                let tip = c + Vec2::from_angle(a + 0.18 * (t * 1.6 + k as f32).sin()) * BOSS_ORBIT_R * 0.45;
                draw_tentacle(&mut gizmos, c, tip, boss.pulse * 1.4 + k as f32 * 1.7, dim(mc, 0.45));
            }
        }
        // the armored VAULT + tracking eye. Blinks only while charging in (its intro invuln);
        // dying no longer blinks — it visibly breaks apart instead.
        let blink = boss.charge > 0.0;
        if !blink || ((boss.pulse * 3.0) as i32) % 2 == 0 {
            draw_warden_body(&mut gizmos, c, BOSS_R * scale, t, ship_eye.map(|s| s - c).unwrap_or(Vec2::X), mc);
        }
        // ── THE WHIRL'S TELEGRAPH (NG+) ── the fairness half of the attack. During the wind-up the
        // core CHARGES (a brightening inner disc) and the sweep's exact reach is drawn as a ring you
        // can stand outside of, with spokes reaching for it — so the danger zone is visible in full
        // BEFORE anything moves fast. Brightness ramps smoothly; nothing here strobes.
        if boss.dying <= 0.0 {
            match boss.whirl {
                Whirl::Wind => {
                    let f = 1.0 - (boss.whirl_t / NGP_WARDEN_WIND).clamp(0.0, 1.0); // 0→1 through the wind
                    let reach = BOSS_ORBIT_R * NGP_WARDEN_WHIRL_REACH * scale;
                    // the zone the sweep will cover, drawn from the first frame of the wind-up
                    gizmos.circle_2d(Isometry2d::from_translation(c), reach, dim(mc, 0.18 + 0.5 * f));
                    // spokes stretching out to it — the arms "reaching" before they rip around
                    for k in 0..BOSS_ARMS {
                        let a = boss.rot + k as f32 / BOSS_ARMS as f32 * TAU;
                        let d = Vec2::from_angle(a);
                        gizmos.line_2d(c + d * BOSS_ORBIT_R * scale, c + d * (BOSS_ORBIT_R + (reach - BOSS_ORBIT_R * scale) * f), dim(mc, 0.3 + 0.6 * f));
                    }
                    // the core winding up
                    gizmos.circle_2d(Isometry2d::from_translation(c), BOSS_R * scale * (0.34 + 0.5 * f), dim(Color::srgb(6.0, 3.4, 5.8), 0.4 + 0.6 * f));
                }
                Whirl::Spin => {
                    // mid-sweep: keep the reach ring lit so the live hazard boundary stays readable
                    let reach = BOSS_ORBIT_R * whirl_reach(boss.whirl, boss.whirl_t) * scale;
                    gizmos.circle_2d(Isometry2d::from_translation(c), reach, dim(mc, 0.75));
                }
                _ => {}
            }
        }
        // HP bar (top-center), hidden once it's dying (the fight's over)
        if boss.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, boss.hp as f32 / BOSS_HP as f32, mc);
        }
    }

    // ── the Slinger (boss 3): a large ice-blue GUNSHIP, its nose (cannon) tracking the player ──
    let sc = slinger_color();
    let ship_pos = players.iter().next().map(|t| t.translation.truncate());
    let slinger_pos = slingers.iter().next().map(|(_, t)| t.translation.truncate());
    for (sl, sltf) in &slingers {
        let c = sltf.translation.truncate();
        let scale = if sl.dying > 0.0 { (sl.dying / SLINGER_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let throb = 1.0 + 0.05 * sl.pulse.sin();
        let s = SLINGER_R * throb * scale;
        // point the hull at the player (nose = cannon); default facing down if there's no ship
        let face = ship_pos.map(|sp| (sp - c).to_angle()).unwrap_or(-std::f32::consts::FRAC_PI_2);
        let blink = sl.charge > 0.0;
        if !blink || ((sl.pulse * 3.0) as i32) % 2 == 0 {
            // DYING it LISTS — the hull rolls off its facing as it falls, wings/pods shearing away
            // (staged via `stage`; the bursts fire in slinger_update)
            let stage = if sl.dying > 0.0 { scale } else { 1.0 };
            let list = (1.0 - stage) * 1.1;
            let rot2 = Vec2::from_angle(face + list);
            draw_slinger_body(&mut gizmos, c, rot2, s, t, sl.recoil, stage, sc);
        }
        if sl.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, sl.hp as f32 / SLINGER_HP as f32, sc);
        }
    }
    // the Slinger's round: a tractor BEAM to a rock it's reeling in (not launched) + a hot core marking it
    for (cb, cbt) in &cannonballs {
        let rc = cbt.translation.truncate();
        if !cb.launched {
            if let Some(sp) = slinger_pos {
                let beam = 0.5 + 0.5 * (t * 15.0).sin(); // ~2.4 Hz (was 3.5) — no strobe on the tractor beam
                let perp = (rc - sp).perp().normalize_or_zero() * 7.0;
                gizmos.line_2d(sp, rc, dim(sc, 0.9 * beam)); // core beam
                gizmos.line_2d(sp + perp, rc + perp * 2.2, dim(sc, 0.35 * beam)); // cone edges
                gizmos.line_2d(sp - perp, rc - perp * 2.2, dim(sc, 0.35 * beam));
            }
        }
        let pulse = 0.6 + 0.4 * (t * 12.0).sin();
        let glow = if cb.launched { 1.4 } else { pulse };
        let r = asteroid_radius(3);
        gizmos.circle_2d(Isometry2d::from_translation(rc), r * 0.5, dim(sc, glow));
        gizmos.circle_2d(Isometry2d::from_translation(rc), r * 0.24, dim(sc, glow)); // hotter center
    }
}

// A boss HP bar across the top: a dim full-width track with a bright fill in the boss's colour.
// Shared by the Warden and the Devourer so they read identically.
fn boss_hp_bar(gizmos: &mut Gizmos, top_y: f32, frac: f32, color: Color) {
    let frac = frac.clamp(0.0, 1.0);
    let bw = 380.0;
    let x0 = -bw / 2.0;
    for i in 0..6 {
        let yy = top_y + (i as f32 - 2.5) * 2.2;
        gizmos.line_2d(Vec2::new(x0, yy), Vec2::new(x0 + bw, yy), dim(color, 0.18)); // track
        gizmos.line_2d(Vec2::new(x0, yy), Vec2::new(x0 + bw * frac, yy), color); // fill
    }
}

// Chain beams + the reward pickup orb (split out of `render` for the 16-param limit).
fn render_extras(
    mut gizmos: Gizmos,
    time: Res<Time>,
    chains: Query<(&Transform, &ChainShot)>,
    pickups: Query<(&Transform, &Pickup)>,
    tenders: Query<(&Tender, &Transform)>,
    tender_targets: Query<&Transform, With<Asteroid>>,
    drones: Query<(&Drone, &Transform)>,
    wells: Query<(&Well, &Transform)>,
) {
    let t = time.elapsed_secs();
    let cc = chain_color();
    let white = Color::srgb(5.0, 4.6, 5.6);
    // chain beams — a jagged lightning bolt between the two ends + bright end dots
    for (ct, cs) in &chains {
        let c = ct.translation.truncate();
        let a = c + cs.perp * CHAIN_HALF;
        let b = c - cs.perp * CHAIN_HALF;
        let segs = 7;
        let along = b - a;
        let perp = cs.perp.perp(); // unit perpendicular to the beam (i.e. the travel axis)
        let pts: Vec<Vec2> = (0..=segs)
            .map(|i| {
                let f = i as f32 / segs as f32;
                let jag = if i == 0 || i == segs { 0.0 } else { (t * 45.0 + i as f32 * 2.3).sin() * 12.0 };
                a + along * f + perp * jag
            })
            .collect();
        gizmos.linestrip_2d(pts, cc);
        gizmos.circle_2d(Isometry2d::from_translation(a), 4.0, white);
        gizmos.circle_2d(Isometry2d::from_translation(b), 4.0, white);
    }
    // TENDERS — a squat maintenance frame (deliberately MACHINED-looking: hard angles, no organic
    // curves, so it reads as built rather than grown) plus the two tractor beams when it's working.
    for (tender, tt, ) in &tenders {
        let c = tt.translation.truncate();
        let col = enemy_color();
        let spin = t * 0.7;
        // outer frame: a hexagonal cage
        let cage: Vec<Vec2> = (0..=6).map(|i| c + Vec2::from_angle(spin + i as f32 / 6.0 * TAU) * TENDER_R).collect();
        gizmos.linestrip_2d(cage, col);
        // inner core + three struts, machined and symmetrical
        gizmos.circle_2d(Isometry2d::from_translation(c), TENDER_R * 0.35, dim(col, 0.9));
        for i in 0..3 {
            let d = Vec2::from_angle(-spin * 1.4 + i as f32 / 3.0 * TAU);
            gizmos.line_2d(c + d * TENDER_R * 0.35, c + d * TENDER_R * 0.95, dim(col, 0.75));
        }
        // the TRACTOR BEAMS: two dashed lines to whatever it's hauling — the tell that a fusion is
        // in progress, and that shooting either fragment (or the drone) will stop it.
        if let Some((a, b)) = tender.job {
            let pulse = 0.55 + 0.45 * (t * 5.0).sin().abs();
            for target in [a, b] {
                if let Ok(rt) = tender_targets.get(target) {
                    let to = rt.translation.truncate();
                    let n = 7;
                    for k in 0..n {
                        if k % 2 == 1 {
                            continue; // dashed
                        }
                        let f0 = k as f32 / n as f32;
                        let f1 = (k as f32 + 0.9) / n as f32;
                        gizmos.line_2d(c + (to - c) * f0, c + (to - c) * f1, dim(col, 0.35 + 0.4 * pulse));
                    }
                }
            }
        }
    }
    // reward orb — a pulsing hexagon with a bright core, tinted for the weapon it grants
    for (pt, pk) in &pickups {
        let c = pt.translation.truncate();
        let throb = 1.0 + 0.14 * pk.pulse.sin();
        let col = match pk.kind {
            PickupKind::Chain => cc,
            PickupKind::Mass => mass_color(),
            PickupKind::Drone => drone_color(),
            PickupKind::Warhead => orange_color(),
            PickupKind::Nova => pulsar_color(), // the orb wears its boss's electric white-cyan (like Warhead wears the Detonator's orange); the granted shield itself is player-purple
            PickupKind::Aegis => boss_color(), // wears the Warden's magenta — the boss it was taken from
            PickupKind::Gorge => devourer_color(), // wears the Glutton's red — same rule
        };
        let hex: Vec<Vec2> = (0..=6)
            .map(|i| c + Vec2::from_angle(i as f32 / 6.0 * TAU + pk.rot) * PICKUP_R * throb)
            .collect();
        gizmos.linestrip_2d(hex, col);
        gizmos.circle_2d(Isometry2d::from_translation(c), PICKUP_R * 0.3 * throb, white);
    }
    // the ally drone — a small spinning violet craft with a bright core
    let dcol = drone_color();
    for (dr, dtf) in &drones {
        let c = dtf.translation.truncate();
        let spin = dr.angle * 2.0 + t * 3.0;
        let tri: Vec<Vec2> = (0..=3).map(|i| c + Vec2::from_angle(spin + i as f32 / 3.0 * TAU) * DRONE_R).collect();
        gizmos.linestrip_2d(tri, dcol);
        gizmos.circle_2d(Isometry2d::from_translation(c), DRONE_R * 0.4, white);
    }
    // gravity wells — inward-spiraling rose-red arms + a pulsing core, fading in on spawn / out on collapse
    let wc = well_color();
    for (w, wt) in &wells {
        let c = wt.translation.truncate();
        let fade = (w.life / 1.2).clamp(0.0, 1.0).min((WELL_LIFE - w.life) / 0.4).clamp(0.0, 1.0); // in then out
        let arms = 7;
        let segs = 10;
        for a in 0..arms {
            let a0 = a as f32 / arms as f32 * TAU;
            let pts: Vec<Vec2> = (0..=segs)
                .map(|s| {
                    let p = s as f32 / segs as f32;
                    let rad = WELL_R * 3.6 * (1.0 - 0.85 * p);
                    c + Vec2::from_angle(a0 + 5.0 * p + w.spin) * rad // MANY arms sweeping ~0.8 of a turn — a whirlpool, not a 4-arm cross
                })
                .collect();
            gizmos.linestrip_2d(pts, dim(wc, 0.7 * fade * (0.4 + 0.6 * (t * 6.0).sin().abs())));
        }
        gizmos.circle_2d(Isometry2d::from_translation(c), WELL_R * (1.0 + 0.12 * (t * 5.0).sin()), dim(wc, fade));
        gizmos.circle_2d(Isometry2d::from_translation(c), WELL_R * 0.45, dim(Color::srgb(6.0, 2.0, 3.5), fade)); // hot core
    }
}

// ─────────────────────────────── pause / game-over ────────────────────
fn pause_toggle(
    keys: Res<ButtonInput<KeyCode>>,
    input: Res<ActionState>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut clicks: EventReader<MenuClick>,
) {
    let actions: Vec<MenuAction> = clicks.read().map(|c| c.0).collect();
    match state.get() {
        GameState::Playing => {
            if input.pause {
                next.set(GameState::Paused);
            }
        }
        GameState::Paused => {
            // the Pause action (Esc / Start) resumes, Q / the buttons quit
            if input.pause || actions.contains(&MenuAction::Resume) {
                next.set(GameState::Playing); // resume
            } else if keys.just_pressed(KeyCode::KeyQ) || actions.contains(&MenuAction::Quit) {
                next.set(GameState::Menu); // quit the run → OnEnter(Menu) wipes the field
            }
        }
        GameState::Splash | GameState::Menu | GameState::Achievements | GameState::Lore | GameState::Gallery | GameState::Controls | GameState::Briefing | GameState::GameOver | GameState::Victory => {}
    }
}

// A full-screen centered overlay root. Returns EntityCommands so the caller adds children.
fn overlay(commands: &mut Commands, marker: impl Component, alpha: f32) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                // keep the gap TIGHT: every screen pays it between every child, and the busy screens
                // (Controls, Lore) must fit the 800px design height with margin — no clipping
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.01, 0.06, alpha)),
        ))
        .id()
}

fn text(font_size: f32, color: Color, s: &str) -> (Text, TextFont, TextColor) {
    (Text::new(s), TextFont { font_size, ..default() }, TextColor(color))
}

// The embedded Orbitron display font — used across the menu screens (the tiny in-game HUD keeps
// the default mono for crispness).
#[derive(Resource)]
struct MenuFont(Handle<Font>);

// Embed Orbitron and install `MenuFont` at BUILD time (not a Startup system): the initial
// OnEnter(Menu) → spawn_menu_ui runs before a Startup command flush would land, so it must
// already exist. Called from `main` after DefaultPlugins (which provides `Assets<Font>`).
fn install_menu_font(app: &mut App) {
    let bytes = include_bytes!("../assets/fonts/static/Orbitron-Bold.ttf").to_vec();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::try_from_bytes(bytes).expect("Orbitron-Bold.ttf is a valid TTF"));
    app.insert_resource(MenuFont(handle));
}

// The logo (purple spear), embedded so the exe stays self-contained.
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");
// The BAZ STUDIOS logo + sting for the boot splash — the same files Wingman ships, so every
// studio game opens identically.
const BAZ_LOGO_PNG: &[u8] = include_bytes!("../assets/baz_logo.png");
const BAZ_STING_MP3: &[u8] = include_bytes!("../assets/logo_sound.mp3");
// PRODUCED MUSIC (2026-07-30): the first externally-produced track in the score — the GAME OVER
// theme, generated in Antigravity from the procedural dirge as reference (melancholic ambient
// synthwave: analog pads over Am-Fmaj7-Dm7-E7, Rhodes arpeggios, sub bass; 15s, 192kbps, loops
// gap-free — verified no silence at either edge). Embedded like every other asset so the exe stays
// self-contained. The procedural `audio::gameover_track_wav` is no longer shipped — it survives as
// the reference render + fallback.
const GAMEOVER_MP3: &[u8] = include_bytes!("../assets/gameover.mp3");
// MAIN + BOSS, also produced in Antigravity (2026-07-30). MAIN ships WITHOUT corruption tiers for
// now — `mains` holds this one track and the tier index clamps to it, so the score never restarts
// mid-run. The tier plumbing (`MusicCue::Main(tier)`) is intact but DORMANT: drop in per-act
// produced variants and it wakes up untouched. See DESIGN.md "PRODUCED music".
const MAIN_MP3: &[u8] = include_bytes!("../assets/main.mp3");
const BOSS_MP3: &[u8] = include_bytes!("../assets/boss.mp3");
// Measured gain trims so every cue sits at one level (mean loudness vs the procedural score, which
// MUSIC_VOLUME was tuned against): game-over came in 1.6 dB quiet; main is 4.3 dB HOT and boss
// 2.7 dB hot, and both peak at full scale — trimming also restores headroom over the sfx layer.
const GAMEOVER_GAIN: f32 = 1.2;
const MAIN_GAIN: f32 = 0.61;
const BOSS_GAIN: f32 = 0.73;

#[derive(Resource)]
struct LogoImage(Handle<Image>);
#[derive(Resource)]
struct BazLogoImage(Handle<Image>);

// Decode an embedded PNG. Keeps the CPU copy (RenderAssetUsages::default) so the window-icon
// system can read its RGBA bytes.
fn decode_png(bytes: &[u8], what: &str) -> Image {
    Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true, // colour image (sRGB)
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .unwrap_or_else(|_| panic!("{what} is a valid PNG"))
}

fn decode_logo() -> Image {
    decode_png(LOGO_PNG, "assets/logo.png")
}

// Install the menu-masthead + splash logos at BUILD time (like the font) so the initial
// OnEnter screens can use them.
fn install_logo(app: &mut App) {
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(decode_logo());
    app.insert_resource(LogoImage(handle));
    let studio = app.world_mut().resource_mut::<Assets<Image>>().add(decode_png(BAZ_LOGO_PNG, "assets/baz_logo.png"));
    app.insert_resource(BazLogoImage(studio));
}

// ─────────────────────────────── boot splash (Baz Studios) ────────────
// The studio card every Baz game opens on: black screen, the BAZ STUDIOS logo fading in with the
// sting, auto-dismissing into the menu. Timings mirror Wingman's splash (0.5s in, dismiss at 3.5s);
// any key/click/pad press skips ahead to the fade-out — never a hard cut. One slow fade, no pulses
// (photosensitivity rule). Music is held silent until the Menu so the sting owns the moment.
const SPLASH_FADE_IN: f32 = 0.5;
const SPLASH_HOLD_UNTIL: f32 = 3.5; // fade-out begins here (or on any input, whichever is first)
const SPLASH_FADE_OUT: f32 = 0.8;
const SPLASH_STING_VOLUME: f32 = 0.7;

#[derive(Component)]
struct SplashUi;
#[derive(Component)]
struct SplashLogo;
#[derive(Resource, Default)]
struct SplashClock(f32);

fn spawn_splash(mut commands: Commands, logo: Res<BazLogoImage>, mut sources: ResMut<Assets<AudioSource>>) {
    commands
        .spawn((
            SplashUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK), // opaque card — the starfield stays hidden until the menu
        ))
        .with_children(|p| {
            p.spawn((
                SplashLogo,
                ImageNode { image: logo.0.clone(), color: Color::WHITE.with_alpha(0.0), ..default() },
                Node { width: Val::Px(560.0), ..default() },
            ));
        });
    let sting = sources.add(AudioSource { bytes: BAZ_STING_MP3.to_vec().into() });
    one_shot(&mut commands, sting, SPLASH_STING_VOLUME);
}

fn splash_update(
    time: Res<Time>,
    mut clock: ResMut<SplashClock>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    mut next: ResMut<NextState<GameState>>,
    mut logo: Query<&mut ImageNode, With<SplashLogo>>,
) {
    clock.0 += time.delta_secs();
    let skip = keys.get_just_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some()
        || pads.iter().any(|g| g.get_just_pressed().next().is_some());
    if skip && clock.0 < SPLASH_HOLD_UNTIL {
        clock.0 = SPLASH_HOLD_UNTIL; // skip = jump to the dismiss point, keeping the fade
    }
    let a = if clock.0 < SPLASH_HOLD_UNTIL {
        (clock.0 / SPLASH_FADE_IN).clamp(0.0, 1.0)
    } else {
        1.0 - ((clock.0 - SPLASH_HOLD_UNTIL) / SPLASH_FADE_OUT).clamp(0.0, 1.0)
    };
    for mut img in &mut logo {
        img.color = Color::WHITE.with_alpha(a);
    }
    if clock.0 >= SPLASH_HOLD_UNTIL + SPLASH_FADE_OUT {
        next.set(GameState::Menu);
    }
}

fn despawn_splash(mut commands: Commands, q: Query<Entity, With<SplashUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// Set the window / taskbar icon from the same logo. Startup system — the primary window exists by
// then on desktop. `NonSend` because winit's window handle isn't `Send`.
fn set_window_icon(windows: NonSend<WinitWindows>, primary: Query<Entity, With<PrimaryWindow>>) {
    let Ok(entity) = primary.single() else {
        return;
    };
    let Some(win) = windows.get_window(entity) else {
        return;
    };
    let img = decode_logo();
    let (w, h) = (img.width(), img.height());
    let Some(rgba) = img.data else {
        return;
    };
    if let Ok(icon) = winit::window::Icon::from_rgba(rgba, w, h) {
        win.set_window_icon(Some(icon));
    }
}

// Like `text`, but in the menu (Orbitron) font.
fn text_f(font: &Handle<Font>, font_size: f32, color: Color, s: &str) -> (Text, TextFont, TextColor) {
    (Text::new(s), TextFont { font: font.clone(), font_size, ..default() }, TextColor(color))
}

fn spawn_pause_ui(
    mut commands: Commands,
    bindings: Res<Bindings>,
    method: Res<InputMethod>,
    pads: Query<(), With<Gamepad>>,
    font: Res<MenuFont>,
) {
    let root = overlay(&mut commands, PauseUi, 0.72);
    let f = &font.0;
    // CONTROLS reference (read-only): the ACTIVE device's current binds, so nobody has to quit a run
    // to remember a key. Snapshotted at pause time — rebinding only happens on the menu's Controls
    // screen, so the binds can't change while this is up. Rebind hint at the bottom.
    let active = method.active(!pads.is_empty());
    let list = if active == InputMethod::Controller { &bindings.pad } else { &bindings.kbm };
    let rows: [(&str, Action); 8] = [
        ("THRUST", Action::Thrust),
        ("TURN LEFT", Action::TurnLeft),
        ("TURN RIGHT", Action::TurnRight),
        ("FIRE", Action::Fire),
        ("WARP", Action::Warp),
        ("CHAIN BEAM", Action::Chain),
        ("SHOT MODE", Action::ToggleShot),
        ("MUTE", Action::Mute),
    ];
    let head = Color::srgb(0.72, 0.76, 0.9);
    let key_col = Color::srgb(0.85, 0.88, 1.0);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 44.0, title_color(), "PAUSED"));
        menu_button(p, f, MenuAction::Resume, "RESUME  (Esc)");
        menu_button(p, f, MenuAction::Quit, "QUIT TO MENU  (Q)");
        p.spawn((text_f(f, 20.0, title_color(), "CONTROLS"), Node { margin: UiRect::top(Val::Px(18.0)), ..default() }));
        p.spawn(text_f(f, 12.0, dim(head, 0.8), &format!("({})", active.label())));
        for (name, action) in rows {
            // compact two-column row (narrower than the menu screens' table — it's a reference card)
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                width: Val::Px(340.0),
                padding: UiRect::vertical(Val::Px(2.0)),
                ..default()
            })
            .with_children(|row| {
                row.spawn((text_f(f, 14.0, head, name), Node { width: Val::Px(150.0), ..default() }));
                row.spawn(text_f(f, 14.0, key_col, &binds_label(list, action)));
            });
        }
        p.spawn((text_f(f, 12.0, dim(head, 0.75), "Rebind from the main menu's CONTROLS screen."), Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
    });
}

fn spawn_gameover_ui(mut commands: Commands, score: Res<Score>, hs: Res<HighScores>, wave: Res<Wave>, stats: Res<Stats>, font: Res<MenuFont>) {
    let root = overlay(&mut commands, GameOverUi, 0.72);
    let f = &font.0;
    let gold = Color::srgb(0.98, 0.85, 0.35);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 62.0, Color::srgb(1.0, 0.3, 0.3), "GAME OVER"));
        p.spawn(text_f(f, 24.0, Color::srgb(0.85, 0.9, 1.2), &format!("SCORE   {}", score.0)));
        // the "you were close" line — a death must still read as measurable progress. best_wave was
        // already refreshed by record_high_score (it runs first), so reached >= best ⇔ a new record.
        let reached = wave.level as u32;
        if reached >= stats.best_wave {
            p.spawn(text_f(f, 18.0, gold, &format!("REACHED WAVE {reached}   —   NEW BEST!")));
        } else {
            p.spawn(text_f(f, 18.0, Color::srgb(0.72, 0.78, 1.0), &format!("REACHED WAVE {reached}   —   BEST {}", stats.best_wave)));
        }
        // the "one more run" hook: the lifetime grind closest to unlocking still ticked up this run
        if let Some((a, c, t)) = nearest_grind(&stats) {
            let (name, _) = ach_meta(a);
            p.spawn(text_f(f, 13.0, Color::srgb(0.55, 0.6, 0.78), &format!("{}   {c} / {t}", name.to_uppercase())));
        }
        // banner if this run cracked the table
        match hs.just_placed {
            Some(0) => {
                p.spawn(text_f(f, 26.0, gold, "NEW BEST!"));
            }
            Some(_) => {
                p.spawn(text_f(f, 22.0, gold, "TOP 5!"));
            }
            None => {}
        }
        // the top-5 table, with this run's placement lit up
        p.spawn((text_f(f, 18.0, title_color(), "HIGH SCORES"), Node { margin: UiRect::top(Val::Px(10.0)), ..default() }));
        for (i, &s) in hs.top.iter().enumerate() {
            let col = if hs.just_placed == Some(i) { gold } else { Color::srgb(0.7, 0.75, 0.9) };
            p.spawn(text_f(f, 18.0, col, &format!("{}.   {}", i + 1, s)));
        }
        p.spawn((text_f(f, 20.0, Color::srgb(0.7, 0.85, 1.2), "Restart  (Enter)"), Node { margin: UiRect::top(Val::Px(10.0)), ..default() }));
        p.spawn(text_f(f, 20.0, Color::srgb(0.7, 0.85, 1.2), "Main Menu  (Esc)"));
    });
}

fn despawn_pause_ui(mut commands: Commands, q: Query<Entity, With<PauseUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn despawn_gameover_ui(mut commands: Commands, q: Query<Entity, With<GameOverUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// The WIN screen — shown once the Phantom (wave 30) falls. Its NG+ line is a teaser; the New Game+
// mode itself (a separate 1–30-harder run) is designed later.
fn spawn_victory_ui(mut commands: Commands, score: Res<Score>, font: Res<MenuFont>, mut reveal: ResMut<VictoryReveal>) {
    reveal.0 = 0.0; // restart the reveal
    let root = overlay(&mut commands, VictoryUi, 0.78);
    let f = &font.0;
    let gold = Color::srgb(0.98, 0.85, 0.35);
    let title = Color::srgb(0.5, 1.0, 0.7);
    let body = Color::srgb(0.82, 0.9, 1.2);
    let dim_c = Color::srgb(0.7, 0.75, 0.9);
    let prompt = Color::srgb(0.7, 0.85, 1.2);
    // each line FADES IN on a stagger (see victory_reveal) — a slow, credits-style reveal, not a pop
    commands.entity(root).with_children(|p| {
        p.spawn((text_f(f, 54.0, title.with_alpha(0.0), "YOU SAVED THE PLANET"), VictoryLine { delay: 0.3, color: title }));
        // (the fleeing core is never NAMED — it's the odd thing only players really watching will catch)
        p.spawn((text_f(f, 19.0, body.with_alpha(0.0), "The Belt is still. The Phantom is gone. Home is safe - for now."), VictoryLine { delay: 1.6, color: body }));
        // the canon's sequel hook, revealed last among the story beats: who the Haunt answered to
        p.spawn((text_f(f, 16.0, dim_c.with_alpha(0.0), "Far past the edge, the ARCHITECT is still building."), VictoryLine { delay: 2.9, color: dim_c }));
        p.spawn((text_f(f, 24.0, body.with_alpha(0.0), &format!("FINAL SCORE   {}", score.0)), VictoryLine { delay: 4.0, color: body }, Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
        p.spawn((text_f(f, 26.0, gold.with_alpha(0.0), "*  NEW GAME+ UNLOCKED  *"), VictoryLine { delay: 5.4, color: gold }, Node { margin: UiRect::top(Val::Px(16.0)), ..default() }));
        p.spawn((text_f(f, 15.0, dim_c.with_alpha(0.0), "Replay waves 1-30 at higher difficulty - coming soon."), VictoryLine { delay: 6.4, color: dim_c }));
        p.spawn((text_f(f, 20.0, prompt.with_alpha(0.0), "Main Menu  (Enter)"), VictoryLine { delay: 7.8, color: prompt }, Node { margin: UiRect::top(Val::Px(16.0)), ..default() }));
    });
}

// Slow, credits-style reveal: fade each victory line in on its own stagger.
fn victory_reveal(time: Res<Time>, mut reveal: ResMut<VictoryReveal>, mut q: Query<(&VictoryLine, &mut TextColor)>) {
    reveal.0 += time.delta_secs();
    for (line, mut color) in &mut q {
        let a = ((reveal.0 - line.delay) / 1.3).clamp(0.0, 1.0);
        color.0 = line.color.with_alpha(a);
    }
}

fn despawn_victory_ui(mut commands: Commands, q: Query<Entity, With<VictoryUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// Reset every run resource and spawn a fresh ship. Shared by the menu Start and the restart.
fn reset_run(
    commands: &mut Commands,
    run: &mut Run,
    score: &mut Score,
    wave: &mut Wave,
    banner: &mut WaveBanner,
    warp: &mut Warp,
    boss: &mut BossState,
    chain: &mut Chain,
    mass: &mut MassShot,
    warhead: &mut Warhead,
    gorge: &mut Gorge,
    flags: &mut RunFlags,
    gold: &mut GoldRush,
    stats: &mut Stats,
    watch: &mut PacifistWatch,
) {
    run.lives = START_LIVES;
    run.respawn = 0.0;
    run.nova = Nova::default(); // the Nova Shield must be re-earned (like every other pickup)
    run.aegis = Aegis::default(); // …and so must the Aegis Shards
    run.died = false; // fresh deathless slate (achievement: Untouchable)
    run.powerup_fires = 0;
    // prime the Pacifist watch on wave 1 with the CURRENT lifetime totals (streaks never span runs)
    *watch = PacifistWatch { primed_at_level: 1, breaks: total_breaks(stats), fires: 0, streak: 0 };
    stats.runs += 1; // every launch counts — dying a lot is the expected way to play
    save_progress(stats); // persist immediately: a rage-quit mid-run still counts the attempt
    score.0 = 0;
    wave.level = 1;
    wave.timer = WAVE_SECS;
    wave.calm = 0.0;
    banner.timer = WAVE_BANNER_SECS; // flash "WAVE 1"
    warp.charges = WARP_MAX_CHARGES;
    warp.cooldown = 0.0;
    boss.fought = 0; // so the next boss wave spawns a fresh boss
    *chain = Chain::default(); // must re-earn the chain shot…
    *mass = MassShot::default(); // …and the mass shot
    *warhead = Warhead::default(); // …and the Warhead rounds
    *gorge = Gorge::default(); // …and the Gorge round
    *flags = RunFlags::default(); // fresh "no powerups used" flag for Purist
    *gold = GoldRush::default(); // no stale gold hunt carried into the new run…
    gold.cooldown = GOLD_INITIAL_DELAY; // …and a grace before the first gold rock can appear
    spawn_player(commands);
}

// Wipe the run's entities when entering the menu (after a quit or game-over → menu). The
// starfield + camera are excluded by `GameplayEntity`, so the backdrop persists.
fn clear_field(mut commands: Commands, field: Query<Entity, GameplayEntity>) {
    for e in &field {
        commands.entity(e).despawn();
    }
}

// Main menu: Enter / Space begins a fresh run.
fn menu_start(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut next: ResMut<NextState<GameState>>,
    mut run: ResMut<Run>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut warp: ResMut<Warp>,
    mut progress: (ResMut<BossState>, ResMut<Chain>, ResMut<MassShot>, ResMut<RunFlags>, ResMut<GoldRush>, ResMut<Warhead>, ResMut<Stats>, ResMut<PacifistWatch>, ResMut<NewGamePlus>, ResMut<Gorge>), // bundled (16-param limit)
    mut clicks: EventReader<MenuClick>,
) {
    let actions: Vec<MenuAction> = clicks.read().map(|c| c.0).collect(); // read once, then test
    // sub-screens: their button, or a keyboard shortcut
    if keys.just_pressed(KeyCode::KeyA) || actions.contains(&MenuAction::Achievements) {
        next.set(GameState::Achievements);
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) || actions.contains(&MenuAction::Controls) {
        next.set(GameState::Controls);
        return;
    }
    if keys.just_pressed(KeyCode::KeyB) || actions.contains(&MenuAction::Briefing) {
        next.set(GameState::Briefing);
        return;
    }
    if keys.just_pressed(KeyCode::KeyL) || actions.contains(&MenuAction::Lore) {
        next.set(GameState::Lore);
        return;
    }
    if keys.just_pressed(KeyCode::KeyG) || actions.contains(&MenuAction::Gallery) {
        next.set(GameState::Gallery);
        return;
    }
    // Play: Enter/Space or the button. NEW GAME+ is BUTTON-ONLY (no shortcut) — deliberate friction:
    // the second lap is chosen, never stumbled into. Keyboard launch is always a normal run.
    let play_plus = actions.contains(&MenuAction::PlayPlus);
    if !(keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || actions.contains(&MenuAction::Play) || play_plus) {
        return;
    }
    progress.8 .0 = play_plus; // the mode holds for the whole run (restarts included); normal PLAY clears it
    reset_run(&mut commands, &mut run, &mut score, &mut wave, &mut banner, &mut warp, &mut progress.0, &mut progress.1, &mut progress.2, &mut progress.5, &mut progress.9, &mut progress.3, &mut progress.4, &mut progress.6, &mut progress.7);
    next.set(GameState::Playing);
}

// Deep neon violet for menu titles. UI TextColor CLAMPS each channel to 1.0, so the old
// HDR-style (2.2, .35, 5.5) collapsed to (1, .35, 1) = hot pink. Kept ≤1 and B-dominant → violet.
fn title_color() -> Color {
    Color::srgb(0.62, 0.18, 1.0)
}
// Bright violet for earned achievements (≤1 so it doesn't clamp to white in the UI).
fn ach_earned_color() -> Color {
    Color::srgb(0.82, 0.45, 1.0)
}

// A slick menu button — a bordered violet pill with a label. `button_shimmer` animates the hover
// glow and `button_click` fires its `MenuAction`; the keyboard shortcuts do the same thing.
fn menu_button(p: &mut ChildSpawnerCommands, font: &Handle<Font>, action: MenuAction, label: &str) {
    p.spawn((
        MenuButton(action),
        Button,
        Node {
            // slimmer than the original (30,12)/7/24px — the button-heavy screens (Controls: five)
            // must fit the design height alongside their tables
            padding: UiRect::axes(Val::Px(24.0), Val::Px(8.0)),
            margin: UiRect::all(Val::Px(5.0)),
            border: UiRect::all(Val::Px(2.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor(Color::srgb(0.38, 0.24, 0.66)),
        BorderRadius::all(Val::Px(12.0)),
        BackgroundColor(Color::srgba(0.10, 0.04, 0.20, 0.45)),
    ))
    .with_children(|b| {
        b.spawn(text_f(font, 20.0, Color::srgb(0.72, 0.82, 1.0), label));
    });
}

// A glowing violet border framing the screen (behind the content, so it never eats clicks).
// `MenuFrame` lets `menu_title_fx` pulse it in sync with the title.
fn spawn_frame(commands: &mut Commands, marker: impl Component) {
    commands.spawn((
        marker,
        MenuFrame,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(24.0),
            left: Val::Px(24.0),
            right: Val::Px(24.0),
            bottom: Val::Px(24.0),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BorderColor(Color::srgb(0.5, 0.25, 0.9)),
        BorderRadius::all(Val::Px(16.0)),
    ));
}

fn spawn_menu_ui(mut commands: Commands, achieved: Res<Achievements>, stats: Res<Stats>, intro: Res<TitleIntroPlayed>, hs: Res<HighScores>, logo: Res<LogoImage>, font: Res<MenuFont>) {
    spawn_frame(&mut commands, MenuUi); // behind the content (spawned first)
    let root = overlay(&mut commands, MenuUi, 0.25); // light — let the starfield show through
    let done = achieved.unlocked.iter().filter(|u| **u).count();
    let lore_n = lore_entries(&stats).iter().filter(|(_, _, u, _)| *u).count();
    let f = &font.0;
    // flicker the title on the FIRST show only; later returns start it already lit (past the warm-up)
    let title_age = if intro.0 { NEON_WARMUP } else { 0.0 };
    let best = hs.top[0];
    commands.entity(root).with_children(|p| {
        // logo masthead above the wordmark. The menu carries SEVEN buttons now, so it earns its own
        // breathing room: the masthead and wordmark are trimmed a little, and the buttons live in
        // their OWN column with a wider gap. That keeps the overlay's shared tight `row_gap` (which
        // the dense screens — Controls, Pilot Log — depend on to fit) untouched, while this screen
        // reads open instead of stacked. Budget: ~720px of the 800px design height.
        p.spawn((ImageNode::new(logo.0.clone()), Node { width: Val::Px(144.0), height: Val::Px(144.0), margin: UiRect::bottom(Val::Px(-16.0)), ..default() }));
        p.spawn((MenuTitle { age: title_age }, text_f(f, 74.0, title_color(), "VIOLET EDGE")));
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(12.0), // the airiness lives here, not in the shared overlay gap
            margin: UiRect::top(Val::Px(10.0)),
            ..default()
        })
        .with_children(|m| {
            menu_button(m, f, MenuAction::Play, "PLAY");
            if stats.phantom {
                // the second lap exists only for pilots who've finished the first — beat the game
                // once (ever, any run) and NEW GAME+ is on the menu forever
                menu_button(m, f, MenuAction::PlayPlus, "NEW GAME+");
            }
            menu_button(m, f, MenuAction::Controls, "CONTROLS");
            menu_button(m, f, MenuAction::Briefing, "BRIEFING");
            menu_button(m, f, MenuAction::Lore, &format!("PILOT LOG  ({lore_n} / 8)"));
            let gal = gallery_entries(&stats);
            menu_button(m, f, MenuAction::Gallery, &format!("GALLERY  ({} / {})", gal.iter().filter(|e| e.4).count(), gal.len()));
            menu_button(m, f, MenuAction::Achievements, &format!("ACHIEVEMENTS  ({done} / {})", ACHIEVEMENTS.len()));
        });
        if best > 0 {
            p.spawn((text_f(f, 18.0, Color::srgb(0.72, 0.76, 0.95), &format!("BEST   {best}")), Node { margin: UiRect::top(Val::Px(14.0)), ..default() }));
        }
    });
}

// One row of the achievements table (name | description). Compact on purpose: 23 rows plus the
// title and BACK button must fit the 800px design height, so the rows are dense and the container
// below (not the overlay's row_gap) owns the spacing.
fn table_row(p: &mut ChildSpawnerCommands, font: &Handle<Font>, left: &str, left_col: Color, left_w: f32, right: &str, right_col: Color) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(24.0),
        width: Val::Px(740.0),
        padding: UiRect::vertical(Val::Px(1.0)),
        ..default()
    })
    .with_children(|row| {
        row.spawn((text_f(font, 14.0, left_col, left), Node { width: Val::Px(left_w), ..default() }));
        row.spawn(text_f(font, 12.5, right_col, right));
    });
}

fn spawn_achievements_ui(mut commands: Commands, achieved: Res<Achievements>, font: Res<MenuFont>) {
    spawn_frame(&mut commands, AchievementsUi);
    let root = overlay(&mut commands, AchievementsUi, 0.5);
    let f = &font.0;
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 40.0, title_color(), "ACHIEVEMENTS")); // static — no neon warm-up here
        // two-column table: name | description (aligns cleanly, no separator glyph). All 23 rows
        // live in ONE column node so the overlay's row_gap is paid once, not 23 times.
        p.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|table| {
            for (i, &a) in ACHIEVEMENTS.iter().enumerate() {
                let (name, desc) = ach_meta(a);
                let (namecol, desccol) = if achieved.unlocked[i] {
                    (ach_earned_color(), Color::srgb(0.78, 0.82, 0.95))
                } else {
                    (Color::srgb(0.5, 0.52, 0.62), Color::srgb(0.38, 0.4, 0.5))
                };
                table_row(table, f, name, namecol, 300.0, desc, desccol);
            }
        });
        menu_button(p, f, MenuAction::Back, "BACK");
    });
}

// The controls reference — a key | action table, reached from the main menu.
// The controls screen IS the rebinding screen: pick the input method and remap any action for
// keyboard/mouse and controller. The cells show the LIVE bindings (updated by controls_display).
fn spawn_controls_ui(mut commands: Commands, font: Res<MenuFont>) {
    spawn_frame(&mut commands, ControlsUi);
    let root = overlay(&mut commands, ControlsUi, 0.6);
    let f = &font.0;
    let head = Color::srgb(0.72, 0.76, 0.9);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 36.0, title_color(), "CONTROLS"));
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
            menu_button(row, f, MenuAction::SetInput(InputMethod::Auto), "AUTO");
            menu_button(row, f, MenuAction::SetInput(InputMethod::KeyboardMouse), "KB + MOUSE");
            menu_button(row, f, MenuAction::SetInput(InputMethod::Controller), "CONTROLLER");
        });
        p.spawn((InputLabel, text_f(f, 14.0, head, "")));
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(14.0), width: Val::Px(514.0), margin: UiRect::top(Val::Px(2.0)), ..default() }).with_children(|row| {
            row.spawn((text_f(f, 13.0, head, "ACTION"), Node { width: Val::Px(150.0), ..default() }));
            row.spawn((text_f(f, 13.0, head, "KEYBOARD / MOUSE"), Node { width: Val::Px(168.0), ..default() }));
            row.spawn(text_f(f, 13.0, head, "CONTROLLER"));
        });
        for &a in ACTIONS.iter() {
            p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(14.0), width: Val::Px(514.0), ..default() }).with_children(|row| {
                row.spawn((text_f(f, 13.0, head, action_label(a)), Node { width: Val::Px(150.0), ..default() }));
                rebind_slot(row, f, a, false);
                rebind_slot(row, f, a, true);
            });
        }
        menu_button(p, f, MenuAction::ResetBinds, "RESET TO DEFAULTS");
        menu_button(p, f, MenuAction::Back, "BACK");
    });
}

// The briefing — a light lore intro plus the run objectives. (Flavor text is placeholder; swap in
// the real lore whenever it's written.)
fn spawn_briefing_ui(mut commands: Commands, font: Res<MenuFont>) {
    spawn_frame(&mut commands, BriefingUi);
    let root = overlay(&mut commands, BriefingUi, 0.5);
    let f = &font.0;
    let flavor = Color::srgb(0.7, 0.74, 0.92);
    let obj = Color::srgb(0.8, 0.85, 1.05);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 48.0, title_color(), "BRIEFING"));
        p.spawn(text_f(f, 17.0, flavor, "Reports indicate a large mass approaching the planet — fast."));
        p.spawn(text_f(f, 17.0, flavor, "There was no time to plan. The VIOLET CUTTER has been deployed:"));
        p.spawn(text_f(f, 17.0, flavor, "a prototype ship, one pilot, and possibly the only chance."));
        p.spawn((text_f(f, 22.0, title_color(), "OBJECTIVE"), Node { margin: UiRect::top(Val::Px(14.0)), ..default() }));
        for line in [
            "Investigate and hold back the approaching mass.",
            "Survive each wave's timer to advance.",
        ] {
            p.spawn(text_f(f, 16.0, obj, line));
        }
        menu_button(p, f, MenuAction::Back, "BACK");
    });
}

fn despawn_controls_ui(mut commands: Commands, mut rebinding: ResMut<Rebinding>, q: Query<Entity, With<ControlsUi>>) {
    rebinding.target = None; // don't leave a capture dangling when leaving the screen
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ─────────────────────────────── pilot log (lore archive) ─────────────
// The story, told as the PILOT'S FIELD REPORTS — transmissions the Violet Cutter sends home, one
// per contact. The mystery is PACED: the early entries only observe (a field holding formation, a
// thing penning rocks); the dread accumulates through details (wrong minerals, strata in the rock,
// a plotted heading) and only the FINAL entry says it plainly — THE ARCHITECT breaks worlds for
// parts, the Belt is its collection, and every asteroid was somebody's ground. Entries decrypt as
// their boss first falls (gated on the lifetime Stats flags), so the truth assembles across runs;
// the wave-30 win opens the last two. The core fleeing east + "I'm going after it" = the sequel.
// Returns (title, body, unlocked, accent) — accents are UI-safe (TextColor clamps channels ≤ 1).
fn lore_entries(s: &Stats) -> [(&'static str, [&'static str; 2], bool, Color); 8] {
    [
        (
            "THE BELT",
            [
                "It's not a mass — it's a field. Thousands of rocks, dense, holding formation.",
                "Natural belts drift. This one is keeping station. Beginning my sweep.",
            ],
            s.runs >= 1, // sent on the FIRST deployment — before you've flown, there's nothing to read
            Color::srgb(0.35, 0.7, 1.0),
        ),
        (
            "THE WARDEN",
            [
                "Contact. Something big was PENNING the rocks — caging them on its arms like stock.",
                "It fought like it was protecting them. Since when does a belt need a keeper?",
            ],
            s.warden,
            Color::srgb(1.0, 0.45, 0.9),
        ),
        (
            "THE GLUTTON",
            [
                "It ate the field and wore the mass. I sampled the debris it shed afterward.",
                "Composition's wrong for asteroids: core minerals, mantle iron. Rerunning the assay.",
            ],
            s.glutton,
            Color::srgb(1.0, 0.35, 0.3),
        ),
        (
            "THE SLINGER",
            [
                "It didn't throw rocks at me. It LOADED them. Aimed. Fired. Reloaded.",
                "These things aren't guarding the field — they're operating it. Like instruments.",
            ],
            s.slinger,
            Color::srgb(0.4, 0.65, 1.0),
        ),
        (
            "THE DETONATOR",
            [
                "It was arming the rocks. I cracked one open after the fight and found strata —",
                "layers, pressure lines. Rocks don't have strata. I don't want to write down what does.",
            ],
            s.detonator,
            Color::srgb(0.75, 1.0, 0.3),
        ),
        (
            "THE PULSAR",
            [
                "The whole field moves to its beat. Not drifting — DRIVEN. Herded, on a heading.",
                "I plotted the heading. It's home. This isn't a belt; it's a delivery.",
            ],
            s.pulsar,
            Color::srgb(0.6, 0.95, 1.0),
        ),
        (
            "THE PHANTOM",
            [
                "The steersman. It knew our world's name, and it called it an acquisition.",
                "I broke its mask; its core fled east. It wasn't destroyed — and it wasn't in charge.",
            ],
            s.phantom,
            Color::srgb(0.55, 1.0, 0.8),
        ),
        (
            "THE ARCHITECT",
            [
                "Final entry. The thing the steersman answered to breaks worlds for parts and shelves them.",
                "Every rock I've shot was somebody's ground. It's still building. I'm going after it.",
            ],
            s.phantom, // the win reveals who the Phantom answered to
            Color::srgb(1.0, 0.9, 0.6),
        ),
    ]
}

fn spawn_lore_ui(mut commands: Commands, stats: Res<Stats>, font: Res<MenuFont>) {
    spawn_frame(&mut commands, LoreUi);
    let root = overlay(&mut commands, LoreUi, 0.6);
    let f = &font.0;
    let body_col = Color::srgb(0.74, 0.78, 0.95);
    let locked_t = Color::srgb(0.42, 0.44, 0.55);
    let locked_b = Color::srgb(0.32, 0.34, 0.44);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 40.0, title_color(), "PILOT LOG"));
        p.spawn(text_f(f, 14.0, locked_b, "Transmissions from the VIOLET CUTTER, relayed home."));
        // all 8 reports live in ONE column node: the overlay's row_gap is paid once for the whole
        // journal (26 per-line gaps used to push the stack past the design height and clip both ends)
        p.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(2.0), ..default() }).with_children(|log| {
            for (i, (title, body, unlocked, accent)) in lore_entries(&stats).into_iter().enumerate() {
                // the boss ladder gates each report: waves 5,10,15,20,25 then the two wave-30 reveals
                let gap = if i == 0 { 0.0 } else { 10.0 };
                if unlocked {
                    log.spawn((text_f(f, 18.0, accent, title), Node { margin: UiRect::top(Val::Px(gap)), ..default() }));
                    for line in body {
                        log.spawn(text_f(f, 14.0, body_col, line));
                    }
                } else {
                    let hint = match i {
                        0 => "Awaiting deployment.".to_string(), // decrypts the moment a first run launches
                        7 => "Awaiting the final transmission.".to_string(), // follows the Phantom's record
                        _ => format!("Awaiting transmission — survive wave {}.", i * 5),
                    };
                    log.spawn((text_f(f, 18.0, locked_t, "▮▮▮▮▮▮▮▮  NO SIGNAL"), Node { margin: UiRect::top(Val::Px(gap)), ..default() }));
                    log.spawn(text_f(f, 14.0, locked_b, &hint));
                }
            }
        });
        menu_button(p, f, MenuAction::Back, "BACK");
    });
}

fn despawn_lore_ui(mut commands: Commands, q: Query<Entity, With<LoreUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn despawn_briefing_ui(mut commands: Commands, q: Query<Entity, With<BriefingUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn despawn_achievements_ui(mut commands: Commands, q: Query<Entity, With<AchievementsUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ─────────────────────────────── rebinding (on the Controls screen) ────
#[derive(Component)]
struct InputLabel; // text showing the current input method + the device actually in use
#[derive(Component, Clone, Copy)]
struct RebindSlot {
    action: Action,
    pad: bool, // false = keyboard/mouse cell, true = controller cell
}
// Which cell (if any) is capturing a new bind. `armed` skips the click frame that started it.
#[derive(Resource, Default)]
struct Rebinding {
    target: Option<(Action, bool)>,
    armed: bool,
}

// Gamepad buttons we scan while capturing a controller bind.
const PAD_BUTTONS: [GamepadButton; 14] = [
    GamepadButton::South,
    GamepadButton::East,
    GamepadButton::West,
    GamepadButton::North,
    GamepadButton::LeftTrigger,
    GamepadButton::RightTrigger,
    GamepadButton::LeftTrigger2,
    GamepadButton::RightTrigger2,
    GamepadButton::Select,
    GamepadButton::Start,
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];

// One clickable bind cell (its text is filled/updated by controls_display).
fn rebind_slot(p: &mut ChildSpawnerCommands, font: &Handle<Font>, action: Action, pad: bool) {
    p.spawn((
        RebindSlot { action, pad },
        Button,
        Node {
            // wide enough that "ShiftLeft / ShiftRight" fits ONE line at 13px — a wrapped bind used
            // to double the row height and push the screen past the design height
            width: Val::Px(168.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.5)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BorderColor(Color::srgb(0.4, 0.3, 0.7)),
        BorderRadius::all(Val::Px(6.0)),
    ))
    .with_children(|c| {
        c.spawn(text_f(font, 13.0, Color::srgb(0.85, 0.88, 1.0), "—"));
    });
}

// Each frame on the Controls screen: refresh the input-method label + every cell's bound-input text/border.
fn controls_display(
    bindings: Res<Bindings>,
    rebinding: Res<Rebinding>,
    method: Res<InputMethod>,
    pads: Query<(), With<Gamepad>>,
    label_q: Query<Entity, With<InputLabel>>,
    mut slots: Query<(&RebindSlot, &Children, &Interaction, &mut BorderColor)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let active = method.active(!pads.is_empty());
    if let Ok(e) = label_q.single() {
        if let Ok((mut t, _)) = texts.get_mut(e) {
            t.0 = format!("Input: {}  (using {})", method.label(), active.label());
        }
    }
    for (slot, children, interaction, mut border) in &mut slots {
        let capturing = rebinding.target == Some((slot.action, slot.pad));
        if let Some(&child) = children.first() {
            if let Ok((mut t, mut c)) = texts.get_mut(child) {
                if capturing {
                    t.0 = "press…".into();
                    *c = TextColor(Color::srgb(0.98, 0.85, 0.35));
                } else {
                    let list = if slot.pad { &bindings.pad } else { &bindings.kbm };
                    t.0 = binds_label(list, slot.action);
                    *c = TextColor(Color::srgb(0.85, 0.88, 1.0));
                }
            }
        }
        *border = BorderColor(if capturing {
            Color::srgb(0.98, 0.85, 0.35)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.7, 0.5, 1.0)
        } else {
            Color::srgb(0.4, 0.3, 0.7)
        });
    }
}

// Click a bind cell → begin capturing a new input for it.
fn rebind_slot_click(slots: Query<(&Interaction, &RebindSlot), Changed<Interaction>>, mut rebinding: ResMut<Rebinding>) {
    for (interaction, slot) in &slots {
        if *interaction == Interaction::Pressed {
            rebinding.target = Some((slot.action, slot.pad));
            rebinding.armed = false;
        }
    }
}

// While a cell is capturing, bind the next input pressed (skipping the click frame). Esc is reserved
// for cancel (handled in controls_input), so it's never captured.
fn rebind_capture(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    mut rebinding: ResMut<Rebinding>,
    mut bindings: ResMut<Bindings>,
) {
    let Some((action, pad)) = rebinding.target else {
        return;
    };
    if !rebinding.armed {
        rebinding.armed = true;
        return;
    }
    let new: Option<Bind> = if pad {
        gamepads.iter().flat_map(|g| PAD_BUTTONS.iter().copied().filter(move |b| g.just_pressed(*b))).map(Bind::Pad).next()
    } else {
        keys.get_just_pressed().find(|k| **k != KeyCode::Escape).map(|k| Bind::Key(*k)).or_else(|| mouse.get_just_pressed().next().map(|m| Bind::Mouse(*m)))
    };
    if let Some(bind) = new {
        let list = if pad { &mut bindings.pad } else { &mut bindings.kbm };
        list.retain(|(a, _)| *a != action); // one bind per action per device (replace)
        list.push((action, bind));
        rebinding.target = None;
    }
}

// Controls-screen buttons + navigation: input-method selection, reset, and BACK. Esc cancels an
// in-progress capture; otherwise Esc / BACK returns to the main menu.
fn controls_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: EventReader<MenuClick>,
    mut method: ResMut<InputMethod>,
    mut bindings: ResMut<Bindings>,
    mut rebinding: ResMut<Rebinding>,
    mut next: ResMut<NextState<GameState>>,
) {
    let mut back = false;
    for c in clicks.read() {
        match c.0 {
            MenuAction::SetInput(m) => *method = m,
            MenuAction::ResetBinds => {
                *bindings = Bindings::default();
                rebinding.target = None;
            }
            MenuAction::Back => back = true,
            _ => {}
        }
    }
    if rebinding.target.is_some() {
        if keys.just_pressed(KeyCode::Escape) {
            rebinding.target = None; // cancel the capture
        }
        return;
    }
    if back || keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Menu);
    }
}

// Read-only sub-screens (achievements / briefing): Esc/Enter or the Back button returns to the main
// menu. Runs only in those states, so it never interferes with gameplay input. (Controls has its own
// handler, controls_input, because it also owns rebind-capture and input-method buttons.)
// ─────────────────────────────── the GALLERY (bestiary) ───────────────
// Every rock, hazard and boss in the game, ONE PER PAGE (user call): a page each means each thing is
// drawn big from its own canonical body fn, and no two entries are ever side by side — which is also
// the answer to the palette problem (the neon spectrum is full, so several entities share a hue; in
// a grid that reads as a mistake, on separate pages it never comes up). Unlock state is DERIVED from
// the lifetime Stats we already persist — no new save fields: if you've broken one/killed one/beaten
// one, its page is open.
#[derive(Resource, Default)]
struct GalleryPage(usize);

#[derive(Clone, Copy, PartialEq, Eq)]
enum GalleryArt {
    Rock(RockKind),
    Gold,
    Mine,
    Well,
    Mob,
    Tender,
    Boss(BossKind),
}

// A STABLE bit per gallery subject, stored in `Stats.seen`. ⚠️ APPEND ONLY — never renumber these,
// or every existing save's gallery scrambles. (32 subjects fit; add a second word past that.)
fn gallery_bit(art: GalleryArt) -> u32 {
    let i = match art {
        GalleryArt::Rock(RockKind::Blue) => 0,
        GalleryArt::Rock(RockKind::Green) => 1,
        GalleryArt::Rock(RockKind::Hunter) => 2,
        GalleryArt::Rock(RockKind::Lapse) => 18,
        GalleryArt::Rock(RockKind::Facet) => 20,
        GalleryArt::Rock(RockKind::Husk) => 21,
        GalleryArt::Rock(RockKind::Orange) => 3,
        GalleryArt::Rock(RockKind::Pulser) => 4,
        GalleryArt::Rock(RockKind::Red) => 5,
        GalleryArt::Rock(RockKind::Cluster) => 6,
        GalleryArt::Rock(RockKind::Beacon) => 7,
        GalleryArt::Gold => 8,
        GalleryArt::Mine => 9,
        GalleryArt::Mob => 10,
        GalleryArt::Well => 11,
        GalleryArt::Tender => 19,
        GalleryArt::Boss(BossKind::Warden) => 12,
        GalleryArt::Boss(BossKind::Devourer) => 13,
        GalleryArt::Boss(BossKind::Slinger) => 14,
        GalleryArt::Boss(BossKind::Detonator) => 15,
        GalleryArt::Boss(BossKind::Pulsar) => 16,
        GalleryArt::Boss(BossKind::Phantom) => 17,
    };
    1 << i
}

fn gallery_seen(s: &Stats, art: GalleryArt) -> bool {
    s.seen & gallery_bit(art) != 0
}

// Mark a subject as INTRODUCED. Returns true if this was the first sighting (so the caller knows to
// persist). One flag, flipped the moment the thing shows up on your field — see `gallery_sightings`.
fn mark_seen(stats: &mut Stats, art: GalleryArt) -> bool {
    let bit = gallery_bit(art);
    let fresh = stats.seen & bit == 0;
    stats.seen |= bit;
    fresh
}

// GALLERY entry type: (art, name, role line, the field-report description, seen?).
type GalleryEntry = (GalleryArt, &'static str, &'static str, &'static str, bool);

// The book AS SHOWN: only subjects you've actually met. A page is ADDED when something is introduced
// (user rule, 2026-07-31) — there are no blank/locked pages to leaf through, so the gallery can never
// spoil what's still coming. `gallery_entries` keeps the full ordered table (bit numbering + the
// "n of 18" counter live off it); this is what the screen pages through.
fn gallery_book(s: &Stats) -> Vec<GalleryEntry> {
    gallery_entries(s).into_iter().filter(|e| e.4).collect()
}

// (art, name, one-line role, the long description, unlocked?)
// A page opens on its SEEN FLAG — set when the thing was first introduced to your field. No
// inference, no kill thresholds: if you've laid eyes on it, it's in the book.
fn gallery_entries(s: &Stats) -> Vec<(GalleryArt, &'static str, &'static str, &'static str, bool)> {
    vec![
        (GalleryArt::Rock(RockKind::Blue), "DRIFT ROCK", "Assay: mantle rock", "Cracks like slate and takes one hit from anything. I ran the composition twice: mantle stone, the kind you find a long way under a crust. Rocks that formed out here shouldn't have layers at all. Most of the Belt is this - and I try not to think about what that means.", gallery_seen(s, GalleryArt::Rock(RockKind::Blue))),
        (GalleryArt::Rock(RockKind::Green), "DENSE ROCK", "Assay: core iron", "Packed so tight a standard round just chips it - hit it once per size, or open it in one with the mass shot, the beam, or a mine blast. The samples come back heavy with core iron. You do not get core iron without a world to take it from.", gallery_seen(s, GalleryArt::Rock(RockKind::Green))),
        (GalleryArt::Rock(RockKind::Hunter), "HUNTER", "It knows where I am", "It turns. Nothing else out here turns. It comes on slowly at first and drives harder the longer I leave it alive, and while it's tracking, that bright ring on its face is pointed dead at my hull. It's slower than the Cutter, so I can always leave - but it never stops, and breaking it resets the chase rather than doubling it.", gallery_seen(s, GalleryArt::Rock(RockKind::Hunter))),
        (GalleryArt::Rock(RockKind::Orange), "EXPLOSIVE", "Charged, not cracked", "This one doesn't split - it lights, and a heartbeat later it takes out everything inside the blast, including me and including its neighbours. Anything sets it off: my guns, my beam, a mine. Something loaded these. I cracked one open afterward and found pressure lines, laid in deliberate.", gallery_seen(s, GalleryArt::Rock(RockKind::Orange))),
        (GalleryArt::Rock(RockKind::Pulser), "PULSER", "On somebody's clock", "It brightens and dims on a slow beat, and while it's lit nothing I have touches it - rounds just spark off. Hit it on the dark half. Its pieces keep the same beat. The unsettling part isn't the shield; it's that every one of them keeps time with all the others.", gallery_seen(s, GalleryArt::Rock(RockKind::Pulser))),
        (GalleryArt::Rock(RockKind::Red), "GROWER", "Appetite", "It pulls in whatever drifts near and swells with it - other growers included. Shooting it plainly only makes two of them, and both start feeding again. The mass shot, a warhead, the beam or a mine ends one outright. It eats the way the big one in the deep field ate. I don't think that's coincidence.", gallery_seen(s, GalleryArt::Rock(RockKind::Red))),
        (GalleryArt::Rock(RockKind::Cluster), "CLUSTER", "Fractured through", "Riddled with cracks before I ever fired. Break it close and it bursts into a ring of fast shards that will take the ship with it - keep your distance, vaporize it with the mass shot, or let the warp swallow it whole. Whatever shattered this did it a long way from here.", gallery_seen(s, GalleryArt::Rock(RockKind::Cluster))),
        (GalleryArt::Rock(RockKind::Beacon), "BEACON", "A keeper, not a rock", "It holds a field around itself and everything inside goes untouchable - my rounds and my beam simply wash over them. Kill the beacon and the field drops. Blasts, the warp and a grower's appetite ignore it entirely. It isn't defending itself. It's defending the others, and something taught it to.", gallery_seen(s, GalleryArt::Rock(RockKind::Beacon))),
        (GalleryArt::Rock(RockKind::Husk), "HUSK", "It was carrying something",
         "It passes for a drift rock right up until it comes apart, and then it is not rock at all - it is a SHELL, and there were two of the chasers folded up inside it. They come out slow and confused, which is the only mercy in it. I have started checking for the hollow before I fire. Rocks do not grow hollow, and nothing out here is carrying young. Something PACKED these.", gallery_seen(s, GalleryArt::Rock(RockKind::Husk))),
        (GalleryArt::Rock(RockKind::Facet), "FACET", "It gives the shot back",
         "Faces like cut glass, and they throw my rounds straight back - I have put my own fire through my own hull twice learning that. There is exactly ONE open face and it travels as the rock turns, so it is a matter of waiting for the gap rather than leaning on the trigger. The beam and any blast go through it regardless; they are not rounds, and whatever polished those faces did not plan for them.", gallery_seen(s, GalleryArt::Rock(RockKind::Facet))),
        (GalleryArt::Rock(RockKind::Lapse), "LAPSE", "Here, then not",
         "It thins out until there's nothing where it was, waits somewhere I can't see it, then comes back - and it drifts the whole time it's gone, so it never returns where I lost it. While it's coming back it's only an outline; nothing I fire touches it and it can't touch me until it's finished. That's the only reason I'm still flying. Whatever the Belt is, parts of it are not always present.", gallery_seen(s, GalleryArt::Rock(RockKind::Lapse))),
        (GalleryArt::Gold, "LIFE ROCK", "Salvage", "Gold all the way through and worth more than the rest of the Belt combined: break the whole lineage, every last fragment, and there's enough intact hull plating in it to put a life back on the board. Let a piece drift past the edge and it's gone. Only my guns can open it - mines bounce off, and the big feeder won't touch it.", gallery_seen(s, GalleryArt::Gold)),
        (GalleryArt::Mine, "MINE", "Not from the Belt", "Machined. Not grown, not broken off anything - machined, and left drifting where a ship would pass. It wakes when I get close and goes off a moment later. Shooting one detonates it early and takes the rocks around it with it. Somebody is seeding the ground I have to cross.", gallery_seen(s, GalleryArt::Mine)),
        (GalleryArt::Mob, "RAIDER", "Crewed or not, it aims", "A small gunship that keeps its distance and fires slow enough to dodge. It steers around the rocks, which means it can see them, and it leaves when it's been out too long. I've never gotten close enough to know whether there's anyone inside.", gallery_seen(s, GalleryArt::Mob)),
        (GalleryArt::Tender, "TENDER", "It puts the Belt back",
         "It doesn't shoot at me. It finds two pieces of what I've already broken, takes hold of both with a beam, and drags them together until they're one rock again. I watched it undo a minute of work in under three seconds. Shoot the drone or shoot either piece and the weld fails - but leave it alone and the field stops shrinking. Something out here MAINTAINS this. That's the part I can't get past.", gallery_seen(s, GalleryArt::Tender)),
        (GalleryArt::Well, "GRAVITY WELL", "The warp, inverted", "A pocket of pull that opens without warning and drags the ship instead of the rocks. It's weaker than my thrust, so I can always climb out - the danger is what it does to a dodge I'd already committed to. Same shape as my own warp. Someone else has the technology, and they're using it on me.", gallery_seen(s, GalleryArt::Well)),
        (GalleryArt::Boss(BossKind::Warden), "THE WARDEN", "Keeper - Belt station 1", "It had the rocks penned on its arms, wheeling them around itself like stock, and it threw them at me rather than let me through. The shield ate almost everything I fired. Since when does a belt need a keeper? Its wreck gave up the chain beam.", gallery_seen(s, GalleryArt::Boss(BossKind::Warden))),
        (GalleryArt::Boss(BossKind::Devourer), "THE GLUTTON", "Keeper - Belt station 2", "It hunted the field and wore what it ate, healing as it grew. Starve it or overfeed it - past a certain point it can't hold what it's swallowed and comes apart. The debris it shed assayed as core minerals and mantle iron. That was the day I stopped believing these were asteroids.", gallery_seen(s, GalleryArt::Boss(BossKind::Devourer))),
        (GalleryArt::Boss(BossKind::Slinger), "THE SLINGER", "Keeper - Belt station 3", "It didn't throw rocks. It LOADED them, aimed, and fired, then reloaded - and its core sat exposed the whole time, daring me to trade. These things aren't guarding the field. They're operating it, like instruments.", gallery_seen(s, GalleryArt::Boss(BossKind::Slinger))),
        (GalleryArt::Boss(BossKind::Detonator), "THE DETONATOR", "Keeper - Belt station 4", "Armored shut except in the moments it was priming a rock into a bomb, and those windows were the only way in. It armed the field faster than I could clear it. It doesn't make the explosives - it finds them. That means they were already here, waiting.", gallery_seen(s, GalleryArt::Boss(BossKind::Detonator))),
        (GalleryArt::Boss(BossKind::Pulsar), "THE PULSAR", "Keeper - Belt station 5", "It beats like the rocks that share its name: sealed while lit, open while dark, and shoving the whole field outward every pulse. The entire Belt moves to its rhythm - not drifting, DRIVEN. I plotted the heading it was driving them on. It points home.", gallery_seen(s, GalleryArt::Boss(BossKind::Pulsar))),
        (GalleryArt::Boss(BossKind::Phantom), "THE PHANTOM", "The steersman", "The thing that was steering all of it. It wore the field against me, opened the arena with a ray, and threw itself at me at the end like something that had run out of options. It knew our world's name and called it an acquisition. I broke the mask - and what came out didn't die, it left.", gallery_seen(s, GalleryArt::Boss(BossKind::Phantom))),
    ]
}

// GALLERY SIGHTINGS — the single place the "introduced" flags get set. One scan of the live field per
// frame: whatever is on screen right now is marked seen. Doing it here rather than at every spawn site
// keeps Stats out of a dozen spawn functions, and it's the honest test — the thing is in front of you.
// Bosses mark themselves in `boss_director` (it's the only place that knows which kind spawned).
// Persists ONLY on a change, so this never touches the disk on a normal frame.
fn gallery_sightings(
    mut stats: ResMut<Stats>,
    rocks: Query<(Option<&Gold>, Option<&Explosive>, Option<&Pulser>, Option<&Red>, Option<&Cluster>, Option<&Beacon>, Option<&Hunter>, Option<&Lapse>, Option<&Facet>, Option<&Husk>, &Asteroid)>,
    mines: Query<(), With<Mine>>,
    mobs: Query<(), With<Enemy>>,
    tenders: Query<(), With<Tender>>,
    wells: Query<(), With<Well>>,
) {
    let mut fresh = false;
    for (gold, explosive, pulser, red, cluster, beacon, hunter, lapse, facet, husk, a) in &rocks {
        // one rock, one subject — mirrors `credit_rock_kill`'s priority so the tags can't double up
        let art = if gold.is_some() {
            GalleryArt::Gold
        } else if husk.is_some() {
            GalleryArt::Rock(RockKind::Husk)
        } else if facet.is_some() {
            GalleryArt::Rock(RockKind::Facet)
        } else if lapse.is_some() {
            GalleryArt::Rock(RockKind::Lapse)
        } else if hunter.is_some() {
            GalleryArt::Rock(RockKind::Hunter)
        } else if beacon.is_some() {
            GalleryArt::Rock(RockKind::Beacon)
        } else if pulser.is_some() {
            GalleryArt::Rock(RockKind::Pulser)
        } else if red.is_some() {
            GalleryArt::Rock(RockKind::Red)
        } else if cluster.is_some() {
            GalleryArt::Rock(RockKind::Cluster)
        } else if explosive.is_some() {
            GalleryArt::Rock(RockKind::Orange)
        } else if a.dense {
            GalleryArt::Rock(RockKind::Green)
        } else {
            GalleryArt::Rock(RockKind::Blue)
        };
        fresh |= mark_seen(&mut stats, art);
    }
    if !mines.is_empty() {
        fresh |= mark_seen(&mut stats, GalleryArt::Mine);
    }
    if !mobs.is_empty() {
        fresh |= mark_seen(&mut stats, GalleryArt::Mob);
    }
    if !tenders.is_empty() {
        fresh |= mark_seen(&mut stats, GalleryArt::Tender);
    }
    if !wells.is_empty() {
        fresh |= mark_seen(&mut stats, GalleryArt::Well);
    }
    if fresh {
        save_progress(&stats); // a NEW subject — worth the write
    }
}

// The gallery's rock silhouette: ONE deterministic jagged ring so every rock page is drawn at the
// same recognisable shape and only its colour + signature marks differ (the field's rocks are random
// per-spawn, which is wrong for a reference page).
fn gallery_rock_ring(c: Vec2, r: f32) -> Vec<Vec2> {
    // 14 vertices with SHALLOW variance: the sharp 9-gon joins were showing as bright notches across
    // the outline under bloom (butt-capped segments meeting at steep angles). Near-collinear joins
    // hide the seams while still reading as a rough rock.
    let bumps = [1.0f32, 0.95, 1.03, 0.97, 1.0, 0.93, 1.04, 0.98, 1.01, 0.94, 1.02, 0.97, 1.0, 0.96];
    let mut pts: Vec<Vec2> = (0..bumps.len())
        .map(|i| {
            let a = i as f32 / bumps.len() as f32 * TAU;
            c + Vec2::from_angle(a) * r * bumps[i]
        })
        .collect();
    if let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    pts
}

// Draw one gallery entry's ART, centered on `c`. Bosses reuse their CANONICAL body fns — the exact
// silhouettes the fight, the warning banner and the background cameo all share, so the reference can
// never drift from the real thing. Rocks use the shared gallery ring plus that type's signature
// marks (the hunter's tracking eye, the beacon's aura, the pulser's shield ring, and so on).
fn draw_gallery_art(gizmos: &mut Gizmos, c: Vec2, t: f32, art: GalleryArt, zoom: f32) {
    // `zoom` scales EVERY radius so the whole drawing fits GALLERY_ART_R (see gallery_art_extent).
    match art {
        GalleryArt::Boss(k) => {
            let col = boss_kind_color(k);
            let r = 74.0 * zoom;
            match k {
                BossKind::Warden => {
                    draw_warden_body(gizmos, c, r, t, Vec2::from_angle(t * 0.6), col);
                    // a couple of penned rocks on the arms, so its whole trick reads at a glance
                    for i in 0..2 {
                        let a = t * 0.5 + i as f32 * TAU / 2.0;
                        gizmos.linestrip_2d(gallery_rock_ring(c + Vec2::from_angle(a) * r * 1.6, 15.0 * zoom), rock_color());
                    }
                }
                BossKind::Devourer => draw_glutton_body(gizmos, c, r, t, 0.0, 0.45, col),
                BossKind::Slinger => draw_slinger_body(gizmos, c, Vec2::from_angle(-TAU / 4.0), r * 0.95, t, 0.0, 1.0, col),
                BossKind::Detonator => draw_detonator_body(gizmos, c, r, t, 0.35, 6, col),
                BossKind::Pulsar => draw_pulsar_body(gizmos, c, r, t, 0.5, 8, col),
                BossKind::Phantom => draw_haunt_skull(gizmos, c, r * 0.92, col, 0.8, t, true, 0.45),
            }
        }
        GalleryArt::Rock(kind) => {
            let r = 58.0 * zoom;
            let col = match kind {
                RockKind::Blue => rock_color(),
                RockKind::Green => dense_color(),
                RockKind::Hunter => hunter_color(),
                RockKind::Lapse => lapse_color(),
                RockKind::Facet => facet_color(),
                RockKind::Husk => husk_color(),
                RockKind::Orange => orange_color(),
                RockKind::Pulser => Color::srgb(6.0, 6.2, 7.0),
                RockKind::Red => red_color(),
                RockKind::Cluster => cluster_color(),
                RockKind::Beacon => beacon_color(),
            };
            gizmos.linestrip_2d(gallery_rock_ring(c, r), col);
            match kind {
                RockKind::Green => {
                    gizmos.linestrip_2d(gallery_rock_ring(c, r * 0.62), col); // the hp core ring
                }
                RockKind::Hunter => {
                    // THE EYE — the signature, aimed at the viewer's side of the page
                    let look = Vec2::from_angle(t * 0.8);
                    let eye = c + look * r * 0.34;
                    let er = r * 0.15;
                    gizmos.circle_2d(Isometry2d::from_translation(eye), er, Color::srgb(7.0, 5.6, 4.6));
                    gizmos.circle_2d(Isometry2d::from_translation(eye), er * 0.45, Color::srgb(9.0, 2.0, 1.0));
                    gizmos.line_2d(eye + look * er * 1.4, eye + look * er * 2.6, dim(col, 0.85));
                }
                RockKind::Orange => {
                    gizmos.circle_2d(Isometry2d::from_translation(c), r * 1.9, dim(orange_color(), 0.2)); // blast reach
                }
                RockKind::Pulser => {
                    gizmos.linestrip_2d(gallery_rock_ring(c, r * 0.55), col); // the lit shield ring
                }
                RockKind::Cluster => {
                    let ring = gallery_rock_ring(c, r);
                    for k in [0usize, 2, 4] {
                        gizmos.line_2d(ring[k], ring[(k + ring.len() / 2) % (ring.len() - 1)], dim(col, 0.7)); // fractures
                    }
                }
                RockKind::Beacon => {
                    gizmos.circle_2d(Isometry2d::from_translation(c), r * 2.3, dim(col, 0.28)); // the aura
                    gizmos.linestrip_2d(gallery_rock_ring(c, r * 0.5), col);
                }
                RockKind::Red => {
                    // two smaller rocks being drawn in — the growth read
                    for i in 0..2 {
                        let a = t * 0.7 + i as f32 * TAU / 2.0;
                        gizmos.linestrip_2d(gallery_rock_ring(c + Vec2::from_angle(a) * r * 1.7, 13.0 * zoom), dim(col, 0.6));
                    }
                }
                RockKind::Husk => {
                    // an ordinary-looking shell, cracked open, with the brood coming out
                    gizmos.linestrip_2d(gallery_rock_ring(c, r), col);
                    gizmos.linestrip_2d(gallery_rock_ring(c, r * 0.5), dim(col, 0.5)); // the HOLLOW
                    // two hunters scattering, eyes leading
                    for s in [-1.0f32, 1.0] {
                        let out = Vec2::new(s * 0.82, 0.34).normalize();
                        let hp = c + out * r * 1.15;
                        gizmos.linestrip_2d(gallery_rock_ring(hp, r * 0.3), hunter_color());
                        let eye = hp + out * r * 0.14;
                        gizmos.circle_2d(Isometry2d::from_translation(eye), r * 0.07, Color::srgb(7.0, 5.6, 4.6));
                        gizmos.circle_2d(Isometry2d::from_translation(eye), r * 0.032, Color::srgb(9.0, 2.0, 1.0));
                    }
                }
                RockKind::Facet => {
                    // flat mirror faces with ONE open notch — the whole mechanic as a silhouette
                    let n = 7usize;
                    let open = 2usize;
                    for k in 0..n {
                        if k == open {
                            continue; // the gap: the only way in
                        }
                        let a0 = k as f32 / n as f32 * TAU + t * 0.25;
                        let a1 = (k + 1) as f32 / n as f32 * TAU + t * 0.25;
                        gizmos.line_2d(c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a1) * r, col);
                    }
                    for k in [open, open + 1] {
                        let a0 = k as f32 / n as f32 * TAU + t * 0.25;
                        gizmos.line_2d(c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a0) * r * 0.62, dim(col, 0.8));
                    }
                    // a round arriving and being thrown straight back
                    let ba = 4.4f32;
                    let hit = c + Vec2::from_angle(ba) * r;
                    gizmos.line_2d(hit + Vec2::from_angle(ba) * r * 0.75, hit, dim(bullet_color(), 0.9));
                    gizmos.line_2d(hit, hit + Vec2::from_angle(ba + 2.0) * r * 0.75, bullet_color());
                }
                RockKind::Lapse => {
                    // caught mid-dissolve: the solid arc on one side, the ghost of it on the other
                    let ring = gallery_rock_ring(c, r);
                    let half = ring.len() / 2;
                    gizmos.linestrip_2d(ring[..half].to_vec(), dim(col, 0.25));
                    gizmos.linestrip_2d(gallery_rock_ring(c, r * 0.55), dim(col, 0.45));
                }
                RockKind::Blue => {}
            }
        }
        GalleryArt::Gold => {
            let g = dim(gold_color(), 0.75 + 0.25 * (t * 2.0).sin());
            gizmos.linestrip_2d(gallery_rock_ring(c, 58.0 * zoom), g);
            gizmos.linestrip_2d(gallery_rock_ring(c, 34.0 * zoom), g);
        }
        GalleryArt::Mine => {
            // MUST match the field mine exactly (user caught a mismatch): a crimson DIAMOND with a
            // small core — same construction as the in-game draw, just scaled up for the page.
            let col = mine_color();
            let r = 40.0 * zoom;
            let pts = [
                c + Vec2::new(0.0, r),
                c + Vec2::new(r, 0.0),
                c + Vec2::new(0.0, -r),
                c + Vec2::new(-r, 0.0),
                c + Vec2::new(0.0, r),
            ];
            gizmos.linestrip_2d(pts, col);
            gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.4, col);
            // its lethal reach, to scale against the body (MINE_BLAST_R / MINE_R ≈ 2.9)
            gizmos.circle_2d(Isometry2d::from_translation(c), r * (MINE_BLAST_R / MINE_R), dim(col, 0.2));
        }
        GalleryArt::Tender => {
            let col = enemy_color();
            let spin = t * 0.7;
            let cage: Vec<Vec2> = (0..=6).map(|i| c + Vec2::from_angle(spin + i as f32 / 6.0 * TAU) * 34.0 * zoom).collect();
            gizmos.linestrip_2d(cage, col);
            gizmos.circle_2d(Isometry2d::from_translation(c), 12.0 * zoom, dim(col, 0.9));
            for i in 0..3 {
                let d = Vec2::from_angle(-spin * 1.4 + i as f32 / 3.0 * TAU);
                gizmos.line_2d(c + d * 12.0 * zoom, c + d * 32.0 * zoom, dim(col, 0.75));
            }
            // two fragments under tow, mid-fusion — the whole mechanic in one picture
            for s in [-1.0f32, 1.0] {
                let to = c + Vec2::new(s * 78.0 * zoom, -6.0 * zoom);
                gizmos.linestrip_2d(gallery_rock_ring(to, 14.0 * zoom), rock_color());
                for k in [0, 2, 4] {
                    let f0 = k as f32 / 6.0;
                    let f1 = (k as f32 + 0.9) / 6.0;
                    gizmos.line_2d(c + (to - c) * f0, c + (to - c) * f1, dim(col, 0.6));
                }
            }
        }
        GalleryArt::Mob => {
            // matches the field raider: a throbbing yellow orb with a concentric inner ring
            let col = enemy_color();
            let throb = 1.0 + 0.1 * (t * 6.0).sin();
            gizmos.circle_2d(Isometry2d::from_translation(c), 40.0 * zoom * throb, col);
            gizmos.circle_2d(Isometry2d::from_translation(c), 40.0 * zoom * 0.45 * throb, col);
            // one of its slow shots, so "it shoots back" reads without motion
            let sp = c + Vec2::new(0.0, -66.0 * zoom);
            gizmos.circle_2d(Isometry2d::from_translation(sp), 7.0 * zoom, col);
            gizmos.circle_2d(Isometry2d::from_translation(sp), 3.5 * zoom, Color::srgb(5.0, 5.0, 4.0));
        }
        GalleryArt::Well => {
            // matches the field well: SEVEN arms sweeping inward (a whirlpool, not a cross) + a hot core
            let col = warp_color();
            let r = 64.0 * zoom;
            let (arms, segs) = (7, 10);
            for a in 0..arms {
                let a0 = a as f32 / arms as f32 * TAU;
                let pts: Vec<Vec2> = (0..=segs)
                    .map(|s| {
                        let p = s as f32 / segs as f32;
                        c + Vec2::from_angle(a0 + 5.0 * p + t * 0.9) * (r * (1.0 - 0.85 * p))
                    })
                    .collect();
                gizmos.linestrip_2d(pts, dim(col, 0.7));
            }
            gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.28, col);
            gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.125, Color::srgb(6.0, 2.0, 3.5));
        }
    }
}

// The gallery's art layer: draws the current page's entry in world space, behind the UI text. Locked
// entries get a dim question-mark silhouette instead — you can see the shape of what you haven't met.
fn gallery_draw(time: Res<Time<Real>>, page: Res<GalleryPage>, stats: Res<Stats>, mut gizmos: Gizmos) {
    let book = gallery_book(&stats);
    if book.is_empty() {
        return; // nothing catalogued — the screen says so in text, no art to draw
    }
    let &(art, ..) = &book[page.0.min(book.len() - 1)];
    // fit the budget (and never blow small art up too far)
    let zoom = (GALLERY_ART_R / gallery_art_extent(art)).min(1.35);
    draw_gallery_art(&mut gizmos, Vec2::new(0.0, GALLERY_ART_Y), time.elapsed_secs(), art, zoom);
}

const GALLERY_ART_Y: f32 = 96.0; // world-space centre of the art, above the name/description block
// HARD BUDGET: no page's art may draw beyond this radius from its centre. The band it lives in is
// 262px tall (±131), so 96 leaves a real margin — the Beacon's aura used to reach 133 and printed
// straight through the name text. Every entry is scaled by BUDGET / its own extent, so this holds
// for all of them and for anything added later (give a new entry an honest `gallery_art_extent`).
const GALLERY_ART_R: f32 = 96.0;
// The band the UI reserves for the art is 262px tall (±131). The budget must keep real clearance
// inside it or wide entries print through the name/description — enforced at compile time.
const GALLERY_ART_BAND_HALF: f32 = 131.0;
const _: () = assert!(GALLERY_ART_R < GALLERY_ART_BAND_HALF - 20.0);

// The farthest radius each entry draws to at scale 1.0 — auras, blast reaches and towed rocks
// included, since those are what actually overflow.
fn gallery_art_extent(art: GalleryArt) -> f32 {
    match art {
        GalleryArt::Rock(RockKind::Beacon) => 58.0 * 2.3, // the aura is the widest thing in the book
        GalleryArt::Rock(RockKind::Orange) => 58.0 * 1.9, // blast reach
        GalleryArt::Rock(RockKind::Red) => 58.0 * 1.7 + 13.0,
        GalleryArt::Rock(RockKind::Husk) => 58.0 * 1.15 + 58.0 * 0.3, // the brood sits outside the shell // the two rocks it's pulling in
        GalleryArt::Rock(_) => 58.0,
        GalleryArt::Gold => 58.0,
        GalleryArt::Mine => 40.0 * (MINE_BLAST_R / MINE_R), // lethal reach ring
        GalleryArt::Mob => 73.0, // body + the shot below it
        GalleryArt::Well => 64.0,
        GalleryArt::Tender => 78.0 + 14.0, // the towed fragments sit out to the sides
        GalleryArt::Boss(BossKind::Warden) => 74.0 * 1.6 + 15.0, // penned rocks on its arms
        GalleryArt::Boss(_) => 74.0 * 1.25, // bodies with spikes/petals/limbs
    }
}

#[derive(Component)]
struct GalleryUi;

// The page: title + a gap the world-space art shows through + the entry's name, role and description.
// Rebuilt on every page turn (cheap — one screen of text), so `gallery_page_turn` just moves the index
// and re-enters the screen's spawn.
fn spawn_gallery_ui(mut commands: Commands, page: Res<GalleryPage>, stats: Res<Stats>, font: Res<MenuFont>) {
    spawn_frame(&mut commands, GalleryUi);
    let root = overlay(&mut commands, GalleryUi, 0.3); // light — the art is drawn behind this
    let f = &font.0;
    let book = gallery_book(&stats);
    let total = gallery_entries(&stats).len(); // the full roster, for the "n of N" completion line
    let body = Color::srgb(0.76, 0.8, 0.96);
    let dimc = Color::srgb(0.45, 0.48, 0.6);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 34.0, title_color(), "GALLERY"));
        p.spawn(text_f(f, 13.0, dimc, &format!("Catalogued {} of {total}", book.len())));
        // the band the world-space art renders into (`gallery_draw`)
        p.spawn(Node { height: Val::Px(262.0), ..default() });
        if book.is_empty() {
            // nothing met yet - the book doesn't exist, so say so rather than showing blank pages
            p.spawn(text_f(f, 26.0, dimc, "NOTHING CATALOGUED"));
            p.spawn(Node { width: Val::Px(620.0), margin: UiRect::top(Val::Px(10.0)), ..default() }).with_children(|d| {
                d.spawn((
                    text_f(f, 14.0, body, "The Cutter logs what it meets. Fly the Belt and this fills itself in - a page for every rock, hazard and keeper you come across."),
                    Node { width: Val::Px(620.0), ..default() },
                ));
            });
        } else {
            let i = page.0.min(book.len() - 1);
            let (_, name, role, desc, _) = book[i];
            p.spawn(text_f(f, 30.0, title_color(), name));
            p.spawn(text_f(f, 14.0, dimc, role));
            // the report, wrapped to a fixed column so long entries never run off the frame
            p.spawn(Node { width: Val::Px(620.0), margin: UiRect::top(Val::Px(8.0)), ..default() }).with_children(|d| {
                d.spawn((text_f(f, 14.0, body, desc), Node { width: Val::Px(620.0), ..default() }));
            });
            // paging - only worth showing once there's more than one page
            if book.len() > 1 {
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|row| {
                    menu_button(row, f, MenuAction::PagePrev, "<  PREV");
                    row.spawn(text_f(f, 15.0, dimc, &format!("{} / {}", i + 1, book.len())));
                    menu_button(row, f, MenuAction::PageNext, "NEXT  >");
                });
                p.spawn(text_f(f, 12.0, dimc, "A / D  or  ← →  to turn pages"));
            }
        }
        menu_button(p, f, MenuAction::Back, "BACK");
    });
}

fn despawn_gallery_ui(mut commands: Commands, q: Query<Entity, With<GalleryUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// Page turning: buttons, A/D, or the arrow keys. Wraps at both ends so you can spin through it, and
// respawns the page UI in place (the art layer reads the same index, so both stay in step).
fn gallery_page_turn(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: EventReader<MenuClick>,
    mut page: ResMut<GalleryPage>,
    stats: Res<Stats>,
    font: Res<MenuFont>,
    ui: Query<Entity, With<GalleryUi>>,
) {
    let actions: Vec<MenuAction> = clicks.read().map(|c| c.0).collect();
    let fwd = actions.contains(&MenuAction::PageNext) || keys.just_pressed(KeyCode::KeyD) || keys.just_pressed(KeyCode::ArrowRight);
    let back = actions.contains(&MenuAction::PagePrev) || keys.just_pressed(KeyCode::KeyA) || keys.just_pressed(KeyCode::ArrowLeft);
    if !fwd && !back {
        return;
    }
    let pages = gallery_book(&stats).len();
    if pages < 2 {
        return; // nothing to turn to
    }
    if fwd {
        page.0 = (page.0.min(pages - 1) + 1) % pages;
    } else {
        page.0 = (page.0.min(pages - 1) + pages - 1) % pages;
    }
    for e in &ui {
        commands.entity(e).despawn(); // rebuild the page for the new entry
    }
    spawn_gallery_ui(commands, page.into(), stats, font);
}

fn submenu_back(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut clicks: EventReader<MenuClick>,
) {
    let back = clicks.read().any(|c| c.0 == MenuAction::Back);
    if back || keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Enter) {
        next.set(GameState::Menu);
    }
}

// Hide the persistent HUD on the menu screens; show it during a run.
fn hud_visibility(state: Res<State<GameState>>, mut q: Query<&mut Visibility, With<Hud>>) {
    let vis = if run_active(state.get()) { Visibility::Visible } else { Visibility::Hidden };
    for mut v in &mut q {
        if *v != vis {
            *v = vis;
        }
    }
}

// Style menu buttons by interaction: idle (dim), hovered (a violet border/text SHIMMER that pulses
// with time), pressed (brightest). Runs every frame so the hover glow animates.
fn button_shimmer(
    time: Res<Time>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor, &mut BorderColor, &Children), With<MenuButton>>,
    mut texts: Query<&mut TextColor>,
) {
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 5.0).sin();
    for (interaction, mut bg, mut border, children) in &mut buttons {
        let (b, brd, txt) = match interaction {
            Interaction::Pressed => (
                Color::srgba(0.36, 0.16, 0.60, 0.9),
                Color::srgb(0.95, 0.72, 1.0),
                Color::srgb(1.0, 0.96, 1.0),
            ),
            Interaction::Hovered => (
                Color::srgba(0.24, 0.10, 0.44, 0.82),
                mix(Color::srgb(0.50, 0.30, 0.90), Color::srgb(0.95, 0.75, 1.0), pulse),
                mix(Color::srgb(0.82, 0.86, 1.0), Color::srgb(1.0, 0.98, 1.0), pulse),
            ),
            Interaction::None => (
                Color::srgba(0.10, 0.04, 0.20, 0.45),
                Color::srgb(0.38, 0.24, 0.66),
                Color::srgb(0.72, 0.82, 1.0),
            ),
        };
        *bg = BackgroundColor(b);
        *border = BorderColor(brd);
        for &c in children {
            if let Ok(mut tc) = texts.get_mut(c) {
                *tc = TextColor(txt);
            }
        }
    }
}

// Fire a MenuClick on the press edge (Changed → once per click).
fn button_click(mut clicks: EventWriter<MenuClick>, q: Query<(&Interaction, &MenuButton), Changed<Interaction>>) {
    for (interaction, btn) in &q {
        if *interaction == Interaction::Pressed {
            clicks.write(MenuClick(btn.0));
        }
    }
}

const NEON_WARMUP: f32 = 2.3; // seconds the title spends warming up like a neon sign
// Two crisp blinks and a dark pause; the third strike is a soft glow-up (below), not a hard snap.
// Reads as a deliberate "1, 2 …… and it catches" instead of a fast erratic buzz.
const NEON_BLINKS: [(f32, f32); 2] = [(0.25, 0.45), (0.75, 0.95)];
const NEON_FADE_START: f32 = 1.55; // the tube "catches" here and fades up smoothly to NEON_WARMUP

// Neon flicker-on for the title (scripted blinks settling into a steady breathe), and a matching
// pulse on the frame border. `dim` scales the (≤1) UI colours, so b<1 reads as the sign "off".
fn menu_title_fx(time: Res<Time>, mut titles: Query<(&mut MenuTitle, &mut TextColor)>, mut frames: Query<&mut BorderColor, With<MenuFrame>>) {
    let dt = time.delta_secs();
    let base = Color::srgb(0.72, 0.28, 1.0);
    let mut brightness = 0.9;
    for (mut title, mut tc) in &mut titles {
        title.age += dt;
        let a = title.age;
        let b = if a >= NEON_WARMUP {
            // settled: a subtle breathe that starts at full, so the glow-up hands off seamlessly
            0.85 + 0.15 * ((a - NEON_WARMUP) * 1.6).cos()
        } else if a >= NEON_FADE_START {
            // third strike: a soft smoothstep glow-up (dark → full) instead of a hard snap
            let t = (a - NEON_FADE_START) / (NEON_WARMUP - NEON_FADE_START);
            let s = t * t * (3.0 - 2.0 * t);
            0.05 + s * 0.95
        } else if NEON_BLINKS.iter().any(|&(s, e)| a >= s && a < e) {
            1.0 // crisp blink
        } else {
            0.05 // dark between blinks
        };
        brightness = b;
        *tc = TextColor(dim(base, b));
    }
    // frame border tracks the same brightness (uses the last title's value — there's only one)
    let fbase = Color::srgb(0.5, 0.25, 0.9);
    for mut bc in &mut frames {
        *bc = BorderColor(dim(fbase, brightness.max(0.2)));
    }
}

fn despawn_menu_ui(mut commands: Commands, q: Query<Entity, With<MenuUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// Leaving the menu (to a sub-screen or a run) means the intro flicker has been seen — don't replay it.
fn mark_title_intro_played(mut intro: ResMut<TitleIntroPlayed>) {
    intro.0 = true;
}

// ─────────────────────────────── achievement runtime ──────────────────
// One toast card in the top-center column: a small header line over a big name line on a dark tint.
// Shared by achievements, the gold 1UP, and Pilot Log decrypts — one card, three tints.
fn spawn_toast(commands: &mut Commands, root: &Query<Entity, With<ToastRoot>>, bg: Color, header: &str, name: &str, name_col: Color) {
    let Some(r) = root.iter().next() else { return };
    commands.entity(r).with_children(|p| {
        p.spawn((
            Toast { life: TOAST_LIFE },
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(bg),
        ))
        .with_children(|t| {
            t.spawn(text(15.0, Color::srgb(0.7, 0.85, 1.2), header));
            t.spawn(text(22.0, name_col, name));
        });
    });
}

// The persistent top-center column that unlock toasts stack into.
fn spawn_toast_root(mut commands: Commands) {
    commands.spawn((
        ToastRoot,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(66.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
    ));
}

// Load lifetime progress at startup and mark already-earned achievements + already-decrypted log
// entries as seen (so neither re-toasts on boot).
fn load_progress(mut stats: ResMut<Stats>, mut unlocked: ResMut<Achievements>, mut seen: ResMut<LoreSeen>) {
    if let Some(saved) = read_progress() {
        *stats = saved;
    }
    for (i, &a) in ACHIEVEMENTS.iter().enumerate() {
        unlocked.unlocked[i] = ach_met(a, &stats);
    }
    for (i, (.., open, _)) in lore_entries(&stats).into_iter().enumerate() {
        seen.0[i] = open;
    }
}

// Poll the lifetime Stats; the first frame an achievement's condition is met, flip its flag, pop a
// toast, chime, and persist. Cheap — 12 checks a frame.
fn achievements(
    mut commands: Commands,
    stats: Res<Stats>,
    mut unlocked: ResMut<Achievements>,
    bank: Option<Res<SfxBank>>,
    root: Query<Entity, With<ToastRoot>>,
) {
    for (i, &a) in ACHIEVEMENTS.iter().enumerate() {
        if unlocked.unlocked[i] || !ach_met(a, &stats) {
            continue;
        }
        unlocked.unlocked[i] = true;
        let (name, _) = ach_meta(a);
        spawn_toast(&mut commands, &root, Color::srgba(0.10, 0.03, 0.18, 0.92), "ACHIEVEMENT UNLOCKED", name, mass_color());
        if let Some(b) = &bank {
            one_shot(&mut commands, b.achievement.clone(), 0.6);
        }
        save_progress(&stats);
    }
}

// Poll the Pilot Log gates; the first frame an entry decrypts, pop a PILOT LOG UPDATED toast in the
// entry's accent + a radio blip. The story advancing is a reward — say so in the moment it happens
// (THE BELT lands during the first launch; boss records land as their boss dies).
fn lore_watch(
    mut commands: Commands,
    stats: Res<Stats>,
    mut seen: ResMut<LoreSeen>,
    bank: Option<Res<SfxBank>>,
    root: Query<Entity, With<ToastRoot>>,
) {
    for (i, (title, _, open, accent)) in lore_entries(&stats).into_iter().enumerate() {
        if seen.0[i] || !open {
            continue;
        }
        seen.0[i] = true;
        spawn_toast(&mut commands, &root, Color::srgba(0.03, 0.10, 0.16, 0.92), "PILOT LOG UPDATED", title, accent);
        if let Some(b) = &bank {
            one_shot(&mut commands, b.log.clone(), 0.55);
        }
    }
}

// A reflected round is YOUR shot, coming back. While its `Ricochet` timer runs it kills the ship on
// contact; when it expires the round goes inert again (it stays a normal bullet and dies to its own
// lifetime), so the arena never accumulates permanent hazards.
fn ricochet_update(
    time: Res<Time>,
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: EventWriter<SoundFx>,
    dev: Res<Dev>,
    ships: Query<(Entity, &Transform, &Ship)>,
    mut shots: Query<(Entity, &Transform, &mut Ricochet)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    for (re, rt, mut r) in &mut shots {
        r.0 -= dt;
        if r.0 <= 0.0 {
            commands.entity(re).remove::<Ricochet>(); // spent — inert from here
            continue;
        }
        if run.respawn > 0.0 {
            continue;
        }
        let p = rt.translation.truncate();
        for (se, st, sh) in &ships {
            if immune(sh, &dev) {
                continue;
            }
            let sp = st.translation.truncate();
            if p.distance(sp) < SHIP_R + BULLET_R {
                commands.entity(re).despawn();
                kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                break;
            }
        }
    }
}

// Toasts pop for a few seconds, then vanish.
fn toast_update(time: Res<Time>, mut commands: Commands, mut toasts: Query<(Entity, &mut Toast)>) {
    let dt = time.delta_secs();
    for (e, mut toast) in &mut toasts {
        toast.life -= dt;
        if toast.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

// The gold-rock hunt resolves here: once the whole gold lineage is gone, award +1 life — but only if
// the player cleared it. If a piece drifted off after its (long) grace, `forfeited` latched in
// asteroid_bounds and the life is denied. Capped at LIFE_CAP; grants at most once per lineage
// (clears `active`). A 1-frame lag on the count is fine — the grant just waits a tick.
fn gold_rush_update(
    mut commands: Commands,
    mut rush: ResMut<GoldRush>,
    mut run: ResMut<Run>,
    mut flash: ResMut<HudFlash>,
    mut stats: Option<ResMut<Stats>>, // Option so headless tests needn't insert it
    gold: Query<(), With<Gold>>,
    bank: Option<Res<SfxBank>>,
    root: Query<Entity, With<ToastRoot>>,
) {
    if !rush.active || !gold.is_empty() {
        return; // no hunt running, or gold pieces are still out there to clear
    }
    if !rush.forfeited && run.lives < LIFE_CAP {
        run.lives += 1;
        if let Some(s) = stats.as_mut() {
            s.golds += 1; // achievement progress: an extra life earned from a gold lineage
        }
        flash.life = HUD_FLASH_TIME; // flicker the life icons on the new life
        // UI colours must stay <= 1 (TextColor clamps per-channel), so a plain gold here
        spawn_toast(&mut commands, &root, Color::srgba(0.16, 0.11, 0.02, 0.92), "EXTRA LIFE", "GOLD ROCK CLEARED", Color::srgb(0.95, 0.8, 0.35));
        if let Some(b) = &bank {
            one_shot(&mut commands, b.life.clone(), 0.6);
        }
    }
    rush.active = false;
    rush.forfeited = false;
    // note: the cooldown to the next gold is armed at SPAWN time (in gold_spawn), measured from when
    // the rock appeared — so a slow hunt eats into the wait rather than adding to it.
}

// Lifetime progress persists to a tiny best-effort save file (space-separated numbers, 12 fields).
// Fields 7+ (bosses 3-6, mines, golds) were added after release — an OLDER save simply lacks them, so
// they read as their defaults (nothing lost, nothing wrongly granted). File I/O is compiled out of
// tests so the suite never touches the disk.
#[cfg(not(test))]
const SAVE_PATH: &str = "violet-edge.save";
#[cfg(not(test))]
fn read_progress() -> Option<Stats> {
    let text = std::fs::read_to_string(SAVE_PATH).ok()?;
    let n: Vec<&str> = text.split_whitespace().collect();
    if n.len() < 6 {
        return None; // the original six are the minimum a valid save carries
    }
    let flag = |i: usize| n.get(i).is_some_and(|v| *v == "1");
    let num = |i: usize| n.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
    Some(Stats {
        blue: n[0].parse().ok()?,
        green: n[1].parse().ok()?,
        enemies: n[2].parse().ok()?,
        warden: n[3] == "1",
        glutton: n[4] == "1",
        no_powerups: n[5] == "1",
        slinger: flag(6),
        detonator: flag(7),
        pulsar: flag(8),
        phantom: flag(9),
        mines: num(10),
        golds: num(11),
        orange: num(12),
        pulser: num(13),
        red: num(14),
        cluster: num(15),
        beacon: num(16),
        runs: num(17),
        waves: num(18),
        warps: num(19),
        deathless: flag(20),
        best_wave: num(21),
        pacifist: flag(22),
        hunter: num(23),
        seen: num(24),
        lapse: num(25),
        tenders: num(26),
        facet: num(27),
        husk: num(28),
    })
}
#[cfg(not(test))]
fn save_progress(s: &Stats) {
    let line = format!(
        "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
        s.blue,
        s.green,
        s.enemies,
        s.warden as u8,
        s.glutton as u8,
        s.no_powerups as u8,
        s.slinger as u8,
        s.detonator as u8,
        s.pulsar as u8,
        s.phantom as u8,
        s.mines,
        s.golds,
        s.orange,
        s.pulser,
        s.red,
        s.cluster,
        s.beacon,
        s.runs,
        s.waves,
        s.warps,
        s.deathless as u8,
        s.best_wave,
        s.pacifist as u8,
        s.hunter,
        s.seen,
        s.lapse,
        s.tenders,
        s.facet,
        s.husk
    );
    let _ = std::fs::write(SAVE_PATH, line); // best-effort — never block gameplay on I/O
}
#[cfg(test)]
fn read_progress() -> Option<Stats> {
    None
}
#[cfg(test)]
fn save_progress(_s: &Stats) {}

// ─────────────────────────────── high scores (top 5) ──────────────────
// On game over, slot the final score into the top-5 table if it qualifies, remember where it landed
// (for the game-over highlight), and persist. Runs before spawn_gameover_ui so the screen sees it.
// Also records the deepest wave REACHED (the screen's "you were close" marker — failure must still
// read as measurable progress, or a long run lost feels like nothing happened).
fn record_high_score(score: Res<Score>, mut hs: ResMut<HighScores>, wave: Res<Wave>, mut stats: Option<ResMut<Stats>>) {
    hs.just_placed = None;
    let s = score.0;
    if let Some(i) = hs.top.iter().position(|&h| s > h) {
        for j in (i + 1..hs.top.len()).rev() {
            hs.top[j] = hs.top[j - 1]; // shift the rest down
        }
        hs.top[i] = s;
        hs.just_placed = Some(i);
        save_high_scores(&hs);
    }
    if let Some(st) = stats.as_mut() {
        if wave.level as u32 > st.best_wave {
            st.best_wave = wave.level as u32;
            save_progress(st);
        }
    }
}

fn load_high_scores(mut hs: ResMut<HighScores>) {
    hs.top = read_high_scores();
}

#[cfg(not(test))]
const HISCORE_PATH: &str = "violet-edge.hiscore";
#[cfg(not(test))]
fn read_high_scores() -> [u32; 5] {
    let mut top = [0u32; 5];
    if let Ok(text) = std::fs::read_to_string(HISCORE_PATH) {
        for (i, tok) in text.split_whitespace().take(5).enumerate() {
            top[i] = tok.parse().unwrap_or(0);
        }
    }
    top
}
#[cfg(not(test))]
fn save_high_scores(hs: &HighScores) {
    let line: Vec<String> = hs.top.iter().map(|s| s.to_string()).collect();
    let _ = std::fs::write(HISCORE_PATH, line.join(" ")); // best-effort
}
#[cfg(test)]
fn read_high_scores() -> [u32; 5] {
    [0; 5]
}
#[cfg(test)]
fn save_high_scores(_hs: &HighScores) {}

// Game-Over screen: Enter restarts immediately; Esc quits to the main menu.
fn gameover_restart(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut next: ResMut<NextState<GameState>>,
    mut run: ResMut<Run>,
    mut score: ResMut<Score>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut warp: ResMut<Warp>,
    mut progress: (ResMut<BossState>, ResMut<Chain>, ResMut<MassShot>, ResMut<RunFlags>, ResMut<GoldRush>, ResMut<Warhead>, ResMut<Stats>, ResMut<PacifistWatch>, ResMut<Gorge>),
    field: Query<Entity, GameplayEntity>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Menu); // OnEnter(Menu) wipes the field
        return;
    }
    if !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    for e in &field {
        commands.entity(e).despawn();
    }
    reset_run(&mut commands, &mut run, &mut score, &mut wave, &mut banner, &mut warp, &mut progress.0, &mut progress.1, &mut progress.2, &mut progress.5, &mut progress.8, &mut progress.3, &mut progress.4, &mut progress.6, &mut progress.7);
    next.set(GameState::Playing); // field refills from the edges via top_up_asteroids
}

// The win screen: any confirm returns to the main menu (OnEnter(Menu) wipes the field). NG+ hooks here later.
fn victory_continue(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Space) {
        next.set(GameState::Menu);
    }
}

// ─────────────────────────────── music ────────────────────────────────
#[derive(Component)]
struct Music;

const MUSIC_VOLUME: f32 = 0.55;

// What the soundtrack should be playing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MusicCue {
    Main(u8), // the full-length main track at a CORRUPTION TIER (0 = clean … 5 = the last act's snarl)
    Buildup,  // the ~10 s riser in the run-up to a boss (one-shot)
    Boss,     // the boss track (loops)
    GameOver, // the produced ambient-synthwave game-over theme (loops) — see GAMEOVER_MP3
    Silence,  // the post-boss calm — a deliberate breather, no music
}

// The soundtrack director. Normal play loops the main track; the last 10 s before a boss play a
// buildup riser; boss waves loop the boss track; the post-boss calm is silent.
#[derive(Resource)]
struct MusicDirector {
    mains: Vec<Handle<AudioSource>>, // the main track at every corruption tier (index = bosses down)
    boss: Handle<AudioSource>,
    buildup: Handle<AudioSource>,
    gameover: Handle<AudioSource>,
    cue: Option<MusicCue>, // what's live (None = nothing spawned yet)
    muted: bool,
}

// Synthesize the tracks up front and install the director. The first cue is spawned by
// `music_director` on its first run.
fn start_music(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    // MAIN / BOSS / GAME OVER are all PRODUCED mp3s now (decoded by bevy's `mp3` feature); the
    // procedural score in audio.rs is retired to reference-render + fallback duty. `mains` is a
    // ONE-entry vec until corruption variants exist — `music_director` clamps the tier index to it.
    let mains = vec![sources.add(AudioSource { bytes: MAIN_MP3.to_vec().into() })];
    let boss = sources.add(AudioSource { bytes: BOSS_MP3.to_vec().into() });
    let gameover = sources.add(AudioSource { bytes: GAMEOVER_MP3.to_vec().into() });
    let buildup = sources.add(AudioSource { bytes: audio::boss_buildup_wav().into() });
    commands.insert_resource(MusicDirector { mains, boss, buildup, gameover, cue: None, muted: false });
}

// Spawn a Music player. Loops for the main/boss tracks; one-shot (Despawn) for the buildup riser.
// `gain` trims a track that masters at a different level from the rest of the score (1.0 = as-is).
fn play_track(commands: &mut Commands, handle: Handle<AudioSource>, muted: bool, looping: bool, gain: f32) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: if looping { PlaybackMode::Loop } else { PlaybackMode::Despawn },
            volume: Volume::Linear(if muted { 0.0 } else { MUSIC_VOLUME * gain }),
            ..default()
        },
        Music,
    ));
}

// Pick the right cue for the current moment and swap to it when it changes; `M` mutes.
fn music_director(
    input: Res<ActionState>,
    wave: Res<Wave>,
    state: Res<State<GameState>>,
    plus: Res<NewGamePlus>,
    mut dir: ResMut<MusicDirector>,
    mut commands: Commands,
    music: Query<Entity, With<Music>>,
    mut sinks: Query<&mut AudioSink, With<Music>>,
) {
    // Mute action (M) — mute/unmute by volume
    if input.mute {
        dir.muted = !dir.muted;
        let v = Volume::Linear(if dir.muted { 0.0 } else { MUSIC_VOLUME });
        for mut sink in &mut sinks {
            sink.set_volume(v);
        }
    }

    // Hold the current track through a pause (don't restart it); the mute toggle above still applies.
    if *state.get() == GameState::Paused {
        return;
    }
    // The wave-based cue only applies IN PLAYING. Otherwise force a screen-appropriate track — critically,
    // never leave the boss loop running over the Victory or menu screens after a win. The SPLASH is
    // silent too: the Baz sting owns the boot moment, and the menu track starting is the handoff.
    // Corruption tier = bosses down this run (wave 1-5 → 0 … 26-30 → 5). Off-run screens play the
    // CLEAN tier — including Victory: beating the Phantom hands the uncorrupted track back.
    // NEW GAME+: the Belt is ALREADY wrong when you arrive — the floor is tier 1, climbing from there.
    // CLAMPED to the variants that actually exist: the produced main currently ships as ONE track,
    // and without this the cue would change at every act boundary and RESTART the music mid-run.
    let top_tier = dir.mains.len().max(1) as u8 - 1;
    let tier = (((wave.level - 1) / 5).clamp(0, 5) as u8).max(if plus.0 { 1 } else { 0 }).min(top_tier);
    let desired = if *state.get() != GameState::Playing {
        match *state.get() {
            GameState::GameOver => MusicCue::GameOver, // its own somber track — a run ending SOUNDS different from playing one
            GameState::Splash => MusicCue::Silence,
            _ => MusicCue::Main(0),
        }
    } else if wave.calm > 0.0 {
        MusicCue::Silence // post-boss breather — let it be quiet, don't slam the track back on
    } else if is_boss_wave(wave.level) {
        MusicCue::Boss
    } else if is_boss_wave(wave.level + 1) && wave.timer <= BOSS_CAMEO_SECS {
        MusicCue::Buildup // last 10 s before the boss wave → riser leads in
    } else {
        MusicCue::Main(tier) // one boss down = one tier wronger, ONCE per-act variants exist (see top_tier)
    };

    if dir.cue != Some(desired) {
        for e in &music {
            commands.entity(e).despawn(); // matches by marker, fires even before the sink exists
        }
        match desired {
            MusicCue::Silence => {}
            MusicCue::Main(t) => {
                // clamp via get() rather than len()-1: an empty `mains` would underflow the index
                let h = dir.mains.get(t as usize).or_else(|| dir.mains.last()).cloned();
                if let Some(h) = h {
                    play_track(&mut commands, h, dir.muted, true, MAIN_GAIN); // produced track, hot master
                }
            }
            MusicCue::Boss => {
                let h = dir.boss.clone();
                play_track(&mut commands, h, dir.muted, true, BOSS_GAIN); // produced track, hot master
            }
            MusicCue::Buildup => {
                let h = dir.buildup.clone();
                play_track(&mut commands, h, dir.muted, false, 1.0);
            }
            MusicCue::GameOver => {
                let h = dir.gameover.clone();
                play_track(&mut commands, h, dir.muted, true, GAMEOVER_GAIN); // produced track, quieter master
            }
        }
        dir.cue = Some(desired);
    }
}

// ─────────────────────────────── sound effects ────────────────────────
// One event per SFX; gameplay systems fire them, `play_sfx` turns them into one-shot
// sounds — deduped to at most one of each kind per frame so a mine blast / chain sweep
// hitting many rocks doesn't stack into a wall of noise.
#[derive(Event, Clone, Copy)]
enum SoundFx {
    Fire,
    Break(u8), // asteroid size (1..3) → picks a deeper clip for bigger rocks
    Mine,
    Death,     // the player ship being destroyed
    EnemyShot, // an enemy mob firing
    EnemyDie,  // an enemy mob destroyed
    Warp,      // the warp/black-hole launch
    Toggle,    // switching standard ↔ mass shot
    Haunt,     // the Phantom's own spectral cue (ray, possession, death) — NOT the warp sound
    NovaPop,   // the Nova Shield eating a hit (glassy shatter)
    NovaUp,    // the Nova Shield flickering back online (soft rising shimmer)
    BossDown,  // a boss core detonating — the biggest single kill in the game
    Vortex,    // the warp hole OPEN — its 2.6s feeding churn + the collapse thump (launch is `Warp`)
}

// ─────────────────────────────── juice (hit-stop + screenshake) ───────
// The FEEL layer: big moments freeze the world for a breath and rattle the camera. Both are driven
// off the SoundFx events the game already emits — one director, zero flags in the kill sites (and
// anything that SOUNDS big automatically FEELS big). Photosensitivity: hit-stop is a freeze (no
// flash), shake is smooth multi-sine translation capped at a few px — motion, not strobe.
const HITSTOP_MAX: f32 = 0.14; // never freeze longer than this, no stacking
const SHAKE_MAX_PX: f32 = 14.0; // camera offset at FULL trauma (trauma² curve keeps usual shakes ~2-4 px)
const SHAKE_DECAY: f32 = 1.8; // trauma lost per second

#[derive(Resource, Default)]
struct HitStop(f32); // seconds of world-freeze left (ticked on REAL time)
#[derive(Resource, Default)]
struct Shake(f32); // screenshake trauma 0..1 — sources ADD, offset scales with trauma²

// Map this frame's sound events to juice. Loud = felt: the player's death and a boss core going
// down hit hardest; blasts rattle; a single big rock barely registers but a STREAK of them stacks
// into a visible rumble (trauma is additive, the trauma² curve hides one-offs).
fn juice_director(mut events: EventReader<SoundFx>, mut stop: ResMut<HitStop>, mut shake: ResMut<Shake>) {
    for e in events.read() {
        let (freeze, trauma) = match e {
            SoundFx::Death => (0.12, 0.55),
            SoundFx::BossDown => (0.14, 0.55),
            SoundFx::NovaPop => (0.07, 0.30), // the shield ate a lethal hit — a big save reads like one
            SoundFx::Mine => (0.0, 0.28),
            SoundFx::Warp => (0.0, 0.15),
            SoundFx::Haunt => (0.0, 0.20),
            SoundFx::Break(3) => (0.0, 0.10),
            _ => (0.0, 0.0),
        };
        stop.0 = stop.0.max(freeze).min(HITSTOP_MAX);
        shake.0 = (shake.0 + trauma).min(1.0);
    }
}

// Apply the juice on REAL time (so it works while the virtual clock is the thing being frozen):
// hold the virtual clock at zero while a hit-stop runs, decay trauma, and jiggle the camera with
// smooth layered sines (never per-frame randomness — that reads as strobe-jitter, not impact).
fn juice_apply(
    real: Res<Time<Real>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
    mut shake: ResMut<Shake>,
    mut cams: Query<&mut Transform, With<Camera2d>>,
) {
    let dt = real.delta_secs();
    if stop.0 > 0.0 {
        stop.0 -= dt;
        vtime.set_relative_speed(0.0);
    } else if vtime.relative_speed() != 1.0 {
        vtime.set_relative_speed(1.0);
    }
    shake.0 = (shake.0 - SHAKE_DECAY * dt).max(0.0);
    let t = real.elapsed_secs();
    let amp = shake.0 * shake.0 * SHAKE_MAX_PX;
    let off = Vec2::new((t * 39.7).sin() + 0.6 * (t * 71.3).sin(), (t * 47.9).cos() + 0.6 * (t * 83.1).sin()) * (amp / 1.6);
    for mut tf in &mut cams {
        tf.translation.x = off.x;
        tf.translation.y = off.y;
    }
}

// Pre-synthesized SFX clips (see `audio.rs`), built once at startup.
#[derive(Resource)]
struct SfxBank {
    fire: Handle<AudioSource>,
    break_rock: [Handle<AudioSource>; 3], // indexed by size-1: [small, mid, large] — big = deeper
    mine: Handle<AudioSource>,
    death: Handle<AudioSource>,
    enemy_shot: Handle<AudioSource>,
    enemy_die: Handle<AudioSource>,
    warp: Handle<AudioSource>,
    haunt: Handle<AudioSource>, // the Phantom's spectral cue
    nova_pop: Handle<AudioSource>, // the Nova Shield eating a hit
    nova_up: Handle<AudioSource>,  // the Nova Shield re-lighting
    achievement: Handle<AudioSource>,
    life: Handle<AudioSource>, // gold-rock 1UP jingle
    toggle: Handle<AudioSource>, // standard ↔ mass shot switch
    log: Handle<AudioSource>, // Pilot Log transmission-received blip
    boss_down: Handle<AudioSource>, // boss-core detonation boom
    vortex: Handle<AudioSource>, // the open warp hole's churn (matched to WARP_HOLE_LIFE)
}

fn start_sfx(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    commands.insert_resource(SfxBank {
        fire: sources.add(AudioSource { bytes: audio::fire_sfx_wav().into() }),
        break_rock: [1u8, 2, 3].map(|s| sources.add(AudioSource { bytes: audio::break_sfx_wav(s).into() })),
        mine: sources.add(AudioSource { bytes: audio::mine_sfx_wav().into() }),
        death: sources.add(AudioSource { bytes: audio::death_sfx_wav().into() }),
        enemy_shot: sources.add(AudioSource { bytes: audio::enemy_shot_wav().into() }),
        enemy_die: sources.add(AudioSource { bytes: audio::enemy_die_wav().into() }),
        warp: sources.add(AudioSource { bytes: audio::warp_wav().into() }),
        haunt: sources.add(AudioSource { bytes: audio::haunt_sfx_wav().into() }),
        nova_pop: sources.add(AudioSource { bytes: audio::nova_pop_sfx_wav().into() }),
        nova_up: sources.add(AudioSource { bytes: audio::nova_up_sfx_wav().into() }),
        achievement: sources.add(AudioSource { bytes: audio::achievement_sfx_wav().into() }),
        life: sources.add(AudioSource { bytes: audio::life_sfx_wav().into() }),
        toggle: sources.add(AudioSource { bytes: audio::toggle_sfx_wav().into() }),
        log: sources.add(AudioSource { bytes: audio::log_sfx_wav().into() }),
        boss_down: sources.add(AudioSource { bytes: audio::boss_down_sfx_wav().into() }),
        vortex: sources.add(AudioSource { bytes: audio::vortex_sfx_wav().into() }),
    });
}

// Spawn a one-shot sound that despawns itself when it finishes.
fn one_shot(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            ..default()
        },
    ));
}

fn play_sfx(mut commands: Commands, bank: Option<Res<SfxBank>>, mut events: EventReader<SoundFx>) {
    let Some(bank) = bank else {
        events.clear();
        return;
    };
    let (mut fire, mut mine, mut death, mut eshot, mut edie, mut warp, mut toggle, mut haunt, mut npop, mut nup, mut bdown, mut vortex) =
        (false, false, false, false, false, false, false, false, false, false, false, false);
    let mut brk: Option<u8> = None; // deepest (largest) rock that broke this frame
    for e in events.read() {
        match e {
            SoundFx::Fire => fire = true,
            SoundFx::Break(sz) => brk = Some(brk.unwrap_or(0).max(*sz)),
            SoundFx::Mine => mine = true,
            SoundFx::Death => death = true,
            SoundFx::EnemyShot => eshot = true,
            SoundFx::EnemyDie => edie = true,
            SoundFx::Warp => warp = true,
            SoundFx::Toggle => toggle = true,
            SoundFx::Haunt => haunt = true,
            SoundFx::NovaPop => npop = true,
            SoundFx::NovaUp => nup = true,
            SoundFx::BossDown => bdown = true,
            SoundFx::Vortex => vortex = true,
        }
    }
    if fire {
        one_shot(&mut commands, bank.fire.clone(), 0.3);
    }
    if let Some(sz) = brk {
        // one break sound per frame (the biggest rock's), kept well under the music (0.55) —
        // breaks are constant, so they mustn't dominate
        let clip = bank.break_rock[(sz.clamp(1, 3) - 1) as usize].clone();
        one_shot(&mut commands, clip, 0.3);
    }
    if mine {
        one_shot(&mut commands, bank.mine.clone(), 0.55); // present but softer — the old 0.8 was harsh on headphones
    }
    if death {
        one_shot(&mut commands, bank.death.clone(), 0.7); // losing a life is a big, clear event
    }
    if eshot {
        one_shot(&mut commands, bank.enemy_shot.clone(), 0.28); // incoming fire — audible, not naggy
    }
    if edie {
        one_shot(&mut commands, bank.enemy_die.clone(), 0.45);
    }
    if warp {
        one_shot(&mut commands, bank.warp.clone(), 0.6); // the ultimate — a big, distinct whoosh
    }
    if haunt {
        one_shot(&mut commands, bank.haunt.clone(), 0.35); // the Phantom's spectral cue — present, not naggy
    }
    if toggle {
        one_shot(&mut commands, bank.toggle.clone(), 0.4); // weapon-switch click
    }
    if npop {
        one_shot(&mut commands, bank.nova_pop.clone(), 0.6); // the shield eating a hit — a big save, heard clearly
    }
    if nup {
        one_shot(&mut commands, bank.nova_up.clone(), 0.35); // back online — soft, informative
    }
    if bdown {
        one_shot(&mut commands, bank.boss_down.clone(), 0.75); // the biggest kill in the game, heard as one
    }
    if vortex {
        one_shot(&mut commands, bank.vortex.clone(), 0.5); // the hole's churn — present under the field, not over it
    }
}

// ─────────────────────────────── app ──────────────────────────────────
// DEV: F1 toggles invincibility. Compiled into debug builds ONLY — a release build
// has no system that can flip `Dev`, so god-mode can't ship by accident.
#[cfg(debug_assertions)]
fn dev_toggle(keys: Res<ButtonInput<KeyCode>>, mut dev: ResMut<Dev>) {
    if keys.just_pressed(KeyCode::F1) {
        dev.invincible = !dev.invincible;
        info!("DEV invincibility: {}", if dev.invincible { "ON" } else { "OFF" });
    }
}

// DEV: F2 skips to the next wave — on a boss wave it kills whichever boss is present (so it advances
// via that boss's normal death path); otherwise it expires the timer. Handles ALL boss types (Warden /
// Devourer / Slinger), so you can always skip past a boss without fighting it. Debug builds only.
#[cfg(debug_assertions)]
fn dev_wave_skip(
    keys: Res<ButtonInput<KeyCode>>,
    mut wave: ResMut<Wave>,
    mut bosses: Query<&mut Boss>,
    mut devourers: Query<&mut Devourer>,
    mut slingers: Query<&mut Slinger>,
    mut detonators: Query<&mut Detonator>,
    mut pulsars: Query<&mut Pulsar>,
    mut phantoms: Query<&mut Phantom>,
) {
    if keys.just_pressed(KeyCode::F2) {
        // kill any boss present (each boss's death advances the run — the Phantom's death WINS it)
        let mut killed = false;
        for mut b in &mut bosses { b.hp = 0; killed = true; }
        for mut d in &mut devourers { d.hp = 0; killed = true; }
        for mut s in &mut slingers { s.hp = 0; killed = true; }
        for mut d in &mut detonators { d.hp = 0; killed = true; }
        for mut p in &mut pulsars { p.hp = 0; killed = true; }
        for mut s in &mut phantoms { s.phase = 3; s.hp = 0; s.transition = 0.0; s.charge = 0.0; killed = true; } // final phase + zero, clear any reset/intro → the win path (one press)
        if !killed {
            wave.timer = 0.0; // a normal wave → advance ONE wave (step through each; it kills every boss type too)
        }
        info!("DEV skip");
    }
}

// DEV: F3 drifts in a large explosive (orange) rock so it can be eyeballed before it's wired into
// the wave content. Debug builds only.
#[cfg(debug_assertions)]
fn dev_spawn_orange(keys: Res<ButtonInput<KeyCode>>, arena: Res<Arena>, mut commands: Commands) {
    if keys.just_pressed(KeyCode::F3) {
        let mut rng = rand::thread_rng();
        let h = arena.half;
        // drop it MID-FIELD (not from an edge) so there are rocks around to show the blast + chain
        let pos = Vec2::new(rng.gen_range(-h.x * 0.5..h.x * 0.5), rng.gen_range(-h.y * 0.5..h.y * 0.5));
        let vel = Vec2::from_angle(rng.gen_range(0.0..TAU)) * 40.0;
        let e = spawn_asteroid(&mut commands, pos, 3, vel, &mut rng, false);
        commands.entity(e).insert(Explosive);
        info!("DEV spawn orange (mid-field)");
    }
}

// DEV: F4 jumps straight to the wave-30 FINALE — wipes the field (sparing the ship) and drops the wave to
// 30 with the boss counter reset, so the boss director spawns a fresh Phantom next frame. Lets the final
// boss be tested without clearing 29 waves first (pair with F1 for invincibility). Debug builds only.
#[cfg(debug_assertions)]
fn dev_face_phantom(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut wave: ResMut<Wave>,
    mut state: ResMut<BossState>,
    field: Query<Entity, (GameplayEntity, Without<Ship>)>,
) {
    if keys.just_pressed(KeyCode::F4) {
        for e in &field {
            commands.entity(e).despawn(); // wipe rocks / mobs / any current boss — the ship is spared
        }
        wave.level = 30; // the Phantom's wave (the finale)
        wave.timer = WAVE_SECS;
        wave.calm = 0.0;
        state.fought = 0; // ≠ 30 → boss_director spawns the Phantom this frame
        info!("DEV: face the Phantom (wave 30)");
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VIOLET EDGE".into(),
                resolution: (1280.0_f32, 800.0_f32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.02, 0.01, 0.06)))
        .insert_resource(Score(0))
        .insert_resource(Run { lives: START_LIVES, respawn: 0.0, ..default() })
        .insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 })
        .insert_resource(WaveBanner { timer: WAVE_BANNER_SECS }) // flash "WAVE 1" at start
        .insert_resource(SpawnClock::default())
        .insert_resource(MineClock::default())
        .insert_resource(EnemyClock::default())
        .insert_resource(WellClock::default())
        .insert_resource(Warp { charges: WARP_MAX_CHARGES, cooldown: 0.0 })
        .insert_resource(WarpField::default())
        .insert_resource(Arena { half: Vec2::new(640.0, 400.0) })
        .insert_resource(Dev::default())
        .insert_resource(BossState::default())
        .insert_resource(Chain::default())
        .insert_resource(MassShot::default())
        .insert_resource(Warhead::default())
        .insert_resource(Stats::default())
        .insert_resource(Achievements::default())
        .insert_resource(LoreSeen::default())
        .insert_resource(SplashClock::default())
        .insert_resource(GalleryPage::default())
        .insert_resource(TenderClock::default())
        .insert_resource(Gorge::default())
        .insert_resource(NewGamePlus::default())
        .insert_resource(PacifistWatch::default())
        .insert_resource(RunFlags::default())
        .insert_resource(GoldRush::default())
        .insert_resource(FireArmed::default())
        .insert_resource(TitleIntroPlayed::default())
        .insert_resource(HighScores::default())
        .insert_resource(Bindings::default())
        .insert_resource(ActionState::default())
        .insert_resource(InputMethod::default())
        .insert_resource(Rebinding::default())
        .add_systems(PreUpdate, gather_input)
        .insert_resource(HudFlash::default())
        .insert_resource(ShotModeFlash::default())
        .insert_resource(VictoryReveal::default())
        .add_event::<SoundFx>()
        .add_event::<MenuClick>()
        .init_state::<GameState>()
        .add_systems(Startup, (setup, spawn_hud, spawn_toast_root, load_progress, load_high_scores, start_music, start_sfx, set_window_icon))
        // always: keep the arena sized, handle pause input, refresh the HUD text
        .add_systems(Update, (update_arena, update_ui_scale, pause_toggle, update_wave_text, update_score_text, wave_banner_update, calm_countdown_update, boss_warning_update).chain())
        // always: watch for achievement unlocks + age out toasts + hide the HUD off-run + menu buttons
        .add_systems(Update, (achievements, lore_watch, toast_update, hud_visibility, hud_ability_labels, button_shimmer, button_click, hud_flash_tick, shot_mode_update))
        .insert_resource(HitStop::default())
        .insert_resource(Shake::default())
        .add_systems(Update, (juice_director, juice_apply).chain())
        // the neon warm-up + frame pulse is a START-MENU flourish only (not the achievements screen)
        .add_systems(Update, menu_title_fx.run_if(in_state(GameState::Menu)))
        // render in PostUpdate so it ALWAYS runs after every Update system (incl.
        // ship_bounds) — draws final positions, no border ghosting; runs in all states
        .add_systems(PostUpdate, (render, render_boss, render_extras, render_shockwaves))
        // gameplay only while Playing
        .add_systems(
            Update,
            // split into THREE chained groups (Bevy's tuple limit is 20 systems);
            // the groups still run fully in order, first → second → third.
            (
                (
                    ship_control,
                    fire,
                    chain_fire,
                    warp_fire,
                    integrate,
                    bullet_trail,
                    ship_trail,
                    warp_missile_update,
                    black_hole_update,
                    update_warp_field,
                    asteroid_collisions,
                )
                    .chain(),
                (
                    nova_tick,  // both shield ticks run their regen/grace BEFORE the frame's
                    aegis_tick, // death checks, so a shield that came back this tick can save you
                    ship_death,
                    mine_update,
                    tender_update,
                    enemy_update,
                    enemy_bullets,
                    well_update,
                    // the boss systems nested as one unit: Bevy caps a tuple at 20 systems, and
                    // `.chain()` on the outer tuple still runs these in exactly this order
                    (
                        boss_director,
                        boss_update,
                        devourer_update,
                        slinger_update,
                        detonator_update,
                        pulsar_update,
                        phantom_update,
                        boss_shield,
                        shield_deflect,
                    )
                        .chain(),
                    chain_update,
                    pickup_update,
                    drone_update,
                    respawn,
                )
                    .chain(),
                (
                    particle_update,
                    shockwave_update,
                    spin_asteroids,
                    ship_bounds,
                    asteroid_bounds,
                    bullet_bounds,
                    collisions,
                    wave_timer,
                    // the spawners nested for the same 20-system tuple limit; order preserved
                    (top_up_asteroids, top_up_mines, top_up_enemies, top_up_tenders, top_up_wells).chain(),
                    clear_calm_field,
                    gold_spawn,
                    gold_rush_update,
                    red_growth,
                    hunter_update,
                    lapse_update,
                    ricochet_update,
                    gallery_sightings,
                    detonate,
                )
                    .chain(),
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(Update, (possessed_update, phantom_dissolve, spectral_trail_update, escape_shard_update, departing_ship_update).run_if(in_state(GameState::Playing))) // the Haunt's p2 vessels, its rock-dissolving body, p3 wake + the finale's fleeing core & departing ship (no ordering needs — kept off the main chain)
        .add_systems(Update, (music_director, play_sfx))
        .add_systems(Update, menu_start.run_if(in_state(GameState::Menu)))
        .add_systems(
            Update,
            submenu_back.run_if(in_state(GameState::Achievements).or(in_state(GameState::Briefing)).or(in_state(GameState::Lore)).or(in_state(GameState::Gallery))),
        )
        .add_systems(Update, gameover_restart.run_if(in_state(GameState::GameOver)))
        .add_systems(Update, (victory_continue, victory_reveal).run_if(in_state(GameState::Victory)))
        .add_systems(OnEnter(GameState::Playing), disarm_fire)
        .add_systems(OnEnter(GameState::Splash), spawn_splash)
        .add_systems(OnExit(GameState::Splash), despawn_splash)
        .add_systems(Update, splash_update.run_if(in_state(GameState::Splash)))
        .add_systems(OnEnter(GameState::Menu), (clear_field, spawn_menu_ui))
        .add_systems(OnExit(GameState::Menu), (despawn_menu_ui, mark_title_intro_played))
        .add_systems(OnEnter(GameState::Achievements), spawn_achievements_ui)
        .add_systems(OnExit(GameState::Achievements), despawn_achievements_ui)
        .add_systems(OnEnter(GameState::Controls), spawn_controls_ui)
        .add_systems(OnExit(GameState::Controls), despawn_controls_ui)
        .add_systems(Update, (controls_input, rebind_slot_click, rebind_capture, controls_display).run_if(in_state(GameState::Controls)))
        .add_systems(OnEnter(GameState::Briefing), spawn_briefing_ui)
        .add_systems(OnExit(GameState::Briefing), despawn_briefing_ui)
        .add_systems(OnEnter(GameState::Gallery), spawn_gallery_ui)
        .add_systems(OnExit(GameState::Gallery), despawn_gallery_ui)
        .add_systems(Update, (gallery_draw, gallery_page_turn).run_if(in_state(GameState::Gallery)))
        .add_systems(OnEnter(GameState::Lore), spawn_lore_ui)
        .add_systems(OnExit(GameState::Lore), despawn_lore_ui)
        .add_systems(OnEnter(GameState::Paused), spawn_pause_ui)
        .add_systems(OnExit(GameState::Paused), despawn_pause_ui)
        .add_systems(OnEnter(GameState::GameOver), (record_high_score, spawn_gameover_ui).chain())
        .add_systems(OnExit(GameState::GameOver), despawn_gameover_ui)
        .add_systems(OnEnter(GameState::Victory), (record_high_score, spawn_victory_ui).chain())
        .add_systems(OnExit(GameState::Victory), despawn_victory_ui);
    // dev-only tools (F1 invincibility, F2 wave-skip, F3 spawn-orange, F4 face-the-Phantom); compiled out of release builds
    #[cfg(debug_assertions)]
    app.add_systems(Update, (dev_toggle, dev_wave_skip, dev_spawn_orange, dev_face_phantom).run_if(in_state(GameState::Playing)));
    install_menu_font(&mut app); // must exist before the initial OnEnter(Menu)
    install_logo(&mut app); // ditto — the menu masthead needs the LogoImage at first OnEnter(Menu)
    app.run();
}

// ─────────────────────────────── headless tests ───────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_spawns_a_bullet() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(MassShot::default());
        app.insert_resource(Warhead::default());
        app.insert_resource(Gorge::default());
        app.insert_resource(ShotModeFlash::default());
        app.insert_resource(FireArmed(true)); // mid-run: the gun is armed
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Run { lives: 3, ..default() });
        app.insert_resource(ActionState { fire_held: true, ..default() }); // holding fire
        app.world_mut().spawn((
            Ship { angle: TAU / 4.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, fire);
        app.update();
        let n = app.world_mut().query::<&Bullet>().iter(app.world()).count();
        assert!(n > 0, "holding fire should spawn a bullet, got {n}");
    }

    #[test]
    fn a_held_fire_button_at_start_does_not_shoot_until_released() {
        // the click/press that starts a run must not leak into an instant shot: with FireArmed(false)
        // and the button already held, no bullet spawns until the button is released and pressed again
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(MassShot::default());
        app.insert_resource(Warhead::default());
        app.insert_resource(Gorge::default());
        app.insert_resource(ShotModeFlash::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(FireArmed(false)); // just entered Playing (disarm_fire ran)
        app.insert_resource(Run { lives: 3, ..default() });
        app.insert_resource(ActionState { fire_held: true, ..default() }); // still holding fire from the click that started the run
        app.world_mut().spawn((
            Ship { angle: TAU / 4.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, fire);
        app.update();
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 0, "a held button at start must NOT fire");
        // release the button → the gun arms
        app.world_mut().resource_mut::<ActionState>().fire_held = false;
        app.update();
        assert!(app.world().resource::<FireArmed>().0, "releasing the button arms the gun");
        // press again → now it fires
        app.world_mut().resource_mut::<ActionState>().fire_held = true;
        app.update();
        assert!(app.world_mut().query::<&Bullet>().iter(app.world()).count() > 0, "a fresh press after release fires");
    }

    #[test]
    fn gather_input_maps_keys_to_actions() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Bindings::default());
        app.insert_resource(ActionState::default());
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ArrowLeft); // turn left
        keys.press(KeyCode::ArrowUp); // thrust
        keys.press(KeyCode::Space); // fire
        keys.press(KeyCode::KeyQ); // toggle shot
        app.insert_resource(keys);
        app.insert_resource(ButtonInput::<MouseButton>::default());
        app.add_systems(Update, gather_input);
        app.update();
        let s = app.world().resource::<ActionState>();
        assert!(s.turn > 0.5, "ArrowLeft = turn left (+turn), got {}", s.turn);
        assert!(s.thrust > 0.5, "ArrowUp = thrust");
        assert!(s.fire_held, "Space = fire (held)");
        assert!(s.toggle, "Q = toggle shot");
        assert!(!s.warp && !s.chain && !s.pause, "unpressed actions stay false");
    }

    #[test]
    fn rebind_capture_replaces_an_actions_bind() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Bindings::default());
        app.insert_resource(Rebinding { target: Some((Action::Fire, false)), armed: true });
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyF);
        app.insert_resource(keys);
        app.insert_resource(ButtonInput::<MouseButton>::default());
        app.add_systems(Update, rebind_capture);
        app.update();
        let b = app.world().resource::<Bindings>();
        assert_eq!(binds_label(&b.kbm, Action::Fire), "F", "Fire's keyboard bind is replaced with F");
        assert!(app.world().resource::<Rebinding>().target.is_none(), "capture ends once a bind is set");
    }

    #[test]
    fn rebind_capture_ignores_escape() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Bindings::default());
        app.insert_resource(Rebinding { target: Some((Action::Fire, false)), armed: true });
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Escape);
        app.insert_resource(keys);
        app.insert_resource(ButtonInput::<MouseButton>::default());
        app.add_systems(Update, rebind_capture);
        app.update();
        assert!(app.world().resource::<Rebinding>().target.is_some(), "Esc is reserved for cancel, never captured as a bind");
    }

    #[test]
    fn bullet_destroys_overlapping_asteroid() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new(), mass: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        assert!(app.world().resource::<Score>().0 >= 20, "a bullet on an asteroid should score a hit");
    }

    #[test]
    fn dense_rock_chips_before_it_breaks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a dense size-2 rock = 2 hp: the first hit only cracks it. RED so the final break is a
        // GUARANTEED pair (the split economy rolls otherwise, and gold no longer makes smalls) —
        // this test is about the chip and the density inheritance.
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: true, hp: 2 },
            Red { cool: RED_ABSORB_EVERY },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        // still one rock, now at 1 hp, and nothing scored — a chip, not a break
        let rocks: Vec<(bool, i32)> = app.world_mut().query::<&Asteroid>().iter(app.world()).map(|a| (a.dense, a.hp)).collect();
        assert_eq!(rocks.len(), 1, "the dense rock survives the first hit");
        assert_eq!(rocks[0], (true, 1), "the first hit chips hp from 2 to 1");
        assert_eq!(app.world().resource::<Score>().0, 0, "a chip scores nothing");

        // a second bullet finishes it: it shatters into two dense chunks and scores double
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.update();
        let chunks: Vec<bool> = app.world_mut().query::<&Asteroid>().iter(app.world()).map(|a| a.dense).collect();
        assert_eq!(chunks.len(), 2, "the second hit shatters it into two chunks");
        assert!(chunks.iter().all(|&d| d), "dense chunks inherit the density");
        assert!(app.world().resource::<Score>().0 >= 100, "a dense size-2 break scores double (>=100)");
    }

    #[test]
    fn split_chunks_fly_apart_not_stack() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a size-2 RED rock (guaranteed pair — this test is about the PAIR's geometry) with a bullet on it
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Red { cool: RED_ABSORB_EVERY },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new(), mass: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        let chunks: Vec<(Vec2, Vec2)> = app
            .world_mut()
            .query::<(&Transform, &Velocity, &Asteroid)>()
            .iter(app.world())
            .map(|(t, v, _)| (t.translation.truncate(), v.0))
            .collect();
        assert_eq!(chunks.len(), 2, "a split pair spawns two chunks");
        // they must spawn clear of each other, not stacked at the break point
        let sep = chunks[0].0.distance(chunks[1].0);
        assert!(sep > asteroid_radius(1) * 2.0, "chunks must spawn clear of each other, got separation {sep}");
        // both must actually be launched, in opposing directions (fly apart)
        assert!(chunks[0].1.length() > 1.0 && chunks[1].1.length() > 1.0, "both chunks need a launch velocity");
        assert!(chunks[0].1.dot(chunks[1].1) < 0.0, "chunks should head in opposing directions");
    }

    #[test]
    fn overlapping_asteroids_push_apart() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        let a = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::new(10.0, 0.0)),
                Transform::from_xyz(-10.0, 0.0, 0.0),
            ))
            .id();
        let b = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::new(-10.0, 0.0)),
                Transform::from_xyz(10.0, 0.0, 0.0),
            ))
            .id();
        app.add_systems(Update, asteroid_collisions);
        for _ in 0..40 {
            app.update();
        }
        let pa = app.world().entity(a).get::<Transform>().unwrap().translation.truncate();
        let pb = app.world().entity(b).get::<Transform>().unwrap().translation.truncate();
        assert!(pa.distance(pb) >= 129.0, "overlapping asteroids should separate; got {}", pa.distance(pb));
    }

    #[test]
    fn a_stopped_rock_keeps_drifting() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(GoldRush::default());
        // a rock at rest, mid-arena (elastic hits could have zeroed it → "stuck")
        let rock = app
            .world_mut()
            .spawn((Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, asteroid_bounds);
        app.update();
        let v = app.world().entity(rock).get::<Velocity>().unwrap().0;
        assert!((v.length() - MIN_DRIFT).abs() < 1.0, "a stopped rock is nudged back to a slow drift, got {}", v.length());
    }

    #[test]
    fn small_rocks_thin_out_offscreen_but_large_ones_persist() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let h = Vec2::new(640.0, 400.0);
        app.insert_resource(Arena { half: h });
        app.insert_resource(GoldRush::default());
        // 60 small + 60 large rocks, all parked just off the left edge and drifting further out
        // (fast enough that the MIN_DRIFT nudge won't fire and pull them back on-screen).
        for _ in 0..60 {
            for size in [1u8, 3u8] {
                let r = asteroid_radius(size);
                app.world_mut().spawn((
                    Asteroid { size, verts: vec![Vec2::X * r], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                    Velocity(Vec2::new(-MIN_DRIFT * 2.0, 0.0)),
                    Transform::from_xyz(-h.x - r - 5.0, 0.0, 0.0),
                ));
            }
        }
        app.add_systems(Update, asteroid_bounds);
        app.update();
        let mut q = app.world_mut().query::<&Asteroid>();
        let smalls = q.iter(app.world()).filter(|a| a.size == 1).count();
        let larges = q.iter(app.world()).filter(|a| a.size == 3).count();
        assert!(smalls < 60, "some small rocks should be culled off-screen, not all recycled; {smalls}/60 remain");
        assert_eq!(larges, 60, "large rocks are never culled off-screen; {larges}/60 remain");
    }

    #[test]
    fn fresh_fragments_are_not_culled_off_screen() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let h = Vec2::new(640.0, 400.0);
        app.insert_resource(Arena { half: h });
        app.insert_resource(GoldRush::default());
        let r = asteroid_radius(1);
        // a small fragment that broke at the edge and flew off — but it's still in its grace window,
        // so it must recycle back into play rather than being culled (the near-edge-break case)
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * r], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::new(-MIN_DRIFT * 2.0, 0.0)),
            Transform::from_xyz(-h.x - r - 5.0, 0.0, 0.0),
            Fresh(FRAGMENT_GRACE),
        ));
        app.add_systems(Update, asteroid_bounds);
        app.update();
        let alive = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(alive, 1, "a fresh fragment must recycle back in, not be culled off-screen");
    }

    #[test]
    fn clearing_the_gold_lineage_grants_a_life() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        app.insert_resource(HudFlash::default());
        app.insert_resource(Run { lives: 1, respawn: 0.0, ..default() }); // below the cap, so a life can be restored
        // no Gold entities remain → the player cleared the whole lineage
        app.add_systems(Update, gold_rush_update);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, 2, "clearing the whole gold lineage restores +1 life");
        assert!(!app.world().resource::<GoldRush>().active, "the hunt resets after granting (grants once)");
    }

    #[test]
    fn warping_the_gold_rock_grants_the_life() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        app.insert_resource(HudFlash::default());
        app.insert_resource(Run { lives: LIFE_CAP - 1, respawn: 0.0, ..default() });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a gold rock sitting on an open hole → the warp swallows it (a player action, so it should pay out)
        app.world_mut().spawn((BlackHole { life: 5.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Gold,
        ));
        app.add_systems(Update, (black_hole_update, gold_rush_update));
        app.update();
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 0, "the warp swallows the gold rock");
        assert_eq!(app.world().resource::<Run>().lives, LIFE_CAP, "warping the whole gold lineage grants +1 life — the warp is a player action");
    }

    #[test]
    fn gold_lives_are_capped() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        app.insert_resource(HudFlash::default());
        app.insert_resource(Run { lives: LIFE_CAP, respawn: 0.0, ..default() });
        app.add_systems(Update, gold_rush_update);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, LIFE_CAP, "lives never exceed LIFE_CAP");
    }

    #[test]
    fn a_forfeited_gold_hunt_grants_no_life() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(GoldRush { active: true, forfeited: true, cooldown: 0.0 }); // a piece was lost
        app.insert_resource(HudFlash::default());
        app.insert_resource(Run { lives: 1, respawn: 0.0, ..default() }); // below the cap, so only the forfeit blocks it
        app.add_systems(Update, gold_rush_update);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, 1, "a forfeited hunt grants nothing");
    }

    #[test]
    fn gold_pieces_recycle_during_their_grace() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let h = Vec2::new(640.0, 400.0);
        app.insert_resource(Arena { half: h });
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        let r = asteroid_radius(1);
        // gold pieces still WITHIN their grace drift off-screen → all recycle (protected), no forfeit
        for _ in 0..60 {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * r], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::new(-MIN_DRIFT * 2.0, 0.0)),
                Transform::from_xyz(-h.x - r - 5.0, 0.0, 0.0),
                Gold,
                Fresh(GOLD_GRACE),
            ));
        }
        app.add_systems(Update, asteroid_bounds);
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 60, "gold pieces within their grace all recycle — none lost");
        assert!(!app.world().resource::<GoldRush>().forfeited, "no forfeit while the pieces are still protected");
    }

    #[test]
    fn a_gold_piece_lost_after_its_grace_forfeits_the_hunt() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let h = Vec2::new(640.0, 400.0);
        app.insert_resource(Arena { half: h });
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        let r = asteroid_radius(1);
        // grace expired (no Fresh) → a gold piece that drifts off CAN be culled, forfeiting the hunt
        for _ in 0..60 {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * r], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::new(-MIN_DRIFT * 2.0, 0.0)),
                Transform::from_xyz(-h.x - r - 5.0, 0.0, 0.0),
                Gold,
            ));
        }
        app.add_systems(Update, asteroid_bounds);
        app.update();
        assert!(app.world().resource::<GoldRush>().forfeited, "a gold piece drifting off after its grace forfeits the hunt");
    }

    #[test]
    fn a_gold_rock_spawns_as_one_large_rock() {
        // the spawn helper itself: exactly one gold rock, and it's LARGE (a full lineage to clear)
        fn spawner(mut commands: Commands, arena: Res<Arena>) {
            let mut rng = rand::thread_rng();
            spawn_gold_rock(&mut commands, arena.half, &mut rng);
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, spawner);
        app.update();
        let sizes: Vec<u8> = app.world_mut().query_filtered::<&Asteroid, With<Gold>>().iter(app.world()).map(|a| a.size).collect();
        assert_eq!(sizes, vec![3], "one large (size-3) gold rock spawns");
    }

    #[test]
    fn a_gold_lineage_is_short_and_makes_no_small_fragments() {
        // The 1UP hunt: a LARGE gold sheds two MID golds (lineage stays gold, so it must all be
        // cleared) and those mids die CLEAN. Three hittable targets, and crucially no smalls — the
        // tiny stragglers that used to drift off the edge and forfeit the life.
        fn break_gold(size: u8) -> (usize, usize, Vec<u8>) {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Stats::default());
            app.insert_resource(RunFlags::default());
            app.insert_resource(Score(0));
            let r = asteroid_radius(size);
            app.world_mut().spawn((
                Asteroid { size, verts: vec![Vec2::X * r], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
                Gold,
            ));
            app.world_mut().spawn((
                Bullet { life: 1.0, trail: Vec::new(), mass: false },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
            app.add_systems(Update, collisions);
            app.update();
            let sizes: Vec<u8> = {
                let mut q = app.world_mut().query::<&Asteroid>();
                q.iter(app.world()).map(|a| a.size).collect()
            };
            let gold = app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count();
            (sizes.len(), gold, sizes)
        }
        let (total, gold, sizes) = break_gold(3);
        assert_eq!(total, 2, "a LARGE gold sheds exactly two chunks");
        assert_eq!(gold, 2, "both stay gold, so the whole lineage must still be cleared");
        assert!(sizes.iter().all(|&s| s == 2), "…and they're MIDS, not smalls: {sizes:?}");
        let (total, ..) = break_gold(2);
        assert_eq!(total, 0, "a gold MID dies clean — the hunt never leaves small stragglers");
    }

    #[test]
    fn gold_spawn_drifts_one_in_when_the_countdown_elapses() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 }); // wave 2+: gold is eligible (wave 1 is gated)
        app.insert_resource(GoldRush { active: false, forfeited: false, cooldown: 0.0 }); // due now
        app.add_systems(Update, gold_spawn);
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 1, "the countdown elapsing spawns exactly one gold rock");
        let rush = app.world().resource::<GoldRush>();
        assert!(rush.active, "spawning starts the hunt");
        assert!(rush.cooldown >= GOLD_GAP_EARLY_MIN, "a long gap to the next gold is armed at spawn (no back-to-back)");
    }

    #[test]
    fn gold_never_spawns_in_wave_one() {
        // A spare life in wave 1 was useless (nothing threatening yet), so gold is gated to wave 2+.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(GoldRush { active: false, forfeited: false, cooldown: 0.0 }); // "due", but wave 1 blocks it
        app.add_systems(Update, gold_spawn);
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 0, "no gold rock in wave 1");
        assert!(!app.world().resource::<GoldRush>().active, "and no hunt is started");
    }

    #[test]
    fn gold_spawn_holds_during_its_cooldown() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(GoldRush { active: false, forfeited: false, cooldown: 30.0 }); // still waiting
        app.add_systems(Update, gold_spawn);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 0, "no gold spawns while the cooldown is still running (no back-to-back)");
    }

    #[test]
    fn gold_spawn_is_blocked_during_a_hunt_or_the_calm() {
        for (active, calm) in [(true, 0.0f32), (false, 5.0f32)] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
            app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm });
            app.insert_resource(GoldRush { active, forfeited: false, cooldown: 0.0 });
            app.add_systems(Update, gold_spawn);
            app.update();
            assert_eq!(
                app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(),
                0,
                "no gold spawns during an active hunt or the post-boss calm (active={active}, calm={calm})"
            );
        }
    }

    #[test]
    fn a_top5_score_is_recorded_in_rank_order() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Score(350));
        app.insert_resource(HighScores { top: [500, 400, 300, 200, 100], just_placed: None });
        app.insert_resource(Wave { level: 7, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, record_high_score);
        app.update();
        let hs = app.world().resource::<HighScores>();
        assert_eq!(hs.top, [500, 400, 350, 300, 200], "the score slots in by rank and pushes the rest down");
        assert_eq!(hs.just_placed, Some(2), "its placement is remembered for the game-over highlight");
    }

    #[test]
    fn a_score_below_the_table_is_not_recorded() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Score(50));
        app.insert_resource(HighScores { top: [500, 400, 300, 200, 100], just_placed: None });
        app.insert_resource(Wave { level: 7, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, record_high_score);
        app.update();
        let hs = app.world().resource::<HighScores>();
        assert_eq!(hs.top, [500, 400, 300, 200, 100], "a sub-table score leaves the board unchanged");
        assert_eq!(hs.just_placed, None, "and doesn't count as a placement");
    }

    #[test]
    fn a_bullet_lights_an_orange_instead_of_splitting_it() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Explosive,
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new(), mass: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "an orange detonates, it does NOT split into chunks");
        assert_eq!(app.world_mut().query_filtered::<(), With<Detonating>>().iter(app.world()).count(), 1, "the bullet lights the orange (marked Detonating)");
    }

    #[test]
    fn a_lit_orange_detonates_chains_and_spares_gold() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Dev::default());
        app.insert_resource(Score(0));
        app.insert_resource(Stats::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(NextState::<GameState>::default());
        // the lit orange at origin (fuse already elapsed → blows this update)
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Explosive,
            Detonating { fuse: 0.0, friendly: false },
        ));
        // a plain LARGE rock in range → obliterated outright (a normal break would leave 2 chunks)
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(30.0, 0.0, 0.0),
        ));
        // a second orange in range → should be lit (chain)
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(-30.0, 0.0, 0.0),
            Explosive,
        ));
        // a gold rock in range → spared
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 30.0, 0.0),
            Gold,
        ));
        app.add_systems(Update, detonate);
        app.update();
        // the original orange detonated → only the chained orange remains, and it's now lit
        assert_eq!(app.world_mut().query_filtered::<(), With<Explosive>>().iter(app.world()).count(), 1, "the detonated orange is gone; the chained one remains");
        assert_eq!(app.world_mut().query_filtered::<(), With<Detonating>>().iter(app.world()).count(), 1, "the nearby orange is lit — a chain reaction");
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 1, "gold is spared by the blast");
        let plain = app.world_mut().query_filtered::<(), (With<Asteroid>, Without<Explosive>, Without<Gold>)>().iter(app.world()).count();
        assert_eq!(plain, 0, "the large plain rock is obliterated outright — no leftover chunks");
    }

    #[test]
    fn ship_dies_on_asteroid_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 0, "ship should die on contact");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life should be lost");
        assert!(app.world().resource::<Run>().respawn > 0.0, "a respawn should be scheduled");
    }

    #[test]
    fn nova_shield_absorbs_one_hit_then_down_costs_a_life() {
        // Same lethal overlap as ship_dies_on_asteroid_contact, but with the Nova Shield UP:
        // the first hit is eaten (ship lives, no life lost, shield collapses into its regen);
        // once the pop-grace has passed, the still-overlapping rock kills for real.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, nova: Nova { unlocked: true, down: 0.0, grace: 0.0 }, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 1, "the shield eats the hit — the ship lives");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life is lost on an absorb");
        assert!(app.world().resource::<Run>().nova.down > 0.0, "the shield collapses into its regen");
        assert!(app.world().resource::<Run>().nova.grace > 0.0, "a brief pop-grace opens");
        // grace over, shield still down, rock still overlapping → this one costs the life
        app.world_mut().resource_mut::<Run>().nova.grace = 0.0;
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 0, "a hit while the shield is down kills");
        assert_eq!(app.world().resource::<Run>().lives, 2, "and costs the life");
    }

    #[test]
    fn nova_pickup_raises_the_shield() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Nova },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, pickup_update);
        app.update();
        let run = app.world().resource::<Run>();
        assert!(run.nova.unlocked, "grabbing the Nova orb raises the shield");
        assert!(run.nova.down <= 0.0, "and it comes up immediately");
        assert!(app.world().resource::<RunFlags>().powerup_used, "it counts as a powerup (blocks Purist)");
    }

    #[test]
    fn rock_reeling_in_does_not_kill_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a rock still reeling in (grab in progress), sitting on the ship, must NOT cost a life
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: 0.0 },
        ));
        app.add_systems(Update, ship_death);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 1, "the ship survives a rock the boss is dragging in");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life is lost mid-grab");
    }

    #[test]
    fn settled_shield_rock_still_kills_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a rock that has FINISHED reeling in (settled into orbit) is a live hazard again
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: BOSS_GRAB_TIME },
        ));
        app.add_systems(Update, ship_death);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 0, "flying into the orbiting shield still kills");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life is lost to a settled shield rock");
    }

    #[test]
    fn ship_death_emits_a_sound() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let sounds: Vec<SoundFx> = app.world_mut().resource_mut::<Events<SoundFx>>().drain().collect();
        assert!(sounds.iter().any(|&s| matches!(s, SoundFx::Death)), "destroying the ship should emit a death sound");
    }

    #[test]
    fn invulnerable_ship_survives_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 2.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 1, "an invulnerable ship should NOT die");
    }

    #[test]
    fn dev_invincibility_prevents_death() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev { invincible: true }); // god-mode ON
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 1, "dev invincibility should keep the ship alive through a lethal hit");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life lost while invincible");
    }

    #[test]
    fn last_life_ends_the_run() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 1, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, ship_death);
        app.update();
        // last life → NOT an instant Game Over: a short countdown is armed so the death plays out,
        // and `respawn` makes the transition once it elapses.
        assert_eq!(app.world().resource::<Run>().lives, 0);
        assert_eq!(app.world().resource::<Run>().respawn, GAMEOVER_DELAY, "the final death arms a game-over beat, not an instant screen");
    }

    #[test]
    fn respawn_flips_to_game_over_when_out_of_lives() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(NextState::<GameState>::default());
        // a game-over countdown all but elapsed, with no lives left — any dt drives it <= 0
        app.insert_resource(Run { lives: 0, respawn: f32::EPSILON, ..default() });
        app.add_systems(Update, respawn);
        for _ in 0..5 {
            app.update();
        }
        assert!(
            matches!(app.world().resource::<NextState<GameState>>(), NextState::Pending(GameState::GameOver)),
            "once the game-over countdown elapses with no lives, respawn transitions to Game Over"
        );
    }

    #[test]
    fn wave_advances_when_timer_expires() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Wave { level: 1, timer: 0.0, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, wave_timer);
        app.update();
        assert_eq!(app.world().resource::<Wave>().level, 2, "expiring the timer should advance the wave");
    }

    #[test]
    fn boss_warning_names_and_flashes_during_the_runup() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        // run-up to wave 5 (the Warden): one below a boss wave, inside the cameo window, no calm
        app.insert_resource(Wave { level: 4, timer: 5.0, calm: 0.0 });
        let txt = app.world_mut().spawn((BossWarnText, Text::new(""), TextColor(Color::NONE))).id();
        let flash = app.world_mut().spawn((BossWarnFlash, BackgroundColor(Color::NONE))).id();
        app.add_systems(Update, boss_warning_update);
        app.update();
        let name = app.world().entity(txt).get::<Text>().unwrap().0.clone();
        assert!(name.contains("THE WARDEN"), "the run-up names the incoming boss, got {name:?}");
        assert!(app.world().entity(txt).get::<TextColor>().unwrap().0.alpha() > 0.0, "warning text is visible");
        assert!(app.world().entity(flash).get::<BackgroundColor>().unwrap().0.alpha() > 0.0, "screen flash is visible");
    }

    #[test]
    fn boss_warning_hidden_when_no_boss_imminent() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        // an ordinary wave with a non-boss wave next → no warning
        app.insert_resource(Wave { level: 2, timer: 5.0, calm: 0.0 });
        let txt = app.world_mut().spawn((BossWarnText, Text::new("stale"), TextColor(Color::WHITE))).id();
        let flash = app.world_mut().spawn((BossWarnFlash, BackgroundColor(Color::WHITE))).id();
        app.add_systems(Update, boss_warning_update);
        app.update();
        assert_eq!(app.world().entity(txt).get::<TextColor>().unwrap().0.alpha(), 0.0, "no warning text off the run-up");
        assert_eq!(app.world().entity(flash).get::<BackgroundColor>().unwrap().0.alpha(), 0.0, "no screen flash off the run-up");
    }

    #[test]
    fn boss_warning_names_match_each_boss_wave() {
        // the run-up sees `level + 1`; waves 5/10/15/20 are the four bosses
        assert_eq!(boss_kind_name(boss_kind(5)), "THE WARDEN");
        assert_eq!(boss_kind_name(boss_kind(10)), "THE GLUTTON");
        assert_eq!(boss_kind_name(boss_kind(15)), "THE SLINGER");
        assert_eq!(boss_kind_name(boss_kind(20)), "THE DETONATOR");
    }

    #[test]
    fn boss_wave_hud_shows_the_boss_name() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 10, timer: WAVE_SECS, calm: 0.0 });
        let e = app.world_mut().spawn((WaveText, Text::new(""))).id();
        app.add_systems(Update, update_wave_text);
        app.update();
        assert_eq!(
            app.world().entity(e).get::<Text>().unwrap().0,
            "WAVE 10    THE GLUTTON",
            "the boss-wave HUD names the boss instead of a generic BOSS"
        );
    }

    #[test]
    fn act_iii_introduces_red_asteroids() {
        fn reds(level: i32, n: usize) -> usize {
            let mut rng = rand::thread_rng();
            (0..n).filter(|_| matches!(roll_rock_kind(level, false, &mut rng), RockKind::Red)).count()
        }
        assert!(reds(23, 600) > 0, "wave 23 (Act III) spawns red growing asteroids");
        assert_eq!(reds(12, 600), 0, "no red asteroids before wave 21");
        assert_eq!(reds(21, 200), 200, "wave 21 is ALL red — the act's teaching wave (red is Act III's carrier)");
        assert_eq!(reds(25, 200), 200, "the Pulsar's wave (25) is fought over a pure red field");
    }

    #[test]
    fn red_growth_absorbs_a_neighbor_and_grows() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        // a small red beside a normal rock within absorb range, cooldown ready
        let red = app
            .world_mut()
            .spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 15.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Red { cool: 0.0 },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Transform::from_xyz(40.0, 0.0, 0.0), // within RED_ABSORB_R
        ));
        app.add_systems(Update, red_growth);
        app.update();
        assert_eq!(app.world().entity(red).get::<Asteroid>().unwrap().size, 2, "the red absorbs its neighbor and swells a size");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "the eaten rock is gone — only the grown red remains");
    }

    #[test]
    fn a_red_absorbs_another_red_in_a_mono_pack() {
        // the wave-30 finale spawns all-red groups: with no other kind to eat, a red must still be able to
        // consume a fellow red (and the pair must NOT annihilate each other — one grows, one is gone).
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 15.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Red { cool: 0.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 15.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Red { cool: 0.0 },
            Transform::from_xyz(40.0, 0.0, 0.0), // within RED_ABSORB_R
        ));
        app.add_systems(Update, red_growth);
        app.update();
        let reds = app.world_mut().query_filtered::<&Asteroid, With<Red>>().iter(app.world()).collect::<Vec<_>>().len();
        assert_eq!(reds, 1, "one red eats the other — an all-red pack consolidates instead of drifting inert");
        let grown = app.world_mut().query::<(&Asteroid, &Red)>().iter(app.world()).next().unwrap().0.size;
        assert_eq!(grown, 2, "the surviving red swelled a size from the meal");
    }

    #[test]
    fn a_red_shot_splits_into_smaller_reds() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a large red + a plain (non-mass) bullet on it → the whack-a-mole split
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Red { cool: RED_ABSORB_EVERY },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let reds = app.world_mut().query_filtered::<(), With<Red>>().iter(app.world()).count();
        assert_eq!(reds, 2, "a plain shot splits a red into two smaller reds — the whack-a-mole");
    }

    #[test]
    fn a_cluster_shatters_into_a_shard_ring() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a large cluster + a plain bullet on it → it SHATTERS into the shard ring, not two chunks
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Cluster,
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let shards: Vec<u8> = app.world_mut().query_filtered::<&Asteroid, With<Cluster>>().iter(app.world()).map(|a| a.size).collect();
        assert_eq!(shards.len(), CLUSTER_SHARDS, "the cluster shatters into its full shard ring");
        assert!(shards.iter().all(|&s| s == 1), "every shard is the smallest size (no re-shatter chain)");
    }

    #[test]
    fn a_beacon_aura_blocks_gunfire_until_it_falls() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a beacon, a plain rock INSIDE its aura, and a bullet on that rock
        let beacon = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: true, hp: 3 },
                Beacon,
                Velocity(Vec2::ZERO),
                Transform::from_xyz(120.0, 0.0, 0.0), // 120 < BEACON_AURA_R from the target rock
            ))
            .id();
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(
            app.world_mut().query_filtered::<(), (With<Asteroid>, Without<Beacon>)>().iter(app.world()).count(),
            1,
            "the aura-shielded rock survives the shot"
        );
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 0, "the round fizzled on the aura");
        // the beacon falls → the same rock is shootable again
        app.world_mut().entity_mut(beacon).despawn();
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.update();
        let plain_left = app.world_mut().query_filtered::<&Asteroid, Without<Beacon>>().iter(app.world()).filter(|a| a.size == 2).count();
        assert_eq!(plain_left, 0, "with the beacon down, the rock breaks normally");
    }

    #[test]
    fn a_shot_mine_pays_its_bounty_but_blast_rocks_are_free() {
        // The scoring rule: destroying the MINE is aimed play and pays MINE_SCORE; the rocks its
        // blast happens to shatter pay NOTHING (points never come from standing near an explosion).
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((Mine { armed: false, fuse: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        // a small rock inside the blast radius — it must break, for free
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(MINE_BLAST_R - 12.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 0, "the shot pops the mine");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "and its blast breaks the rock");
        assert_eq!(app.world().resource::<Score>().0, MINE_SCORE, "score is the mine bounty EXACTLY — the blasted rock added nothing");
        assert_eq!(app.world().resource::<Stats>().mines, 1, "the Minesweeper counter still ticks");
    }

    #[test]
    fn the_aura_stops_warheads_inside_and_nothing_outside() {
        // The two edges the basic aura test doesn't pin: (1) even a WARHEAD round fizzles on a
        // shielded rock — the aura's whole point is that no gun answers it; (2) the shield is a
        // RADIUS, not a blanket — a rock past BEACON_AURA_R breaks normally while the beacon lives.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: true, hp: 3 },
            Beacon,
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // one rock just inside the reach, one safely past it (size 1 so neither is size-2-ambiguous)
        let inside = Vec2::new(BEACON_AURA_R - 30.0, 0.0);
        let outside = Vec2::new(BEACON_AURA_R + 120.0, 0.0);
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(inside.x, inside.y, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(outside.x, outside.y, 0.0),
        ));
        // a WARHEAD round on the shielded rock, a standard round on the free one
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, WarheadShot, Velocity(Vec2::ZERO), Transform::from_xyz(inside.x, inside.y, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(outside.x, outside.y, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let rocks: Vec<Vec2> = {
            let mut q = app.world_mut().query_filtered::<(&Transform, &Asteroid), Without<Beacon>>();
            q.iter(app.world()).map(|(t, _)| t.translation.truncate()).collect()
        };
        assert_eq!(rocks.len(), 1, "exactly one of the two rocks survives the volley");
        assert!(rocks[0].distance(inside) < 1.0, "the SHIELDED rock is the survivor — the warhead fizzled on the aura");
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 0, "both rounds are spent: the warhead consumed by the aura, the standard by its kill");
    }


    #[test]
    fn pulsar_wave_spawns_the_fifth_boss() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 25, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Pulsar>().iter(app.world()).count(), 1, "wave 25 spawns the Pulsar");
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0, "and not the Warden placeholder");
    }

    #[test]
    fn pulsar_takes_gunfire_only_while_dark() {
        fn hp_after(phase: f32) -> i32 {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Stats::default());
            app.insert_resource(RunFlags::default());
            app.insert_resource(Score(0));
            let boss = app
                .world_mut()
                .spawn((
                    Pulsar { hp: PULSAR_HP, entered: true, charge: 0.0, phase, shock_cool: PULSAR_SHOCK_EVERY, pulse: 0.0, dying: 0.0 },
                    Transform::from_xyz(0.0, 0.0, 0.0),
                ))
                .id();
            app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
            app.add_systems(Update, collisions);
            app.update();
            app.world().entity(boss).get::<Pulsar>().unwrap().hp
        }
        assert_eq!(hp_after(0.0), PULSAR_HP - 1, "a DARK Pulsar (phase 0 → sin≈0, below the lit threshold) takes the hit");
        assert_eq!(hp_after(std::f32::consts::FRAC_PI_2), PULSAR_HP, "a LIT Pulsar (phase π/2 → sin≈1) shrugs the shot off");
    }

    #[test]
    fn phantom_wave_spawns_the_final_boss() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Phantom>().iter(app.world()).count(), 1, "wave 30 spawns the Phantom");
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0, "and not the Warden placeholder");
    }

    #[test]
    fn phantom_is_a_ghost_until_it_surfaces() {
        // THE HAUNT: intangible by default (shots sail through); solid + hittable only while `vuln` runs
        fn hp_after(vuln: f32) -> i32 {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Stats::default());
            app.insert_resource(Score(0));
            let mut ph = Phantom::new(PHANTOM_PHASE_HP, true, 0.0);
            ph.vuln = vuln;
            let boss = app.world_mut().spawn((ph, Transform::from_xyz(0.0, 0.0, 0.0))).id();
            app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
            app.add_systems(Update, collisions);
            app.update();
            app.world().entity(boss).get::<Phantom>().unwrap().hp
        }
        assert_eq!(hp_after(0.0), PHANTOM_PHASE_HP, "a ghost Phantom shrugs the shot straight through");
        assert_eq!(hp_after(1.0), PHANTOM_PHASE_HP - 1, "a SURFACED Phantom takes the hit — the punish window");
    }

    #[test]
    fn defeating_the_phantom_wins_the_run() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Run { lives: 3, respawn: 1.0, ..default() });
        app.insert_resource(Dev::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(NextState::<GameState>::default());
        // the FINAL phase's pool just emptied → the death SCENE sets up (NOT an instant Victory screen)
        let mut ph = Phantom::new(0, true, 0.0);
        ph.phase = 3; // clearing the last phase wins; earlier phases only reset
        let boss = app.world_mut().spawn((ph, Transform::from_xyz(0.0, 0.0, 0.0))).id();
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Stats::default()); // the win must RECORD (Edgelord + Purist)
        app.insert_resource(RunFlags::default()); // no powerup grabbed this run
        app.add_systems(Update, phantom_update);
        app.update();
        {
            let p = app.world().entity(boss).get::<Phantom>().unwrap();
            assert!(p.victory > 0.0, "the finale kill starts the death scene");
            assert!(!p.erupted, "it gathers to the middle first — hasn't erupted yet");
        }
        assert!(!matches!(app.world().resource::<NextState<GameState>>(), NextState::Pending(GameState::Victory)), "the Victory screen does NOT pop while the send-off plays");
        assert_eq!(app.world().resource::<Score>().0, BOSS_SCORE, "the kill banks the finale score");
        // the boss is already at centre → next tick it ERUPTS and the true-form core tears free east
        app.update();
        assert!(app.world().entity(boss).get::<Phantom>().unwrap().erupted, "gathered in → erupts");
        assert_eq!(app.world_mut().query::<&EscapeShard>().iter(app.world()).count(), 1, "the true-form core tears free and flees east");
        // the erupt records the win: this is what Edgelord ("beat the game") actually keys on now —
        // and with no powerup grabbed this run, Purist lands too
        let s = app.world().resource::<Stats>();
        assert!(s.phantom, "beating the Haunt records the wave-30 win");
        assert!(s.no_powerups, "a clean (no-powerup) win records Purist");
        assert!(ach_met(Ach::Edgelord, s), "Edgelord = the real wave-30 win");
        // the core flies off the arena (simulate it gone). With no ship left to launch, the send-off is
        // complete → NOW the Victory screen, and the boss is despawned.
        let shards: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<EscapeShard>>().iter(app.world()).collect();
        for e in shards {
            app.world_mut().entity_mut(e).despawn();
        }
        app.update();
        assert!(
            matches!(app.world().resource::<NextState<GameState>>(), NextState::Pending(GameState::Victory)),
            "once everything's cleared, the run transitions to the Victory screen"
        );
        assert_eq!(app.world_mut().query::<&Phantom>().iter(app.world()).count(), 0, "and the boss is despawned");
    }

    #[test]
    fn lore_entries_decrypt_with_their_boss_flags() {
        // a save that has never LAUNCHED shows nothing — the log only begins once the pilot flies
        let fresh = lore_entries(&Stats::default());
        assert_eq!(fresh.iter().filter(|e| e.2).count(), 0, "no entry decrypts before the first launch");
        let first_flight = lore_entries(&Stats { runs: 1, ..default() });
        assert!(first_flight[0].2, "THE BELT decrypts on the first deployment");
        assert_eq!(first_flight.iter().filter(|e| e.2).count(), 1, "and only THE BELT");
        // each boss flag decrypts exactly its record (spot-check the ladder)
        let after_warden = lore_entries(&Stats { warden: true, ..default() });
        assert!(after_warden[1].2, "the Warden's fall decrypts THE WARDEN");
        assert!(!after_warden[7].2, "the ARCHITECT stays hidden until the game is beaten");
        // the wave-30 win reveals both the Haunt's record AND who it answered to
        let after_win = lore_entries(&Stats { phantom: true, ..default() });
        assert!(after_win[6].2 && after_win[7].2, "beating the Haunt decrypts THE HAUNT and THE ARCHITECT");
    }

    #[test]
    fn a_log_decrypt_pops_one_toast_and_only_once() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let stats = Stats { runs: 1, ..default() };
        // LoreSeen seeded from the loaded save, exactly as load_progress does at startup
        app.insert_resource(LoreSeen(lore_entries(&stats).map(|(.., open, _)| open)));
        app.insert_resource(stats);
        app.world_mut().spawn((ToastRoot, Node::default()));
        app.add_systems(Update, lore_watch);
        app.update();
        assert_eq!(app.world_mut().query::<&Toast>().iter(app.world()).count(), 0, "already-decrypted entries never re-toast on boot");
        app.world_mut().resource_mut::<Stats>().warden = true;
        app.update();
        assert_eq!(app.world_mut().query::<&Toast>().iter(app.world()).count(), 1, "the Warden's record decrypting pops exactly one toast");
        app.update();
        assert_eq!(app.world_mut().query::<&Toast>().iter(app.world()).count(), 1, "and never a second one for the same entry");
    }

    #[test]
    fn achievement_triggers_map_to_the_right_stats() {
        // The regression this guards: Edgelord ("beat the game") used to fire on BOSS 2 — the old
        // 10-wave arc — so it unlocked a third of the way into the real 30-wave run.
        let mut s = Stats { glutton: true, ..default() };
        assert!(!ach_met(Ach::Edgelord, &s), "boss 2 alone must NOT count as beating the game");
        assert!(ach_met(Ach::Glutton, &s), "boss 2 has its own achievement");
        s.phantom = true;
        assert!(ach_met(Ach::Edgelord, &s), "the Haunt kill (wave 30) IS beating the game");
        // each boss keys its own flag
        let bosses = [
            (Ach::Warden, Stats { warden: true, ..default() }),
            (Ach::Slinger, Stats { slinger: true, ..default() }),
            (Ach::Detonator, Stats { detonator: true, ..default() }),
            (Ach::Pulsar, Stats { pulsar: true, ..default() }),
        ];
        for (a, st) in bosses {
            assert!(ach_met(a, &st), "the boss flag unlocks its achievement");
            assert!(!ach_met(a, &Stats::default()), "and stays locked without it");
        }
        // the lifetime grinds sit at their (deliberately steep) thresholds — one below misses,
        // exactly at unlocks. Table-driven so every counter achievement is pinned the same way.
        let grinds: [(Ach, fn(u32) -> Stats, u32); 11] = [
            (Ach::TrueBlue, |n| Stats { blue: n, ..default() }, ACH_BLUE),
            (Ach::GreenThumb, |n| Stats { green: n, ..default() }, ACH_GREEN),
            (Ach::Demolition, |n| Stats { orange: n, ..default() }, ACH_ORANGE),
            (Ach::BeatIt, |n| Stats { pulser: n, ..default() }, ACH_PULSER),
            (Ach::SeeingRed, |n| Stats { red: n, ..default() }, ACH_RED),
            (Ach::IceBreaker, |n| Stats { cluster: n, ..default() }, ACH_CLUSTER),
            (Ach::Keymaster, |n| Stats { beacon: n, ..default() }, ACH_BEACON),
            (Ach::Minesweeper, |n| Stats { mines: n, ..default() }, ACH_MINES),
            (Ach::GoldRush, |n| Stats { golds: n, ..default() }, ACH_GOLDS),
            (Ach::WaveGoodbye, |n| Stats { waves: n, ..default() }, ACH_WAVES),
            (Ach::EventHorizon, |n| Stats { warps: n, ..default() }, ACH_WARPS),
        ];
        for (a, make, at) in grinds {
            assert!(!ach_met(a, &make(at - 1)), "one short of the threshold must stay locked");
            assert!(ach_met(a, &make(at)), "hitting the threshold unlocks");
        }
        // the restart ladder — dying a lot is the expected way to play, so it's celebrated
        for (a, at) in [(Ach::Runs10, 10), (Ach::Runs25, 25), (Ach::Runs50, 50)] {
            assert!(!ach_met(a, &Stats { runs: at - 1, ..default() }));
            assert!(ach_met(a, &Stats { runs: at, ..default() }));
        }
        // the capstones key their own dedicated flags, not each other's
        assert!(ach_met(Ach::Untouchable, &Stats { deathless: true, ..default() }));
        assert!(!ach_met(Ach::Untouchable, &Stats { phantom: true, no_powerups: true, ..default() }));
        assert!(ach_met(Ach::Pacifist, &Stats { pacifist: true, ..default() }));
        assert!(!ach_met(Ach::Pacifist, &Stats { deathless: true, phantom: true, ..default() }));
        assert!(ach_met(Ach::Purist, &Stats { no_powerups: true, ..default() }));
        assert!(!ach_met(Ach::Purist, &Stats { phantom: true, deathless: true, ..default() }));
    }

    #[test]
    fn rock_kills_credit_the_right_lifetime_counter() {
        // beacon/pulser/red/cluster rocks are ALSO dense internally (2-hp bodies), so the helper must
        // check the special kinds BEFORE falling through to the plain green/blue split.
        let mut s = Stats::default();
        credit_rock_kill(&mut s, Flavor { dense: true, beacon: true, ..default() }); // dense + beacon
        assert_eq!((s.beacon, s.green), (1, 0), "a beacon credits beacon, not green");
        credit_rock_kill(&mut s, Flavor { dense: true, pulser: true, ..default() }); // dense + pulser
        assert_eq!((s.pulser, s.green), (1, 0), "a pulser credits pulser, not green");
        credit_rock_kill(&mut s, Flavor { dense: true, red: true, ..default() }); // dense + red
        assert_eq!((s.red, s.green), (1, 0), "a red credits red, not green");
        credit_rock_kill(&mut s, Flavor { dense: true, cluster: true, ..default() }); // dense + cluster
        assert_eq!((s.cluster, s.green), (1, 0), "a cluster credits cluster, not green");
        credit_rock_kill(&mut s, Flavor { dense: true, ..default() }); // plain dense
        credit_rock_kill(&mut s, Flavor::default()); // plain blue
        assert_eq!((s.green, s.blue), (1, 1), "plain rocks split on the dense flag");
    }

    #[test]
    fn the_save_line_round_trips_every_stat_field() {
        // The save is a positional space-separated line; a field written out of order would silently
        // swap two lifetime counters. Serialize a fully-populated Stats and re-read it via the same
        // positional contract read_progress uses.
        let s = Stats {
            blue: 1,
            green: 2,
            enemies: 3,
            warden: true,
            glutton: false,
            no_powerups: true,
            slinger: false,
            detonator: true,
            pulsar: false,
            phantom: true,
            mines: 4,
            golds: 5,
            orange: 6,
            pulser: 7,
            red: 8,
            cluster: 9,
            beacon: 10,
            runs: 11,
            waves: 12,
            warps: 13,
            deathless: true,
            best_wave: 14,
            pacifist: true,
            hunter: 15,
            seen: 0b1010_1010,
            lapse: 17,
            tenders: 19,
            facet: 21,
            husk: 23,
        };
        // mirror save_progress's field order (the real fn is a test no-op so runs can't clobber saves)
        let line = format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            s.blue,
            s.green,
            s.enemies,
            s.warden as u8,
            s.glutton as u8,
            s.no_powerups as u8,
            s.slinger as u8,
            s.detonator as u8,
            s.pulsar as u8,
            s.phantom as u8,
            s.mines,
            s.golds,
            s.orange,
            s.pulser,
            s.red,
            s.cluster,
            s.beacon,
            s.runs,
            s.waves,
            s.warps,
            s.deathless as u8,
            s.best_wave,
            s.pacifist as u8,
            s.hunter,
            s.seen,
            s.lapse,
            s.tenders,
            s.facet,
            s.husk
        );
        let n: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(n.len(), 29, "the save line carries all 29 fields");
        let flag = |i: usize| n[i] == "1";
        let num = |i: usize| n[i].parse::<u32>().unwrap();
        assert_eq!((num(0), num(1), num(2)), (s.blue, s.green, s.enemies));
        assert_eq!((flag(3), flag(4), flag(5)), (s.warden, s.glutton, s.no_powerups));
        assert_eq!((flag(6), flag(7), flag(8), flag(9)), (s.slinger, s.detonator, s.pulsar, s.phantom));
        assert_eq!((num(10), num(11)), (s.mines, s.golds));
        assert_eq!((num(12), num(13), num(14), num(15), num(16)), (s.orange, s.pulser, s.red, s.cluster, s.beacon));
        assert_eq!((num(17), num(18), num(19)), (s.runs, s.waves, s.warps));
        assert!(flag(20), "deathless rides in slot 20");
        assert_eq!(num(21), s.best_wave, "best_wave rides in slot 21");
        assert!(flag(22), "pacifist rides in slot 22");
        assert_eq!(num(23), s.hunter, "hunter kills ride in slot 23");
        assert_eq!(num(24), s.seen, "the gallery sightings bitmask rides in slot 24");
        assert_eq!((num(25), num(26), num(27), num(28)), (s.lapse, s.tenders, s.facet, s.husk), "the lapse / tender / facet / husk tallies ride in the final slots");
        // an OLD 12-field save (pre-expansion) must still load — new counters default to zero
        let old = "5 4 3 1 0 0 1 0 0 0 2 1";
        assert_eq!(old.split_whitespace().count(), 12);
        // (read_progress itself is stubbed under test; this pins the compatibility CONTRACT — the
        // .get()-with-default reads — by mirroring its accessors over the short line)
        let o: Vec<&str> = old.split_whitespace().collect();
        let onum = |i: usize| o.get(i).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let oflag = |i: usize| o.get(i).is_some_and(|v| *v == "1");
        assert_eq!(onum(11), 1, "the old save's last field still reads");
        assert_eq!(onum(17), 0, "missing runs field defaults to 0");
        assert!(!oflag(20), "missing deathless field defaults to false");
        assert_eq!(onum(23), 0, "missing hunter-kill field defaults to 0");
        assert_eq!(onum(24), 0, "missing GALLERY sightings mask defaults to 0 — an old save just re-discovers");
    }

    #[test]
    fn ship_control_signs_and_drift_behave() {
        // The flight contract: +turn rotates CCW and never adds velocity; thrust accelerates along
        // the facing; releasing everything only ever DECAYS speed (drag, no phantom forces).
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(ActionState { turn: 1.0, ..default() });
        let ship = app
            .world_mut()
            .spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO)))
            .id();
        app.add_systems(Update, ship_control);
        app.update();
        app.update(); // first tick's dt is 0 under MinimalPlugins — the second does the moving
        assert!(app.world().entity(ship).get::<Ship>().unwrap().angle > 0.0, "+turn = counter-clockwise");
        assert_eq!(app.world().entity(ship).get::<Velocity>().unwrap().0, Vec2::ZERO, "turning alone never adds velocity");
        // face +X and burn: velocity grows along the facing only
        app.world_mut().entity_mut(ship).get_mut::<Ship>().unwrap().angle = 0.0;
        app.insert_resource(ActionState { thrust: 1.0, ..default() });
        app.update();
        let v = app.world().entity(ship).get::<Velocity>().unwrap().0;
        assert!(v.x > 0.0 && v.y.abs() < f32::EPSILON, "thrust accelerates along the facing");
        // hands off: speed decays, direction holds (pure drag — no hidden steering)
        app.insert_resource(ActionState::default());
        app.update();
        let after = app.world().entity(ship).get::<Velocity>().unwrap().0;
        assert!(after.x < v.x && after.x > 0.0 && after.y.abs() < f32::EPSILON, "coasting only bleeds speed");
    }

    #[test]
    fn two_clean_waves_unlock_pacifist_and_breaks_reset_the_streak() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 1, timer: 0.0, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Stats::default());
        app.insert_resource(Run { lives: 3, ..default() });
        app.insert_resource(PacifistWatch { primed_at_level: 1, breaks: 0, fires: 0, streak: 0 });
        app.add_systems(Update, wave_timer);
        // dying mid-wave does NOT matter — the test is restraint, not survival
        app.world_mut().resource_mut::<Run>().died = true;
        app.update(); // wave 1 ends: nothing broken → streak 1
        assert!(!app.world().resource::<Stats>().pacifist, "one clean wave isn't enough");
        app.world_mut().resource_mut::<Wave>().timer = 0.0;
        app.update(); // wave 2 ends clean → streak 2 → the unlock (deaths and all)
        assert!(app.world().resource::<Stats>().pacifist, "two straight clean waves = Pacifist, dying included");
        // a wave where the player broke a rock: the streak resets instead of counting
        app.world_mut().resource_mut::<Stats>().pacifist = false;
        let breaks_now = total_breaks(app.world().resource::<Stats>());
        *app.world_mut().resource_mut::<PacifistWatch>() = PacifistWatch { primed_at_level: 3, breaks: breaks_now, fires: 0, streak: 1 };
        app.world_mut().resource_mut::<Stats>().blue += 1; // a kill mid-wave
        app.world_mut().resource_mut::<Wave>().timer = 0.0;
        app.update();
        assert_eq!(app.world().resource::<PacifistWatch>().streak, 0, "breaking anything resets the streak");
        assert!(!app.world().resource::<Stats>().pacifist, "a dirty pair never unlocks it");
        // firing a powerup (chain/mass/warhead) is also a break, even if it hits nothing
        let breaks_now = total_breaks(app.world().resource::<Stats>());
        *app.world_mut().resource_mut::<PacifistWatch>() = PacifistWatch { primed_at_level: 4, breaks: breaks_now, fires: 0, streak: 1 };
        app.world_mut().resource_mut::<Run>().powerup_fires = 1; // a chain beam left the ship mid-wave
        app.world_mut().resource_mut::<Wave>().timer = 0.0;
        app.update();
        assert_eq!(app.world().resource::<PacifistWatch>().streak, 0, "reaching for a powerup resets the streak too");
    }

    #[test]
    fn juice_freezes_time_on_big_hits_and_pops_rings_on_kills() {
        // director: loud events become hit-stop + trauma; apply: the virtual clock actually freezes
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(HitStop::default());
        app.insert_resource(Shake::default());
        app.add_systems(Update, (juice_director, juice_apply).chain());
        app.world_mut().send_event(SoundFx::BossDown);
        app.update();
        assert!(app.world().resource::<HitStop>().0 > 0.0, "a boss going down freezes the world for a beat");
        assert!(app.world().resource::<Shake>().0 > 0.0, "and rattles the camera");
        assert_eq!(app.world().resource::<Time<Virtual>>().relative_speed(), 0.0, "the virtual clock is held at zero");
        // a Fire event is NOT juice — quiet frames drain the stop and give the clock back
        app.world_mut().resource_mut::<HitStop>().0 = 0.0;
        app.world_mut().send_event(SoundFx::Fire);
        app.update();
        assert_eq!(app.world().resource::<Time<Virtual>>().relative_speed(), 1.0, "normal speed returns after the stop");

        // kill pop: a size-2 rock dying leaves a type-colored ring (plus its two children)
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Shockwave>().iter(app.world()).count(), 1, "the kill leaves one pop ring");
        let kids = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert!(kids == 0 || kids == 2, "a medium sheds 2 smalls or dies clean (split economy), got {kids}");
    }

    #[test]
    fn the_splash_fades_skips_and_hands_off_to_the_menu() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(SplashClock(1.0)); // mid-hold: logo fully in, nothing dismissing yet
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.insert_resource(ButtonInput::<MouseButton>::default());
        app.insert_resource(NextState::<GameState>::default());
        app.add_systems(Update, splash_update);
        app.update();
        assert!(
            matches!(*app.world().resource::<NextState<GameState>>(), NextState::Unchanged),
            "mid-hold the splash stays put"
        );
        // any key skips AHEAD to the dismiss point (fade-out start) — a skip is never a hard cut
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        app.update();
        assert!(
            (app.world().resource::<SplashClock>().0 - SPLASH_HOLD_UNTIL).abs() < 0.05,
            "a keypress jumps the clock to the fade-out"
        );
        // once the fade-out has run its course, the splash hands off to the menu
        app.world_mut().resource_mut::<SplashClock>().0 = SPLASH_HOLD_UNTIL + SPLASH_FADE_OUT;
        app.update();
        assert!(
            matches!(*app.world().resource::<NextState<GameState>>(), NextState::Pending(GameState::Menu)),
            "the splash ends on the menu"
        );
    }

    #[test]
    fn game_over_records_the_deepest_wave_reached() {
        // record_high_score is the game-over entry (chained before the screen spawns): it must raise
        // best_wave when the run went deeper and never lower it when it didn't.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Score(0));
        app.insert_resource(HighScores::default());
        app.insert_resource(Wave { level: 23, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Stats { best_wave: 27, ..default() });
        app.add_systems(Update, record_high_score);
        app.update();
        assert_eq!(app.world().resource::<Stats>().best_wave, 27, "a shallower death never lowers the record");
        app.world_mut().resource_mut::<Wave>().level = 28;
        app.update();
        assert_eq!(app.world().resource::<Stats>().best_wave, 28, "a deeper run raises it");
    }

    #[test]
    fn the_nearest_grind_ticker_picks_the_closest_counter() {
        // runs at 9/10 (90%) beats blue at 500/1000 (50%) — the ticker points at the closest unlock
        let s = Stats { runs: 9, blue: 500, ..default() };
        let (a, c, t) = nearest_grind(&s).expect("counters are in progress");
        assert!(matches!(a, Ach::Runs10), "Back for More at 9/10 is the nearest grind");
        assert_eq!((c, t), (9, 10));
        // a finished counter drops out — with the 10-run rung done, 990/1000 blue leads (Runs25 is 40%)
        let s = Stats { runs: 10, blue: 990, ..default() };
        let (a, ..) = nearest_grind(&s).expect("blue is still unfinished");
        assert!(matches!(a, Ach::TrueBlue), "completed grinds leave the ticker");
        // boss flags and capstones never appear, even on a maxed save — they're binary, not progress
        let done = Stats {
            blue: ACH_BLUE, green: ACH_GREEN, orange: ACH_ORANGE, pulser: ACH_PULSER, red: ACH_RED,
            cluster: ACH_CLUSTER, beacon: ACH_BEACON, hunter: ACH_HUNTER, lapse: ACH_LAPSE,
            tenders: ACH_TENDERS, facet: ACH_FACET, husk: ACH_HUSK, mines: ACH_MINES,
            golds: ACH_GOLDS, waves: ACH_WAVES, warps: ACH_WARPS, runs: 50, ..default()
        };
        assert!(nearest_grind(&done).is_none(), "with every counter capped the ticker goes quiet");
    }

    #[test]
    fn the_sweep_ray_vaporizes_a_rock_in_its_path() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Run { lives: 3, respawn: 1.0, ..default() });
        app.insert_resource(Dev::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(NextState::<GameState>::default());
        // a Phantom mid-sweep, its beam starting along +X (bearing 0)
        let mut ph = Phantom::new(PHANTOM_PHASE_HP, true, 0.0);
        ph.ray = RayPhase::Fire;
        ph.ray_from = 0.0;
        ph.ray_span = std::f32::consts::FRAC_PI_2;
        ph.ray_t = 0.0;
        app.world_mut().spawn((ph, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a rock sitting on that start bearing (+X), well inside the arena and past the core's inner radius
        app.world_mut()
            .spawn((Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(200.0, 0.0, 0.0)));
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, phantom_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "the sweeping beam vaporizes a rock in its path");
    }

    #[test]
    fn clearing_a_phase_triggers_a_reset_not_a_win() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Run { lives: 3, respawn: 1.0, ..default() });
        app.insert_resource(Dev::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(NextState::<GameState>::default());
        // phase 1's pool just hit zero → it should begin the invulnerable RESET beat, not advance or win
        let ph = Phantom::new(0, true, 0.0); // phase 1 (default), pool depleted
        let boss = app.world_mut().spawn((ph, Transform::from_xyz(0.0, 0.0, 0.0))).id();
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, phantom_update);
        app.update();
        let p = app.world().entity(boss).get::<Phantom>().unwrap();
        assert!(p.transition > 0.0, "clearing a non-final phase begins a reset beat");
        assert_eq!(p.phase, 1, "the phase only advances once the reset completes");
        assert!(
            !matches!(app.world().resource::<NextState<GameState>>(), NextState::Pending(GameState::Victory)),
            "clearing a non-final phase must NOT win the run"
        );
    }

    // An entered, past-intro Phantom at the given phase health; the tweak sets the phase / cadences under test.
    fn phantom_app(hp: i32, tweak: impl FnOnce(&mut Phantom)) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Run { lives: 3, respawn: 1.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(NextState::<GameState>::default());
        let mut ph = Phantom::new(hp, true, 0.0);
        tweak(&mut ph);
        app.world_mut().spawn((ph, Transform::from_xyz(0.0, 0.0, 0.0)));
        app
    }

    #[test]
    fn phase_2_seeks_and_possesses_a_field_rock() {
        // phase 2 hunts an EXISTING field rock, glides to it, and dives IN (that rock becomes the vessel) —
        // it does NOT conjure one from nothing
        let mut app = phantom_app(PHANTOM_PHASE_HP, |ph| {
            ph.phase = 2;
            ph.dive = 0.0; // the beat has elapsed → it goes hunting this tick
        });
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        // a field rock sitting on the ghost so the glide reaches it at once
        let rock = app
            .world_mut()
            .spawn((Asteroid { size: 2, verts: vec![Vec2::X * 20.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        let boss = app.world_mut().query_filtered::<Entity, With<Phantom>>().iter(app.world()).next().unwrap();
        app.add_systems(Update, phantom_update);
        app.update(); // fixes on the field rock (seeking) rather than conjuring one
        assert_eq!(app.world().entity(boss).get::<Phantom>().unwrap().seeking, Some(rock), "it seeks an existing field rock");
        assert!(app.world().entity(boss).get::<Phantom>().unwrap().possessed.is_none(), "…and hasn't dived in yet");
        app.update(); // reaches the rock (it's right on it) → dives in
        assert!(app.world().entity(boss).get::<Phantom>().unwrap().possessed.is_some(), "on reaching it, it possesses the rock");
        assert_eq!(app.world_mut().query::<&Possessed>().iter(app.world()).count(), 1, "the rock is reborn as one haunted vessel");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "the possessed field rock is consumed");
    }

    #[test]
    fn clearing_phase_2_dispels_the_vessel() {
        // phase 2's pool empties → the possessed vessel dispels with the phase (like the decoys used to)
        let mut app = phantom_app(0, |ph| ph.phase = 2);
        app.world_mut().spawn((Possessed { hp: PHANTOM_POSSESS_HP, pulse: 0.0, verts: vec![Vec2::X * 20.0] }, Transform::from_xyz(200.0, 0.0, 0.0)));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, phantom_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Possessed>().iter(app.world()).count(), 0, "the vessel dispels with the phase that made it");
    }

    #[test]
    fn breaking_the_vessel_rips_the_haunt_out() {
        // while its vessel holds the Haunt stays hidden; destroy the vessel and it's torn out into the open
        let mut app = phantom_app(PHANTOM_PHASE_HP, |ph| ph.phase = 2);
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        let vessel = app.world_mut().spawn((Possessed { hp: PHANTOM_POSSESS_HP, pulse: 0.0, verts: vec![Vec2::X * 20.0] }, Transform::from_xyz(100.0, 0.0, 0.0))).id();
        let boss = app.world_mut().query_filtered::<Entity, With<Phantom>>().iter(app.world()).next().unwrap();
        app.world_mut().entity_mut(boss).get_mut::<Phantom>().unwrap().possessed = Some(vessel);
        app.add_systems(Update, phantom_update);
        app.update(); // rides the vessel — hidden, not yet exposed
        {
            let ph = app.world().entity(boss).get::<Phantom>().unwrap();
            assert!(ph.possessed.is_some() && ph.vuln <= 0.0, "while the vessel holds, the Haunt stays hidden inside it");
        }
        app.world_mut().entity_mut(vessel).despawn(); // the vessel is broken
        app.update();
        let ph = app.world().entity(boss).get::<Phantom>().unwrap();
        assert!(ph.possessed.is_none() && ph.vuln > 0.0, "breaking the vessel rips the Haunt out — surfaced for the punish window");
    }

    #[test]
    fn gunfire_breaks_a_possessed_vessel() {
        // a shot on the vessel chips its hp — the entry point for forcing the Haunt out
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        let vessel = app.world_mut().spawn((Possessed { hp: 1, pulse: 0.0, verts: vec![Vec2::X * 20.0] }, Transform::from_xyz(0.0, 0.0, 0.0))).id();
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert!(app.world().entity(vessel).get::<Possessed>().unwrap().hp <= 0, "a shot chips the vessel's hp toward breaking");
    }

    #[test]
    fn a_phase_3_charge_sears_a_lethal_trail() {
        let mut app = phantom_app(PHANTOM_PHASE_HP, |ph| {
            ph.phase = 3;
            ph.charging = PHANTOM_CHARGE_SECS; // mid-dash
            ph.charge_dir = Vec2::X;
            ph.ray_cool = 999.0; // isolate the charge
        });
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, phantom_update);
        app.update();
        let n = app.world_mut().query::<&SpectralTrail>().iter(app.world()).count();
        assert!(n >= 1, "the dash sears spectral afterimages into the arena (got {n})");
    }

    #[test]
    fn firing_the_ray_forces_it_to_surface() {
        // the Haunt must MATERIALIZE to attack: the moment its sweep completes, the vuln window opens
        let mut app = phantom_app(PHANTOM_PHASE_HP, |ph| {
            ph.ray = RayPhase::Fire;
            ph.ray_t = PHANTOM_RAY_FIRE; // the sweep completes on the next tick
        });
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, phantom_update);
        app.update();
        let ph = app.world_mut().query::<&Phantom>().iter(app.world()).next().unwrap();
        assert!(ph.ray == RayPhase::Idle, "the sweep has ended");
        assert!(ph.vuln > 0.0, "…and firing forced it to SURFACE (vuln window open)");
    }

    #[test]
    fn a_surfaced_phantom_kills_on_contact_but_a_ghost_does_not() {
        fn lives_after(vuln: f32) -> i32 {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
            app.insert_resource(Dev::default());
            app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
            app.insert_resource(NextState::<GameState>::default());
            let mut ph = Phantom::new(PHANTOM_PHASE_HP, true, 0.0);
            ph.vuln = vuln;
            ph.ray_cool = 999.0; // no ray this frame — isolate the contact rule
            app.world_mut().spawn((ph, Transform::from_xyz(0.0, 0.0, 0.0)));
            // the ship overlapping the boss
            app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(10.0, 0.0, 0.0)));
            app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.add_systems(Update, phantom_update);
            app.update();
            app.world().resource::<Run>().lives
        }
        assert_eq!(lives_after(0.0), 3, "a GHOST Phantom passes harmlessly through the ship");
        assert_eq!(lives_after(1.0), 2, "a SURFACED (solid) Phantom kills on contact");
    }

    #[test]
    fn field_wipe_catches_the_new_bosses() {
        // GameplayEntity must include the new bosses, or quitting/restarting mid-fight leaves one alive —
        // a stale Phantom would resume next run and fire a false victory.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.world_mut().spawn((Detonator { hp: 1, entered: true, charge: 0.0, cool: 0.0, prime: 0.0, target: None, pulse: 0.0, dying: 0.0 }, Transform::default()));
        app.world_mut().spawn((Pulsar { hp: 1, entered: true, charge: 0.0, phase: 0.0, shock_cool: 0.0, pulse: 0.0, dying: 0.0 }, Transform::default()));
        app.world_mut().spawn((Phantom::new(1, true, 0.0), Transform::default()));
        app.world_mut().spawn((Possessed { hp: PHANTOM_POSSESS_HP, pulse: 0.0, verts: vec![Vec2::X * 20.0] }, Transform::default()));
        app.world_mut().spawn((SpectralTrail { ttl: 1.0 }, Transform::default()));
        let n = app.world_mut().query_filtered::<Entity, GameplayEntity>().iter(app.world()).count();
        assert_eq!(n, 5, "the field-wipe filter catches the Detonator, Pulsar, Phantom, its possessed vessel, and its wake");
    }

    #[test]
    fn black_hole_consumes_nearby_asteroid() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(10.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(n, 0, "an asteroid within the consume radius should be eaten");
        assert_eq!(app.world().resource::<Score>().0, WARP_ROCK_SCORE, "a warp-consumed rock scores the low flat value");
    }

    #[test]
    fn warp_fired_inward_from_an_edge_keeps_flying() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // launched from just inside the RIGHT edge, heading INWARD (left) — must NOT pop at the launch edge
        app.world_mut().spawn((
            WarpMissile { life: WARP_MISSILE_LIFE },
            Velocity(Vec2::new(-WARP_MISSILE_SPEED, 0.0)),
            Transform::from_xyz(600.0, 0.0, 0.0), // within WARP_CONSUME_R of the right edge
        ));
        app.add_systems(Update, warp_missile_update);
        app.update();
        assert_eq!(app.world_mut().query::<&WarpMissile>().iter(app.world()).count(), 1, "a warp fired inward from an edge keeps flying");
        assert_eq!(app.world_mut().query::<&BlackHole>().iter(app.world()).count(), 0, "no hole opens at the launch edge");
    }

    #[test]
    fn warp_detonates_at_the_wall_it_heads_toward() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // near the right edge, heading TOWARD it → opens the hole there
        app.world_mut().spawn((
            WarpMissile { life: WARP_MISSILE_LIFE },
            Velocity(Vec2::new(WARP_MISSILE_SPEED, 0.0)),
            Transform::from_xyz(600.0, 0.0, 0.0),
        ));
        app.add_systems(Update, warp_missile_update);
        app.update();
        assert_eq!(app.world_mut().query::<&WarpMissile>().iter(app.world()).count(), 0, "the missile is consumed");
        assert_eq!(app.world_mut().query::<&BlackHole>().iter(app.world()).count(), 1, "it opens a hole at the wall it's heading for");
    }

    #[test]
    fn warp_spends_three_charges_then_starts_cooldown() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Warp { charges: WARP_MAX_CHARGES, cooldown: 0.0 });
        app.insert_resource(HudFlash::default());
        app.insert_resource(ActionState { warp: true, ..default() }); // held true across frames → fires until charges run out
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, warp_fire);
        for _ in 0..5 {
            app.update();
        }
        let warp = app.world().resource::<Warp>();
        assert_eq!(warp.charges, 0, "all three charges should be spent");
        assert!(warp.cooldown > 0.0, "spending the last charge starts the long cooldown");
        let missiles = app.world_mut().query::<&WarpMissile>().iter(app.world()).count();
        assert_eq!(missiles, 3, "exactly three warp missiles should have fired");
    }

    #[test]
    fn top_up_streams_rocks_when_below_target() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(SpawnClock(0.0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, top_up_asteroids);
        app.update();
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert!(n >= 1, "an empty field below target should stream in a replacement rock");
    }

    #[test]
    fn no_top_up_during_calm() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 5.0 }); // in the post-boss calm
        app.insert_resource(SpawnClock(0.0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, top_up_asteroids);
        app.update();
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(n, 0, "no rocks should spawn during the post-boss calm");
    }

    #[test]
    fn finale_field_trickles_in_not_all_at_once() {
        // wave 30 streams RANDOM-type rocks in one at a time, up to its field cap — never a wall
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(SpawnClock(0.0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.add_systems(Update, top_up_asteroids);

        // the first beat drops ONE rock, not a wall
        app.update();
        let n1 = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(n1, 1, "the finale trickles in — one rock on the first beat (got {n1})");

        // stepping the spawn clock feeds the field up to (and never past) the cap
        for _ in 0..(FINALE_FIELD_CAP + 4) {
            app.world_mut().resource_mut::<SpawnClock>().0 = 0.0; // the trickle interval elapsed
            app.update();
        }
        let n = app.world_mut().query::<&Asteroid>().iter(app.world()).count() as i32;
        assert_eq!(n, FINALE_FIELD_CAP, "the finale field fills to its cap and holds (got {n})");
    }

    #[test]
    fn a_lingering_gold_does_not_stall_the_finale() {
        // a gold 1UP rock is an Asteroid too; it must NOT count against the finale's field cap
        // (regression: no asteroids spawned while a gold lingered)
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 30, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(SpawnClock(0.0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // ONLY a gold rock on the field (Asteroid + Gold)
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 60.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Gold,
            Velocity(Vec2::ZERO),
            Transform::from_xyz(120.0, 0.0, 0.0),
        ));
        app.add_systems(Update, top_up_asteroids);
        app.update();
        let non_gold = app.world_mut().query_filtered::<(), (With<Asteroid>, Without<Gold>)>().iter(app.world()).count();
        assert_eq!(non_gold, 1, "the finale keeps trickling despite the lingering gold");
    }

    #[test]
    fn act_iii_late_waves_roll_the_new_types() {
        let mut rng = rand::thread_rng();
        let n = 600;
        let beacons23 = (0..n).filter(|_| matches!(roll_rock_kind(23, false, &mut rng), RockKind::Beacon)).count();
        assert!(beacons23 > 0, "wave 23 debuts the beacon");
        let clusters26 = (0..n).filter(|_| matches!(roll_rock_kind(26, false, &mut rng), RockKind::Cluster)).count();
        assert!(clusters26 > 0, "wave 26 debuts the cluster");
        let early = (0..n).filter(|_| matches!(roll_rock_kind(21, false, &mut rng), RockKind::Cluster | RockKind::Beacon)).count();
        assert_eq!(early, 0, "waves before 23 roll neither new type");
    }

    #[test]
    fn finale_roll_covers_every_rock_type() {
        // the wave-30 field is fully random across ALL types — over a big sample every kind shows up
        let mut rng = rand::thread_rng();
        let (mut blue, mut green, mut orange, mut pulser, mut red, mut cluster, mut beacon, mut hunter) = (0, 0, 0, 0, 0, 0, 0, 0);
        for _ in 0..4000 {
            match roll_finale_kind(&mut rng) {
                RockKind::Blue => blue += 1,
                RockKind::Green => green += 1,
                RockKind::Orange => orange += 1,
                RockKind::Pulser => pulser += 1,
                RockKind::Red => red += 1,
                RockKind::Cluster => cluster += 1,
                RockKind::Beacon => beacon += 1,
                RockKind::Hunter => hunter += 1,
                RockKind::Lapse | RockKind::Facet | RockKind::Husk => panic!("Lapse/Facet/Husk are NG+-only and must never appear in the base finale mix"),
            }
        }
        for (name, n) in [("blue", blue), ("green", green), ("orange", orange), ("pulser", pulser), ("red", red), ("cluster", cluster), ("beacon", beacon), ("hunter", hunter)] {
            assert!(n > 0, "the finale mix must include {name} rocks");
        }
        assert!(beacon < blue, "the beacon stays the RARE spice of the finale mix");
    }

    #[test]
    fn wave_rock_mix_matches_the_authored_content() {
        // returns (blue, green, orange, pulser) counts — hunters are checked by their own test
        fn sample(level: i32, n: usize, rng: &mut rand::rngs::ThreadRng) -> (i32, i32, i32, i32) {
            let (mut blue, mut green, mut orange, mut pulser) = (0, 0, 0, 0);
            for _ in 0..n {
                match roll_rock_kind(level, false, rng) {
                    RockKind::Blue => blue += 1,
                    RockKind::Green => green += 1,
                    RockKind::Orange => orange += 1,
                    RockKind::Pulser => pulser += 1,
                    // Hunter (Act I) + the Act III-only kinds; covered by their own tests. The Lapse is
                    // NG+-only, so a base-game roll must never produce one.
                    RockKind::Hunter | RockKind::Red | RockKind::Cluster | RockKind::Beacon => {}
                    RockKind::Lapse | RockKind::Facet | RockKind::Husk => panic!("Lapse/Facet/Husk are NG+-only and must never appear in a base-game wave"),
                }
            }
            (blue, green, orange, pulser)
        }
        let mut rng = rand::thread_rng();
        // wave 14 is the ALL-orange danger wave
        let (b, g, o, p) = sample(14, 200, &mut rng);
        assert_eq!((b, g, o, p), (0, 0, 200, 0), "wave 14 is nothing but orange");
        // wave 15 (boss) is green-only
        let (b, g, o, p) = sample(15, 200, &mut rng);
        assert_eq!((b, g, o, p), (0, 200, 0, 0), "wave 15 is green-only");
        // wave 11 "green + orange": every rock is one or the other, never plain blue
        let (b, _g, o, p) = sample(11, 400, &mut rng);
        assert_eq!((b, p), (0, 0), "wave 11 has no blue and no pulsers");
        assert!(o > 0, "wave 11 has orange");
        // wave 12: no plain BLUE past wave 10 — non-orange rocks are green
        let (b, g, o, _p) = sample(12, 400, &mut rng);
        assert_eq!(b, 0, "wave 12 has no plain blue rocks (none past wave 10)");
        assert!(g > 0 && o > 0, "wave 12 mixes green and orange");
        // wave 16 is pulser-ONLY (a pure timing wave to debut the mechanic)
        let (b, g, o, p) = sample(16, 200, &mut rng);
        assert_eq!((b, g, o, p), (0, 0, 0, 200), "wave 16 is nothing but pulsers");
        // ACT OWNERSHIP: no Act I/II rock survives into Act III — wave 23 rolls NOTHING but the act's
        // own types (red carrier + beacon; cluster from 26)
        let (b, g, o, p) = sample(23, 600, &mut rng);
        assert_eq!((b, g, o, p), (0, 0, 0, 0), "Act III sheds every earlier type — red/beacon/cluster only");
        // green retires WITH its act: gone from wave 21 on
        let (_b, g22, _o, _p) = sample(22, 400, &mut rng);
        assert_eq!(g22, 0, "no green in Act III (each act owns its roster)");
        // wave 20 (the Detonator): pure GREEN fodder — the boss can't prime an explosive (it's already
        // a bomb), so any orange was a dead slot that stalled the fight with it hunting, armored, for
        // a green. The boss brings the explosions; the field brings the prey.
        let (b, g, o, p) = sample(20, 400, &mut rng);
        assert_eq!((b, o, p), (0, 0, 0), "wave 20 spawns no blue, no orange, no pulsers");
        assert_eq!(g, 400, "wave 20 is nothing but primeable green fodder");
        // the devourer wave (10) stays plain blue food so it can be starved
        let (_b, g, o, p) = sample(10, 200, &mut rng);
        assert_eq!((g, o, p), (0, 0, 0), "the devourer wave is plain blue food");
        // NO blue past wave 10 — Act III's fallback is RED (its carrier), never blue
        let (b, ..) = sample(21, 200, &mut rng);
        assert_eq!(b, 0, "wave 21 has no blue (Act III's fallback is red)");
    }

    #[test]
    fn mine_target_gates_and_caps() {
        assert_eq!(mine_target(1, 10), 0, "no mines before wave 2");
        assert_eq!(mine_target(2, 10), 1, "wave 2: (2-2+1)*1 = 1");
        assert_eq!(mine_target(5, 4), 1, "capped at 30% of 4 asteroids");
        assert_eq!(mine_target(9, 100), 6, "deep waves hit the hard cap of 6, never a wall");
        assert_eq!(mine_target(31, 100), 0, "loop past 30: wave 31 = content 1 → no mines");
        assert_eq!(mine_target(32, 100), 1, "loop past 30: wave 32 = content 2 → back to 1");
    }

    #[test]
    fn no_gallery_page_can_overflow_into_the_text() {
        // The Beacon's aura used to reach 133px and print straight through the name line. Every entry
        // is now scaled by GALLERY_ART_R / its own extent, so this asserts the invariant holds for
        // ALL of them — including anything added later, which is the point of the test.
        let band_half = GALLERY_ART_BAND_HALF; // the 262px art band the UI reserves
        for (art, name, ..) in gallery_entries(&Stats::default()) {
            let extent = gallery_art_extent(art);
            assert!(extent > 0.0, "'{name}' needs an honest extent");
            let zoom = (GALLERY_ART_R / extent).min(1.35);
            let drawn = extent * zoom;
            assert!(
                drawn <= GALLERY_ART_R + 0.01,
                "'{name}' draws to {drawn:.1}px, over the {GALLERY_ART_R}px budget"
            );
            assert!(drawn < band_half, "'{name}' at {drawn:.1}px would reach outside the art band and hit the text");
        }
    }

    #[test]
    fn the_gallery_opens_pages_on_the_seen_flag() {
        // One boolean per subject, set when the thing is INTRODUCED to your field. No inference from
        // kill counts or wave depth — flags only.
        let fresh = gallery_entries(&Stats::default());
        assert_eq!(fresh.iter().filter(|e| e.4).count(), 0, "a fresh pilot has met nothing");
        assert!(fresh.len() >= 18, "every rock, hazard and boss gets a page, got {}", fresh.len());
        for (_, name, role, desc, _) in &fresh {
            assert!(!name.is_empty() && !role.is_empty(), "every page is titled");
            assert!(desc.len() > 60, "'{name}' needs a real description, got {} chars", desc.len());
        }
        // flipping one flag opens exactly one page — and killing things does NOT open pages
        let mut s = Stats { hunter: 999, blue: 999, best_wave: 30, phantom: true, ..default() };
        assert_eq!(gallery_entries(&s).iter().filter(|e| e.4).count(), 0, "counters and depth don't open pages — only sightings do");
        assert!(mark_seen(&mut s, GalleryArt::Rock(RockKind::Hunter)), "first sighting reports fresh");
        assert!(!mark_seen(&mut s, GalleryArt::Rock(RockKind::Hunter)), "and a repeat sighting does not");
        let seen = gallery_entries(&s);
        assert_eq!(seen.iter().filter(|e| e.4).count(), 1, "exactly one page opened");
        assert!(seen.iter().any(|e| e.1 == "HUNTER" && e.4), "…the hunter's");
        // every subject has a UNIQUE, stable bit — a collision would open the wrong page
        let mut bits = std::collections::HashSet::new();
        for (art, name, ..) in gallery_entries(&Stats::default()) {
            assert!(bits.insert(gallery_bit(art)), "'{name}' shares its bit with another subject");
        }
        assert_eq!(bits.len(), gallery_entries(&Stats::default()).len(), "one bit per page");
        // and marking everything opens the whole book
        let mut all = Stats::default();
        for (art, ..) in gallery_entries(&Stats::default()) {
            mark_seen(&mut all, art);
        }
        assert!(gallery_entries(&all).iter().all(|e| e.4), "every subject seen = every page open");
        // THE BOOK GROWS (user rule): it contains ONLY what you've met — no blank pages to leaf
        // through, so it can never spoil what's still coming. Empty is a valid state (the screen
        // must not index into it — that used to underflow).
        assert!(gallery_book(&Stats::default()).is_empty(), "a fresh pilot's book has no pages at all");
        assert_eq!(gallery_book(&s).len(), 1, "one sighting = exactly one page");
        assert!(gallery_book(&s).iter().all(|e| e.4), "every page in the book is one you've met");
        assert_eq!(gallery_book(&all).len(), gallery_entries(&all).len(), "…and a full save shows the whole roster");
    }
    #[test]
    fn aegis_shards_grind_rocks_then_run_out() {
        // The anti-invincibility contract: each shard eats exactly ONE rock (vaporized, no chunks),
        // and once the ring is spent the next rock kills you.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Dev::default());
        app.insert_resource(Run {
            lives: 3,
            aegis: Aegis { unlocked: true, shards: AEGIS_SHARDS, regen: AEGIS_REGEN, spin: 0.0 },
            ..default()
        });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // one rock sitting on the ship per shard, plus one extra to get through
        for i in 0..=AEGIS_SHARDS {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(i as f32 * 0.1, 0.0, 0.0),
            ));
        }
        app.add_systems(Update, ship_death);
        app.update();
        let run = *app.world().resource::<Run>();
        assert_eq!(run.aegis.shards, 0, "every shard was spent grinding a rock");
        assert_eq!(
            app.world_mut().query::<&Asteroid>().iter(app.world()).count(),
            1,
            "AEGIS_SHARDS rocks were vaporized (no chunks left behind); the extra one survived"
        );
        assert_eq!(run.lives, 2, "with the ring empty that last rock still killed the ship");

        // …and the regrowth is ONE at a time on the cooldown, never the whole ring at once. Its own
        // world: in the app above, ship_death would immediately grind the regrown shard away.
        let mut regen = App::new();
        regen.add_plugins(MinimalPlugins);
        regen.insert_resource(Run { lives: 3, aegis: Aegis { unlocked: true, shards: 0, regen: 0.0, spin: 0.0 }, ..default() });
        regen.add_systems(Update, aegis_tick);
        regen.update();
        let a = regen.world().resource::<Run>().aegis;
        assert_eq!(a.shards, 1, "one shard back, not the full ring");
        assert_eq!(a.regen, AEGIS_REGEN, "and the timer re-armed for the next one");
    }

    #[test]
    fn a_tender_fuses_two_fragments_back_into_one_rock() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // an ARRIVED tender with two small fragments in reach
        let tender = app
            .world_mut()
            .spawn((
                Tender { life: TENDER_LIFETIME, entered: true, fleeing: false, cool: 0.0, job: None, progress: 0.0 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        // placed already close enough to weld, so the fusion resolves on the proximity branch —
        // MinimalPlugins barely advances the clock, so a dt-driven timer would never elapse here
        for x in [-15.0f32, 15.0] {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(x, 0.0, 0.0),
            ));
        }
        app.add_systems(Update, tender_update);
        app.update(); // picks up the job
        assert!(app.world().entity(tender).get::<Tender>().unwrap().job.is_some(), "it locks onto a pair of fragments");
        app.update(); // hauls + welds
        let sizes: Vec<u8> = {
            let mut q = app.world_mut().query::<&Asteroid>();
            q.iter(app.world()).map(|a| a.size).collect()
        };
        assert_eq!(sizes, vec![2], "two smalls became one MID rock — a split, run backwards");
        assert!(app.world().entity(tender).get::<Tender>().unwrap().job.is_none(), "and the job cleared");

        // INTERRUPTION: kill one fragment mid-haul and the weld fails
        let mut app2 = App::new();
        app2.add_plugins(MinimalPlugins);
        app2.add_event::<SoundFx>();
        app2.insert_resource(Score(0));
        app2.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        let t2 = app2
            .world_mut()
            .spawn((
                Tender { life: TENDER_LIFETIME, entered: true, fleeing: false, cool: 0.0, job: None, progress: 0.0 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        let mut frags = vec![];
        for x in [-120.0f32, 120.0] {
            frags.push(
                app2.world_mut()
                    .spawn((
                        Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                        Velocity(Vec2::ZERO),
                        Transform::from_xyz(x, 0.0, 0.0),
                    ))
                    .id(),
            );
        }
        app2.add_systems(Update, tender_update);
        app2.update();
        assert!(app2.world().entity(t2).get::<Tender>().unwrap().job.is_some());
        app2.world_mut().entity_mut(frags[0]).despawn(); // shoot one out from under it
        app2.update();
        let td = app2.world().entity(t2).get::<Tender>().unwrap();
        assert!(td.job.is_none() && td.progress == 0.0, "destroying either fragment aborts the fusion");
        assert_eq!(app2.world_mut().query::<&Asteroid>().iter(app2.world()).count(), 1, "and nothing was welded");
    }

    #[test]
    fn the_glutton_plus_inhale_is_escapable_and_coned() {
        // The fairness constants (pull < THRUST, a coned wedge, a readable gape) are compile-time
        // asserts beside their definitions. What's left to check here is the PHASE MACHINE:

        // and the phase machine: gaping first (harmless), only then inhaling
        let winding = Devourer { hp: 10, grow: 0.0, fed: 0, dying: 0.0, pulse: 0.0, inhale: NGP_GLUT_INHALE_DUR + 0.5, inhale_cd: 0.0, spit: 0.0 };
        assert!(winding.inhale_winding() && !winding.inhaling(), "the gape pulls nothing");
        let pulling = Devourer { inhale: NGP_GLUT_INHALE_DUR - 0.1, ..winding };
        assert!(pulling.inhaling() && !pulling.inhale_winding(), "then it bites");
        let idle = Devourer { inhale: 0.0, ..winding };
        assert!(!idle.inhaling() && !idle.inhale_winding(), "and idles between");
    }

    #[test]
    fn a_husk_cracks_open_into_hunters_and_never_cascades() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Husk,
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new(), mass: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        // the shell releases a BROOD, not chunks
        let (hunters, husks, total) = {
            let mut q = app.world_mut().query::<(&Asteroid, Option<&Hunter>, Option<&Husk>)>();
            let all: Vec<_> = q.iter(app.world()).map(|(a, h, k)| (a.size, h.is_some(), k.is_some())).collect();
            (all.iter().filter(|x| x.1).count(), all.iter().filter(|x| x.2).count(), all.len())
        };
        assert_eq!(hunters, HUSK_BROOD, "the shell lets out its brood of hunters");
        assert_eq!(total, HUSK_BROOD, "…and nothing else — no ordinary chunks alongside them");
        assert_eq!(husks, 0, "NO CASCADE: a husk never contains another husk");
        assert_eq!(app.world().resource::<Stats>().husk, 1, "cracking it credits the husk tally");
        // the brood starts docile, so you get a beat to react
        let charges: Vec<f32> = {
            let mut q = app.world_mut().query::<&Hunter>();
            q.iter(app.world()).map(|h| h.charge).collect()
        };
        assert!(charges.iter().all(|&c| c == 0.0), "the brood emerges at zero charge");
    }

    #[test]
    fn a_facet_reflects_closed_faces_and_takes_the_open_one() {
        // The whole mechanic: a round on a CLOSED face comes back live; a round through the OPEN
        // face kills the rock normally. `open` is relative to the rock's rotation, so the gap moves.
        fn shoot(open: f32, from: Vec2) -> (usize, usize, bool) {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Stats::default());
            app.insert_resource(Score(0));
            app.world_mut().spawn((
                Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Facet { open },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
            app.world_mut().spawn((
                Bullet { life: 1.0, trail: Vec::new(), mass: false },
                Velocity(-from.normalize() * BULLET_SPEED), // flying inward at the rock
                Transform::from_xyz(from.x, from.y, 0.0),
            ));
            app.add_systems(Update, collisions);
            app.update();
            let rocks = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
            let bullets = app.world_mut().query::<&Bullet>().iter(app.world()).count();
            let ricochets = app.world_mut().query::<&Ricochet>().iter(app.world()).count() > 0;
            (rocks, bullets, ricochets)
        }
        // hit the face OPPOSITE the gap → reflected, rock survives, and the round is now live
        let (rocks, bullets, ric) = shoot(0.0, Vec2::new(-30.0, 0.0));
        assert_eq!(rocks, 1, "a closed face takes no damage");
        assert_eq!(bullets, 1, "the round isn't consumed — it's handed back");
        assert!(ric, "…and it comes back LIVE (a ricochet that can kill you)");
        // hit THROUGH the gap → normal kill, no ricochet
        let (rocks, _b, ric) = shoot(0.0, Vec2::new(30.0, 0.0));
        assert!(rocks != 1, "the open face takes damage like any rock");
        assert!(!ric, "and nothing bounces back off it");
        // the ricochet is slower than the shot that made it, so it can be dodged on the way back
    }

    #[test]
    fn a_lapse_rock_is_harmless_and_unhittable_until_it_is_really_there() {
        // THE FAIRNESS CONTRACT: absent or still materializing = it cannot kill you and you cannot
        // hit it. That's what makes a rock reappearing on your hull always avoidable rather than a
        // cheap death, so it's worth pinning hard.
        assert!(Lapse { phase: LapsePhase::Solid, t: 1.0 }.tangible(), "solid is dangerous");
        assert!(Lapse { phase: LapsePhase::FadingOut, t: 1.0 }.tangible(), "still on its way out, still solid");
        assert!(!Lapse { phase: LapsePhase::Gone, t: 1.0 }.tangible(), "absent is harmless");
        assert!(!Lapse { phase: LapsePhase::FadingIn, t: 1.0 }.tangible(), "MATERIALIZING is harmless — the telegraph is free");

        // an ABSENT rock sitting on the ship does not kill it…
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Dev::default());
        app.insert_resource(Run { lives: 3, ..default() });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        let rock = app
            .world_mut()
            .spawn((
                Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Lapse { phase: LapsePhase::Gone, t: 5.0 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        app.add_systems(Update, ship_death);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, 3, "an absent lapse rock cannot kill");
        // …and neither does one that's only halfway back
        app.world_mut().entity_mut(rock).insert(Lapse { phase: LapsePhase::FadingIn, t: LAPSE_FADE_IN * 0.5 });
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, 3, "a materializing lapse rock cannot kill either");
        // once it's solid, it's a rock like any other
        app.world_mut().entity_mut(rock).insert(Lapse { phase: LapsePhase::Solid, t: 5.0 });
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, 2, "solid again = lethal again");
    }

    #[test]
    fn the_lapse_strike_stays_inside_the_photosensitivity_budget() {
        // The materialize is a NEON TUBE STRIKE, not a linear fade — but it's still large-area
        // flashing, so it must stay at or under 3 flashes/sec. Count the brightness peaks across the
        // whole fade-in and check the implied rate.
        let mut peaks = 0;
        let (mut prev, mut rising) = (0.0f32, false);
        for i in 0..=400 {
            let f = i as f32 / 400.0;
            let l = Lapse { phase: LapsePhase::FadingIn, t: LAPSE_FADE_IN * (1.0 - f) };
            let c = lapse_glow(&l).to_srgba();
            let b = c.red + c.green + c.blue;
            if b > prev && !rising {
                rising = true;
            } else if b < prev && rising {
                peaks += 1;
                rising = false;
            }
            prev = b;
        }
        let hz = peaks as f32 / LAPSE_FADE_IN;
        assert!(hz <= 3.0, "the strike flickers at {hz:.1} Hz — over the 3 flashes/sec limit");
        assert!(peaks >= 2, "…but it should still read as a STRIKE, not a plain fade (got {peaks} peaks)");
        // and it ignites WARM, settling to its own cold colour
        let early = lapse_glow(&Lapse { phase: LapsePhase::FadingIn, t: LAPSE_FADE_IN * 0.8 }).to_srgba();
        let late = lapse_glow(&Lapse { phase: LapsePhase::Solid, t: 1.0 }).to_srgba();
        assert!(early.red / early.blue.max(0.01) > late.red / late.blue.max(0.01), "the strike is warmer than the settled tube");
    }

    #[test]
    fn the_lapse_phase_clock_cycles_in_order() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        let e = app.world_mut().spawn((Lapse { phase: LapsePhase::Solid, t: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0))).id();
        app.add_systems(Update, lapse_update);
        // each tick with an expired timer advances exactly one phase, in cycle order
        for expect in [LapsePhase::FadingOut, LapsePhase::Gone, LapsePhase::FadingIn, LapsePhase::Solid] {
            app.update();
            assert!(app.world().entity(e).get::<Lapse>().unwrap().phase == expect, "phases advance in order");
            app.world_mut().entity_mut(e).get_mut::<Lapse>().unwrap().t = 0.0; // expire it again
        }
        // it comes back ELSEWHERE: leaving the Gone phase relocates it, and never onto the ship
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().entity_mut(e).insert(Lapse { phase: LapsePhase::Gone, t: 0.0 });
        app.world_mut().entity_mut(e).insert(Transform::from_xyz(0.0, 0.0, 0.0));
        app.update();
        let moved = app.world().entity(e).get::<Transform>().unwrap().translation.truncate();
        assert!(app.world().entity(e).get::<Lapse>().unwrap().phase == LapsePhase::FadingIn, "Gone → FadingIn");
        assert!(moved.length() >= LAPSE_REAPPEAR_CLEAR - 1.0, "it materializes clear of the ship, got {moved:?}");
        // and every phase it lands in has a real, positive duration (no zero-length flicker)
        app.world_mut().entity_mut(e).insert(Lapse { phase: LapsePhase::Solid, t: 0.0 });
        for _ in 0..4 {
            app.update();
            let l = *app.world().entity(e).get::<Lapse>().unwrap();
            assert!(l.t > 0.5, "every phase lasts long enough to read (got {})", l.t);
            app.world_mut().entity_mut(e).get_mut::<Lapse>().unwrap().t = 0.0;
        }
    }

    #[test]
    fn a_hunter_chases_the_ship_and_ramps_up() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        // ship at the origin, a hunter parked out to the right with no velocity
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        let rock = app
            .world_mut()
            .spawn((
                Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Hunter { charge: 0.0, look: Vec2::X },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(300.0, 0.0, 0.0),
            ))
            .id();
        app.add_systems(Update, hunter_update);
        app.update();
        app.update(); // first tick's dt is 0 under MinimalPlugins
        let (v, h) = {
            let e = app.world().entity(rock);
            (e.get::<Velocity>().unwrap().0, *e.get::<Hunter>().unwrap())
        };
        assert!(v.x < 0.0, "it accelerates TOWARD the ship (leftward from +x), got {v:?}");
        assert!(h.charge > 0.0, "its aggression ramps with time alive");
        assert!(h.look.x < 0.0, "the eye tracks the ship");
        // the speed cap keeps it outrunnable — never faster than the ship's top speed
        for _ in 0..400 {
            app.update();
        }
        let v = app.world().entity(rock).get::<Velocity>().unwrap().0;
        assert!(v.length() <= HUNTER_MAX_SPEED + 1.0, "capped at HUNTER_MAX_SPEED, got {}", v.length());
    }

    #[test]
    fn hunters_debut_on_wave_six_and_retire_with_their_act() {
        let mut rng = rand::thread_rng();
        let count = |level: i32, n: usize, rng: &mut rand::rngs::ThreadRng| {
            (0..n).filter(|_| matches!(roll_rock_kind(level, false, rng), RockKind::Hunter)).count()
        };
        assert_eq!(count(5, 300, &mut rng), 0, "no hunters before wave 6 (wave 5 is the Warden anyway)");
        let w6 = count(6, 400, &mut rng);
        assert!(w6 > 200, "wave 6 is the hunter's teaching wave — mostly hunters, got {w6}/400");
        assert!(count(8, 400, &mut rng) > 0, "they garnish waves 7-9");
        // ACT OWNERSHIP: the hunter dies with Act I — nothing past wave 10 (the finale rolls its own mix)
        for level in [11, 14, 19, 23, 26] {
            assert_eq!(count(level, 300, &mut rng), 0, "no hunters in wave {level} — Act I owns them");
        }
        // NG+ waves 1-5 recap the OLD roster ONLY — the new bestiary is held back until wave 6
        for _ in 0..400 {
            match roll_rock_kind(3, true, &mut rng) {
                RockKind::Hunter | RockKind::Lapse | RockKind::Facet | RockKind::Husk => panic!("the new roster must not appear in NG+ waves 1-5"),
                _ => {}
            }
        }
        // NEW GAME+ retires the OLD roster after wave 5 instead: lap two runs the new bestiary, and
        // EVERY roll past wave 5 must come from it — no old-roster rock may leak back in.
        let (mut hunters, mut lapses) = (0, 0);
        for _ in 0..600 {
            match roll_rock_kind(12, true, &mut rng) {
                RockKind::Hunter => hunters += 1,
                RockKind::Lapse => lapses += 1,
                RockKind::Facet | RockKind::Husk => {} // debut at waves 8/9; counted by their own tests
                other => panic!("an OLD-roster rock leaked into NG+ past wave 5: {:?}", other as u8),
            }
        }
        assert!(hunters > 0 && lapses > 0, "the NG+ bestiary mixes its types ({hunters} hunters / {lapses} lapses)");
    }

    #[test]
    fn the_split_economy_rolls_as_designed() {
        // large → 1 or 2 mediums (never 0); medium → 2 smalls or nothing (never 1); smalls die
        // clean; gold and red lineages always get the guaranteed pair (economy / identity exempt)
        let mut rng = rand::thread_rng();
        let n = 2000;
        let (mut l1, mut l2, mut m0, mut m2) = (0, 0, 0, 0);
        for _ in 0..n {
            match split_children(3, false, false, &mut rng) {
                1 => l1 += 1,
                2 => l2 += 1,
                k => panic!("a large shed {k} children"),
            }
            match split_children(2, false, false, &mut rng) {
                0 => m0 += 1,
                2 => m2 += 1,
                k => panic!("a medium shed {k} children"),
            }
        }
        assert!(l1 > 0 && l2 > 0, "large breaks vary between 1 and 2 mediums ({l1}/{l2})");
        assert!(m0 > 0 && m2 > 0, "medium breaks vary between dying clean and 2 smalls ({m0}/{m2})");
        assert_eq!(split_children(1, false, false, &mut rng), 0, "smalls always die clean");
        for _ in 0..50 {
            // GOLD: a large sheds two mids, and mids die CLEAN — the hunt never makes smalls, which
            // were the fragments that slipped off the edge and forfeited the life.
            assert_eq!(split_children(3, true, false, &mut rng), 2, "a large gold sheds two mids");
            assert_eq!(split_children(2, true, false, &mut rng), 0, "a gold MID dies clean — no small stragglers");
            assert_eq!(split_children(2, false, true, &mut rng), 2, "RED keeps the guaranteed pair (regrow identity)");
        }
    }

    #[test]
    fn slinger_wave_keeps_a_sparse_field() {
        assert_eq!(population_target(15, false), SLINGER_WAVE_ROCKS, "the Slinger wave stays sparse (it makes its own ammo)");
        assert_eq!(population_target(14, false), POP_CAP, "the all-orange wave 14 keeps the full field");
        assert_eq!(population_target(45, false), SLINGER_WAVE_ROCKS, "the looped Slinger wave (content 15 = wave 45) is sparse too");
    }

    #[test]
    fn new_game_plus_scales_at_the_source() {
        // the density dial: every wave gains the bonus, past the cap included — except the Slinger
        // arena, which stays sparse by fight DESIGN, not difficulty
        assert_eq!(population_target(3, true), population_target(3, false) + NGP_POP_BONUS, "NG+ densifies wave 3");
        assert_eq!(population_target(14, true), POP_CAP + NGP_POP_BONUS, "NG+ shifts the whole curve past the cap");
        assert_eq!(population_target(15, true), SLINGER_WAVE_ROCKS, "the Slinger arena stays sparse even in NG+");
        // boss cores: half again as tough, and untouched in a normal run
        assert_eq!(scaled_hp(DETONATOR_HP, false), DETONATOR_HP, "normal runs keep base boss HP");
        assert_eq!(scaled_hp(46, true), 69, "NG+ cores are 1.5x (rounded)");
        // NG+ Act I: waves 1-5 roll the FULL roster (the finale mix) — the lap assumes mastery
        let mut rng = rand::thread_rng();
        let n = 4000;
        let (mut red, mut cluster, mut beacon) = (0, 0, 0);
        for _ in 0..n {
            match roll_rock_kind(2, true, &mut rng) {
                RockKind::Red => red += 1,
                RockKind::Cluster => cluster += 1,
                RockKind::Beacon => beacon += 1,
                _ => {}
            }
        }
        assert!(red > 0 && cluster > 0 && beacon > 0, "NG+ wave 2 already shows the late-game rocks (red {red}, cluster {cluster}, beacon {beacon})");
        assert!(matches!(roll_rock_kind(2, false, &mut rng), RockKind::Blue), "a NORMAL wave 2 is still all-blue");
    }

    #[test]
    fn a_lit_pulser_shrugs_off_shots_but_a_dark_one_breaks() {
        fn rocks_after(offset: f32) -> usize {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Stats::default());
            app.insert_resource(Score(0));
            app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
            // a small dense pulser (hp 1 → one dark hit clears it) with a bullet on top
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: true, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
                Pulser { offset },
            ));
            app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
            app.add_systems(Update, collisions);
            app.update();
            app.world_mut().query::<&Asteroid>().iter(app.world()).count()
        }
        use std::f32::consts::FRAC_PI_2;
        assert_eq!(rocks_after(FRAC_PI_2), 1, "a LIT pulser (sin>threshold) is invulnerable — the shot fizzles, it survives");
        assert_eq!(rocks_after(-FRAC_PI_2), 0, "a DARK pulser takes the hit and breaks");
    }

    #[test]
    fn a_well_pulls_the_ship_but_stays_escapable() {
        // ship at origin, a well to the +x within reach → the pull drags the ship toward it
        let pull = well_pull(Vec2::ZERO, Vec2::new(100.0, 0.0), 1.0 / 60.0);
        assert!(pull.x > 0.0 && pull.y.abs() < 0.001, "the well drags the ship toward it (+x)");
        // and there's no pull beyond its reach (escapability itself is a compile-time invariant, below)
        assert_eq!(well_pull(Vec2::ZERO, Vec2::new(1000.0, 0.0), 1.0 / 60.0), Vec2::ZERO, "no pull past WELL_PULL_RADIUS");
    }

    #[test]
    fn armed_mine_kills_ship_on_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Mine { armed: true, fuse: 0.0 }, // armed, overlapping the ship
            Velocity(Vec2::ZERO),
            Transform::from_xyz(5.0, 0.0, 0.0),
        ));
        app.add_systems(Update, mine_update);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert_eq!(ships, 0, "an armed mine should kill the ship on contact");
        assert_eq!(mines, 0, "the mine detonates and despawns");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life is lost");
    }

    #[test]
    fn mine_drifting_into_a_rock_detonates_and_shatters_it() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a size-2 rock and a mine overlapping it, mid-field, with NO ship present
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Mine { armed: false, fuse: MINE_FUSE },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, mine_update);
        app.update();
        let rocks = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert!(rocks == 0 || rocks == 2, "the blasted medium sheds 2 smalls or dies clean (split economy), got {rocks}");
        assert_eq!(mines, 0, "the mine detonates on contact with the rock and despawns");
        assert_eq!(app.world().resource::<Run>().lives, 3, "no life is lost when a mine hits a rock");
    }

    #[test]
    fn a_mine_bounces_off_a_gold_rock_without_detonating() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a gold rock with a mine drifting straight into it, mid-field, NO ship present
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(30.0, 0.0, 0.0),
            Gold,
        ));
        app.world_mut().spawn((
            Mine { armed: false, fuse: MINE_FUSE },
            Velocity(Vec2::new(120.0, 0.0)), // heading toward the gold rock (+x)
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, mine_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 1, "the mine must NOT detonate on a gold rock");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "the gold rock is unharmed (mines can't break it)");
        let mv = app.world_mut().query_filtered::<&Velocity, With<Mine>>().iter(app.world()).next().unwrap().0;
        assert!(mv.x < 0.0, "the mine bounces off the gold rock (velocity reflected away), got {mv:?}");
    }

    #[test]
    fn a_mine_blast_spares_gold_rocks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 2, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a plain rock the mine detonates on, and a gold rock sitting inside the blast radius
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(20.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 40.0, 0.0),
            Gold,
        ));
        app.world_mut().spawn((
            Mine { armed: false, fuse: MINE_FUSE },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, mine_update);
        app.update();
        let gold = app.world_mut().query_filtered::<Entity, With<Gold>>().iter(app.world()).count();
        assert_eq!(gold, 1, "a gold rock in the blast radius is spared");
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 0, "the mine still detonates on the plain rock");
    }

    #[test]
    fn mines_drift_off_during_a_boss_wave() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Wave { level: 5, timer: WAVE_SECS, calm: 0.0 }); // boss wave
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a mine that has drifted just off the right edge
        app.world_mut().spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::new(50.0, 0.0)), Transform::from_xyz(700.0, 0.0, 0.0)));
        app.add_systems(Update, mine_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 0, "off-edge mines drift off (despawn) during a boss wave instead of recycling");
    }

    #[test]
    fn enemy_target_gates_and_caps() {
        assert_eq!(enemy_target(2, 100), 0, "no mobs before wave 3");
        assert_eq!(enemy_target(3, 100), 2, "wave 3 → 2");
        assert_eq!(enemy_target(4, 100), 4, "wave 4 → 4");
        assert_eq!(enemy_target(6, 100), 0, "no mobs on the green-intro wave 6");
        assert_eq!(enemy_target(7, 100), 0, "still no mobs on wave 7");
        assert_eq!(enemy_target(8, 100), 4, "mobs return on wave 8");
        assert_eq!(enemy_target(9, 100), 6, "wave 9 → 6");
        assert_eq!(enemy_target(9, 10), 3, "capped to a fraction of the rock count");
        assert_eq!(enemy_target(11, 100), 0, "waves 11-15 run no mobs — Act II belongs to the rocks");
        assert_eq!(enemy_target(13, 100), 0, "no mobs on wave 13 either");
        assert_eq!(enemy_target(16, 100), 0, "waves 16-20 run no old-lobber mobs either");
        assert_eq!(enemy_target(21, 100), 0, "Act III (wave 21) runs no old lobber mob");
        assert_eq!(enemy_target(33, 100), 2, "loop past 30: wave 33 = content 3 → 2");
    }

    #[test]
    fn content_wave_loops_and_picks_boss_type() {
        assert_eq!(content_wave(1), 1);
        assert_eq!(content_wave(10), 10);
        assert_eq!(content_wave(30), 30, "waves 1-30 are each their own content slot now");
        assert_eq!(content_wave(31), 1, "wave 31 loops back to content 1");
        assert_eq!(content_wave(35), 5, "wave 35 = content 5 in the loop");
        assert!(is_devourer_wave(10) && !is_devourer_wave(30), "the devourer is content-10 (wave 10), not wave 30");
        assert!(is_slinger_wave(15) && !is_slinger_wave(35), "the Slinger is content-15 (wave 15)");
        assert!(is_detonator_wave(20), "the Detonator is content-20 (wave 20)");
        assert!(is_boss_wave(25) && is_boss_wave(30), "25 & 30 are Act III boss waves (Pulsar / Phantom)");
        assert!(is_boss_wave(5) && is_boss_wave(15) && is_boss_wave(20) && !is_boss_wave(6));
    }

    #[test]
    fn devourer_wave_spawns_the_second_boss() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 10, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Devourer>().iter(app.world()).count(), 1, "wave 10 spawns the devourer");
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0, "and not the shaman");
    }

    #[test]
    fn slinger_wave_spawns_the_third_boss() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 15, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Slinger>().iter(app.world()).count(), 1, "wave 15 spawns the Slinger");
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0, "and not the shaman");
        assert_eq!(app.world_mut().query::<&Devourer>().iter(app.world()).count(), 0, "and not the devourer");
    }

    #[test]
    fn slinger_core_takes_gunfire() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        // an entered, no-longer-charging Slinger at the origin, plus a bullet on top of it
        let boss = app
            .world_mut()
            .spawn((Slinger { hp: SLINGER_HP, entered: true, charge: 0.0, cool: SLINGER_COOL, load: 0.0, ammo: None, pulse: 0.0, recoil: 0.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.world_mut()
            .spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world().entity(boss).get::<Slinger>().unwrap().hp, SLINGER_HP - 1, "a bullet chips the Slinger's exposed core");
    }

    #[test]
    fn detonator_wave_spawns_the_fourth_boss() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Wave { level: 20, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Detonator>().iter(app.world()).count(), 1, "wave 20 spawns the Detonator");
        assert_eq!(app.world_mut().query::<&Slinger>().iter(app.world()).count(), 0, "and not the Slinger");
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0, "and not the shaman placeholder");
    }

    #[test]
    fn detonator_is_armored_except_while_priming() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        // one Detonator mid-PRIMING (core exposed) and one ARMED (sealed), each with a bullet on it
        let priming = app.world_mut().spawn((
            Detonator { hp: DETONATOR_HP, entered: true, charge: 0.0, cool: DETONATOR_COOL, prime: 1.0, target: None, pulse: 0.0, dying: 0.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        )).id();
        let armored = app.world_mut().spawn((
            Detonator { hp: DETONATOR_HP, entered: true, charge: 0.0, cool: DETONATOR_COOL, prime: 0.0, target: None, pulse: 0.0, dying: 0.0 },
            Transform::from_xyz(500.0, 0.0, 0.0),
        )).id();
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(500.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world().entity(priming).get::<Detonator>().unwrap().hp, DETONATOR_HP - 1, "gunfire lands while it's priming");
        assert_eq!(app.world().entity(armored).get::<Detonator>().unwrap().hp, DETONATOR_HP, "but clanks off its armored shell otherwise");
    }

    #[test]
    fn warhead_round_detonates_on_impact_with_real_aoe() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        // the impact rock, a neighbor INSIDE the blast, a bystander far outside it, and a gold
        // rock inside the blast (gold is blast-immune — only aimed shots may break the 1UP)
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(WARHEAD_BLAST_R * 0.6, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(WARHEAD_BLAST_R + 220.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Gold,
            Velocity(Vec2::ZERO),
            Transform::from_xyz(-WARHEAD_BLAST_R * 0.5, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, WarheadShot, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 0, "the round DETONATES on impact — it does not keep going");
        assert_eq!(app.world_mut().query::<&Detonating>().iter(app.world()).count(), 0, "nothing is left ticking");
        let survivors: Vec<bool> = {
            let mut q = app.world_mut().query::<(&Asteroid, Option<&Gold>)>();
            q.iter(app.world()).map(|(_, g)| g.is_some()).collect()
        };
        assert_eq!(survivors.len(), 2, "impact rock + blast neighbor die; the far rock and the gold survive");
        assert!(survivors.contains(&true), "the gold 1UP shrugged off the blast");
        assert_eq!(app.world().resource::<Stats>().blue, 2, "both blast kills are credited to the player");
    }

    // THE GORGE ROUND must snowball WITHOUT becoming a field-clear button: it has to keep flying
    // through a rock (that's the whole point), grow as it does, and then die on a hard bite count.
    #[test]
    fn the_gorge_round_eats_through_rocks_then_breaks_up() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        // three rocks stacked on the round's position — it should chew all three in one pass
        for _ in 0..3 {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
        }
        let round = app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, GorgeShot { eaten: 0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0))).id();
        app.add_systems(Update, collisions);
        app.update();
        let g = app.world().entity(round).get::<GorgeShot>().expect("it keeps flying — a gorge round does not stop on the first rock");
        assert_eq!(g.eaten, 3, "it ate every rock it passed through");
        assert!(g.radius() > GORGE_R0, "and it grew doing it");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "all three are gone");
        assert_eq!(app.world().resource::<Stats>().blue, 3, "and all three are credited to the player");
        // …and it is BOUNDED: one bite short of full, the next rock breaks it up
        app.world_mut().entity_mut(round).insert(GorgeShot { eaten: GORGE_BITES - 1 });
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.update();
        assert_eq!(app.world_mut().query::<&GorgeShot>().iter(app.world()).count(), 0, "gorged to the cap, it comes apart — it can never sweep a whole field");
    }

    // The maw is DRAWN at radius(), so it has to BITE at radius() too — a mouth that visibly
    // swallowed a rock without eating it reads as a broken weapon.
    #[test]
    fn a_grown_gorge_round_bites_as_wide_as_it_looks() {
        let rock_r = asteroid_radius(1);
        // a rock sitting where an EMPTY round can't reach but a fed one can
        let gap = GORGE_R0 + rock_r + (GORGE_R_MAX - GORGE_R0) * 0.5;
        for (eaten, should_eat) in [(0u32, false), (GORGE_BITES - 1, true)] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            app.insert_resource(Stats::default());
            app.insert_resource(Score(0));
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * rock_r], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(gap, 0.0, 0.0),
            ));
            app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, GorgeShot { eaten }, Velocity(Vec2::X), Transform::from_xyz(0.0, 0.0, 0.0)));
            app.add_systems(Update, collisions);
            app.update();
            let left = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
            assert_eq!(left == 0, should_eat, "a round {eaten} bites deep should{} reach {gap:.0}px", if should_eat { "" } else { " NOT" });
        }
    }

    // Its growth curve is the readout the player reads, so it has to be monotonic AND capped.
    #[test]
    fn the_gorge_round_growth_is_capped() {
        let mut prev = 0.0;
        for eaten in 0..=GORGE_BITES {
            let r = GorgeShot { eaten }.radius();
            assert!(r >= prev, "every bite reads as bigger (or at the cap), never smaller");
            assert!(r <= GORGE_R_MAX, "and it never outgrows the cap");
            prev = r;
        }
        assert!(GorgeShot { eaten: 0 }.radius() > bullet_radius(false), "even empty it looks heavier than a standard round");
    }

    #[test]
    fn friendly_warhead_blast_spares_the_player() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        app.insert_resource(Dev::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(NextState::<GameState>::default());
        // a FRIENDLY (warhead) bomb blowing right on top of a non-invincible ship
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 20.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Detonating { fuse: 0.0, friendly: true },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, detonate);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 1, "the player survives their own friendly Warhead blast");
        assert_eq!(app.world().resource::<Run>().lives, 3, "and loses no life to it");
    }

    #[test]
    fn devourer_eats_a_rock_and_grows() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(NewGamePlus::default());
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Run { lives: 3, respawn: 1.0, ..default() }); // respawning → skip ship-contact
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 10, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        let dvr = app
            .world_mut()
            .spawn((Devourer { hp: DEVOURER_HP - 20, grow: 0.0, fed: 0, dying: 0.0, pulse: 0.0, inhale: 0.0, inhale_cd: NGP_GLUT_INHALE_EVERY, spit: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO), // the inhale needs to be able to haul rocks, so the query wants one
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, devourer_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "the overlapping rock is eaten");
        let dv = app.world().entity(dvr).get::<Devourer>().unwrap();
        assert!(dv.grow > 0.0, "eating grows it");
        assert!(dv.hp > DEVOURER_HP - 20 && dv.hp <= DEVOURER_HP, "eating heals damage back toward full, never past its start");
        assert_eq!(dv.fed, 1, "it ate exactly one rock");
    }

    #[test]
    fn a_gorged_devourer_bursts_wipes_the_field_and_shrinks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(NewGamePlus::default());
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 10, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        // fully gorged (grow == 1.0), still alive → it should OVERLOAD this frame
        app.world_mut().spawn((Devourer { hp: 50, grow: 1.0, fed: 20, dying: 0.0, pulse: 0.0, inhale: 0.0, inhale_cd: NGP_GLUT_INHALE_EVERY, spit: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a ship within the burst reach but OUTSIDE contact range → the burst is what kills it
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(300.0, 0.0, 0.0)));
        // field rocks (out of eating reach) → wiped by the burst
        for i in 0..5 {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO), // required by the devourer's (now velocity-mutating) rock query
                Transform::from_xyz(-300.0 + i as f32 * 20.0, 250.0, 0.0),
            ));
        }
        app.add_systems(Update, devourer_update);
        app.update();
        let grow = app.world_mut().query::<&Devourer>().iter(app.world()).next().unwrap().grow;
        assert!(grow < 0.01, "the devourer shrinks back to starting size after bursting");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "the burst wipes the field");
        assert_eq!(app.world().resource::<Run>().lives, 2, "the burst kills the player caught in range");
    }

    #[test]
    fn bullet_chips_the_devourer() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((Devourer { hp: DEVOURER_HP, grow: 0.5, fed: 0, dying: 0.0, pulse: 0.0, inhale: 0.0, inhale_cd: NGP_GLUT_INHALE_EVERY, spit: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let hp = app.world_mut().query::<&Devourer>().iter(app.world()).next().unwrap().hp;
        let grow = app.world_mut().query::<&Devourer>().iter(app.world()).next().unwrap().grow;
        assert_eq!(hp, DEVOURER_HP - 1, "a bullet chips the devourer's core");
        assert!(grow < 0.5, "and shrinks it a little, so gunfire keeps its size manageable");
    }

    #[test]
    fn mass_pickup_unlocks_the_mass_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Mass }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        let m = app.world().resource::<MassShot>();
        assert!(m.unlocked && m.active, "grabbing the mass orb unlocks + activates the mass shot");
        assert!(!app.world().resource::<Chain>().unlocked, "and it does NOT unlock the chain");
    }

    #[test]
    fn mass_shot_one_shots_a_dense_rock_and_splits_it() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a dense size-3 rock (hp 3): a standard shot only chips it, but a mass shot (MASS_POWER=3) breaks it
        // in ONE hit — and it SPLITS into two chunks. Mass is STRONGER than standard now, not an instant wipe.
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: true, hp: 3 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: true }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let sizes: Vec<u8> = app.world_mut().query::<&Asteroid>().iter(app.world()).map(|a| a.size).collect();
        assert!(
            (1..=2).contains(&sizes.len()) && sizes.iter().all(|&s| s == 2),
            "a mass shot one-shots the dense rock (3 dmg vs 3 hp); the large sheds 1-2 mediums (split economy), got {sizes:?}"
        );
    }

    #[test]
    fn mass_shot_cannot_destroy_a_lit_pulser() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a pulser LIT at t≈0 (offset π/2 → sin ≈ 1, above the lit threshold) is invulnerable to everything,
        // mass included — the "invuln white" exception
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 45.0], rot: 0.0, spin: 0.0, dense: true, hp: 2 },
            Pulser { offset: std::f32::consts::FRAC_PI_2 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: true }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let rocks = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert_eq!(rocks, 1, "a mass shot fizzles on a LIT pulser — it stays whole");
    }

    #[test]
    fn mass_shot_hits_a_boss_a_bit_harder_than_standard() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        // vs a boss the mass shot does NOT vaporize — it chips MASS_BOSS_POWER (a bit more than standard's 1)
        let boss = app
            .world_mut()
            .spawn((Slinger { hp: SLINGER_HP, entered: true, charge: 0.0, cool: SLINGER_COOL, load: 0.0, ammo: None, pulse: 0.0, recoil: 0.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: true }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(
            app.world().entity(boss).get::<Slinger>().unwrap().hp,
            SLINGER_HP - MASS_BOSS_POWER,
            "a mass shot chips a boss by MASS_BOSS_POWER — a bit more than standard's 1, but its slow rate keeps standard the better boss DPS"
        );
    }

    #[test]
    fn menu_start_resets_and_spawns_a_ship() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<MenuClick>();
        app.insert_resource(NextState::<GameState>::default());
        // stale end-of-run state that Start must wipe
        app.insert_resource(Run { lives: 0, respawn: 5.0, died: true, ..default() });
        app.insert_resource(Score(999));
        app.insert_resource(Wave { level: 7, timer: 1.0, calm: 3.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Warp { charges: 0, cooldown: 9.0 });
        app.insert_resource(BossState { fought: 5 });
        app.insert_resource(Chain { unlocked: true, charges: 3, recharge: 0.0, cooldown: 0.0 });
        app.insert_resource(MassShot { unlocked: true, active: true });
        app.insert_resource(Warhead { unlocked: true, active: true });
        app.insert_resource(Gorge { unlocked: true, active: true });
        app.insert_resource(RunFlags { powerup_used: true });
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        app.insert_resource(Stats { runs: 9, ..default() });
        app.insert_resource(PacifistWatch { primed_at_level: 7, breaks: 42, fires: 2, streak: 1 }); // stale — Start must re-prime it
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Enter);
        app.insert_resource(input);
        app.add_systems(Update, menu_start);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, START_LIVES, "Start resets lives");
        assert!(!app.world().resource::<Run>().died, "Start opens a fresh deathless slate");
        assert_eq!(app.world().resource::<Score>().0, 0, "Start resets score");
        assert_eq!(app.world().resource::<Wave>().level, 1, "Start resets to wave 1");
        assert!(!app.world().resource::<Chain>().unlocked, "Start relocks the chain shot");
        assert!(!app.world().resource::<MassShot>().unlocked, "Start relocks the mass shot");
        assert!(!app.world().resource::<Warhead>().unlocked, "Start relocks the Warhead rounds");
        assert!(!app.world().resource::<Gorge>().unlocked, "Start relocks the Gorge round");
        assert!(!app.world().resource::<GoldRush>().active, "Start clears any stale gold hunt");
        assert_eq!(app.world().resource::<Stats>().runs, 10, "every Start counts a lifetime run (the restart ladder)");
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 1, "a fresh ship spawns");
        assert!(!app.world().resource::<NewGamePlus>().0, "keyboard launch is always a NORMAL run");
        // the NEW GAME+ button sets the mode — and it's still "essentially a new game": the same
        // reset_run wipes score/powerups/lives, only the difficulty dials differ
        app.world_mut().send_event(MenuClick(MenuAction::PlayPlus));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear(); // no Enter this time
        app.update();
        assert!(app.world().resource::<NewGamePlus>().0, "the NG+ button arms the mode");
        assert_eq!(app.world().resource::<Score>().0, 0, "and still starts a clean run");
        // quitting back and launching normally clears it
        app.world_mut().send_event(MenuClick(MenuAction::Play));
        app.update();
        assert!(!app.world().resource::<NewGamePlus>().0, "a normal PLAY clears the mode");
    }

    #[test]
    fn boss_and_timer_wave_clears_both_feed_the_lifetime_tally() {
        // Wave Goodbye counts EVERY advance — timer waves and boss kills alike. defeat_boss is the
        // boss-side entry; passing None (headless caller) must also stay safe.
        let mut score = Score(0);
        let mut wave = Wave { level: 5, timer: 0.0, calm: 0.0 };
        let mut banner = WaveBanner::default();
        let mut s = Stats::default();
        defeat_boss(&mut score, &mut wave, &mut banner, Some(&mut s));
        assert_eq!(s.waves, 1, "a boss kill credits a cleared wave");
        assert_eq!(wave.level, 6, "and still advances the wave");
        defeat_boss(&mut score, &mut wave, &mut banner, None);
        assert_eq!(wave.level, 7, "a stats-less caller still advances safely");
    }

    #[test]
    fn clear_field_wipes_the_run_but_keeps_the_backdrop() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Star { phase: 0.0, bright: 1.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, clear_field);
        app.update();
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 0, "the ship is wiped");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "asteroids are wiped");
        assert_eq!(app.world_mut().query::<&Star>().iter(app.world()).count(), 1, "the starfield backdrop survives");
    }

    #[test]
    fn achievement_unlocks_when_its_condition_is_met() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Stats { enemies: 1, ..default() }); // one enemy killed → First Blood
        app.insert_resource(Achievements::default());
        app.add_systems(Update, achievements);
        app.update();
        let first_blood = ACHIEVEMENTS.iter().position(|a| *a == Ach::FirstBlood).unwrap();
        let true_blue = ACHIEVEMENTS.iter().position(|a| *a == Ach::TrueBlue).unwrap();
        assert!(app.world().resource::<Achievements>().unlocked[first_blood], "First Blood unlocks after an enemy kill");
        assert!(!app.world().resource::<Achievements>().unlocked[true_blue], "True Blue stays locked with 0 blue destroyed");
    }

    #[test]
    fn bullet_kills_enemy_in_one_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Enemy { fire: 1.0, life: 5.0, strafe: 1.0, entered: true, fleeing: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new(), mass: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        let enemies = app.world_mut().query::<&Enemy>().iter(app.world()).count();
        assert_eq!(enemies, 0, "one bullet should destroy the enemy");
        assert_eq!(app.world().resource::<Score>().0, ENEMY_SCORE, "killing an enemy scores");
    }

    #[test]
    fn enemy_bullet_kills_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Dev::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((
            Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            EnemyBullet { life: 2.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, enemy_bullets);
        app.update();
        let ships = app.world_mut().query::<&Ship>().iter(app.world()).count();
        assert_eq!(ships, 0, "an enemy shot on the ship kills it");
        assert_eq!(app.world().resource::<Run>().lives, 2, "a life is lost");
    }

    #[test]
    fn warp_consumes_enemy() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((
            Enemy { fire: 1.0, life: 5.0, strafe: 1.0, entered: true, fleeing: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        let enemies = app.world_mut().query::<&Enemy>().iter(app.world()).count();
        assert_eq!(enemies, 0, "an enemy at the core is consumed by the warp");
        assert_eq!(app.world().resource::<Score>().0, ENEMY_SCORE, "consuming an enemy scores");
    }

    #[test]
    fn lingering_enemy_flees_and_despawns() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 }); // not a boss run-up (flee is via lifetime)
        // entered, out of life, already past the far edge → the flee branch despawns it
        app.world_mut().spawn((
            Enemy { fire: 5.0, life: 0.0, strafe: 1.0, entered: true, fleeing: true },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(900.0, 0.0, 0.0),
        ));
        app.add_systems(Update, enemy_update);
        app.update();
        let enemies = app.world_mut().query::<&Enemy>().iter(app.world()).count();
        assert_eq!(enemies, 0, "an enemy that has fled off-screen despawns");
    }

    #[test]
    fn boss_wave_detection() {
        assert!(!is_boss_wave(4));
        assert!(is_boss_wave(5));
        assert!(is_boss_wave(10));
        assert!(!is_boss_wave(6));
    }

    #[test]
    fn boss_spawns_and_enemies_flee_on_boss_wave() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Wave { level: 5, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(BossState::default());
        let mine = app
            .world_mut()
            .spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        let enemy = app
            .world_mut()
            .spawn((Enemy { fire: 1.0, life: 5.0, strafe: 1.0, entered: true, fleeing: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, boss_director);
        app.update();
        assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 1, "a boss spawns");
        assert!(app.world().entity(mine).get::<Mine>().is_some(), "mines are NOT wiped — they linger + behave normally");
        assert!(app.world().entity(enemy).get::<Enemy>().unwrap().fleeing, "enemy ships just leave (flee)");
        assert_eq!(app.world().resource::<BossState>().fought, 5);
    }

    #[test]
    fn boss_hp_zero_begins_slow_death() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(NewGamePlus::default());
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0, ..default() });
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 5, timer: 0.0, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: 0, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 1.0, capture: 1.0, dying: 0.0, whirl: Whirl::Idle, whirl_t: NGP_WARDEN_WHIRL_EVERY }, Transform::from_xyz(0.0, 200.0, 0.0)));
        app.add_systems(Update, boss_update);
        app.update();
        // hp<=0 BEGINS the slow death — the boss lingers (dying), wave not yet advanced
        // (the despawn + calm + level-up fire once the death timer elapses, ~2.2s later).
        let mut q = app.world_mut().query::<&Boss>();
        let dying: Vec<f32> = q.iter(app.world()).map(|b| b.dying).collect();
        assert_eq!(dying.len(), 1, "the boss lingers through its death animation");
        assert!(dying[0] > 0.0, "hp<=0 starts the death timer instead of an instant despawn");
        assert_eq!(app.world().resource::<Wave>().level, 5, "the wave advances only when the death finishes");
    }

    #[test]
    fn boss_captures_a_free_rock() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 0.0, dying: 0.0, whirl: Whirl::Idle, whirl_t: NGP_WARDEN_WHIRL_EVERY }, Transform::from_xyz(0.0, 0.0, 0.0)));
        let rock = app
            .world_mut()
            .spawn((Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(100.0, 100.0, 0.0)))
            .id();
        app.add_systems(Update, boss_shield);
        app.update();
        assert!(app.world().entity(rock).get::<Shielded>().is_some(), "the boss grabs a nearby top-half rock onto its shield");
    }

    #[test]
    fn bullet_damages_the_boss_core() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 5.0, dying: 0.0, whirl: Whirl::Idle, whirl_t: NGP_WARDEN_WHIRL_EVERY }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let hp = app.world_mut().query::<&Boss>().iter(app.world()).next().unwrap().hp;
        assert_eq!(hp, BOSS_HP - 1, "a bullet through a gap chips the core");
    }

    #[test]
    fn boss_throws_a_smallest_shield_rock_at_the_ship() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, -200.0, 0.0)));
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 0.0, capture: 5.0, dying: 0.0, whirl: Whirl::Idle, whirl_t: NGP_WARDEN_WHIRL_EVERY }, Transform::from_xyz(0.0, 200.0, 0.0)));
        let rock = app
            .world_mut()
            .spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 20.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 250.0, 0.0),
                Shielded { slot: 0, grab: 1.0 },
            ))
            .id();
        app.add_systems(Update, boss_shield);
        app.update();
        assert!(app.world().entity(rock).get::<Shielded>().is_none(), "the size-1 rock is released");
        assert!(app.world().entity(rock).get::<Thrown>().is_some(), "and flagged as just-thrown");
        assert!(app.world().entity(rock).get::<Detonating>().is_none(), "a NORMAL Warden's throws are plain rocks");
        let v = app.world().entity(rock).get::<Velocity>().unwrap().0;
        assert!(v.length() > 1.0 && v.y < 0.0, "flung toward the ship (which is below it)");
    }

    #[test]
    fn bosses_roam_the_whole_arena_not_a_band() {
        // The Warden used to sweep side-to-side across the TOP with a token 15% dip, so the bottom
        // half of the screen was permanently safe. Bosses must be able to go anywhere.
        let h = Vec2::new(640.0, 400.0);
        let margin = BOSS_R + BOSS_ORBIT_R + 6.0;
        let (mut lo, mut hi, mut left, mut right) = (false, false, false, false);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
        for i in 0..4000 {
            let t = i as f32 * 0.05;
            let p = boss_roam_target(t, h, margin);
            lo |= p.y < -(h.y - margin) * 0.5;
            hi |= p.y > (h.y - margin) * 0.5;
            left |= p.x < -(h.x - margin) * 0.5;
            right |= p.x > (h.x - margin) * 0.5;
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
            // and it must never wander somewhere its hazard geometry would hang off-screen
            assert!(p.x.abs() <= h.x - margin && p.y.abs() <= h.y - margin, "roam target left the safe box");
        }
        assert!(lo && hi && left && right, "the roam must reach all four quadrants (lo={lo} hi={hi} l={left} r={right})");
        // specifically: it genuinely comes DOWN, rather than dipping from a top band
        assert!(min_y < -(h.y - margin) * 0.7, "it has to come right down the screen, got {min_y:.0}");
        assert!(max_y > (h.y - margin) * 0.7, "…and go right back up, got {max_y:.0}");
    }

    #[test]
    fn the_warden_plus_whirl_is_always_telegraphed_first() {
        // THE FAIRNESS CONTRACT for the charged spin: the sweep can NEVER arrive un-announced.
        // Order is fixed (Idle → Wind → Spin → Recover) and the wind-up is long enough to read and
        // fly out of, so reaching Spin without a full Wind before it is impossible by construction.
        // during the wind the ring STALLS and reverses — a tell nothing else in the fight shows
        let early = whirl_spin_mult(Whirl::Wind, NGP_WARDEN_WIND * 0.9);
        let late = whirl_spin_mult(Whirl::Wind, NGP_WARDEN_WIND * 0.05);
        assert!(early < 1.0 && late < 0.0, "the ring stalls then creeps backwards ({early:.2} → {late:.2})");
        // the sweep is genuinely fast, and only the sweep extends the arms
        let peak = whirl_spin_mult(Whirl::Spin, NGP_WARDEN_SPIN * 0.4);
        assert!(peak > 3.0, "the sweep has to actually rip around, got {peak:.1}x");
        for w in [Whirl::Idle, Whirl::Wind, Whirl::Recover] {
            assert_eq!(whirl_reach(w, 0.5), 1.0, "arms only extend during the sweep itself");
        }
        assert!(whirl_reach(Whirl::Spin, NGP_WARDEN_SPIN * 0.5) > 1.2, "…and they do extend during it");
        // recovery is a real punish window: the ring is slow and it can't throw or grab
        assert!(whirl_spin_mult(Whirl::Recover, 1.0) < 0.5, "it hangs there spent afterwards");
        // and the base-game Warden never whirls at all (Idle forever without NG+)
        assert_eq!(whirl_spin_mult(Whirl::Idle, 9.0), 1.0);
        assert_eq!(whirl_reach(Whirl::Idle, 9.0), 1.0);
    }

    #[test]
    fn the_warden_plus_hurls_a_primed_two_rock_volley() {
        // NG+ boss 1: the old throw, meaner — TWO rocks per cadence, and both are LIVE BOMBS
        let mut app = App::new();
        app.insert_resource(NewGamePlus(true));
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, -200.0, 0.0)));
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 0.0, capture: 5.0, dying: 0.0, whirl: Whirl::Idle, whirl_t: NGP_WARDEN_WHIRL_EVERY }, Transform::from_xyz(0.0, 200.0, 0.0)));
        for slot in 0..2 {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 20.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(slot as f32 * 30.0, 250.0, 0.0),
                Shielded { slot, grab: 1.0 },
            ));
        }
        app.add_systems(Update, boss_shield);
        app.update();
        let mut q = app.world_mut().query::<(&Asteroid, Option<&Thrown>, Option<&Detonating>)>();
        let thrown: Vec<bool> = q.iter(app.world()).filter(|(_, t, _)| t.is_some()).map(|(.., d)| d.is_some()).collect();
        assert_eq!(thrown.len(), NGP_WARDEN_VOLLEY, "the Warden+ hurls a full volley in one throw");
        assert!(thrown.iter().all(|primed| *primed), "and every hurled rock is PRIMED (a live bomb)");
    }

    #[test]
    fn warp_consumes_a_mine() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, black_hole_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Mine>().iter(app.world()).count(), 0, "a mine at the core is consumed by the warp");
    }

    #[test]
    fn warp_pulls_a_distant_mine() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a mine 250px out — inside WARP_PULL_RADIUS (360), so it should be tugged inward
        let mine = app
            .world_mut()
            .spawn((Mine { armed: false, fuse: MINE_FUSE }, Velocity(Vec2::ZERO), Transform::from_xyz(250.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, black_hole_update);
        for _ in 0..5 {
            app.update(); // first frame's dt is 0; a few frames give the pull real time
        }
        let v = app.world().entity(mine).get::<Velocity>().unwrap().0;
        assert!(v.x < 0.0, "the mine is pulled back toward the hole (leftward), got {v:?}");
    }

    #[test]
    fn warp_spares_boss_held_rocks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a boss-HELD rock sitting right on the hole — it must survive (can't warp a shield away)
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Shielded { slot: 0, grab: 1.0 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a FREE rock just as close — it must be devoured
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(20.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Shielded>>().iter(app.world()).count(), 1, "the boss-held rock is exempt");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "only the held rock is left — the free one was devoured");
    }

    #[test]
    fn boss_grabs_the_biggest_rock_first() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 0.0, dying: 0.0, whirl: Whirl::Idle, whirl_t: NGP_WARDEN_WHIRL_EVERY }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a small rock CLOSE and a large rock FAR — it should still take the large one
        let small = app
            .world_mut()
            .spawn((Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(60.0, 80.0, 0.0)))
            .id();
        let large = app
            .world_mut()
            .spawn((Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(300.0, 80.0, 0.0)))
            .id();
        app.add_systems(Update, boss_shield);
        app.update();
        assert!(app.world().entity(large).get::<Shielded>().is_some(), "the big rock is grabbed");
        assert!(app.world().entity(small).get::<Shielded>().is_none(), "the near small rock is passed over");
    }

    #[test]
    fn free_rock_bounces_off_a_shield_rock() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        // a held (shield) rock at the origin
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: 1.0 },
        ));
        // a free rock overlapping it, drifting further in (+x… toward the shield centre is -x)
        let free = app
            .world_mut()
            .spawn((Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::new(-50.0, 0.0)), Transform::from_xyz(40.0, 0.0, 0.0)))
            .id();
        app.add_systems(Update, shield_deflect);
        app.update();
        let ft = app.world().entity(free).get::<Transform>().unwrap().translation.truncate();
        let fv = app.world().entity(free).get::<Velocity>().unwrap().0;
        assert!(ft.length() >= asteroid_radius(3) + asteroid_radius(1) - 0.5, "the free rock is pushed clear of the shield rock");
        assert!(fv.x > 0.0, "its inward velocity is reflected back outward");
    }

    #[test]
    fn shooting_a_shield_rock_shrinks_it_in_place() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        let rock = app
            .world_mut()
            .spawn((
                Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
                Velocity(Vec2::ZERO),
                Transform::from_xyz(0.0, 0.0, 0.0),
                Shielded { slot: 0, grab: 1.0 },
            ))
            .id();
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world().entity(rock).get::<Asteroid>().unwrap().size, 2, "a shot shield rock drops one size…");
        assert!(app.world().entity(rock).get::<Shielded>().is_some(), "…but stays on the arm");
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 1, "no free chunks are spawned");
    }

    #[test]
    fn shooting_the_smallest_shield_rock_frees_the_arm() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.world_mut().spawn((
            Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Shielded { slot: 0, grab: 1.0 },
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0, "the smallest shield rock shatters when shot");
    }

    #[test]
    fn pickup_grants_the_chain_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 6, timer: WAVE_SECS, calm: 5.0 }); // calm window open
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Chain }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert!(app.world().resource::<Chain>().unlocked, "flying into the orb unlocks the chain shot");
        assert_eq!(app.world().resource::<Chain>().charges, CHAIN_MAX_CHARGES, "and fills its charges");
        assert_eq!(app.world_mut().query::<&Pickup>().iter(app.world()).count(), 0, "the orb is consumed");
    }

    #[test]
    fn pickup_spawns_the_ally_drone() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Drone }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Drone>().iter(app.world()).count(), 1, "collecting the drone orb spawns one ally drone");
        assert!(app.world().resource::<RunFlags>().powerup_used, "and counts as a powerup used (blocks Purist)");
    }

    #[test]
    fn the_drone_fires_at_a_nearby_asteroid() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Drone { fire: -0.1, angle: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0))); // primed to fire
        app.world_mut()
            .spawn((Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(150.0, 0.0, 0.0)));
        app.add_systems(Update, drone_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 1, "the drone auto-fires at a nearby asteroid");
    }

    #[test]
    fn the_drone_fires_at_a_nearby_boss() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Drone { fire: -0.1, angle: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0))); // primed to fire
        // a boss in range with NO asteroids around — the drone should still fire (at the boss)
        app.world_mut().spawn((Slinger { hp: SLINGER_HP, entered: true, charge: 0.0, cool: SLINGER_COOL, load: 0.0, ammo: None, pulse: 0.0, recoil: 0.0, dying: 0.0 }, Transform::from_xyz(150.0, 0.0, 0.0)));
        app.add_systems(Update, drone_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 1, "the drone auto-fires at a nearby boss, not just asteroids");
    }

    #[test]
    fn ungrabbed_pickup_expires_after_its_life() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        // life already elapsed → the orb leaves for good (a single, missable offer)
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: 0.0, kind: PickupKind::Chain }, Velocity(Vec2::ZERO), Transform::from_xyz(200.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Pickup>().iter(app.world()).count(), 0, "an ungrabbed orb leaves once its life elapses");
        assert!(!app.world().resource::<Chain>().unlocked, "…and the chain shot stays locked");
    }

    #[test]
    fn shooting_the_pickup_grants_the_chain_shot() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        // no ship — a bullet overlapping the orb should grab it on its own
        app.world_mut().spawn((Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Chain }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, pickup_update);
        app.update();
        assert!(app.world().resource::<Chain>().unlocked, "shooting the orb unlocks the chain shot");
        assert_eq!(app.world_mut().query::<&Pickup>().iter(app.world()).count(), 0, "the orb is consumed");
        assert_eq!(app.world_mut().query::<&Bullet>().iter(app.world()).count(), 0, "the shot that grabbed it is spent");
    }

    #[test]
    fn music_cues_follow_the_boss_cycle() {
        let mut app = App::new();
        app.insert_resource(NewGamePlus::default());
        app.add_plugins(MinimalPlugins);
        app.insert_resource(ActionState::default());
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(MusicDirector {
            mains: vec![Handle::default(); 6],
            boss: Handle::default(),
            buildup: Handle::default(),
            gameover: Handle::default(),
            cue: None,
            muted: false,
        });
        app.insert_resource(State::new(GameState::Playing)); // the wave-cue logic only runs in Playing
        app.add_systems(Update, music_director);

        // normal play, act I → the CLEAN main track (tier 0)
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main(0)), "act I plays the clean main track");
        assert_eq!(app.world_mut().query::<&Music>().iter(app.world()).count(), 1, "one track is live");

        // last 10 s before the boss (wave 4, timer low) → the buildup riser
        {
            let mut w = app.world_mut().resource_mut::<Wave>();
            w.level = 4;
            w.timer = 5.0;
        }
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Buildup), "the run-up to a boss plays the buildup");

        // the boss wave → the boss track
        app.world_mut().resource_mut::<Wave>().level = 5;
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Boss), "the boss wave plays the boss track");

        // post-boss calm → silence (no music), even though we've advanced past the boss wave
        {
            let mut w = app.world_mut().resource_mut::<Wave>();
            w.level = 6;
            w.calm = BOSS_CALM;
        }
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Silence), "the post-boss calm is silent");
        assert_eq!(app.world_mut().query::<&Music>().iter(app.world()).count(), 0, "nothing is playing during the calm");

        // calm over (wave 6, one boss down) → the main track returns CORRUPTED one tier
        app.world_mut().resource_mut::<Wave>().calm = 0.0;
        app.update();
        assert_eq!(
            app.world().resource::<MusicDirector>().cue,
            Some(MusicCue::Main(1)),
            "after boss 1 the main track comes back a tier wronger"
        );

        // the Game Over screen → its own somber track, not silence and not the main loop
        app.insert_resource(State::new(GameState::GameOver));
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::GameOver), "game over plays the dirge");

        // and the boot splash stays silent — the Baz sting owns that moment
        app.insert_resource(State::new(GameState::Splash));
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Silence), "the splash holds silence");

        // NEW GAME+: the Belt is already wrong on arrival — wave 1 plays tier 1, never tier 0
        app.insert_resource(State::new(GameState::Playing));
        app.insert_resource(NewGamePlus(true));
        {
            let mut w = app.world_mut().resource_mut::<Wave>();
            w.level = 1;
            w.timer = WAVE_SECS;
            w.calm = 0.0;
        }
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main(1)), "NG+ starts a tier deep in the corruption");
    }

    #[test]
    fn a_single_main_variant_never_restarts_the_track() {
        // The produced main ships as ONE track (no corruption variants yet). The tier index MUST
        // clamp to what exists — otherwise the cue changes at every act boundary and the music
        // restarts mid-run. Regression guard for exactly that.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(ActionState::default());
        app.insert_resource(NewGamePlus(true)); // even the NG+ tier-1 floor must clamp down to 0
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(MusicDirector {
            mains: vec![Handle::default()], // ONE variant, as shipped
            boss: Handle::default(),
            buildup: Handle::default(),
            gameover: Handle::default(),
            cue: None,
            muted: false,
        });
        app.insert_resource(State::new(GameState::Playing));
        app.add_systems(Update, music_director);
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main(0)), "a single variant always plays tier 0");
        let live = app.world_mut().query::<&Music>().iter(app.world()).count();
        // walk deep into the run: every act boundary must leave the SAME cue and the SAME player
        for level in [6, 11, 16, 21, 26, 29] {
            app.world_mut().resource_mut::<Wave>().level = level;
            app.update();
            assert_eq!(
                app.world().resource::<MusicDirector>().cue,
                Some(MusicCue::Main(0)),
                "wave {level} must not switch cues (that would restart the music)"
            );
        }
        assert_eq!(app.world_mut().query::<&Music>().iter(app.world()).count(), live, "the same track keeps playing — never respawned");
    }

    #[test]
    fn right_click_fires_a_chain_beam() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Chain { unlocked: true, charges: 3, recharge: CHAIN_RECHARGE, cooldown: 0.0 });
        app.insert_resource(Run { lives: 3, ..default() });
        app.insert_resource(ActionState { chain: true, ..default() });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, chain_fire);
        app.update();
        assert_eq!(app.world_mut().query::<&ChainShot>().iter(app.world()).count(), 1, "right-click fires a chain beam");
        assert_eq!(app.world().resource::<Chain>().charges, 2, "and spends a charge");
    }

    #[test]
    fn chain_beam_shatters_rocks_in_its_path() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        // a beam at the origin spread along Y (segment (0,-58)..(0,58)), a rock sitting on it
        app.world_mut().spawn((ChainShot { life: 1.0, perp: Vec2::new(0.0, 1.0) }, Velocity(Vec2::new(500.0, 0.0)), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Asteroid { size: 2, verts: vec![Vec2::X * 46.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 30.0, 0.0)));
        app.add_systems(Update, chain_update);
        app.update();
        let kids = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert!(kids == 0 || kids == 2, "the beam mows the size-2 rock down; the medium sheds 2 smalls or dies clean, got {kids}");
    }

    #[test]
    fn shooting_a_mine_shatters_nearby_asteroids() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // bullet + mine overlapping at the origin (bullet detonates the mine)
        app.world_mut().spawn((
            Bullet { life: 1.0, trail: Vec::new(), mass: false },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((
            Mine { armed: false, fuse: MINE_FUSE },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        // a size-2 rock out of the bullet's reach but inside the mine's blast,
        // so it can only be broken by the detonation (not the bullet itself).
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(60.0, 0.0, 0.0),
        ));
        app.add_systems(Update, collisions);
        app.update();
        let rocks = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        assert!(rocks == 0 || rocks == 2, "the blast breaks the size-2 rock; the medium sheds 2 smalls or dies clean, got {rocks}");
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert_eq!(mines, 0, "the mine detonates and despawns");
    }
}
