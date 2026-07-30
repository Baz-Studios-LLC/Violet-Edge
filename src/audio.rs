//! Procedural audio: every SOUND EFFECT in the game, plus the boss BUILDUP riser.
//!
//! Everything here is plain `std` math (no Bevy types) rendered into 16-bit mono WAV byte
//! buffers at startup — no asset files, nothing to download — so it stays testable and
//! decoupled from the engine.
//!
//! **The MUSIC is no longer synthesized** (2026-07-30): main / boss / game-over ship as PRODUCED
//! mp3s, generated in Antigravity using the old synthesized score as the style reference, embedded
//! as `MAIN_MP3` / `BOSS_MP3` / `GAMEOVER_MP3` in main.rs (see DESIGN.md "PRODUCED music"). The
//! procedural score that used to live here — a 128 BPM A-minor club track with a six-tier
//! corruption system — was DELETED rather than left dead; `git log` has it if it's ever wanted
//! back. The buildup riser stayed because nothing has replaced it yet.

use std::f32::consts::TAU;

const SR: f32 = 44_100.0; // sample rate

// --- deterministic value noise (seeded by sample index so the loop matches itself) ---
fn noise(i: usize) -> f32 {
    let mut x = (i as u32).wrapping_mul(2_654_435_761).wrapping_add(1_013_904_223);
    x ^= x >> 13;
    x = x.wrapping_mul(1_274_126_177);
    x ^= x >> 16;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

// --- one-shot voices: `t` is seconds since the hit was triggered ---

// Punchy kick: a sine whose pitch drops 130 → 48 Hz, with a fast amplitude decay
// and a touch of saturation so it thumps.
fn kick(t: f32) -> f32 {
    let env = (-t * 9.0).exp();
    let phase = TAU * (48.0 * t + (82.0 / 30.0) * (1.0 - (-30.0 * t).exp()));
    (phase.sin() * env * 1.3).tanh()
}

// --- oscillator shapes (phase `ph` in cycles) ---
fn saw(ph: f32) -> f32 {
    2.0 * (ph - (ph + 0.5).floor())
}
fn square(ph: f32) -> f32 {
    if ph - ph.floor() < 0.5 {
        1.0
    } else {
        -1.0
    }
}
// The open VORTEX — the black hole's own voice for its life. v2: the first cut pulsed the noise
// with a 2-9 Hz tremolo and read as "a dog sniffing" (user) — so NO rhythmic pulsing at all now.
// A CONTINUOUS suction: filtered noise whose cutoff sweeps DOWN (a deepening roar), a falling air
// whistle, a low drone — one unbroken inhale, collapsing into the deep thump as the hole shuts.
// Rendered to match WARP_HOLE_LIFE (2.6s) + a short tail, so one shot covers the hole exactly.
pub fn vortex_sfx_wav() -> Vec<u8> {
    let mut lp = 0.0f32;
    render_sfx(2.9, move |t, i| {
        let p = (t / 2.6).min(1.0);
        let swell = (t / 0.35).min(1.0) * (1.0 - ((t - 2.35) / 0.55).clamp(0.0, 1.0));
        // suction roar: one-pole lowpassed noise, cutoff sweeping 2800 → 350 Hz (deepening, never pulsing)
        let fc = 2800.0 * (350.0f32 / 2800.0).powf(p);
        lp += (1.0 - (-TAU * fc / SR).exp()) * (noise(i) - lp);
        let roar = lp * 0.55 * swell;
        // falling air whistle — the "rushing in" read, fading as the roar takes over
        let wf = 1400.0 * (300.0f32 / 1400.0).powf(p);
        let whistle = (TAU * wf * t).sin() * 0.06 * swell * (1.0 - p * 0.6);
        // low drone deepening 90 → 50 Hz under it all
        let drone = (TAU * (90.0 - 40.0 * p) * t).sin() * 0.3 * swell;
        // the collapse: a fast swell into a deep falling thump right as the hole closes
        let coll = if t > 2.35 {
            let k = ((t - 2.35) / 0.25).min(1.0);
            (TAU * (48.0 - 20.0 * k) * t).sin() * k * 0.8 * (1.0 - ((t - 2.6) / 0.3).clamp(0.0, 1.0))
        } else {
            0.0
        };
        roar + whistle + drone + coll
    })
}

// Add a voice into the buffer starting at `start`, wrapping past the end so the
// hit's tail bleeds into the loop start (seamless looping).
fn add_voice(buf: &mut [f32], start: usize, dur: f32, gain: f32, mut voice: impl FnMut(f32, usize) -> f32) {
    let len = buf.len();
    let count = (dur * SR) as usize;
    let fade = (0.004 * SR) as usize; // 4 ms release so a truncated tail never clicks
    for i in 0..count {
        let idx = (start + i) % len;
        let rel = if i + fade > count { (count - i) as f32 / fade as f32 } else { 1.0 };
        buf[idx] += voice(i as f32 / SR, i) * gain * rel;
    }
}

// Normalize → gentle saturation → 16-bit PCM WAV. Shared master stage for both tracks.
fn master(buf: &[f32]) -> Vec<u8> {
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.9 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| ((v * norm).tanh() * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// A ~10 s tension RISER played in the run-up to a boss wave, so the boss doesn't slam in cold:
/// a low A drone that swells, a noise sweep whose cutoff climbs, and heartbeat kicks that speed
/// up and get louder. Crescendos into the boss loop (one-shot, not faded).
pub fn boss_buildup_wav() -> Vec<u8> {
    let dur = 10.0;
    let n = (dur * SR) as usize;
    let mut buf = vec![0f32; n];

    // per-sample bed: a swelling low drone + a rising-cutoff noise sweep
    let mut lp = 0.0f32;
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let p = t / dur; // 0..1
        let drone = ((TAU * 55.0 * t).sin() * 0.5 + saw(55.0 * t) * 0.15) * (0.15 + 0.55 * p);
        let fc = 200.0 * (5000.0f32 / 200.0).powf(p); // cutoff climbs 200 → 5000 Hz
        lp += (1.0 - (-TAU * fc / SR).exp()) * (noise(i) - lp);
        let sweep = lp * (0.08 + 0.5 * p * p); // swells toward the drop
        *out = drone + sweep;
    }

    // heartbeat kicks that accelerate and intensify as the boss nears
    let mut kt = 0.0f32;
    let mut interval = 0.6f32;
    while kt < dur - 0.25 {
        let start = (kt * SR) as usize;
        let g = 0.5 + 0.5 * (kt / dur);
        add_voice(&mut buf, start, 0.30, g, |t, _| kick(t));
        interval = (interval * 0.9).max(0.14);
        kt += interval;
    }

    master(&buf)
}

// The boss-down detonation — a deep sub thump + mid boom + long noise wash. Bosses used to die
// with particles only; a kill this big needs a sound this big (and the juice pass keys off it).
pub fn boss_down_sfx_wav() -> Vec<u8> {
    render_sfx(1.2, |t, i| {
        let sub = (TAU * (52.0 - 18.0 * t) * t).sin() * (-t * 3.2).exp() * 0.9; // pitch-falling sub
        let boom = (TAU * 110.0 * t).sin() * (1.0 - (-t * 90.0).exp()) * (-t * 6.0).exp() * 0.5;
        let wash = (noise(i) - noise(i + 7)) * 0.5 * (-t * 2.4).exp() * 0.35; // long tail
        sub + boom + wash
    })
}

// Minimal 16-bit mono PCM WAV container.
fn wav_bytes(samples: &[i16], sr: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // channels = mono
    v.extend_from_slice(&sr.to_le_bytes());
    v.extend_from_slice(&(sr * 2).to_le_bytes()); // byte rate = sr * channels * 2
    v.extend_from_slice(&2u16.to_le_bytes()); // block align = channels * 2
    v.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

// ─────────────────────────────── one-shot sound effects ───────────────
// Short procedural WAVs (no asset files). Each renders `dur` seconds via a per-sample
// closure, soft-clips, and packs to a mono WAV — played once via Bevy's AudioPlayer.
fn render_sfx(dur: f32, mut voice: impl FnMut(f32, usize) -> f32) -> Vec<u8> {
    let n = (dur * SR) as usize;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            ((voice(t, i)).clamp(-1.0, 1.0) * i16::MAX as f32 * 0.9) as i16
        })
        .collect();
    wav_bytes(&samples, SR as u32)
}

/// Ship firing: a short descending "pew" — a saw whose pitch drops fast.
pub fn fire_sfx_wav() -> Vec<u8> {
    render_sfx(0.14, |t, _| {
        let freq = 380.0 + 900.0 * (-t * 45.0).exp();
        saw(freq * t) * (-t * 26.0).exp() * 0.5
    })
}

/// Asteroid breaking — a faithful port of the JS `playBreak`: a white-noise burst through a
/// LOWPASS whose cutoff sweeps DOWNWARD, giving a deep filtered "boom/whoosh" (no tone, so no
/// woodblock "tok"). Size-aware like the JS: a big rock uses a low cutoff (deep boom), a small
/// one a high cutoff (crack). `size` is 1 (small) … 3 (large).
///
/// The cutoff has to move sample-to-sample, which needs filter state, so this can't use the
/// stateless `render_sfx` — we run two cascaded one-pole lowpasses (≈ the JS biquad's rolloff)
/// by hand, then normalize (a low cutoff passes little energy, so levels vary by size).
pub fn break_sfx_wav(size: u8) -> Vec<u8> {
    let sz = size.clamp(1, 3) as f32;
    let f0 = 520.0 + (3.0 - sz) * 430.0; // size3 ~520 Hz (boom) … size1 ~1380 Hz (crack)
    let f1 = (f0 * 0.3).max(120.0); // cutoff glides down to here over `sweep`
    let dur = 0.12 + sz * 0.05; // size3 ~0.27 s … size1 ~0.17 s
    let sweep = 0.18;
    let n = (dur * SR) as usize;
    let (mut lp1, mut lp2) = (0.0f32, 0.0f32);
    let mut buf = vec![0f32; n];
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // exponential cutoff glide f0 → f1 over `sweep` seconds
        let fc = f0 * (f1 / f0).powf((t / sweep).min(1.0));
        let alpha = 1.0 - (-TAU * fc / SR).exp();
        let x = noise(i); // white noise
        lp1 += alpha * (x - lp1);
        lp2 += alpha * (lp1 - lp2);
        // gain env: ~6 ms attack, exp decay to ≈0 at `dur` (matches the JS ramps)
        let env = (t / 0.006).min(1.0) * (-t * (9.21 / dur)).exp();
        *out = lp2 * env;
    }
    // normalize to a consistent peak — the 2-pole lowpass output level drops with the cutoff
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.9 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| (v * norm * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// Mine explosion: a big, punchy detonation. A sharp CRACK snap on the attack, a deep sine
/// BOOM sweeping down in pitch, and a broadband NOISE blast — driven HARD into saturation so
/// it lands heavy instead of a soft, clean sine.
pub fn mine_sfx_wav() -> Vec<u8> {
    render_sfx(0.55, |t, i| {
        // Softer than before — the bright crack + broadband roar + heavy overdrive were harsh on
        // headphones. Keep the deep boom; trim the crack, the noise roar, and the saturation.
        let crack = (noise(i) - noise(i + 2)) * (-t * 55.0).exp(); // shorter, less ringing transient
        // BOOM: deep sine sweeping 160 → 28 Hz — the round, non-harsh body of the thud
        let freq = 28.0 + 132.0 * (-t * 11.0).exp();
        let boom = (TAU * freq * t).sin() * (-t * 5.0).exp();
        let blast = (noise(i) - noise(i + 5)) * 0.5 * (-t * 9.0).exp(); // gentler broadband tail
        ((boom * 1.5 + crack * 0.4 + blast * 0.3) * 1.25).tanh()
    })
}

/// Ship destroyed: a dramatic descending "doom" — a tone falling 420 → 60 Hz (the death cry)
/// over an explosion burst and a deep sub thump. Longer and more mournful than a mine blast so
/// losing a life reads as a bigger deal. The falling pitch uses an integral-phase sweep (like
/// the kick) so it glides cleanly instead of warbling.
pub fn death_sfx_wav() -> Vec<u8> {
    render_sfx(0.6, |t, i| {
        // descending doom: instantaneous freq 420 → 60 Hz; phase = ∫f dt in cycles
        let cyc = 60.0 * t + 80.0 * (1.0 - (-4.5 * t).exp());
        let doom = ((TAU * cyc).sin() * 0.6 + saw(cyc) * 0.3) * (-t * 4.0).exp();
        // explosion burst on the attack (broadband noise)
        let blast = (noise(i) - noise(i + 4)) * 0.5 * (-t * 7.0).exp();
        // deep sub thump underneath for weight
        let sub = (TAU * 45.0 * t).sin() * (-t * 6.0).exp();
        ((doom + blast * 0.7 + sub * 0.8) * 1.3).tanh()
    })
}

/// Enemy mob firing: a low, buzzy descending blip — hostile and clearly NOT the player's
/// brighter square-wave "pew" (which sweeps 1280→380 Hz). This one sits down at 460→120 Hz.
pub fn enemy_shot_wav() -> Vec<u8> {
    render_sfx(0.13, |t, _| {
        let f = 120.0 + 340.0 * (-t * 30.0).exp(); // 460 → 120 Hz
        saw(f * t) * (-t * 22.0).exp() * 0.45
    })
}

/// Enemy mob destroyed: a small zap-pop — a quick descending tone plus a noise burst. Lighter
/// and shorter than the player-ship death (which is a long mournful doom), so a mob popping
/// reads as a minor event.
pub fn enemy_die_wav() -> Vec<u8> {
    render_sfx(0.28, |t, i| {
        let f = 90.0 + 300.0 * (-t * 18.0).exp(); // 390 → 90 Hz
        let tone = (TAU * f * t).sin() * (-t * 12.0).exp();
        let burst = (noise(i) - noise(i + 3)) * 0.5 * (-t * 16.0).exp();
        ((tone * 0.7 + burst * 0.6) * 1.4).tanh()
    })
}

/// Warp launch — a port of the JS `playVortex`: two tones PLUNGING in pitch (saw 640→52 Hz,
/// sine 1020→80 Hz) under a swept band-passed NOISE whoosh (center 1900→220 Hz), so it reads as
/// a portal tearing open. Distinct from every other effect. Swept filters need state, so this
/// runs its own sample loop rather than the stateless `render_sfx`.
pub fn warp_wav() -> Vec<u8> {
    let dur = 0.56;
    let n = (dur * SR) as usize;
    let mut buf = vec![0f32; n];
    let (mut la, mut lb) = (0.0f32, 0.0f32); // two one-poles → a crude swept bandpass (hi − lo)
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // descending tones: instantaneous f = f_end + (f0−f_end)·e^(−6t); phase (cycles) = ∫f dt
        let cyc1 = 52.0 * t + (640.0 - 52.0) / 6.0 * (1.0 - (-6.0 * t).exp());
        let cyc2 = 80.0 * t + (1020.0 - 80.0) / 6.0 * (1.0 - (-6.0 * t).exp());
        let tone = saw(cyc1) * 0.5 + (TAU * cyc2).sin() * 0.5;
        // swept-noise whoosh: band center glides 1900 → 220 Hz
        let fc = 1900.0 * (220.0f32 / 1900.0).powf((t / 0.5).min(1.0));
        let x = noise(i);
        la += (1.0 - (-TAU * (fc * 1.6) / SR).exp()) * (x - la);
        lb += (1.0 - (-TAU * (fc * 0.6) / SR).exp()) * (x - lb);
        let whoosh = (la - lb) * 2.0;
        let tone_env = (t / 0.03).min(1.0) * (-t * 5.0).exp();
        let whoosh_env = (t / 0.05).min(1.0) * (-t * 5.5).exp();
        *out = tone * tone_env * 0.6 + whoosh * whoosh_env * 0.5;
    }
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.9 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| (v * norm * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// The Haunt (final boss) — its OWN signature: a cold SPECTRAL whisper-whoosh, distinct from the warp.
/// A faint hollow detuned dyad drifting down (the "presence") under an airy band-swept noise breath whose
/// centre glides down — eerie and ghostly, not the portal-tearing plunge of `warp_wav`. Swept filters need
/// state, so it runs its own loop.
pub fn haunt_sfx_wav() -> Vec<u8> {
    let dur = 0.4;
    let n = (dur * SR) as usize;
    let mut buf = vec![0f32; n];
    let (mut la, mut lb) = (0.0f32, 0.0f32);
    for (i, out) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // faint hollow tone (fundamental + a hollow fifth) drifting down — the spectral "presence"
        let f = 240.0 * (1.0 - 0.2 * (t / dur));
        let tone = (TAU * f * t).sin() * 0.4 + (TAU * f * 1.5 * t).sin() * 0.18;
        // airy band-swept noise (a breath), band centre gliding 2600 → 520 Hz
        let fc = 2600.0 * (520.0f32 / 2600.0).powf((t / dur).min(1.0));
        let x = noise(i);
        la += (1.0 - (-TAU * (fc * 1.5) / SR).exp()) * (x - la);
        lb += (1.0 - (-TAU * (fc * 0.5) / SR).exp()) * (x - lb);
        let breath = (la - lb) * 1.8;
        let env = (t / 0.04).min(1.0) * (-t * 4.5).exp(); // soft attack, medium tail
        *out = (tone * 0.5 + breath * 0.5) * env;
    }
    let peak = buf.iter().fold(0f32, |m, &v| m.max(v.abs())).max(1e-6);
    let norm = 0.85 / peak;
    let samples: Vec<i16> = buf.iter().map(|&v| (v * norm * i16::MAX as f32) as i16).collect();
    wav_bytes(&samples, SR as u32)
}

/// Achievement unlocked: a bright rolled major arpeggio (C6-E6-G6-C7, each note entering slightly
/// later so they ring together) — a positive, sparkly "flourish".
pub fn achievement_sfx_wav() -> Vec<u8> {
    render_sfx(0.7, |t, _| {
        let voice = |f: f32, delay: f32| {
            if t < delay {
                return 0.0;
            }
            let nt = t - delay;
            (TAU * f * nt).sin() * (1.0 - (-nt * 80.0).exp()) * (-nt * 3.5).exp()
        };
        (voice(1046.5, 0.0) + voice(1318.5, 0.06) + voice(1568.0, 0.12) + voice(2093.0, 0.18)) * 0.25
    })
}

// Two clipped data-blips — the TRANSMISSION RECEIVED cue for a Pilot Log decrypt. Deliberately
// radio-flavored (soft-square, dry, just two notes) so it never reads as the achievement chime's
// celebration: a log entry is information arriving, not a fanfare.
pub fn log_sfx_wav() -> Vec<u8> {
    render_sfx(0.5, |t, _| {
        let blip = |f: f32, delay: f32| {
            if t < delay {
                return 0.0;
            }
            let nt = t - delay;
            let s = (TAU * f * nt).sin();
            (s.signum() * 0.55 + s * 0.45) * (1.0 - (-nt * 200.0).exp()) * (-nt * 16.0).exp()
        };
        (blip(880.0, 0.0) + blip(1174.7, 0.16)) * 0.2
    })
}

// A bright, fast six-note ascending run — a classic "1UP" jingle for the gold rock. Deliberately
// quicker, higher and sparklier than the achievement chime so an extra life reads as its own event.
pub fn life_sfx_wav() -> Vec<u8> {
    let notes = [783.99, 1046.5, 1318.5, 1568.0, 2093.0, 2637.0]; // G5 C6 E6 G6 C7 E7
    render_sfx(0.6, |t, _| {
        let mut s = 0.0;
        for (i, &f) in notes.iter().enumerate() {
            let delay = i as f32 * 0.07;
            if t >= delay {
                let nt = t - delay;
                let env = (1.0 - (-nt * 120.0).exp()) * (-nt * 9.0).exp(); // fast pluck, quick decay
                s += ((TAU * f * nt).sin() + 0.3 * (TAU * 2.0 * f * nt).sin()) * env; // + a shimmering octave
            }
        }
        s * 0.22
    })
}

// Weapon-switch blip (standard ↔ mass): a short, crisp two-step "chk-chk" that rises in pitch.
pub fn toggle_sfx_wav() -> Vec<u8> {
    render_sfx(0.13, |t, _| {
        let (f, start) = if t < 0.05 { (600.0, 0.0) } else { (900.0, 0.05) }; // step up on the 2nd click
        let env = (-((t - start) * 55.0)).exp(); // re-pluck at each step
        (square(f * t) * 0.45 + (TAU * f * t).sin() * 0.55) * env * 0.5
    })
}

// Nova Shield POP — the shield eating a hit: a glassy crystal-shatter (bright detuned partials
// snapping off, a sparkle of noise) over a low thump, so a big save reads instantly — and sounds
// nothing like a rock break (noise boom) or the ship death.
pub fn nova_pop_sfx_wav() -> Vec<u8> {
    render_sfx(0.38, |t, i| {
        // three high glass partials, slightly detuned, each dying at its own rate
        let glass: f32 = [1960.0, 2420.0, 3140.0]
            .iter()
            .enumerate()
            .map(|(k, &f)| (TAU * f * t).sin() * (-t * (16.0 + k as f32 * 6.0)).exp())
            .sum();
        let sparkle = (noise(i) - noise(i + 7)) * (-t * 22.0).exp(); // high-passed shatter dust
        let thump = (TAU * (120.0 * (1.0 - 0.4 * t)) * t).sin() * (-t * 12.0).exp(); // the hit landing on the barrier
        (glass * 0.22 + sparkle * 0.5 + thump * 0.55) * 0.8
    })
}

// Nova Shield RE-LIGHT — back online: a soft exponential rise with a warm fifth above, blooming in
// and settling (a quiet "power returns" cue — nothing like the weapon-toggle's crisp click).
pub fn nova_up_sfx_wav() -> Vec<u8> {
    render_sfx(0.45, |t, _| {
        let k = (t / 0.45).min(1.0);
        let f = 320.0 * (960.0f32 / 320.0).powf(k); // glide up 320 → 960 Hz
        let env = (t / 0.09).min(1.0) * (-t * 6.5).exp(); // gentle attack, soft tail
        ((TAU * f * t).sin() + 0.35 * (TAU * f * 1.5 * t).sin()) * env * 0.5
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfx_are_valid_nonsilent_wavs() {
        for wav in [
            fire_sfx_wav(),
            break_sfx_wav(1),
            break_sfx_wav(2),
            break_sfx_wav(3),
            mine_sfx_wav(),
            death_sfx_wav(),
            enemy_shot_wav(),
            enemy_die_wav(),
            warp_wav(),
            haunt_sfx_wav(),
            achievement_sfx_wav(),
            life_sfx_wav(),
            toggle_sfx_wav(),
            nova_pop_sfx_wav(),
            nova_up_sfx_wav(),
            log_sfx_wav(),
            boss_down_sfx_wav(),
            vortex_sfx_wav(),
        ] {
            assert_eq!(&wav[0..4], b"RIFF", "sfx starts with a RIFF header");
            assert_eq!(&wav[8..12], b"WAVE", "sfx is a WAVE file");
            assert!(wav.len() > 44 + 2000, "sfx should carry audio data, got {}", wav.len());
            let loud = wav[44..].chunks_exact(2).any(|b| i16::from_le_bytes([b[0], b[1]]).abs() > 2000);
            assert!(loud, "sfx should contain audible samples");
        }
    }
}
