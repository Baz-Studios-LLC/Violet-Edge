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
use bevy::render::render_asset::RenderAssetUsages;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitWindows;

mod audio;
use rand::Rng;
use std::collections::HashSet;

// ─────────────────────────────── config ───────────────────────────────
const TAU: f32 = std::f32::consts::TAU;

const SHIP_R: f32 = 15.0;
const TURN_RATE: f32 = 4.6; // rad/s — snappier aim for precision shooting
const THRUST: f32 = 1000.0; // px/s^2 — raised to keep a usable top speed against the heavier drag below
const FRICTION: f32 = 0.15; // velocity kept per second — much heavier drag than before, so the ship sheds momentum fast (~0.37s half-life) for precise, deliberate flying instead of a long glide
const MAX_SPEED: f32 = 560.0; // px/s (a cap; sustained thrust settles a bit under it)
const FIRE_COOLDOWN: f32 = 0.18; // s

const BULLET_SPEED: f32 = 720.0; // px/s
const BULLET_LIFE: f32 = 1.6; // s — MINIMUM range floor (small windows); real range scales with the arena
const BULLET_RANGE_FRAC: f32 = 1.5; // bullet travels this × the arena half-width, so reach scales with the screen (fixes "too short on a big display")
const BULLET_R: f32 = 3.0;

// Mass shot (pickup after boss 2): a bigger, slower, harder-hitting primary. Toggle standard↔mass.
const MASS_COOLDOWN: f32 = 0.5; // s between mass shots (vs 0.18 standard — much slower)
const MASS_BULLET_R: f32 = 7.0; // fat round (vs 3.0 standard)
const MASS_POWER: i32 = 3; // damage per hit (standard = 1): 1-shots dense rocks, chunks bosses

const GRID_CELL: f32 = 52.0;
const WAVE_SECS: f32 = 180.0; // 3-minute waves — survive the timer to advance
const POP_BASE: i32 = 5; // asteroids on screen = POP_BASE + wave...
const POP_CAP: i32 = 18; // ...capped so the field never becomes an unavoidable wall
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
const WARP_GRID_STRENGTH: f32 = 82.0; // max inward grid displacement (px) at the hole
const WARP_SNAP_DUR: f32 = 0.7; // rubber-band snapback time after the hole closes

// Mines (wave 2+): drifting proximity crimson mines.
const MINE_FIRST_WAVE: i32 = 2;
const MINE_PER_WAVE: i32 = 2; // target = (wave - first + 1) * per...
const MINE_MAX_FRACTION: f32 = 0.5; // ...never more than this fraction of the asteroid count
const MINE_R: f32 = 13.0;
const MINE_SPEED: f32 = 62.0; // px/s drift
const MINE_TRIGGER_R: f32 = 92.0; // ship within → the mine arms (blinks)
const MINE_BLAST_R: f32 = 52.0; // armed + ship within → detonate (kills the ship)
const MINE_FUSE: f32 = 0.6; // arming time before it can detonate (time to escape)
const MINE_SCORE: u32 = 150;
const WARP_ROCK_SCORE: u32 = 25; // a rock swallowed by the warp scores a low flat value (no farming)
const MINE_SPAWN_INTERVAL: f32 = 2.6;
const MINE_CHUNK_MULT: f32 = 1.9; // HIDDEN: rocks shattered by a mine blast fling chunks this much faster

// Enemy ships (wave 3+): drift in, hover-and-strafe while firing at the ship, dodge
// mines/rocks, get sucked into the warp, and bug out if they linger too long.
const ENEMY_MAX_FRACTION: f32 = 0.3; // mob count is capped well below the rock count (a garnish)
const ENEMY_R: f32 = 14.0;
const ENEMY_MAX_SPEED: f32 = 125.0; // px/s
const ENEMY_ACCEL: f32 = 640.0; // px/s² steering force
const ENEMY_PREF_DIST: f32 = 260.0; // hovers around this range from the ship
const ENEMY_AVOID_R: f32 = 95.0; // steer away from mines/rocks within this
const ENEMY_SEP_R: f32 = ENEMY_R * 4.0; // steer away from EACH OTHER within this (no stacking)
const ENEMY_FIRE_EVERY: f32 = 2.4; // s between shots (deliberately slow, dodgeable)
const ENEMY_FIRE_JITTER: f32 = 0.9;
const ENEMY_BULLET_SPEED: f32 = 250.0; // px/s (ship is faster, so it's dodgeable)
const ENEMY_BULLET_R: f32 = 5.0;
const ENEMY_BULLET_LIFE: f32 = 4.5; // s
const ENEMY_LIFETIME: f32 = 11.0; // s on-screen before it flees (never overstays)
const ENEMY_SCORE: u32 = 300;
const ENEMY_SPAWN_INTERVAL: f32 = 3.0;

// The Limpet (waves 12-13): a parasite mob that TETHERS to a large rock and hides on the far side,
// peeking out to fire. It reuses the enemy's slow EnemyBullet. Shots from its rock-side are blocked
// by the host (the `guard` half-plane); to kill it you FLANK the exposed side, or break its host so
// it has to scramble to another rock — it re-tethers until IT is destroyed. Reuses ENEMY_BULLET_*.
const LIMPET_R: f32 = 13.0;
const LIMPET_HP: i32 = 1; // one clean hit — a mob never has more effective health than the ship (which dies in one)
const LIMPET_SPEED: f32 = 140.0; // reposition speed — slow enough to swing around and flank, fast enough to track a drifting rock
const LIMPET_FIRE_EVERY: f32 = 1.3; // s hiding between pop-outs
const LIMPET_FIRE_JITTER: f32 = 0.8;
const LIMPET_SCORE: u32 = 350; // a bit more than the lobber — harder to reach
const LIMPET_TURN: f32 = 2.4; // rad/s it slides around the rim (to hide, or to pop out to the ship-side to fire)
const LIMPET_MAX: i32 = 3; // hard cap on live limpets
const LIMPET_HOST_MIN_SIZE: u8 = 3; // only tethers to LARGE rocks
const LIMPET_SPAWN_INTERVAL: f32 = 4.0;

// Dense (green) asteroids take multiple bullet hits to crack (hp = size); chain/mine still break
// them at once. The per-wave rock mix (blue / green / orange) lives in `roll_rock_kind`.

// Octopus boss (every 5th wave): a magenta core that captures field asteroids into a
// rotating orbital shield (its "arms") and hurls the smallest held rocks at the ship.
const BOSS_WAVE_INTERVAL: i32 = 5; // waves 5, 10, 15, … are boss waves
const BOSS_R: f32 = 38.0;
const BOSS_HP: i32 = 28; // core hits to kill (the shield blocks most shots)
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

// Boss 2 — the devourer (wave 10): a red seeker that eats rocks to grow + heal.
const DEVOURER_HP: i32 = 70; // core HP; it STARTS full — the bar reads 100% at the start (much tankier than the shaman's 28)
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
const PICKUP_DRIFT: f32 = 32.0; // px/s slow drift
const PICKUP_LIFE: f32 = 20.0; // the orb lingers this long (well past the 10s boss calm) before vanishing

const MAX_SEP: f32 = 6.0; // px/frame cap on overlap push-out
const RESTITUTION: f32 = 1.0; // fully elastic bounce
const MIN_DRIFT: f32 = 30.0; // px/s — rocks never fully stop (elastic hits can zero them → "stuck")
const FRAGMENT_GRACE: f32 = 1.8; // s a freshly-broken fragment is protected from off-screen culling
const GOLD_GRACE: f32 = 6.0; // gold fragments get a longer window (recycle, not culled) — a fair chance to catch them before one can drift off and forfeit the life
const ORANGE_BLAST_R: f32 = 250.0; // explosive-asteroid kill/chain radius (+ the victim's own radius). Was 150 — too small on big screens, so it looked huge (the particle burst throws to ~440) but barely caught neighbours. Now the reach matches the visual.
const ORANGE_FUSE: f32 = 0.09; // brief lit flash after a lethal hit before it detonates (a visible "pop")

const RESPAWN_DELAY: f32 = 1.3; // s the ship stays gone after dying
const GAMEOVER_DELAY: f32 = 1.5; // s to let the final death play out before the Game Over screen
const HUD_FLASH_TIME: f32 = 0.7; // s the warp pips / life icons flicker after refilling / gaining a life
const SHOT_MODE_SHOW: f32 = 1.4; // s the "MASS/STANDARD SHOT" label lingers after a toggle
const SPAWN_INVULN: f32 = 2.0; // s of blink-invulnerability on (re)spawn
const TRAIL_LEN: usize = 10; // bullet trail points kept
const STAR_COUNT: usize = 90;
const START_LIVES: i32 = 3;
const LIFE_CAP: i32 = START_LIVES; // gold restores a LOST life only — never above the starting count
// The gold 1UP rock drifts in at a randomized time during play (a countdown), not at wave starts.
const GOLD_INITIAL_DELAY: f32 = 45.0; // grace before the first gold rock can appear in a run
// Gap measured from when a gold rock APPEARS to the earliest the next one may — long enough that you
// get at most ~1 per (3-minute) wave. A fresh random value in this range is rolled on each spawn.
const GOLD_MIN_GAP: f32 = 240.0; // ~4 minutes minimum between appearances
const GOLD_MAX_GAP: f32 = 360.0; // ~6 minutes at the outside
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
fn bullet_power(mass: bool) -> i32 {
    if mass {
        MASS_POWER
    } else {
        1
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
fn limpet_color() -> Color {
    Color::srgb(0.4, 3.6, 4.2)
} // cold cyan — the Limpet parasite (balanced green+blue reads apart from the blue rocks it clings to)

fn mine_target(level: i32, asteroids: i32) -> i32 {
    if level < MINE_FIRST_WAVE {
        return 0;
    }
    let raw = (level - MINE_FIRST_WAVE + 1) * MINE_PER_WAVE;
    raw.min((asteroids as f32 * MINE_MAX_FRACTION) as i32)
}

fn asteroid_radius(size: u8) -> f32 {
    match size {
        3 => 88.0, // LARGE
        2 => 46.0, // MID
        _ => 22.0, // SMALL
    }
}
fn body_mass(r: f32) -> f32 {
    r * r
}
fn population_target(level: i32) -> i32 {
    (POP_BASE + level).min(POP_CAP)
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

// Ease angle `from` toward `to` (radians) by at most `max_step`, taking the short way around.
fn step_angle(from: f32, to: f32, max_step: f32) -> f32 {
    let mut d = (to - from).rem_euclid(TAU);
    if d > TAU * 0.5 {
        d -= TAU;
    }
    from + d.clamp(-max_step, max_step)
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
    burst(commands, pos, ship_color(), 30, 340.0, rng);
    burst(commands, pos, Color::srgb(4.0, 4.0, 5.0), 12, 220.0, rng);
    sfx.write(SoundFx::Death);
    commands.entity(ship_e).despawn();
    run.lives -= 1;
    // Even on the last life we DON'T jump straight to Game Over — set a timer so the death
    // explosion plays out; `respawn` makes the transition once it elapses (less abrupt).
    run.respawn = if run.lives <= 0 { GAMEOVER_DELAY } else { RESPAWN_DELAY };
}

// A combat kill of an enemy mob: award score, splash debris, play the death zap, despawn.
// Shared by the bullet hit and the chain-beam hit so the two can't drift apart.
fn kill_enemy(commands: &mut Commands, score: &mut Score, sfx: &mut EventWriter<SoundFx>, e: Entity, pos: Vec2, rng: &mut impl Rng) {
    score.0 += ENEMY_SCORE;
    burst(commands, pos, enemy_color(), 20, 320.0, rng);
    sfx.write(SoundFx::EnemyDie);
    commands.entity(e).despawn();
}

// A combat kill of a Limpet: its own score + cyan splash, then despawn. Mirrors `kill_enemy` so the
// mob-death shape stays consistent. Callers bump `stats.enemies` (it counts as a mob).
fn kill_limpet(commands: &mut Commands, score: &mut Score, sfx: &mut EventWriter<SoundFx>, e: Entity, pos: Vec2, rng: &mut impl Rng) {
    score.0 += LIMPET_SCORE;
    burst(commands, pos, limpet_color(), 20, 320.0, rng);
    sfx.write(SoundFx::EnemyDie);
    commands.entity(e).despawn();
}

// ─────────────────────────────── state / components / resources ───────
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum GameState {
    #[default]
    Menu,
    Achievements, // the achievements screen, reached from the main menu
    Controls,     // input method + key/button rebinding, reached from the main menu
    Briefing,     // the lore + objectives screen, reached from the main menu
    Playing,
    Paused,
    GameOver,
}

// A run is "active" (grid + HUD drawn) in these states, not on the menu screens.
fn run_active(state: &GameState) -> bool {
    matches!(state, GameState::Playing | GameState::Paused | GameState::GameOver)
}

// One filter for "everything spawned during a run" — used to wipe the field when quitting to the
// menu or restarting (the starfield + camera are NOT in here, so the backdrop survives).
type GameplayEntity = Or<(
    With<Ship>,
    With<Asteroid>,
    With<Bullet>,
    With<Particle>,
    With<BlackHole>,
    With<WarpMissile>,
    With<Mine>,
    With<Enemy>,
    With<EnemyBullet>,
    With<Limpet>,
    With<Shockwave>,
    With<Boss>,
    With<Devourer>,
    With<ChainShot>,
    With<Pickup>,
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

// A slow enemy shot. Distinct from the player's `Bullet` (never purple).
#[derive(Component)]
struct EnemyBullet {
    life: f32,
}

// The Limpet parasite (waves 12-13). Tethers to a large rock and hides on its far side; only dies to
// direct damage to ITSELF. See the LIMPET_* consts.
#[derive(Component)]
struct Limpet {
    hp: i32,
    fire: f32,             // countdown to the next peek-shot
    host: Option<Entity>,  // the large rock it's riding (re-acquired when the old one is destroyed)
    angle: f32,            // its position AROUND the host rim (radians); eases toward "hide from ship"
    guard: Option<Vec2>,   // while tethered: the exposed direction (host→limpet). Shots from the other
                           // side are blocked by the host. None while transiting → fully vulnerable.
}

// The octopus boss core.
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
    fed: i32,  // rocks eaten (flavor / telemetry)
    dying: f32,
    pulse: f32,
}

// A rock the boss just hurled — briefly un-grabbable so it can't be re-captured instantly.
#[derive(Component)]
struct Thrown(f32);

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

// An orange rock that's been lit and is about to blow. The brief fuse gives a visible flash, then
// `detonate` blasts a radius and chains any other oranges caught in it.
#[derive(Component)]
struct Detonating {
    fuse: f32,
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

// Which weapon a reward orb unlocks. Chain drops after boss 1, mass shot after boss 2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickupKind {
    Chain,
    Mass,
}

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
struct MenuUi;
#[derive(Component)]
struct AchievementsUi;
#[derive(Component)]
struct ControlsUi;
#[derive(Component)]
struct BriefingUi;
#[derive(Component)]
struct Hud; // HUD roots — hidden on the menu screens

// Clickable menu buttons (mouse), mirrored by the keyboard shortcuts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Play,
    Achievements,
    Controls, // main menu → the controls / input-rebinding screen
    Briefing,
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
#[derive(Component)]
struct ShotModeText; // bottom-center "MASS/STANDARD SHOT" label, fades after a Q toggle

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

#[derive(Resource)]
struct Run {
    lives: i32,
    respawn: f32,
}

#[derive(Resource)]
struct Wave {
    level: i32,
    timer: f32,
    calm: f32, // > 0 during the post-boss calm — pauses spawns + the wave timer
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

// Throttles Limpet spawns.
#[derive(Resource, Default)]
struct LimpetClock(f32);

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

// ─────────────────────────────── achievements ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ach {
    FirstBlood,
    Warden,
    Glutton,
    TrueBlue,
    GreenThumb,
    Edgelord,
    Purist,
}
// Order defines the index into `Achievements.unlocked` and the menu list.
const ACHIEVEMENTS: [Ach; 7] =
    [Ach::FirstBlood, Ach::Warden, Ach::Glutton, Ach::TrueBlue, Ach::GreenThumb, Ach::Edgelord, Ach::Purist];

fn ach_meta(a: Ach) -> (&'static str, &'static str) {
    match a {
        Ach::FirstBlood => ("First Blood", "Destroy an enemy ship"),
        Ach::Warden => ("Warden Off", "Defeat the Warden — boss 1"),
        Ach::Glutton => ("Glutton for Punishment", "Defeat the Glutton — boss 2"),
        Ach::TrueBlue => ("True Blue", "Destroy 100 blue asteroids"),
        Ach::GreenThumb => ("Green Thumb", "Destroy 100 dense green asteroids"),
        Ach::Edgelord => ("Edgelord", "Beat the game (clear the wave 1-10 arc)"),
        Ach::Purist => ("Purist", "Beat the game without a single powerup"),
    }
}

fn ach_met(a: Ach, s: &Stats) -> bool {
    match a {
        Ach::FirstBlood => s.enemies >= 1,
        Ach::Warden => s.warden,
        Ach::Glutton | Ach::Edgelord => s.glutton, // beating boss 2 IS clearing the arc
        Ach::TrueBlue => s.blue >= 100,
        Ach::GreenThumb => s.green >= 100,
        Ach::Purist => s.no_powerups,
    }
}

// LIFETIME progress — accumulates across runs and is persisted to disk (see load/save_progress).
// NOT reset by `reset_run`.
#[derive(Resource, Default, Clone, Copy)]
struct Stats {
    blue: u32,         // blue asteroids destroyed (lifetime)
    green: u32,        // dense green asteroids destroyed (lifetime)
    enemies: u32,      // enemy ships destroyed (lifetime)
    warden: bool,      // ever defeated boss 1
    glutton: bool,     // ever defeated boss 2 (= beat the arc)
    no_powerups: bool, // ever beat boss 2 having grabbed no powerup that run
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

// Waves 1-15 are hand-authored; 16+ loop back over that arc (we perfect 1-15 first, then extend).
fn content_wave(level: i32) -> i32 {
    (level - 1).rem_euclid(15) + 1
}
// Boss waves alternate: content-10 = the devourer (boss 2); content-5 = the shaman (boss 1).
fn is_devourer_wave(level: i32) -> bool {
    is_boss_wave(level) && content_wave(level) == 10
}
fn devourer_radius(grow: f32) -> f32 {
    DEVOURER_BASE_R + grow.clamp(0.0, 1.0) * (DEVOURER_MAX_R - DEVOURER_BASE_R)
}

// Shared "boss defeated" bookkeeping (both bosses use it): reward, then advance into the calm.
fn defeat_boss(score: &mut Score, wave: &mut Wave, banner: &mut WaveBanner) {
    score.0 += BOSS_SCORE;
    wave.level += 1;
    wave.timer = WAVE_SECS;
    wave.calm = BOSS_CALM;
    banner.timer = WAVE_BANNER_SECS;
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
    ));

    // starfield — fixed positions, each with its own twinkle phase
    let mut rng = rand::thread_rng();
    for _ in 0..STAR_COUNT {
        let pos = Vec2::new(rng.gen_range(-720.0..720.0), rng.gen_range(-460.0..460.0));
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
    let label = Color::srgb(0.7, 0.85, 1.2);
    commands.spawn((
        Hud,
        Text::new("LIVES"),
        TextFont { font_size: 18.0, ..default() },
        TextColor(label),
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), right: Val::Px(22.0), ..default() },
    ));
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
    // shot-mode label (bottom-center, above the warp pips) — fades in/out on a Q toggle
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(56.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                ShotModeText,
                Text::new(""),
                TextFont { font_size: 20.0, ..default() },
                TextColor(Color::srgba(0.72, 0.28, 1.0, 0.0)), // violet (player kit), starts hidden
            ));
        });
}

fn spawn_player(commands: &mut Commands) {
    commands.spawn((
        Ship { angle: TAU / 4.0, cooldown: 0.0, invuln: SPAWN_INVULN, flame: 0.0 },
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
    Blue,   // plain
    Green,  // dense / tanky (takes `hp` hits)
    Orange, // explosive (detonates instead of splitting)
}

// Which flavor should a rock spawned for `level` be? One roll shared by every edge-spawn caller,
// so a wave's whole rock mix is defined here. Fractions are the tuning knobs for wave feel.
fn roll_rock_kind(level: i32, rng: &mut impl Rng) -> RockKind {
    // Orange (explosive) fraction. Debuts wave 11; wave 14 is the ALL-orange danger wave.
    let orange = match content_wave(level) {
        11..=13 => 0.25,
        14 => 1.0,
        _ => 0.0,
    };
    if rng.gen_bool(orange) {
        return RockKind::Orange;
    }
    // Green (dense) fraction of what's left. Wave 6 mixes green in; 7-9 are all green; the devourer
    // wave (10) stays plain blue food. Waves 11 & 13 make their non-orange rocks green ("green +
    // orange"), and the wave-15 boss arena is green-only.
    let green = match content_wave(level) {
        6 => 0.5,
        7..=9 => 1.0,
        11 | 13 | 15 => 1.0,
        _ => 0.0,
    };
    if rng.gen_bool(green) {
        RockKind::Green
    } else {
        RockKind::Blue
    }
}

// A fresh large asteroid entering from just off a random edge (wave top-up). `dense`
// spawns the tanky green variant (the caller decides based on the wave). Returns the entity so
// callers can tag it (e.g. the gold 1UP rock).
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
    let e = spawn_asteroid(commands, pos, size, vel, rng, matches!(kind, RockKind::Green));
    if matches!(kind, RockKind::Orange) {
        commands.entity(e).insert(Explosive); // detonates instead of splitting (see `detonate`)
    }
    e
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
fn break_asteroid(commands: &mut Commands, rng: &mut impl Rng, score: &mut Score, e: Entity, pos: Vec2, size: u8, chunk_mult: f32, dense: bool, gold: bool) {
    commands.entity(e).despawn();
    let base = match size {
        3 => 20,
        2 => 50,
        _ => 100,
    };
    score.0 += if dense { base * 2 } else { base }; // dense rocks are worth more
    burst(commands, pos, if dense { dense_color() } else { rock_color() }, 10 + size as usize * 5, 260.0, rng);
    if size > 1 {
        // Split into two chunks that fly APART along a random axis. Each is spawned
        // already clear of the other (offset past their combined radii) so the pair
        // never overlaps — an overlapping spawn lets the collision resolver cancel
        // their motion and leaves them oozing apart at the break point instead of
        // shooting off. Headings get a little jitter so it isn't a rigid mirror.
        // Children inherit density (a dense rock breaks into dense chunks).
        let axis = rng.gen_range(0.0..TAU);
        let out = Vec2::from_angle(axis);
        let offset = asteroid_radius(size - 1) + 3.0;
        for side in [1.0f32, -1.0] {
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
    score: &mut Score,
    asteroids: &Query<(Entity, &Transform, &mut Asteroid, Option<&Gold>, Option<&Explosive>), (Without<Mine>, Without<Shielded>)>,
    broken: &mut HashSet<Entity>,
    center: Vec2,
) {
    // shared &Query → iterates read-only, so we just read size/dense/gold/explosive here
    for (ae, at, a, gold, explosive) in asteroids {
        if broken.contains(&ae) {
            continue;
        }
        if gold.is_some() {
            continue; // gold 1UP rocks are immune to mines — only the player's shots may break them
        }
        let ap = at.translation.truncate();
        let br = MINE_BLAST_R + asteroid_radius(a.size);
        if center.distance_squared(ap) < br * br {
            broken.insert(ae);
            if explosive.is_some() {
                commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE }); // a mine lights the orange → it chain-detonates
            } else {
                break_asteroid(commands, rng, score, ae, ap, a.size, MINE_CHUNK_MULT, a.dense, false); // mine obliterates (ignores hp); never gold (skipped above)
            }
        }
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
        // a big, punchy blast: a dense orange debris burst + a white-hot flash spray, and an expanding
        // shockwave ring that reaches the actual kill radius (so the danger zone is unmistakable).
        burst(&mut commands, c, orange_color(), 64, 560.0, &mut rng);
        burst(&mut commands, c, Color::srgb(6.0, 4.2, 1.6), 20, 300.0, &mut rng);
        commands.spawn((
            Shockwave { age: 0.0, ttl: 0.32, max_r: ORANGE_BLAST_R, color: orange_color() },
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
            let rr = ORANGE_BLAST_R + asteroid_radius(a.size);
            if c.distance_squared(at.translation.truncate()) < rr * rr {
                if explosive.is_some() {
                    commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE }); // chain!
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
            let rr = ORANGE_BLAST_R + MINE_R;
            if c.distance_squared(mt.translation.truncate()) < rr * rr {
                burst(&mut commands, mt.translation.truncate(), mine_color(), 18, 300.0, &mut rng);
                commands.entity(me).despawn();
                score.0 += MINE_SCORE;
            }
        }
        for (ee, et) in enemies {
            let rr = ORANGE_BLAST_R + ENEMY_R;
            if c.distance_squared(et.translation.truncate()) < rr * rr {
                burst(&mut commands, et.translation.truncate(), enemy_color(), 18, 300.0, &mut rng);
                commands.entity(ee).despawn();
                score.0 += ENEMY_SCORE;
                stats.enemies += 1;
                sfx.write(SoundFx::EnemyDie);
            }
        }
        // the player is caught too — but not mid-respawn or while blinking/invincible
        if run.respawn <= 0.0 {
            for (se, st, sh) in ships {
                let sp = st.translation.truncate();
                let rr = ORANGE_BLAST_R + SHIP_R;
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
        Action::ToggleShot => "Toggle mass shot",
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
                if h {
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
    // left stick adds analog turn (finer than the d-pad): stick right (+x) = clockwise = negative turn
    for g in &gamepads {
        if let Some(x) = g.get(GamepadAxis::LeftStickX) {
            if x.abs() > STICK_DEADZONE {
                turn -= x;
            }
        }
    }
    *state = ActionState { turn: turn.clamp(-1.0, 1.0), thrust: thrust.clamp(0.0, 1.0), fire_held, warp, chain, toggle, pause, mute };
}

// ─────────────────────────────── gameplay systems (Playing only) ──────
fn ship_control(
    time: Res<Time>,
    input: Res<ActionState>,
    mut commands: Commands,
    mut q: Query<(&mut Ship, &mut Velocity, &Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    for (mut ship, mut vel, t) in &mut q {
        ship.angle += input.turn * TURN_RATE * dt;
        let thrusting = input.thrust > 0.05;
        if thrusting {
            vel.0 += Vec2::from_angle(ship.angle) * THRUST * input.thrust * dt;
            ship.flame = (ship.flame + dt * 5.0).min(1.0);
            // exhaust sparks straight out the BACK (opposite the nose) — NOT blended with
            // the ship's velocity, so it never sprays sideways when drifting. Sparse.
            if rng.gen_bool(0.3) {
                let back = ship.angle + TAU * 0.5 + rng.gen_range(-0.22..0.22);
                let tail = t.translation.truncate() - Vec2::from_angle(ship.angle) * SHIP_R * 0.5;
                commands.spawn((
                    Particle {
                        vel: Vec2::from_angle(back) * rng.gen_range(90.0..150.0),
                        life: 0.26,
                        ttl: 0.26,
                        color: flame_color(),
                    },
                    Transform::from_xyz(tail.x, tail.y, 0.0),
                ));
            }
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
    mut armed: ResMut<FireArmed>,
    mut mode: ResMut<ShotModeFlash>,
    arena: Res<Arena>,
    mut sfx: EventWriter<SoundFx>,
    mut q: Query<(&mut Ship, &Transform)>,
) {
    let dt = time.delta_secs();
    // bullet lifetime scales with the arena so its reach is a consistent fraction of the screen,
    // not a fixed distance that looks tiny on a big display (floored at BULLET_LIFE for small windows)
    let bullet_life = (BULLET_RANGE_FRAC * arena.half.x / BULLET_SPEED).max(BULLET_LIFE);
    // ToggleShot switches standard↔mass once the mass shot is unlocked — with a click + on-screen label
    if mass.unlocked && input.toggle {
        mass.active = !mass.active;
        mode.0 = SHOT_MODE_SHOW;
        sfx.write(SoundFx::Toggle);
    }
    let is_mass = mass.unlocked && mass.active;
    let want_fire = input.fire_held;
    if !want_fire {
        armed.0 = true; // released → the next press is a genuine fire, not the start/resume click
    }
    for (mut ship, t) in &mut q {
        if ship.cooldown > 0.0 {
            ship.cooldown -= dt;
        }
        if want_fire && armed.0 && ship.cooldown <= 0.0 {
            ship.cooldown = if is_mass { MASS_COOLDOWN } else { FIRE_COOLDOWN };
            let dir = Vec2::from_angle(ship.angle);
            let pos = t.translation.truncate() + dir * SHIP_R;
            commands.spawn((
                Bullet { life: bullet_life, trail: Vec::new(), mass: is_mass },
                Velocity(dir * BULLET_SPEED),
                Transform::from_xyz(pos.x, pos.y, 0.0),
            ));
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
    asteroids: Query<(&Transform, &Asteroid, Option<&Shielded>)>,
) {
    if run.respawn > 0.0 {
        return; // already dead/respawning
    }
    for (e, t, ship) in &ships {
        if immune(ship, &dev) {
            continue;
        }
        let sp = t.translation.truncate();
        for (at, a, shielded) in &asteroids {
            if shielded.is_some_and(|sh| sh.grab < BOSS_GRAB_TIME) {
                continue; // mid-grab: harmless while it reels across the field
            }
            let rr = asteroid_radius(a.size) + SHIP_R * 0.6;
            if sp.distance_squared(at.translation.truncate()) < rr * rr {
                let mut rng = rand::thread_rng();
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

fn asteroid_bounds(mut commands: Commands, time: Res<Time>, arena: Res<Arena>, mut rush: ResMut<GoldRush>, mut q: Query<(Entity, &mut Transform, &mut Velocity, &Asteroid, Option<&mut Fresh>, Option<&Gold>), Without<Shielded>>) {
    let h = arena.half;
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    for (e, mut t, mut v, a, fresh, gold) in &mut q {
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
    bullets: Query<(Entity, &Transform, &Bullet)>,
    mut asteroids: Query<(Entity, &Transform, &mut Asteroid, Option<&Gold>, Option<&Explosive>), (Without<Mine>, Without<Shielded>)>,
    mines: Query<(Entity, &Transform), With<Mine>>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    mut limpets: Query<(Entity, &Transform, &mut Limpet)>,
    mut shield_rocks: Query<(Entity, &Transform, &mut Asteroid), With<Shielded>>,
    mut bosses: Query<(&Transform, &mut Boss)>,
    mut devourers: Query<(&Transform, &mut Devourer)>,
    mut score: ResMut<Score>,
    mut sfx: EventWriter<SoundFx>,
    mut stats: ResMut<Stats>,
) {
    let mut rng = rand::thread_rng();
    let mut dead_b: HashSet<Entity> = HashSet::new();
    let mut dead_a: HashSet<Entity> = HashSet::new();
    let mut dead_m: HashSet<Entity> = HashSet::new();
    let mut dead_e: HashSet<Entity> = HashSet::new();
    let mut dead_l: HashSet<Entity> = HashSet::new();
    let mut dead_s: HashSet<Entity> = HashSet::new();
    for (be, bt, b) in &bullets {
        if dead_b.contains(&be) {
            continue;
        }
        let bp = bt.translation.truncate();
        let br = bullet_radius(b.mass); // mass shots are fatter…
        let power = bullet_power(b.mass); // …and hit harder
        for (ae, at, mut a, gold, explosive) in &mut asteroids {
            if dead_a.contains(&ae) {
                continue;
            }
            let ap = at.translation.truncate();
            let rr = asteroid_radius(a.size) + br;
            if bp.distance_squared(ap) < rr * rr {
                dead_b.insert(be);
                commands.entity(be).despawn(); // bullet is spent either way
                a.hp -= power;
                if a.hp > 0 {
                    burst(&mut commands, ap, dense_color(), 6, 160.0, &mut rng); // dense rock cracks but holds
                } else {
                    dead_a.insert(ae);
                    if explosive.is_some() {
                        commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE }); // orange: detonates, doesn't split
                    } else {
                        break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, a.size, 1.0, a.dense, gold.is_some());
                        sfx.write(SoundFx::Break(a.size));
                        if a.dense {
                            stats.green += 1;
                        } else {
                            stats.blue += 1;
                        }
                    }
                }
                break;
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
                let mp = mt.translation.truncate();
                burst(&mut commands, mp, mine_color(), 24, 320.0, &mut rng);
                // shooting a mine detonates it: the blast shatters rocks in range
                // with fast chunks, same as any other detonation.
                blast_asteroids(&mut commands, &mut rng, &mut score, &asteroids, &mut dead_a, mp);
                sfx.write(SoundFx::Mine);
                break;
            }
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on a mine
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
        for (le, lt2, mut lp) in &mut limpets {
            if dead_l.contains(&le) {
                continue;
            }
            let lpp = lt2.translation.truncate();
            let rr = LIMPET_R + br;
            if bp.distance_squared(lpp) >= rr * rr {
                continue;
            }
            // the host rock shields the Limpet: a shot from its rock-side is blocked — only the
            // exposed hemisphere (`guard`) takes damage. Transiting (guard None) → fully open.
            if let Some(g) = lp.guard {
                if (bp - lpp).dot(g) <= 0.0 {
                    continue; // blocked by the host; the shot slips past, no damage
                }
            }
            dead_b.insert(be);
            commands.entity(be).despawn();
            lp.hp -= power;
            if lp.hp <= 0 {
                dead_l.insert(le);
                kill_limpet(&mut commands, &mut score, &mut sfx, le, lpp, &mut rng);
                stats.enemies += 1;
            } else {
                burst(&mut commands, lpp, limpet_color(), 6, 180.0, &mut rng); // chipped but alive
            }
            break;
        }
        if dead_b.contains(&be) {
            continue; // bullet already spent on a limpet
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
    }
}

// 3-minute waves: survive the timer to advance; each new wave streams in more
// rocks (up to the cap). (Boss waves will end on kill instead — added in a later step.)
fn wave_timer(
    time: Res<Time>,
    mut wave: ResMut<Wave>,
    mut banner: ResMut<WaveBanner>,
    mut commands: Commands,
    arena: Res<Arena>,
    asteroids: Query<(), With<Asteroid>>,
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
    let target = population_target(wave.level);
    let have = asteroids.iter().count() as i32;
    let mut rng = rand::thread_rng();
    for _ in 0..(target - have).max(0) {
        let kind = roll_rock_kind(wave.level, &mut rng);
        spawn_edge_asteroid(&mut commands, arena.half, &mut rng, kind, false);
    }
}

// The rare gold 1UP rock drifts in at a randomized time DURING play (not tied to wave starts). Only
// one hunt runs at a time, and a cooldown after each hunt keeps them from spawning back-to-back. It
// may appear on any wave (boss waves included — the Devourer won't eat it and a rock the Warden grabs
// is just a shoot-it-off-the-shield target).
fn gold_spawn(time: Res<Time>, wave: Res<Wave>, arena: Res<Arena>, mut rush: ResMut<GoldRush>, mut commands: Commands) {
    rush.cooldown -= time.delta_secs(); // counts down from the last APPEARANCE (keeps ticking during a hunt)
    if rush.active || rush.cooldown > 0.0 || wave.calm > 0.0 {
        return; // a hunt's still running, the gap hasn't elapsed, or the post-boss field is kept clear
    }
    let mut rng = rand::thread_rng();
    spawn_gold_rock(&mut commands, arena.half, &mut rng);
    rush.active = true;
    rush.forfeited = false;
    rush.cooldown = rng.gen_range(GOLD_MIN_GAP..GOLD_MAX_GAP); // next one is at least ~4 min out
}

// Stream replacement rocks in gradually so the field stays populated as you clear
// it — but NOT during the post-boss calm (kept clear for the reward).
fn top_up_asteroids(
    time: Res<Time>,
    mut clock: ResMut<SpawnClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    asteroids: Query<&Asteroid>,
) {
    if wave.calm > 0.0 {
        return;
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let count = asteroids.iter().count() as i32;
    let bigs = asteroids.iter().filter(|a| a.size == 3).count() as i32;
    // refill toward the count target, AND separately keep big rocks above the floor even at the
    // cap — otherwise breaking large rocks leaves the field as nothing but small debris.
    if count < population_target(wave.level) || bigs < BIG_FLOOR {
        let mut rng = rand::thread_rng();
        let kind = roll_rock_kind(wave.level, &mut rng);
        spawn_edge_asteroid(&mut commands, arena.half, &mut rng, kind, bigs < BIG_FLOOR);
        clock.0 = SPAWN_INTERVAL;
    } else {
        clock.0 = 0.5; // at target — recheck shortly
    }
}

// The post-boss calm is a clean breather (and the pickup window): keep the field empty by
// despawning any leftover asteroids/mines — including the boss's scattered shield — for its whole
// duration. New spawns are already gated (top-ups bail while `calm > 0`).
fn clear_calm_field(wave: Res<Wave>, mut commands: Commands, junk: Query<Entity, Or<(With<Asteroid>, With<Mine>)>>) {
    if wave.calm > 0.0 {
        for e in &junk {
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
    mut score: ResMut<Score>,
    dev: Res<Dev>,
    wave: Res<Wave>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<(Entity, &Transform, &Ship), Without<Mine>>,
    mut mines: Query<(Entity, &mut Transform, &mut Velocity, &mut Mine)>,
    // &mut to match blast_asteroids' type; only read here (iter + shared borrow)
    asteroids: Query<(Entity, &Transform, &mut Asteroid, Option<&Gold>, Option<&Explosive>), (Without<Mine>, Without<Shielded>)>,
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
        for (_, at, a, gold, _) in &asteroids {
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
            && asteroids.iter().any(|(_, at, a, gold, _)| {
                gold.is_none() && {
                    let rr = MINE_R + asteroid_radius(a.size);
                    p.distance_squared(at.translation.truncate()) < rr * rr
                }
            })
        {
            burst(&mut commands, p, mine_color(), 26, 300.0, &mut rng);
            blast_asteroids(&mut commands, &mut rng, &mut score, &asteroids, &mut broken, p);
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
                    blast_asteroids(&mut commands, &mut rng, &mut score, &asteroids, &mut broken, p);
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
    mut q: Query<(Entity, &Transform, &Velocity, &mut WarpMissile)>,
    rocks: Query<(&Transform, &Asteroid), Without<Gold>>, // gold is skipped — the warp shouldn't grief the 1UP
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
        }
    }
}

// ─────────────────────────────── enemy ships (wave 3+) ────────────────
fn enemy_target(level: i32, asteroids: i32) -> i32 {
    // yellow mobs run in two windows: waves 3-4 (before boss 1), then 8-9 (after the green intro,
    // before boss 2). None on 6-7 (green rocks are the focus) or on the boss waves (5, 10). Content
    // waves 11-15 also return 0 here — the Limpet is their mob and spawns from its own system.
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

// Enemy movement + firing. `integrate` moves them (they carry a Velocity); here we
// only steer that velocity. Glide in, then hover + strafe around the ship, steering
// clear of mines and rocks and lobbing slow shots. A live warp overrides all of it
// (they get dragged in — handled in black_hole_update). After ENEMY_LIFETIME they
// flee the nearest edge and despawn, so they never overstay.
fn enemy_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
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

        // lifetime → flee straight out and despawn once gone
        en.life -= dt;
        if en.life <= 0.0 {
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

// A Limpet gliding in from a random edge — it will then seek a large rock to tether to.
fn spawn_edge_limpet(commands: &mut Commands, half: Vec2, rng: &mut impl Rng) {
    let inward = LIMPET_SPEED * 1.6;
    let jitter = rng.gen_range(-0.3..0.3) * LIMPET_SPEED;
    let (pos, vel) = match rng.gen_range(0..4) {
        0 => (Vec2::new(-half.x - LIMPET_R, rng.gen_range(-half.y..half.y)), Vec2::new(inward, jitter)),
        1 => (Vec2::new(half.x + LIMPET_R, rng.gen_range(-half.y..half.y)), Vec2::new(-inward, jitter)),
        2 => (Vec2::new(rng.gen_range(-half.x..half.x), -half.y - LIMPET_R), Vec2::new(jitter, inward)),
        _ => (Vec2::new(rng.gen_range(-half.x..half.x), half.y + LIMPET_R), Vec2::new(jitter, -inward)),
    };
    commands.spawn((
        Limpet { hp: LIMPET_HP, fire: LIMPET_FIRE_EVERY + rng.gen_range(0.0..LIMPET_FIRE_JITTER), host: None, angle: 0.0, guard: None },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
}

// Stream Limpets in on their content waves (12-13), capped at LIMPET_MAX and only while a large rock
// exists to tether to. Not during the calm or boss waves.
fn top_up_limpets(
    time: Res<Time>,
    mut clock: ResMut<LimpetClock>,
    wave: Res<Wave>,
    arena: Res<Arena>,
    mut commands: Commands,
    limpets: Query<(), With<Limpet>>,
    rocks: Query<&Asteroid>,
) {
    if wave.calm > 0.0 || is_boss_wave(wave.level) || !matches!(content_wave(wave.level), 12..=13) {
        return;
    }
    clock.0 -= time.delta_secs();
    if clock.0 > 0.0 {
        return;
    }
    let has_host = rocks.iter().any(|a| a.size >= LIMPET_HOST_MIN_SIZE);
    if has_host && (limpets.iter().count() as i32) < LIMPET_MAX {
        let mut rng = rand::thread_rng();
        spawn_edge_limpet(&mut commands, arena.half, &mut rng);
        clock.0 = LIMPET_SPAWN_INTERVAL;
    } else {
        clock.0 = 1.0;
    }
}

// Limpet AI: tether to the nearest large rock and hide on its far side (relative to the ship),
// peeking out to fire. It repositions at a LIMITED speed, so a nimble player can swing around and
// FLANK its exposed side. When its host is destroyed it just seeks another rock — it only dies to
// direct damage (see `collisions`). `integrate` moves it via Velocity.
fn limpet_update(
    time: Res<Time>,
    mut commands: Commands,
    mut sfx: EventWriter<SoundFx>,
    mut limpets: Query<(&mut Transform, &mut Velocity, &mut Limpet)>,
    rocks: Query<(Entity, &Transform, &Asteroid), Without<Limpet>>,
    ships: Query<&Transform, (With<Ship>, Without<Limpet>)>,
    holes: Query<&Transform, (With<BlackHole>, Without<Limpet>)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next().map(|t| t.translation.truncate());
    for (mut lt, mut lv, mut lp) in &mut limpets {
        let lc = lt.translation.truncate();
        // caught in a warp → yield: let black_hole_update drag it off its rock + consume it (don't
        // fight the pull by rigidly re-gluing to the rim)
        if holes.iter().any(|ht| ht.translation.truncate().distance(lc) < WARP_PULL_RADIUS) {
            lp.guard = None; // exposed while being dragged in
            continue;
        }
        // drop a host that's been destroyed
        if lp.host.is_some_and(|h| rocks.get(h).is_err()) {
            lp.host = None;
        }
        // acquire the nearest LARGE rock if we have none
        if lp.host.is_none() {
            lp.guard = None;
            let mut best: Option<(Entity, f32)> = None;
            for (re, rtf, ra) in &rocks {
                if ra.size < LIMPET_HOST_MIN_SIZE {
                    continue;
                }
                let d = rtf.translation.truncate().distance_squared(lc);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((re, d));
                }
            }
            lp.host = best.map(|(e, _)| e);
        }
        let Some((_, htf, ha)) = lp.host.and_then(|h| rocks.get(h).ok()) else {
            // no large rock anywhere → drift gently toward center, fully exposed
            lp.guard = None;
            lv.0 = (-lc).clamp_length_max(LIMPET_SPEED * 0.5);
            continue;
        };
        let hc = htf.translation.truncate();
        let rr = asteroid_radius(ha.size);
        let cling = (rr - LIMPET_R * 0.35).max(4.0); // center just inside the rim → the body straddles the edge (clings)
        // the ship-facing (near) side is where it pops out to fire; the far side is where it hides
        let to_ship = ship.map(|s| s - hc).unwrap_or(Vec2::Y);
        let near_ang = to_ship.to_angle();
        let hide_ang = (-to_ship).to_angle();
        let from_center = lc - hc;
        if from_center.length() <= rr + LIMPET_R * 2.5 {
            // TETHERED: rigidly ride the rim. It hides on the FAR side, then POPS OUT to the ship-side
            // rim to fire — a clear lane, never through the rock — exposing itself while it shoots.
            lp.fire -= dt;
            let peeking = lp.fire <= 0.0;
            let target = if peeking { near_ang } else { hide_ang };
            lp.angle = step_angle(lp.angle, target, LIMPET_TURN * dt);
            let dir = Vec2::from_angle(lp.angle);
            let rim = hc + dir * cling;
            lt.translation.x = rim.x;
            lt.translation.y = rim.y;
            lv.0 = Vec2::ZERO;
            lp.guard = Some(dir); // far side → the rock shields it; near side (peeking) → it's open to fire
            // fire once it has popped out far enough that the shot clears its own host rock
            if peeking {
                match ship {
                    // rock is behind the muzzle (dot <= 0) → clear lane to the ship
                    Some(s) if (hc - rim).dot(s - rim) <= 0.0 => {
                        let sd = (s - rim).normalize_or_zero();
                        if sd != Vec2::ZERO {
                            commands.spawn((
                                EnemyBullet { life: ENEMY_BULLET_LIFE },
                                Velocity(sd * ENEMY_BULLET_SPEED),
                                Transform::from_xyz(rim.x, rim.y, 0.0),
                            ));
                            sfx.write(SoundFx::EnemyShot);
                        }
                        lp.fire = LIMPET_FIRE_EVERY + rng.gen_range(0.0..LIMPET_FIRE_JITTER); // duck back + dwell
                    }
                    None => lp.fire = LIMPET_FIRE_EVERY, // nothing to shoot → reset
                    _ => {} // still sliding out to a clear lane
                }
            }
        } else {
            // TRANSITING toward the rock → fly to the hide-side rim point; exposed en route
            lp.angle = from_center.to_angle(); // stay synced so there's no snap when it arrives
            let approach = hc + Vec2::from_angle(hide_ang) * cling;
            lv.0 = (approach - lc).clamp_length_max(LIMPET_SPEED);
            lp.guard = None;
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
    mut state: ResMut<BossState>,
    mut enemies: Query<&mut Enemy>,
) {
    if !is_boss_wave(wave.level) || state.fought == wave.level {
        return;
    }
    state.fought = wave.level;
    if is_devourer_wave(wave.level) {
        // Boss 2: the devourer starts small in the upper arena and hunts free rocks to grow.
        commands.spawn((
            Devourer { hp: DEVOURER_HP, grow: 0.0, fed: 0, dying: 0.0, pulse: 0.0 },
            Transform::from_xyz(0.0, arena.half.y * 0.55, 0.0),
        ));
    } else {
        // Boss 1: the shield-shaman glides in from the top.
        commands.spawn((
            Boss {
                hp: BOSS_HP,
                rot: 0.0,
                pulse: 0.0,
                entered: false,
                charge: BOSS_CHARGE,
                fire: BOSS_FIRE_EVERY,
                capture: 0.4,
                dying: 0.0,
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

// Boss movement (glide in → bob near the top), charge-up (invulnerable), ship-contact
// kill, and death → big burst, release the shield, reward calm, advance the wave.
fn boss_update(
    time: Res<Time>,
    mut commands: Commands,
    arena: Res<Arena>,
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
            boss.dying -= dt;
            boss.rot += BOSS_SPIN * 2.5 * dt; // spins up as it comes apart
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
                    commands.spawn((
                        Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Chain },
                        Velocity(dir * PICKUP_DRIFT),
                        Transform::from_xyz(0.0, 0.0, 0.0),
                    ));
                }
                stats.warden = true; // achievement: defeated the Warden
                defeat_boss(&mut score, &mut wave, &mut banner);
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
        boss.rot += BOSS_SPIN * dt;
        let margin = BOSS_R + BOSS_ORBIT_R + 6.0;
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
            // ROAM: ease toward a slow wandering target (a lazy Lissajous across the upper arena)
            // so the boss isn't a stationary target — you have to keep repositioning to hit its
            // core, and its shield sweeps the field. Wide horizontal sweep, gentle vertical dip.
            let cx = (boss.pulse * 0.16).sin() * (h.x - margin) * 0.72;
            let cy = rest_y - (h.y * 0.15) * (0.5 - 0.5 * (boss.pulse * 0.11).cos());
            let target = Vec2::new(cx, cy);
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
    flags: Res<RunFlags>,
    dev: Res<Dev>,
    mut sfx: EventWriter<SoundFx>,
    ships: Query<(Entity, &Transform, &Ship), Without<Devourer>>,
    mut devourers: Query<(Entity, &mut Transform, &mut Devourer)>,
    rocks: Query<(Entity, &Transform, &Asteroid), (Without<Shielded>, Without<Devourer>, Without<Gold>)>, // never eats gold (would grant a false 1UP)
) {
    let dt = time.delta_secs();
    let h = arena.half;
    let mut rng = rand::thread_rng();
    let ship = ships.iter().next();
    for (de, mut tf, mut dv) in &mut devourers {
        dv.pulse += dt * 5.0;
        let p = tf.translation.truncate();
        let r = devourer_radius(dv.grow);

        // ── DYING: crackle, then a big blast → despawn → advance ──
        if dv.dying > 0.0 {
            dv.dying -= dt;
            for _ in 0..3 {
                let off = Vec2::from_angle(rng.gen_range(0.0..TAU)) * rng.gen_range(0.0..r);
                burst(&mut commands, p + off, devourer_color(), 3, 260.0, &mut rng);
            }
            if dv.dying <= 0.0 {
                burst(&mut commands, p, devourer_color(), 60, 500.0, &mut rng);
                burst(&mut commands, p, Color::srgb(6.0, 4.0, 4.0), 26, 320.0, &mut rng);
                commands.entity(de).despawn();
                // drop the mass-shot orb (the boss-2 reward, content wave 10)
                let pdir = Vec2::from_angle(rng.gen_range(0.0..TAU));
                commands.spawn((
                    Pickup { rot: 0.0, pulse: 0.0, life: PICKUP_LIFE, kind: PickupKind::Mass },
                    Velocity(pdir * PICKUP_DRIFT),
                    Transform::from_xyz(0.0, 0.0, 0.0),
                ));
                stats.glutton = true; // achievement: defeated the Glutton (= beat the arc)
                if !flags.powerup_used {
                    stats.no_powerups = true; // …and did it with no powerups
                }
                defeat_boss(&mut score, &mut wave, &mut banner);
            }

            continue;
        }
        if dv.hp <= 0 {
            dv.dying = BOSS_DEATH_SECS;
            burst(&mut commands, p, devourer_color(), 34, 340.0, &mut rng);
            continue;
        }

        // ── eat any rock within reach (grow + heal), and note the nearest to chase ──
        let mut nearest: Option<Vec2> = None;
        let mut nd2 = f32::MAX;
        for (re, rt, ra) in &rocks {
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
            for (re, rt, _) in &rocks {
                burst(&mut commands, rt.translation.truncate(), devourer_color(), 5, 240.0, &mut rng);
                commands.entity(re).despawn(); // wipe the field (gold isn't in `rocks`, so it's spared)
            }
            burst(&mut commands, p, Color::srgb(7.0, 3.0, 3.0), 90, 760.0, &mut rng); // shockwave
            burst(&mut commands, p, devourer_color(), 50, 520.0, &mut rng);
            sfx.write(SoundFx::Mine);
            // caught in the blast → dead (unless mid-respawn or invincible); escapable only by distance
            if run.respawn <= 0.0 {
                if let Some((se, st, sh)) = ship {
                    let sp = st.translation.truncate();
                    if !immune(sh, &dev) && p.distance(sp) < DEVOURER_BURST_R {
                        kill_ship(&mut commands, &mut run, &mut next, &mut sfx, se, sp, &mut rng);
                    }
                }
            }
            dv.grow = 0.0; // shrink back to starting size and gorge again
            continue;
        }

        // ── move toward the nearest rock, or hunt the ship when the field is clear ──
        let goal = nearest.or_else(|| ship.map(|(_, st, _)| st.translation.truncate()));
        if let Some(g) = goal {
            let dir = (g - p).normalize_or_zero();
            let np = p + dir * DEVOURER_SPEED * dt;
            tf.translation = Vec3::new(np.x.clamp(-h.x + r, h.x - r), np.y.clamp(-h.y + r, h.y - r), 0.0);
        }

        // ── contact kills the ship ──
        if run.respawn <= 0.0 {
            if let Some((se, st, sh)) = ship {
                let sp = st.translation.truncate();
                if !immune(sh, &dev) && p.distance(sp) < r + SHIP_R {
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
    ships: Query<&Transform, (With<Ship>, Without<Boss>, Without<Asteroid>)>,
    mut bosses: Query<(&Transform, &mut Boss)>,
    mut shielded: Query<(Entity, &mut Transform, &mut Velocity, &Asteroid, &mut Shielded), Without<Boss>>,
    free: Query<(Entity, &Transform, &Asteroid), (Without<Shielded>, Without<Boss>, Without<Thrown>)>,
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

        // hold: reel in / pin each shield rock to its rotating slot (mark used arms)
        let mut used = [false; BOSS_ARMS];
        for (_se, mut st, mut sv, _a, mut sh) in &mut shielded {
            if sh.slot < BOSS_ARMS {
                used[sh.slot] = true;
            }
            let ang = (sh.slot as f32 / BOSS_ARMS as f32) * TAU + boss.rot;
            let target = bp + Vec2::from_angle(ang) * BOSS_ORBIT_R;
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
        boss.fire -= dt;
        if boss.fire <= 0.0 {
            boss.fire = BOSS_FIRE_EVERY + rng.gen_range(0.0..BOSS_FIRE_JITTER);
            if let Some(sp) = ship {
                for (se, st, mut sv, a, sh) in &mut shielded {
                    if a.size == 1 {
                        let dir = (sp - st.translation.truncate()).normalize_or_zero();
                        if dir != Vec2::ZERO {
                            sv.0 = dir * BOSS_THROW_SPEED;
                            commands.entity(se).remove::<Shielded>();
                            commands.entity(se).insert(Thrown(2.0));
                            if sh.slot < BOSS_ARMS {
                                used[sh.slot] = false; // that arm is now free to refill
                            }
                        }
                        break;
                    }
                }
            }
        }

        // …THEN grab another rock into an empty arm, biggest first (better shield)
        boss.capture -= dt;
        if boss.capture <= 0.0 {
            boss.capture = BOSS_CAPTURE_EVERY;
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
    asteroids: Query<(Entity, &Transform, &Asteroid, Option<&Gold>, Option<&Explosive>), (Without<Mine>, Without<Shielded>)>,
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
        for (ae, at, ast, gold, explosive) in &asteroids {
            if dead.contains(&ae) {
                continue;
            }
            let ap = at.translation.truncate();
            let rr = asteroid_radius(ast.size) + CHAIN_R;
            if seg_dist2(ap, a, b) < rr * rr {
                dead.insert(ae);
                if explosive.is_some() {
                    commands.entity(ae).insert(Detonating { fuse: ORANGE_FUSE }); // the beam lights the orange
                    continue;
                }
                // chain beam shears dense rocks outright — the beam ignores hp, like a mine
                break_asteroid(&mut commands, &mut rng, &mut score, ae, ap, ast.size, 1.0, ast.dense, gold.is_some());
                sfx.write(SoundFx::Break(ast.size));
                if ast.dense {
                    stats.green += 1;
                } else {
                    stats.blue += 1;
                }
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
    mut flags: ResMut<RunFlags>,
    ships: Query<&Transform, With<Ship>>,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
    mut pickups: Query<(Entity, &Transform, &mut Velocity, &mut Pickup)>,
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
                    mass.active = true; // switch to it on grab; Q toggles back to the standard shot
                    mass_color()
                }
            };
            burst(&mut commands, p, col, 30, 300.0, &mut rng);
            commands.entity(pe).despawn();
        }
    }
}

// The black hole drags every nearby asteroid, enemy AND mine inward and consumes
// those that reach its core (the ship is immune — not in these queries).
fn black_hole_update(
    mut commands: Commands,
    time: Res<Time>,
    mut score: ResMut<Score>,
    mut holes: Query<(Entity, &Transform, &mut BlackHole)>,
    // boss-HELD rocks (Shielded) are exempt — you can't warp a boss's shield away; the
    // boss itself carries neither Asteroid nor Enemy, so it's exempt automatically.
    mut asteroids: Query<(Entity, &Transform, &mut Velocity, &Asteroid), Without<Shielded>>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity), (With<Enemy>, Without<Asteroid>)>,
    mut mines: Query<(Entity, &Transform, &mut Velocity), (With<Mine>, Without<Asteroid>, Without<Enemy>)>,
    mut limpets: Query<(Entity, &Transform, &mut Velocity), (With<Limpet>, Without<Asteroid>, Without<Enemy>, Without<Mine>)>,
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
                burst(&mut commands, mp, mine_color(), 16, 300.0, &mut rng);
                commands.entity(me).despawn();
            } else {
                mv.0 += warp_pull(mp, hp, pull_r, dt);
            }
        }
        // Limpets are pulled off their rocks and swallowed like everything else
        for (le, lt, mut lv) in &mut limpets {
            let lp2 = lt.translation.truncate();
            if lp2.distance(hp) < WARP_CONSUME_R + LIMPET_R {
                score.0 += LIMPET_SCORE;
                burst(&mut commands, lp2, limpet_color(), 18, 300.0, &mut rng);
                commands.entity(le).despawn();
            } else {
                lv.0 += warp_pull(lp2, hp, pull_r, dt);
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
        arena.half = Vec2::new(w.width() * 0.5, w.height() * 0.5);
    }
}

fn update_wave_text(wave: Res<Wave>, mut q: Query<&mut Text, With<WaveText>>) {
    let secs = wave.timer.max(0.0) as i32;
    for mut t in &mut q {
        t.0 = if is_boss_wave(wave.level) {
            format!("WAVE {}    BOSS", wave.level)
        } else {
            format!("WAVE {}    {}:{:02}", wave.level, secs / 60, secs % 60)
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

// Tick the HUD flash timers (pips/lives) set at their events.
fn hud_flash_tick(time: Res<Time>, mut flash: ResMut<HudFlash>) {
    let dt = time.delta_secs();
    flash.pips = (flash.pips - dt).max(0.0);
    flash.life = (flash.life - dt).max(0.0);
}

// The "MASS SHOT / STANDARD SHOT" label: shown on a toggle, held, then fades over its last stretch.
fn shot_mode_update(time: Res<Time>, mut flash: ResMut<ShotModeFlash>, mass: Res<MassShot>, mut q: Query<(&mut Text, &mut TextColor), With<ShotModeText>>) {
    if flash.0 > 0.0 {
        flash.0 -= time.delta_secs();
    }
    // Persistent once the mass shot is unlocked (there's a real choice then): a dim baseline that reads
    // at a glance, flaring bright right after a toggle. Hidden entirely before the unlock. Colour-coded
    // so the active mode is obvious — violet (player kit) for MASS, cool steel for STANDARD.
    let base: f32 = if mass.unlocked { 0.5 } else { 0.0 };
    let alpha = base.max((flash.0 / 0.3).clamp(0.0, 1.0));
    let rgb = if mass.active { Color::srgb(0.72, 0.28, 1.0) } else { Color::srgb(0.58, 0.72, 0.9) };
    for (mut text, mut color) in &mut q {
        if mass.unlocked {
            text.0 = if mass.active { "MASS SHOT" } else { "STANDARD SHOT" }.to_string();
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
    // warp + chain + state + hud-flash grouped into one tuple param to stay within Bevy's 16-param limit
    abilities: (Res<Warp>, Res<Chain>, Res<State<GameState>>, Res<HudFlash>),
    wf: Res<WarpField>,
    stars: Query<(&Star, &Transform)>,
    ships: Query<(&Ship, &Transform)>,
    asteroids: Query<(&Asteroid, &Transform, Option<&Gold>, Option<&Explosive>, Option<&Detonating>)>,
    bullets: Query<(&Bullet, &Transform)>,
    particles: Query<(&Particle, &Transform)>,
    holes: Query<(&BlackHole, &Transform)>,
    missiles: Query<&Transform, With<WarpMissile>>,
    mines_q: Query<(&Mine, &Transform)>,
    // grouped into one tuple param to stay within Bevy's 16-param system limit
    foes: (Query<(&Enemy, &Transform)>, Query<&Transform, With<EnemyBullet>>, Query<(&Limpet, &Transform)>),
) {
    let h = arena.half;
    let t = time.elapsed_secs();
    let (warp_res, chain) = (&abilities.0, &abilities.1);
    let show_run = run_active(abilities.2.get()); // grid + HUD icons only while a run is on
    let hud_flash = &abilities.3;
    // a rapid bright shimmer applied to pips/lives right after they refill / a life is gained
    let flick = |active: bool| if active { 1.1 + 0.8 * (t * 40.0).sin() } else { 1.0 };

    // stars (backmost). Subtle during a run so they never distract; on the menu they're a feature —
    // bigger, brighter, with a soft glow and diagonal sparkle rays on the brightest ones.
    let star = star_color();
    let menu = !show_run;
    for (s, st) in &stars {
        let tw = 0.35 + 0.65 * (t * 1.6 + s.phase).sin().max(0.0);
        let c = st.translation.truncate();
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
    // per-line electric flicker: two out-of-phase strobes (a crackle, not a smooth pulse), scaled by
    // the field. The elastic snapback makes `wamt` bounce, so the lines crackle as the hole collapses.
    let warp_flick = |k: f32| {
        if warping {
            let amp = 0.35 + 0.55 * wamt;
            (1.0 + amp * (0.7 * (t * 26.0 + k * 2.1).sin() + 0.5 * (t * 43.0 + k * 3.7).sin())).max(0.05)
        } else {
            1.0
        }
    };
    const SUBDIV: usize = 14;
    let mut i = 0;
    let mut x = -(h.x / GRID_CELL).floor() * GRID_CELL;
    while x <= h.x {
        let sh = 0.5 + 1.1 * (0.5 + 0.5 * (i as f32 * 0.7 + t * 1.2).sin());
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
    let mut y = -(h.y / GRID_CELL).floor() * GRID_CELL;
    while y <= h.y {
        let sh = 0.5 + 1.1 * (0.5 + 0.5 * (j as f32 * 0.7 + t * 1.2 + 1.7).sin());
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
    for (a, at, gold, explosive, det) in &asteroids {
        let c = at.translation.truncate();
        let rot = Vec2::from_angle(a.rot);
        // colour by type: a lit orange flashes white-hot as its fuse burns; a live orange pulses; gold
        // shimmers; green=dense; blue=standard.
        let col = if let Some(d) = det {
            let f = 1.0 - (d.fuse / ORANGE_FUSE).clamp(0.0, 1.0); // ramps up as it's about to blow
            dim(Color::srgb(8.0, 6.0, 3.5), 0.7 + 0.9 * f)
        } else if explosive.is_some() {
            dim(orange_color(), 0.75 + 0.25 * (t * 5.0).sin())
        } else if gold.is_some() {
            dim(gold_color(), 0.7 + 0.3 * (t * 6.0).sin())
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
        if a.dense {
            let frac = a.hp.max(1) as f32 / a.size.max(1) as f32; // full shell → shrinks to a small core
            gizmos.linestrip_2d(ring(0.35 + 0.3 * frac), col);
        }
        // orange + gold rocks get NO extra ring — a single outline like any rock; their pulsing
        // colour is what sets them apart, so broken debris looks the same "chunkiness" as normal
    }

    // mines — crimson diamonds; blink faster once armed (the ship is near)
    let mc = mine_color();
    for (mine, mt) in &mines_q {
        let c = mt.translation.truncate();
        if !mine.armed || ((t * 12.0) as i32) % 2 == 0 {
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
    // Limpets — a cyan crescent/hook that faces its exposed side (the arc you can hit); brighter and
    // wide open while transiting (guard None), a tighter shell while hidden behind a rock.
    let lcol = limpet_color();
    for (lp, lt) in &foes.2 {
        let c = lt.translation.truncate();
        let throb = 1.0 + 0.12 * (t * 7.0).sin();
        // shell arc centered on the exposed direction; a full ring when transiting/exposed
        let (face, span, bright) = match lp.guard {
            Some(g) if g != Vec2::ZERO => (g.to_angle(), 2.0, 1.0), // hidden: a ~115° open shell facing out
            _ => (0.0, std::f32::consts::PI, 1.5), // exposed: a brighter full outline
        };
        let seg = 12;
        let pts: Vec<Vec2> = (0..=seg)
            .map(|i| {
                let a = face - span + 2.0 * span * (i as f32 / seg as f32);
                c + Vec2::from_angle(a) * LIMPET_R * throb
            })
            .collect();
        gizmos.linestrip_2d(pts, dim(lcol, bright));
        gizmos.circle_2d(Isometry2d::from_translation(c), LIMPET_R * 0.42 * throb, dim(lcol, bright)); // core
        // gripping claws reaching INTO the rock (opposite the exposed face) — sells the tether
        if let Some(g) = lp.guard {
            let inward = (-g).to_angle();
            for k in -1..=1 {
                let a = inward + k as f32 * 0.5;
                gizmos.line_2d(c + Vec2::from_angle(a) * LIMPET_R * 0.5, c + Vec2::from_angle(a) * LIMPET_R * 1.6, dim(lcol, 0.85));
            }
        }
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
    for (b, bt) in &bullets {
        let c = bt.translation.truncate();
        let br = bullet_radius(b.mass);
        if b.mass {
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

    // ship — flame + hull (blinks while invulnerable)
    let sc = ship_color();
    for (s, st) in &ships {
        let c = st.translation.truncate();
        // DEV invincibility: a steady shield ring so it's obvious god-mode is on.
        // Drawn before the blink skip so it stays visible through respawn flicker.
        if dev.invincible {
            let pulse = 1.0 + 0.06 * (t * 4.0).sin();
            gizmos.circle_2d(Isometry2d::from_translation(c), SHIP_R * 2.2 * pulse, dim(sc, 0.6));
        }
        if s.invuln > 0.0 && (s.invuln * 12.0) as i32 % 2 == 0 {
            continue;
        }
        let rot = Vec2::from_angle(s.angle);
        if s.flame > 0.02 {
            let f = s.flame * (0.6 + 0.4 * (t * 40.0).sin().abs());
            let flame = [
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5, -5.0)),
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5 - 17.0 * f, 0.0)),
                c + rot.rotate(Vec2::new(-SHIP_R * 0.5, 5.0)),
            ];
            gizmos.linestrip_2d(flame, dim(flame_color(), f));
        }
        let hull = [
            Vec2::new(SHIP_R, 0.0),
            Vec2::new(-SHIP_R * 0.7, -SHIP_R * 0.7),
            Vec2::new(-SHIP_R * 0.4, 0.0),
            Vec2::new(-SHIP_R * 0.7, SHIP_R * 0.7),
            Vec2::new(SHIP_R, 0.0),
        ];
        let pts: Vec<Vec2> = hull.iter().map(|v| c + rot.rotate(*v)).collect();
        gizmos.linestrip_2d(pts, sc);
    }

    // lives HUD icons (top-right, under the "LIVES" label) — only while a run is on
    if show_run {
        let life_col = dim(sc, flick(hud_flash.life > 0.0)); // flickers briefly on a new life
        for k in 0..run.lives.max(0) {
            let p = Vec2::new(h.x - 32.0 - k as f32 * 24.0, h.y - 48.0);
            let icon = [
                p + Vec2::new(0.0, 9.0),
                p + Vec2::new(-7.0, -7.0),
                p + Vec2::new(0.0, -3.0),
                p + Vec2::new(7.0, -7.0),
                p + Vec2::new(0.0, 9.0),
            ];
            gizmos.linestrip_2d(icon, life_col);
        }
    }

    // warp charge pips (bottom-center): lit per available charge + a refill bar — run only.
    // `gap`/`py` are shared with the chain pips below, so they live outside the run gate.
    let gap = 22.0;
    let py = -h.y + 28.0;
    if show_run {
        let pip_lit = dim(warp, flick(hud_flash.pips > 0.0)); // flickers briefly when charges refill
        for k in 0..WARP_MAX_CHARGES {
            let px = (k as f32 - (WARP_MAX_CHARGES as f32 - 1.0) * 0.5) * gap;
            let col = if k < warp_res.charges { pip_lit } else { dim(warp, 0.14) };
            gizmos.circle_2d(Isometry2d::from_translation(Vec2::new(px, py)), 5.0, col);
        }
        if warp_res.cooldown > 0.0 {
            let prog = 1.0 - warp_res.cooldown / WARP_COOLDOWN;
            let w = gap * (WARP_MAX_CHARGES as f32 - 1.0);
            gizmos.line_2d(Vec2::new(-w * 0.5, py - 11.0), Vec2::new(-w * 0.5 + w * prog, py - 11.0), dim(warp, 0.7));
        }
    }

    // chain-shot charges (bottom-left) — shown only once the beam is unlocked. A little
    // bolt glyph + electric-violet pips + a refill bar toward the next charge; mirrors warp.
    if show_run && chain.unlocked {
        let cc = chain_color();
        let bx = -h.x + 30.0;
        let bolt = [
            Vec2::new(bx + 2.0, py + 8.0),
            Vec2::new(bx - 3.0, py + 1.0),
            Vec2::new(bx + 1.0, py + 1.0),
            Vec2::new(bx - 2.0, py - 8.0),
        ];
        gizmos.linestrip_2d(bolt.to_vec(), cc);
        let x0 = bx + 22.0;
        for k in 0..CHAIN_MAX_CHARGES {
            let px = x0 + k as f32 * gap;
            let col = if k < chain.charges { cc } else { dim(cc, 0.14) };
            gizmos.circle_2d(Isometry2d::from_translation(Vec2::new(px, py)), 5.0, col);
        }
        if chain.charges < CHAIN_MAX_CHARGES {
            let prog = 1.0 - chain.recharge / CHAIN_RECHARGE;
            let w = gap * (CHAIN_MAX_CHARGES as f32 - 1.0);
            gizmos.line_2d(Vec2::new(x0, py - 11.0), Vec2::new(x0 + w * prog, py - 11.0), dim(cc, 0.7));
        }
    }
}

// Boss rendering, split out of `render` (it was at the 16-param system limit): the
// background cameo telegraph, the magenta core (a jagged pulsing star, blinking while
// it charges), and the octopus arms — curved tapering tentacles to each shield rock.
// The shield rocks themselves draw as normal asteroids in `render`.
fn render_boss(
    mut gizmos: Gizmos,
    time: Res<Time>,
    arena: Res<Arena>,
    wave: Res<Wave>,
    bosses: Query<(&Boss, &Transform)>,
    shielded: Query<(&Transform, &Shielded)>,
    devourers: Query<(&Devourer, &Transform)>,
) {
    let h = arena.half;
    let t = time.elapsed_secs();
    let mc = boss_color();

    // ── the devourer (boss 2): a jagged red maw that swells as it feeds; a white-hot HP core ──
    for (dv, dt) in &devourers {
        let c = dt.translation.truncate();
        let scale = if dv.dying > 0.0 { (dv.dying / BOSS_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let r = devourer_radius(dv.grow) * scale;
        let throb = 1.0 + 0.06 * dv.pulse.sin();
        // overload telegraph: as it nears full it flashes white-hot (about to burst — get clear!)
        let charge = ((dv.grow - 0.7) / 0.3).clamp(0.0, 1.0);
        let flash = 0.5 + 0.5 * (dv.pulse * (4.0 + 9.0 * charge)).sin();
        let dc = if dv.dying <= 0.0 { mix(devourer_color(), Color::srgb(8.0, 7.5, 7.0), charge * flash) } else { devourer_color() };
        let body: Vec<Vec2> = (0..=18)
            .map(|k| {
                let a = k as f32 / 18.0 * TAU + dv.pulse * 0.2;
                let jag = 0.82 + 0.18 * (a * 3.0 + dv.pulse * 0.5).sin();
                c + Vec2::from_angle(a) * r * throb * jag
            })
            .collect();
        gizmos.linestrip_2d(body, dc);
        // HP core: brighter/bigger the more health it has (a read on how far you've whittled it)
        let hpf = (dv.hp as f32 / DEVOURER_HP as f32).clamp(0.2, 1.0);
        gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.42 * throb, dim(dc, 0.7 * hpf));
        gizmos.circle_2d(Isometry2d::from_translation(c), r * 0.18, Color::srgb(6.0, 4.0, 4.0));
        // HP bar (top-center) — tracks its heal-toward-max; hidden once dying
        if dv.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, dv.hp as f32 / DEVOURER_HP_MAX as f32, devourer_color());
        }
    }

    // cameo: the boss drifts by in the background in the run-up to its wave
    if !is_boss_wave(wave.level) && is_boss_wave(wave.level + 1) && wave.calm <= 0.0 && wave.timer <= BOSS_CAMEO_SECS {
        let prog = ((BOSS_CAMEO_SECS - wave.timer) / BOSS_CAMEO_SECS).clamp(0.0, 1.0);
        let c = Vec2::new(-h.x - 150.0 + (2.0 * h.x + 300.0) * prog, h.y * 0.45);
        let ghost: Vec<Vec2> = (0..=20)
            .map(|k| {
                let a = k as f32 / 20.0 * TAU + t * 0.3;
                let r = BOSS_R * 1.5 * if k % 2 == 0 { 1.0 } else { 0.72 };
                c + Vec2::from_angle(a) * r
            })
            .collect();
        gizmos.linestrip_2d(ghost, dim(mc, 0.22));
    }

    for (boss, bt) in &bosses {
        let c = bt.translation.truncate();
        // arms: a curved, tapering tentacle to each shield rock
        for (st, sh) in &shielded {
            let a = st.translation.truncate();
            let d = a - c;
            let dist = d.length().max(1.0);
            let perp = Vec2::new(-d.y, d.x) / dist;
            let curl = (boss.pulse * 1.4 + sh.slot as f32 * 1.7).sin() * dist * 0.22;
            let mid = c + d * 0.5 + perp * curl;
            let n = 9;
            let pts: Vec<Vec2> = (0..=n)
                .map(|i| {
                    let tt = i as f32 / n as f32;
                    let it = 1.0 - tt;
                    c * (it * it) + mid * (2.0 * it * tt) + a * (tt * tt) // quadratic bezier
                })
                .collect();
            gizmos.linestrip_2d(pts, dim(mc, 0.7));
        }
        // core: jagged pulsing star + glowing center. Blinks while charging (invuln);
        // while DYING it shrinks toward nothing and flickers as it comes apart.
        let throb = 1.0 + 0.1 * boss.pulse.sin();
        let scale = if boss.dying > 0.0 { (boss.dying / BOSS_DEATH_SECS).clamp(0.0, 1.0) } else { 1.0 };
        let blink = boss.charge > 0.0 || boss.dying > 0.0;
        if !blink || ((boss.pulse * 3.0) as i32) % 2 == 0 {
            let star: Vec<Vec2> = (0..=20)
                .map(|k| {
                    let a = k as f32 / 20.0 * TAU + boss.rot;
                    let r = BOSS_R * throb * scale * if k % 2 == 0 { 1.0 } else { 0.72 };
                    c + Vec2::from_angle(a) * r
                })
                .collect();
            gizmos.linestrip_2d(star, mc);
            gizmos.circle_2d(Isometry2d::from_translation(c), BOSS_R * 0.4 * throb * scale, mc);
        }
        // HP bar (top-center), hidden once it's dying (the fight's over)
        if boss.dying <= 0.0 {
            boss_hp_bar(&mut gizmos, h.y - 42.0, boss.hp as f32 / BOSS_HP as f32, mc);
        }
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
    // reward orb — a pulsing hexagon with a bright core, tinted for the weapon it grants
    for (pt, pk) in &pickups {
        let c = pt.translation.truncate();
        let throb = 1.0 + 0.14 * pk.pulse.sin();
        let col = match pk.kind {
            PickupKind::Chain => cc,
            PickupKind::Mass => mass_color(),
        };
        let hex: Vec<Vec2> = (0..=6)
            .map(|i| c + Vec2::from_angle(i as f32 / 6.0 * TAU + pk.rot) * PICKUP_R * throb)
            .collect();
        gizmos.linestrip_2d(hex, col);
        gizmos.circle_2d(Isometry2d::from_translation(c), PICKUP_R * 0.3 * throb, white);
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
        GameState::Menu | GameState::Achievements | GameState::Controls | GameState::Briefing | GameState::GameOver => {}
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
                row_gap: Val::Px(16.0),
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

#[derive(Resource)]
struct LogoImage(Handle<Image>);

// Decode the embedded logo. Keeps the CPU copy (RenderAssetUsages::default) so the window-icon
// system can read its RGBA bytes.
fn decode_logo() -> Image {
    Image::from_buffer(
        LOGO_PNG,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true, // colour image (sRGB)
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .expect("assets/logo.png is a valid PNG")
}

// Install the menu-masthead logo at BUILD time (like the font) so the initial OnEnter(Menu) can use it.
fn install_logo(app: &mut App) {
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(decode_logo());
    app.insert_resource(LogoImage(handle));
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

fn spawn_pause_ui(mut commands: Commands, font: Res<MenuFont>) {
    let root = overlay(&mut commands, PauseUi, 0.72);
    let f = &font.0;
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 54.0, title_color(), "PAUSED"));
        menu_button(p, f, MenuAction::Resume, "RESUME  (Esc)");
        menu_button(p, f, MenuAction::Quit, "QUIT TO MENU  (Q)");
    });
}

fn spawn_gameover_ui(mut commands: Commands, score: Res<Score>, hs: Res<HighScores>, font: Res<MenuFont>) {
    let root = overlay(&mut commands, GameOverUi, 0.72);
    let f = &font.0;
    let gold = Color::srgb(0.98, 0.85, 0.35);
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 62.0, Color::srgb(1.0, 0.3, 0.3), "GAME OVER"));
        p.spawn(text_f(f, 24.0, Color::srgb(0.85, 0.9, 1.2), &format!("SCORE   {}", score.0)));
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
    flags: &mut RunFlags,
    gold: &mut GoldRush,
) {
    run.lives = START_LIVES;
    run.respawn = 0.0;
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
    mut progress: (ResMut<BossState>, ResMut<Chain>, ResMut<MassShot>, ResMut<RunFlags>, ResMut<GoldRush>), // bundled (16-param limit)
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
    // Play: Enter/Space or the button
    if !(keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || actions.contains(&MenuAction::Play)) {
        return;
    }
    reset_run(&mut commands, &mut run, &mut score, &mut wave, &mut banner, &mut warp, &mut progress.0, &mut progress.1, &mut progress.2, &mut progress.3, &mut progress.4);
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
            padding: UiRect::axes(Val::Px(30.0), Val::Px(12.0)),
            margin: UiRect::all(Val::Px(7.0)),
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
        b.spawn(text_f(font, 24.0, Color::srgb(0.72, 0.82, 1.0), label));
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

fn spawn_menu_ui(mut commands: Commands, achieved: Res<Achievements>, intro: Res<TitleIntroPlayed>, hs: Res<HighScores>, logo: Res<LogoImage>, font: Res<MenuFont>) {
    spawn_frame(&mut commands, MenuUi); // behind the content (spawned first)
    let root = overlay(&mut commands, MenuUi, 0.25); // light — let the starfield show through
    let done = achieved.unlocked.iter().filter(|u| **u).count();
    let f = &font.0;
    // flicker the title on the FIRST show only; later returns start it already lit (past the warm-up)
    let title_age = if intro.0 { NEON_WARMUP } else { 0.0 };
    let best = hs.top[0];
    commands.entity(root).with_children(|p| {
        // logo masthead above the wordmark
        p.spawn((ImageNode::new(logo.0.clone()), Node { width: Val::Px(180.0), height: Val::Px(180.0), margin: UiRect::bottom(Val::Px(-18.0)), ..default() }));
        p.spawn((MenuTitle { age: title_age }, text_f(f, 82.0, title_color(), "VIOLET EDGE")));
        menu_button(p, f, MenuAction::Play, "PLAY");
        menu_button(p, f, MenuAction::Controls, "CONTROLS");
        menu_button(p, f, MenuAction::Briefing, "BRIEFING");
        menu_button(p, f, MenuAction::Achievements, &format!("ACHIEVEMENTS  ({done} / {})", ACHIEVEMENTS.len()));
        if best > 0 {
            p.spawn((text_f(f, 18.0, Color::srgb(0.72, 0.76, 0.95), &format!("BEST   {best}")), Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
        }
    });
}

// One row of a two-column reference table (left label | right text). Shared by the achievements and
// controls screens so they align identically and there's a single place to tune the layout.
fn table_row(p: &mut ChildSpawnerCommands, font: &Handle<Font>, left: &str, left_col: Color, left_w: f32, right: &str, right_col: Color) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(28.0),
        width: Val::Px(760.0),
        padding: UiRect::vertical(Val::Px(3.0)),
        ..default()
    })
    .with_children(|row| {
        row.spawn((text_f(font, 17.0, left_col, left), Node { width: Val::Px(left_w), ..default() }));
        row.spawn(text_f(font, 15.0, right_col, right));
    });
}

fn spawn_achievements_ui(mut commands: Commands, achieved: Res<Achievements>, font: Res<MenuFont>) {
    spawn_frame(&mut commands, AchievementsUi);
    let root = overlay(&mut commands, AchievementsUi, 0.5);
    let f = &font.0;
    commands.entity(root).with_children(|p| {
        p.spawn(text_f(f, 48.0, title_color(), "ACHIEVEMENTS")); // static — no neon warm-up here
        // two-column table: name | description (aligns cleanly, no separator glyph)
        for (i, &a) in ACHIEVEMENTS.iter().enumerate() {
            let (name, desc) = ach_meta(a);
            let (namecol, desccol) = if achieved.unlocked[i] {
                (ach_earned_color(), Color::srgb(0.78, 0.82, 0.95))
            } else {
                (Color::srgb(0.5, 0.52, 0.62), Color::srgb(0.38, 0.4, 0.5))
            };
            table_row(p, f, name, namecol, 330.0, desc, desccol);
        }
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
        p.spawn(text_f(f, 44.0, title_color(), "CONTROLS"));
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
            menu_button(row, f, MenuAction::SetInput(InputMethod::Auto), "AUTO");
            menu_button(row, f, MenuAction::SetInput(InputMethod::KeyboardMouse), "KB + MOUSE");
            menu_button(row, f, MenuAction::SetInput(InputMethod::Controller), "CONTROLLER");
        });
        p.spawn((InputLabel, text_f(f, 15.0, head, "")));
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(14.0), width: Val::Px(494.0), margin: UiRect::top(Val::Px(6.0)), ..default() }).with_children(|row| {
            row.spawn((text_f(f, 13.0, head, "ACTION"), Node { width: Val::Px(180.0), ..default() }));
            row.spawn((text_f(f, 13.0, head, "KEYBOARD / MOUSE"), Node { width: Val::Px(150.0), ..default() }));
            row.spawn(text_f(f, 13.0, head, "CONTROLLER"));
        });
        for &a in ACTIONS.iter() {
            p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(14.0), width: Val::Px(494.0), padding: UiRect::vertical(Val::Px(1.0)), ..default() }).with_children(|row| {
                row.spawn((text_f(f, 14.0, head, action_label(a)), Node { width: Val::Px(180.0), ..default() }));
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
        p.spawn(text_f(f, 17.0, flavor, "The Belt has turned hostile. The rocks choke the lanes,"));
        p.spawn(text_f(f, 17.0, flavor, "and something in the dark is steering them."));
        p.spawn(text_f(f, 17.0, flavor, "You hold the last violet-drive cutter. Cut the field. Hold the edge."));
        p.spawn((text_f(f, 22.0, title_color(), "OBJECTIVE"), Node { margin: UiRect::top(Val::Px(14.0)), ..default() }));
        for line in [
            "Survive each wave's timer to advance.",
            "See how far into the run you can push.",
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
            width: Val::Px(150.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.5)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BorderColor(Color::srgb(0.4, 0.3, 0.7)),
        BorderRadius::all(Val::Px(6.0)),
    ))
    .with_children(|c| {
        c.spawn(text_f(font, 15.0, Color::srgb(0.85, 0.88, 1.0), "—"));
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

// Load lifetime progress at startup and mark already-earned achievements as unlocked (so they
// don't re-toast on boot).
fn load_progress(mut stats: ResMut<Stats>, mut unlocked: ResMut<Achievements>) {
    if let Some(saved) = read_progress() {
        *stats = saved;
    }
    for (i, &a) in ACHIEVEMENTS.iter().enumerate() {
        unlocked.unlocked[i] = ach_met(a, &stats);
    }
}

// Poll the lifetime Stats; the first frame an achievement's condition is met, flip its flag, pop a
// toast, chime, and persist. Cheap — 7 checks a frame.
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
        if let Some(r) = root.iter().next() {
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
                    BackgroundColor(Color::srgba(0.10, 0.03, 0.18, 0.92)),
                ))
                .with_children(|t| {
                    t.spawn(text(15.0, Color::srgb(0.7, 0.85, 1.2), "ACHIEVEMENT UNLOCKED"));
                    t.spawn(text(22.0, mass_color(), name));
                });
            });
        }
        if let Some(b) = &bank {
            one_shot(&mut commands, b.achievement.clone(), 0.6);
        }
        save_progress(&stats);
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
    gold: Query<(), With<Gold>>,
    bank: Option<Res<SfxBank>>,
    root: Query<Entity, With<ToastRoot>>,
) {
    if !rush.active || !gold.is_empty() {
        return; // no hunt running, or gold pieces are still out there to clear
    }
    if !rush.forfeited && run.lives < LIFE_CAP {
        run.lives += 1;
        flash.life = HUD_FLASH_TIME; // flicker the life icons on the new life
        if let Some(r) = root.iter().next() {
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
                    BackgroundColor(Color::srgba(0.16, 0.11, 0.02, 0.92)),
                ))
                .with_children(|t| {
                    // UI colours must stay <= 1 (TextColor clamps per-channel), so a plain gold here
                    t.spawn(text(15.0, Color::srgb(0.7, 0.85, 1.2), "EXTRA LIFE"));
                    t.spawn(text(22.0, Color::srgb(0.95, 0.8, 0.35), "GOLD ROCK CLEARED"));
                });
            });
        }
        if let Some(b) = &bank {
            one_shot(&mut commands, b.life.clone(), 0.6);
        }
    }
    rush.active = false;
    rush.forfeited = false;
    // note: the cooldown to the next gold is armed at SPAWN time (in gold_spawn), measured from when
    // the rock appeared — so a slow hunt eats into the wait rather than adding to it.
}

// Lifetime progress persists to a tiny best-effort save file (six space-separated numbers). File
// I/O is compiled out of tests so the suite never touches the disk.
#[cfg(not(test))]
const SAVE_PATH: &str = "violet-edge.save";
#[cfg(not(test))]
fn read_progress() -> Option<Stats> {
    let text = std::fs::read_to_string(SAVE_PATH).ok()?;
    let n: Vec<&str> = text.split_whitespace().collect();
    if n.len() < 6 {
        return None;
    }
    Some(Stats {
        blue: n[0].parse().ok()?,
        green: n[1].parse().ok()?,
        enemies: n[2].parse().ok()?,
        warden: n[3] == "1",
        glutton: n[4] == "1",
        no_powerups: n[5] == "1",
    })
}
#[cfg(not(test))]
fn save_progress(s: &Stats) {
    let line = format!("{} {} {} {} {} {}", s.blue, s.green, s.enemies, s.warden as u8, s.glutton as u8, s.no_powerups as u8);
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
fn record_high_score(score: Res<Score>, mut hs: ResMut<HighScores>) {
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
    mut progress: (ResMut<BossState>, ResMut<Chain>, ResMut<MassShot>, ResMut<RunFlags>, ResMut<GoldRush>),
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
    reset_run(&mut commands, &mut run, &mut score, &mut wave, &mut banner, &mut warp, &mut progress.0, &mut progress.1, &mut progress.2, &mut progress.3, &mut progress.4);
    next.set(GameState::Playing); // field refills from the edges via top_up_asteroids
}

// ─────────────────────────────── music ────────────────────────────────
#[derive(Component)]
struct Music;

const MUSIC_VOLUME: f32 = 0.55;

// What the soundtrack should be playing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MusicCue {
    Main,    // the full-length main track (loops)
    Buildup, // the ~10 s riser in the run-up to a boss (one-shot)
    Boss,    // the boss track (loops)
    Silence, // the post-boss calm — a deliberate breather, no music
}

// The soundtrack director. Normal play loops the main track; the last 10 s before a boss play a
// buildup riser; boss waves loop the boss track; the post-boss calm is silent.
#[derive(Resource)]
struct MusicDirector {
    main: Handle<AudioSource>,
    boss: Handle<AudioSource>,
    buildup: Handle<AudioSource>,
    cue: Option<MusicCue>, // what's live (None = nothing spawned yet)
    muted: bool,
}

// Synthesize the tracks up front and install the director. The first cue is spawned by
// `music_director` on its first run.
fn start_music(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let main = sources.add(AudioSource { bytes: audio::main_track_wav().into() });
    let boss = sources.add(AudioSource { bytes: audio::boss_track_wav().into() });
    let buildup = sources.add(AudioSource { bytes: audio::boss_buildup_wav().into() });
    commands.insert_resource(MusicDirector { main, boss, buildup, cue: None, muted: false });
}

// Spawn a Music player. Loops for the main/boss tracks; one-shot (Despawn) for the buildup riser.
fn play_track(commands: &mut Commands, handle: Handle<AudioSource>, muted: bool, looping: bool) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: if looping { PlaybackMode::Loop } else { PlaybackMode::Despawn },
            volume: Volume::Linear(if muted { 0.0 } else { MUSIC_VOLUME }),
            ..default()
        },
        Music,
    ));
}

// Pick the right cue for the current moment and swap to it when it changes; `M` mutes.
fn music_director(
    input: Res<ActionState>,
    wave: Res<Wave>,
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

    let desired = if wave.calm > 0.0 {
        MusicCue::Silence // post-boss breather — let it be quiet, don't slam the track back on
    } else if is_boss_wave(wave.level) {
        MusicCue::Boss
    } else if is_boss_wave(wave.level + 1) && wave.timer <= BOSS_CAMEO_SECS {
        MusicCue::Buildup // last 10 s before the boss wave → riser leads in
    } else {
        MusicCue::Main
    };

    if dir.cue != Some(desired) {
        for e in &music {
            commands.entity(e).despawn(); // matches by marker, fires even before the sink exists
        }
        match desired {
            MusicCue::Silence => {}
            MusicCue::Main => {
                let h = dir.main.clone();
                play_track(&mut commands, h, dir.muted, true);
            }
            MusicCue::Boss => {
                let h = dir.boss.clone();
                play_track(&mut commands, h, dir.muted, true);
            }
            MusicCue::Buildup => {
                let h = dir.buildup.clone();
                play_track(&mut commands, h, dir.muted, false);
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
    achievement: Handle<AudioSource>,
    life: Handle<AudioSource>, // gold-rock 1UP jingle
    toggle: Handle<AudioSource>, // standard ↔ mass shot switch
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
        achievement: sources.add(AudioSource { bytes: audio::achievement_sfx_wav().into() }),
        life: sources.add(AudioSource { bytes: audio::life_sfx_wav().into() }),
        toggle: sources.add(AudioSource { bytes: audio::toggle_sfx_wav().into() }),
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
    let (mut fire, mut mine, mut death, mut eshot, mut edie, mut warp, mut toggle) =
        (false, false, false, false, false, false, false);
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
    if toggle {
        one_shot(&mut commands, bank.toggle.clone(), 0.4); // weapon-switch click
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

// DEV: F2 skips to the next wave — kills the boss on a boss wave (so it advances via
// the normal death path), otherwise just expires the timer. Debug builds only.
#[cfg(debug_assertions)]
fn dev_wave_skip(keys: Res<ButtonInput<KeyCode>>, mut wave: ResMut<Wave>, mut bosses: Query<&mut Boss>) {
    if keys.just_pressed(KeyCode::F2) {
        if let Some(mut b) = bosses.iter_mut().next() {
            b.hp = 0;
        } else {
            wave.timer = 0.0;
        }
        info!("DEV skip → next wave");
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
        .insert_resource(Run { lives: START_LIVES, respawn: 0.0 })
        .insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 })
        .insert_resource(WaveBanner { timer: WAVE_BANNER_SECS }) // flash "WAVE 1" at start
        .insert_resource(SpawnClock::default())
        .insert_resource(MineClock::default())
        .insert_resource(EnemyClock::default())
        .insert_resource(LimpetClock::default())
        .insert_resource(Warp { charges: WARP_MAX_CHARGES, cooldown: 0.0 })
        .insert_resource(WarpField::default())
        .insert_resource(Arena { half: Vec2::new(640.0, 400.0) })
        .insert_resource(Dev::default())
        .insert_resource(BossState::default())
        .insert_resource(Chain::default())
        .insert_resource(MassShot::default())
        .insert_resource(Stats::default())
        .insert_resource(Achievements::default())
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
        .add_event::<SoundFx>()
        .add_event::<MenuClick>()
        .init_state::<GameState>()
        .add_systems(Startup, (setup, spawn_hud, spawn_toast_root, load_progress, load_high_scores, start_music, start_sfx, set_window_icon))
        // always: keep the arena sized, handle pause input, refresh the HUD text
        .add_systems(Update, (update_arena, pause_toggle, update_wave_text, update_score_text, wave_banner_update).chain())
        // always: watch for achievement unlocks + age out toasts + hide the HUD off-run + menu buttons
        .add_systems(Update, (achievements, toast_update, hud_visibility, button_shimmer, button_click, hud_flash_tick, shot_mode_update))
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
                    warp_missile_update,
                    black_hole_update,
                    update_warp_field,
                    asteroid_collisions,
                )
                    .chain(),
                (
                    ship_death,
                    mine_update,
                    enemy_update,
                    enemy_bullets,
                    limpet_update,
                    boss_director,
                    boss_update,
                    devourer_update,
                    boss_shield,
                    shield_deflect,
                    chain_update,
                    pickup_update,
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
                    top_up_asteroids,
                    top_up_mines,
                    top_up_enemies,
                    top_up_limpets,
                    clear_calm_field,
                    gold_spawn,
                    gold_rush_update,
                    detonate,
                )
                    .chain(),
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(Update, (music_director, play_sfx))
        .add_systems(Update, menu_start.run_if(in_state(GameState::Menu)))
        .add_systems(
            Update,
            submenu_back.run_if(in_state(GameState::Achievements).or(in_state(GameState::Briefing))),
        )
        .add_systems(Update, gameover_restart.run_if(in_state(GameState::GameOver)))
        .add_systems(OnEnter(GameState::Playing), disarm_fire)
        .add_systems(OnEnter(GameState::Menu), (clear_field, spawn_menu_ui))
        .add_systems(OnExit(GameState::Menu), (despawn_menu_ui, mark_title_intro_played))
        .add_systems(OnEnter(GameState::Achievements), spawn_achievements_ui)
        .add_systems(OnExit(GameState::Achievements), despawn_achievements_ui)
        .add_systems(OnEnter(GameState::Controls), spawn_controls_ui)
        .add_systems(OnExit(GameState::Controls), despawn_controls_ui)
        .add_systems(Update, (controls_input, rebind_slot_click, rebind_capture, controls_display).run_if(in_state(GameState::Controls)))
        .add_systems(OnEnter(GameState::Briefing), spawn_briefing_ui)
        .add_systems(OnExit(GameState::Briefing), despawn_briefing_ui)
        .add_systems(OnEnter(GameState::Paused), spawn_pause_ui)
        .add_systems(OnExit(GameState::Paused), despawn_pause_ui)
        .add_systems(OnEnter(GameState::GameOver), (record_high_score, spawn_gameover_ui).chain())
        .add_systems(OnExit(GameState::GameOver), despawn_gameover_ui);
    // dev-only tools (F1 invincibility, F2 wave-skip); compiled out of release builds
    #[cfg(debug_assertions)]
    app.add_systems(Update, (dev_toggle, dev_wave_skip, dev_spawn_orange));
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
        app.insert_resource(ShotModeFlash::default());
        app.insert_resource(FireArmed(true)); // mid-run: the gun is armed
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
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
        app.insert_resource(ShotModeFlash::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(FireArmed(false)); // just entered Playing (disarm_fire ran)
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
        // a dense size-2 rock = 2 hp: the first hit only cracks it
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: true, hp: 2 },
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
        // a size-2 rock with a bullet sitting on it
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
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
        assert_eq!(chunks.len(), 2, "a size-2 rock splits into two chunks");
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
        app.insert_resource(Run { lives: 1, respawn: 0.0 }); // below the cap, so a life can be restored
        // no Gold entities remain → the player cleared the whole lineage
        app.add_systems(Update, gold_rush_update);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, 2, "clearing the whole gold lineage restores +1 life");
        assert!(!app.world().resource::<GoldRush>().active, "the hunt resets after granting (grants once)");
    }

    #[test]
    fn gold_lives_are_capped() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        app.insert_resource(HudFlash::default());
        app.insert_resource(Run { lives: LIFE_CAP, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 1, respawn: 0.0 }); // below the cap, so only the forfeit blocks it
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
    fn a_gold_rock_breaks_into_two_gold_chunks_same_as_normal() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a size-2 GOLD rock with a bullet on it
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
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
        let total = app.world_mut().query::<&Asteroid>().iter(app.world()).count();
        let gold = app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count();
        assert_eq!(total, 2, "a size-2 rock breaks into exactly two chunks — gold is no different");
        assert_eq!(gold, 2, "both chunks stay gold, so the whole lineage must still be cleared");
    }

    #[test]
    fn gold_spawn_drifts_one_in_when_the_countdown_elapses() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(GoldRush { active: false, forfeited: false, cooldown: 0.0 }); // due now
        app.add_systems(Update, gold_spawn);
        app.update();
        assert_eq!(app.world_mut().query_filtered::<(), With<Gold>>().iter(app.world()).count(), 1, "the countdown elapsing spawns exactly one gold rock");
        let rush = app.world().resource::<GoldRush>();
        assert!(rush.active, "spawning starts the hunt");
        assert!(rush.cooldown >= GOLD_MIN_GAP, "a long gap to the next gold is armed at spawn (no back-to-back)");
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(NextState::<GameState>::default());
        // the lit orange at origin (fuse already elapsed → blows this update)
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Explosive,
            Detonating { fuse: 0.0 },
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

    // Fire one bullet at a hidden Limpet (guard = exposed dir +X) from a given side; return its HP after.
    #[cfg(test)]
    fn limpet_hp_after_shot_from(bullet_x: f32) -> i32 {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        let limpet = app
            .world_mut()
            .spawn((Limpet { hp: 2, fire: 1.0, host: None, angle: 0.0, guard: Some(Vec2::X) }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.world_mut()
            .spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(bullet_x, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        app.world().entity(limpet).get::<Limpet>().unwrap().hp
    }

    #[test]
    fn a_flank_shot_hits_the_limpet_but_the_host_side_is_blocked() {
        assert_eq!(limpet_hp_after_shot_from(10.0), 1, "a shot from the exposed side (guard) damages the limpet");
        assert_eq!(limpet_hp_after_shot_from(-10.0), 2, "a shot from the host-rock side is blocked — no damage");
    }

    #[test]
    fn an_exposed_limpet_dies_from_a_direct_hit() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(Score(0));
        // guard None = transiting/exposed, hp 1
        app.world_mut()
            .spawn((Limpet { hp: 1, fire: 1.0, host: None, angle: 0.0, guard: None }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut()
            .spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        assert_eq!(app.world_mut().query::<&Limpet>().iter(app.world()).count(), 0, "an exposed limpet dies from a direct hit");
        assert_eq!(app.world().resource::<Score>().0, LIMPET_SCORE, "and awards its score");
    }

    #[test]
    fn a_limpet_rehosts_instead_of_dying_when_its_rock_breaks() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        let big = |x: f32| (Asteroid { size: 3, verts: vec![Vec2::X * 88.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(x, 0.0, 0.0));
        let rock1 = app.world_mut().spawn(big(200.0)).id();
        let rock2 = app.world_mut().spawn(big(-200.0)).id();
        app.world_mut()
            .spawn((Limpet { hp: 2, fire: 1.0, host: Some(rock1), angle: 0.0, guard: Some(Vec2::X) }, Velocity(Vec2::ZERO), Transform::from_xyz(200.0, 0.0, 0.0)));
        app.add_systems(Update, limpet_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Limpet>().iter(app.world()).next().unwrap().host, Some(rock1), "still tethered to its host");
        // destroy the host rock
        app.world_mut().entity_mut(rock1).despawn();
        app.update();
        assert_eq!(app.world_mut().query::<&Limpet>().iter(app.world()).count(), 1, "the limpet does NOT die when its host is destroyed");
        assert_eq!(app.world_mut().query::<&Limpet>().iter(app.world()).next().unwrap().host, Some(rock2), "it re-tethers to another large rock");
    }

    #[test]
    fn a_limpet_glues_onto_its_host_rim() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        let rr = asteroid_radius(3);
        let rock = app
            .world_mut()
            .spawn((Asteroid { size: 3, verts: vec![Vec2::X * rr], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        // starts just off the rim → within tether range
        app.world_mut()
            .spawn((Limpet { hp: 2, fire: 1.0, host: Some(rock), angle: 0.0, guard: None }, Velocity(Vec2::ZERO), Transform::from_xyz(rr + 10.0, 0.0, 0.0)));
        app.add_systems(Update, limpet_update);
        app.update();
        let (lt, lp) = app.world_mut().query::<(&Transform, &Limpet)>().iter(app.world()).next().unwrap();
        assert!(lp.guard.is_some(), "it raises its guard once tethered to a host");
        let d = lt.translation.truncate().length(); // distance from the rock centre (at origin)
        assert!((d - (rr - LIMPET_R * 0.35)).abs() < 1.0, "it snaps rigidly onto the rim (cling radius), not floating off — got {d}");
    }

    #[test]
    fn a_limpet_pops_out_to_fire_a_clear_lane() {
        fn bullets_after(fire: f32) -> usize {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.add_event::<SoundFx>();
            let rr = asteroid_radius(3);
            let rock = app
                .world_mut()
                .spawn((Asteroid { size: 3, verts: vec![Vec2::X * rr], rot: 0.0, spin: 0.0, dense: false, hp: 1 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)))
                .id();
            app.world_mut()
                .spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(300.0, 0.0, 0.0)));
            // tethered on the ship-side (+x) rim — i.e. already popped out, with a clear lane to the ship
            app.world_mut()
                .spawn((Limpet { hp: 1, fire, host: Some(rock), angle: 0.0, guard: Some(Vec2::X) }, Velocity(Vec2::ZERO), Transform::from_xyz(rr - LIMPET_R * 0.35, 0.0, 0.0)));
            app.add_systems(Update, limpet_update);
            app.update();
            app.world_mut().query::<&EnemyBullet>().iter(app.world()).count()
        }
        assert_eq!(bullets_after(-0.1), 1, "popped out (timer elapsed) on a clear near-side lane → it fires");
        assert_eq!(bullets_after(1.0), 0, "still hiding (timer not elapsed) → it holds fire, never shooting through the rock");
    }

    #[test]
    fn ship_dies_on_asteroid_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default()); // ship_death needs this resource
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
    fn rock_reeling_in_does_not_kill_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 1, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 0, respawn: f32::EPSILON });
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
    fn wave_rock_mix_matches_the_authored_content() {
        fn sample(level: i32, n: usize, rng: &mut rand::rngs::ThreadRng) -> (i32, i32, i32) {
            let (mut blue, mut green, mut orange) = (0, 0, 0);
            for _ in 0..n {
                match roll_rock_kind(level, rng) {
                    RockKind::Blue => blue += 1,
                    RockKind::Green => green += 1,
                    RockKind::Orange => orange += 1,
                }
            }
            (blue, green, orange)
        }
        let mut rng = rand::thread_rng();
        // wave 14 is the ALL-orange danger wave
        let (b, g, o) = sample(14, 200, &mut rng);
        assert_eq!((b, g, o), (0, 0, 200), "wave 14 is nothing but orange");
        // wave 15 (boss) is green-only — no orange
        let (b, g, o) = sample(15, 200, &mut rng);
        assert_eq!((b, g, o), (0, 200, 0), "wave 15 is green-only");
        // wave 11 "green + orange": every rock is one or the other, never plain blue
        let (b, g, o) = sample(11, 400, &mut rng);
        assert_eq!(b, 0, "wave 11 has no plain blue rocks");
        assert!(g > 0 && o > 0, "wave 11 mixes green and orange, got green={g} orange={o}");
        // wave 12 "mob + orange": orange over plain blue, never green
        let (b, g, o) = sample(12, 400, &mut rng);
        assert_eq!(g, 0, "wave 12 has no green rocks");
        assert!(b > 0 && o > 0, "wave 12 mixes blue and orange, got blue={b} orange={o}");
        // the devourer wave (10) stays plain blue food so it can be starved
        let (_b, g, o) = sample(10, 200, &mut rng);
        assert_eq!((g, o), (0, 0), "the devourer wave is plain blue food");
    }

    #[test]
    fn mine_target_gates_and_caps() {
        assert_eq!(mine_target(1, 10), 0, "no mines before wave 2");
        assert_eq!(mine_target(2, 10), 2, "wave 2: (2-2+1)*2 = 2, under the 50%-of-10 cap");
        assert_eq!(mine_target(5, 4), 2, "capped at 50% of 4 asteroids");
    }

    #[test]
    fn armed_mine_kills_ship_on_contact() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        assert_eq!(rocks, 2, "the mine should shatter the size-2 rock into two size-1 chunks, got {rocks}");
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
        assert_eq!(enemy_target(11, 100), 0, "waves 11-15 run no old-lobber mobs (the Limpet covers 12-13)");
        assert_eq!(enemy_target(13, 100), 0, "wave 13's mob is the Limpet, not the lobber");
        assert_eq!(enemy_target(16, 100), 0, "loop: wave 16 = content 1, no mobs");
        assert_eq!(enemy_target(18, 100), 2, "loop: wave 18 = content 3 → 2");
    }

    #[test]
    fn content_wave_loops_and_picks_boss_type() {
        assert_eq!(content_wave(1), 1);
        assert_eq!(content_wave(10), 10);
        assert_eq!(content_wave(15), 15, "waves 1-15 are their own content slots");
        assert_eq!(content_wave(16), 1, "wave 16 loops back to content 1");
        assert_eq!(content_wave(20), 5, "wave 20 = content 5 (a boss wave again)");
        assert_eq!(content_wave(25), 10);
        assert!(is_devourer_wave(10) && is_devourer_wave(25), "content-10 waves are the devourer");
        assert!(!is_devourer_wave(5) && !is_devourer_wave(15) && !is_devourer_wave(20), "content 5 & 15 are other bosses");
        assert!(is_boss_wave(5) && is_boss_wave(10) && is_boss_wave(15) && !is_boss_wave(6));
    }

    #[test]
    fn devourer_wave_spawns_the_second_boss() {
        let mut app = App::new();
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
    fn devourer_eats_a_rock_and_grows() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Run { lives: 3, respawn: 1.0 }); // respawning → skip ship-contact
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 10, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        let dvr = app
            .world_mut()
            .spawn((Devourer { hp: DEVOURER_HP - 20, grow: 0.0, fed: 0, dying: 0.0, pulse: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.world_mut().spawn((
            Asteroid { size: 2, verts: vec![Vec2::X * 40.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
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
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 10, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        // fully gorged (grow == 1.0), still alive → it should OVERLOAD this frame
        app.world_mut().spawn((Devourer { hp: 50, grow: 1.0, fed: 20, dying: 0.0, pulse: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        // a ship within the burst reach but OUTSIDE contact range → the burst is what kills it
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(300.0, 0.0, 0.0)));
        // field rocks (out of eating reach) → wiped by the burst
        for i in 0..5 {
            app.world_mut().spawn((
                Asteroid { size: 1, verts: vec![Vec2::X * 22.0], rot: 0.0, spin: 0.0, dense: false, hp: 1 },
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
        app.world_mut().spawn((Devourer { hp: DEVOURER_HP, grow: 0.5, fed: 0, dying: 0.0, pulse: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
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
    fn mass_shot_one_shots_a_dense_rock() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        // a dense size-3 rock has hp 3 — a standard shot only chips it; a mass shot (power 3) breaks it
        app.world_mut().spawn((
            Asteroid { size: 3, verts: vec![Vec2::X * 65.0], rot: 0.0, spin: 0.0, dense: true, hp: 3 },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: true }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let big = app.world_mut().query::<&Asteroid>().iter(app.world()).filter(|a| a.size == 3).count();
        assert_eq!(big, 0, "a mass shot cracks a dense rock in one hit (power 3 vs hp 3)");
    }

    #[test]
    fn menu_start_resets_and_spawns_a_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<MenuClick>();
        app.insert_resource(NextState::<GameState>::default());
        // stale end-of-run state that Start must wipe
        app.insert_resource(Run { lives: 0, respawn: 5.0 });
        app.insert_resource(Score(999));
        app.insert_resource(Wave { level: 7, timer: 1.0, calm: 3.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Warp { charges: 0, cooldown: 9.0 });
        app.insert_resource(BossState { fought: 5 });
        app.insert_resource(Chain { unlocked: true, charges: 3, recharge: 0.0, cooldown: 0.0 });
        app.insert_resource(MassShot { unlocked: true, active: true });
        app.insert_resource(RunFlags { powerup_used: true });
        app.insert_resource(GoldRush { active: true, forfeited: false, cooldown: 0.0 });
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Enter);
        app.insert_resource(input);
        app.add_systems(Update, menu_start);
        app.update();
        assert_eq!(app.world().resource::<Run>().lives, START_LIVES, "Start resets lives");
        assert_eq!(app.world().resource::<Score>().0, 0, "Start resets score");
        assert_eq!(app.world().resource::<Wave>().level, 1, "Start resets to wave 1");
        assert!(!app.world().resource::<Chain>().unlocked, "Start relocks the chain shot");
        assert!(!app.world().resource::<MassShot>().unlocked, "Start relocks the mass shot");
        assert!(!app.world().resource::<GoldRush>().active, "Start clears any stale gold hunt");
        assert_eq!(app.world_mut().query::<&Ship>().iter(app.world()).count(), 1, "a fresh ship spawns");
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
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
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
    fn warp_consumes_limpet() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Score(0));
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((BlackHole { life: 1.0, spin: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((
            Limpet { hp: 1, fire: 1.0, host: None, angle: 0.0, guard: Some(Vec2::X) },
            Velocity(Vec2::ZERO),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        app.add_systems(Update, black_hole_update);
        app.update();
        assert_eq!(app.world_mut().query::<&Limpet>().iter(app.world()).count(), 0, "a limpet at the core is consumed by the warp");
        assert_eq!(app.world().resource::<Score>().0, LIMPET_SCORE, "consuming a limpet scores");
    }

    #[test]
    fn lingering_enemy_flees_and_despawns() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
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
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(NextState::<GameState>::default());
        app.insert_resource(Run { lives: 3, respawn: 0.0 });
        app.insert_resource(Score(0));
        app.insert_resource(Wave { level: 5, timer: 0.0, calm: 0.0 });
        app.insert_resource(WaveBanner::default());
        app.insert_resource(Dev::default());
        app.insert_resource(Chain::default());
        app.insert_resource(MassShot::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: 0, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 1.0, capture: 1.0, dying: 0.0 }, Transform::from_xyz(0.0, 200.0, 0.0)));
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
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 0.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
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
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 5.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut().spawn((Bullet { life: 1.0, trail: Vec::new(), mass: false }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.add_systems(Update, collisions);
        app.update();
        let hp = app.world_mut().query::<&Boss>().iter(app.world()).next().unwrap().hp;
        assert_eq!(hp, BOSS_HP - 1, "a bullet through a gap chips the core");
    }

    #[test]
    fn boss_throws_a_smallest_shield_rock_at_the_ship() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Ship { angle: 0.0, cooldown: 0.0, invuln: 0.0, flame: 0.0 }, Velocity(Vec2::ZERO), Transform::from_xyz(0.0, -200.0, 0.0)));
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 0.0, capture: 5.0, dying: 0.0 }, Transform::from_xyz(0.0, 200.0, 0.0)));
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
        let v = app.world().entity(rock).get::<Velocity>().unwrap().0;
        assert!(v.length() > 1.0 && v.y < 0.0, "flung toward the ship (which is below it)");
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
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Arena { half: Vec2::new(640.0, 400.0) });
        app.world_mut().spawn((Boss { hp: BOSS_HP, rot: 0.0, pulse: 0.0, entered: true, charge: 0.0, fire: 5.0, capture: 0.0, dying: 0.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
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
        app.add_plugins(MinimalPlugins);
        app.insert_resource(ActionState::default());
        app.insert_resource(Wave { level: 1, timer: WAVE_SECS, calm: 0.0 });
        app.insert_resource(MusicDirector {
            main: Handle::default(),
            boss: Handle::default(),
            buildup: Handle::default(),
            cue: None,
            muted: false,
        });
        app.add_systems(Update, music_director);

        // normal play → the main track
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main), "normal play uses the main track");
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

        // calm over → back to the main track
        app.world_mut().resource_mut::<Wave>().calm = 0.0;
        app.update();
        assert_eq!(app.world().resource::<MusicDirector>().cue, Some(MusicCue::Main), "the main track resumes after the calm");
    }

    #[test]
    fn right_click_fires_a_chain_beam() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_event::<SoundFx>();
        app.insert_resource(Stats::default());
        app.insert_resource(RunFlags::default());
        app.insert_resource(Chain { unlocked: true, charges: 3, recharge: CHAIN_RECHARGE, cooldown: 0.0 });
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
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 2, "the beam mows the size-2 rock into two size-1 chunks");
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
        assert_eq!(rocks, 2, "the blast should shatter the size-2 rock into two size-1 chunks, got {rocks}");
        let mines = app.world_mut().query::<&Mine>().iter(app.world()).count();
        assert_eq!(mines, 0, "the mine detonates and despawns");
    }
}
