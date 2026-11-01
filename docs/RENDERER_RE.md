# Original audio rendering path

Static map of `extracted/app/x64/tts_engine.dll` (SHA-256
`116cc3414019ce93015558da68901440e0a5596f32e326ac6b81fba56ed8bb09`).
Evidence is Ghidra decompilation through REA. It establishes code paths, not
which optional controls were active in any particular reference MP3.

## Entry and branches

`FUN_18002eda0` walks selected `Unit` nodes. A unit with a selected diphone
index loads a pitchmark-based waveform (cache path `FUN_18002e580`, fallback
`FUN_18002c590`). It reads the two neighboring duration values from the
selected database records, chooses a pitchmark near half the first segment's
duration, processes the two halves with `FUN_18002dba0`, and appends the unit
with `FUN_18002cb40`. It then updates `Segment` end positions.

The same renderer has separate branches for a `silence` feature (insert zero
samples for its requested seconds), a `wave` feature (load and optionally
resample a wave, then multiply token and utterance `amplify` settings), and a
plain segment without a selected unit (advance its timing). These branches
are relevant to markup or special tokens, though ordinary text normally uses
selected diphones. Evidence:
`ev_d8de886b35491f4f6bdcca4af253192e6bb819a807b6734feb348e45d0dc0cd8`.

## Per-half transformations

`FUN_18002dba0` retrieves `pitch_stretch`, `duration_stretch`, `compress`,
and `amplify` at both token and utterance levels and multiplies each pair.
Absent values default to 1.0. If pitch stretch differs from 1.0,
`FUN_180030760` changes period lengths, then adjusts the effective duration
factor. If duration stretch differs from 1.0, `FUN_180030120` duplicates or
removes periods. `compress` invokes `FUN_18002fca0`; `amplify` is capped by
`param_amplify_max` and invokes `FUN_18002fa80`. A special pause branch changes
duration to match the `Segment.end` timing. Evidence:
`ev_d08f7003f7d2c08f68d20d345511731b06f6a0b78bbf002f0e5bce85c52f6d25`.

The deeper transformation calls are now traced. `FUN_180030760` changes the
number of samples in each selected pitch period by duplicating or removing
samples (Evidence `ev_4d5dcad1e1b53d0f1b550067c04fa246e40f4976915fec8dbd53d16372931cb0`).
`FUN_180030120` applies duration scaling in stages, each between 0.5 and 2.0,
through `FUN_180030270`; the latter duplicates or removes periods and blends
with the same splice curve (Evidence
`ev_6bcecb4b1a6c2bb8f9ee5b2070c34dbb9627079cd6a6e2a0d70b4def8005f7da`,
`ev_3db13f8a421825ab96fb8dafb0ea873b664bada507b2caac85b56594a3aaec39`).
`FUN_18002fa80` applies amplitude scaling to a selected sample interval,
clamping to ±32766 (Evidence
`ev_a7766e343d5532f8b780841658988b39f98bbba25979685bb94241d73cb2efb8`).
`FUN_18002fca0` performs a level-dependent nonlinear compression with static
fixed-point tables and a running envelope (Evidence
`ev_87f9df5cdc08abfa2cc5f0771eec9e667738eb24953ecd220c88d1e898bbfd95`).

`FUN_18002e740` checks whether either half has pitch or duration stretch other
than 1.0. Only then does the renderer make an editable copy of a compressed
unit with `FUN_180030a40`. Evidence:
`ev_9399d74d99cae1a2aca8a0280cf8d769c01a81cef469f26227ec8417b45e609a`,
`ev_622734a778902e61f458a4f366cb50a9355daed50fa5f91082140af2847b193c`.

## Concatenation

`FUN_18002cb40` copies every pitchmark period except the last into the output
buffer, keeps the final period pending, and overlaps it with the next unit's
first period. `FUN_18002f780` uses a 16-step lookup curve to blend at most the
shorter period length. The Rust `Stitcher` now interpolates the same curve;
the table bytes were read from `0x1800dd790` (Evidence
`ev_43fc31d78b457d2c322af5751ba0546a3877ac25ea58670830d78f9de28b785e`). Evidence:
`ev_8c78003b4eb6c03cb79e0a19c9ecc1cf6de9b7dd9e9b23cf3aa1f048fb72597e`,
`ev_4b8b70c64ba94a92fea51ef90cf0e485176f9f1958593c854e73c4a394cbff4d`.

## Port status

The Rust port decodes selected units and joins pitchmark periods using the
original blend curve. The SILK decoder receives 24 kHz for this VOX codec,
but `FUN_18002c590` assigns the VOX header rate (22.05 kHz) to decoded units.
`FUN_18002cb40` copies their sample periods into the output without another
rate conversion. The Rust port now writes those periods directly at 22.05 kHz;
its earlier 24→22.05 kHz resampling raised pitch and shortened speech by the
ratio 24,000/22,050. On the confirmed reference phrase, removing the extra
resampling changed the rough 20-band spectral distance from 0.1713 to 0.0858;
median voiced pitch changed from 137.8 to 124.6 Hz versus 126.4 Hz in the
reference. This is a diagnostic comparison, not proof of waveform parity.

It does not yet copy the original per-half timing,
period-level pitch and duration edits, compression, or amplification cap. The absence of these
paths alone does not establish that they were active in a supplied recording;
the token and utterance controls need to be traced from the text pipeline.

The pause branch of `FUN_18002dba0` compares the selected unit half's recorded
duration with the `Segment.end` duration target. If the target is shorter,
`FUN_1800306f0` invokes the period-level duration scaler; if longer,
`FUN_18002bb30` inserts 10 ms zero periods near the middle and fades its
neighboring periods. For an internal pause, the target is halved because two
neighboring diphones share it. The Rust `trim_pause_silence` is still a
heuristic and does not implement these period operations. This is an observed
code path from REA decompilation, not a measurement of the supplied MP3.
