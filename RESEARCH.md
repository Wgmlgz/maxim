# Speech2Go Maxim: native port investigation

## Requested result

Offline text input to audio output using the Maxim voice from
`sources/247_S2G_Maxim.exe`, on macOS and Linux without Windows components.

**Status: experimental offline text-to-audio prototype.** The Rust program now
accepts simple Russian text and joins decoded Maxim corpus units into a WAV.
It does not reproduce the original IVONA unit selection or prosody.

## Observed package

The input is a PE32 Inno Setup 5.5.6 installer (SHA-256
`970299e653816450959ea03ab25c3bf7aecce2493231eba70a7492dcdf748083`).
`innoextract` unpacked selected components to `extracted/app/`.

The synthesis components are:

| File | Role | SHA-256 |
| --- | --- | --- |
| `extracted/app/x64/tts_engine.dll` | IVONA synthesis engine, Windows x86-64 PE | `116cc3414019ce93015558da68901440e0a5596f32e326ac6b81fba56ed8bb09` |
| `extracted/app/x64/voice_ru_maxim.dll` | Maxim voice plugin, Windows x86-64 PE | `6c1bb6badc0d6aa3a2b6dbffeb721e027c00fd0eab24b021140e372bf10d5d56` |
| `extracted/app/vox/lang_ru_ru.dat` | Russian language data, 1,480,044 bytes | `9b6509b2c0fc7eb716a46295677bf25d60568ce9f533e75691c0cb6ca1f546e9` |
| `extracted/app/vox/vox_ru_maxim22i` | Maxim voice database, 207,021,376 bytes | `75b61f9295155fc7e69993a4fd50d558c70471d49eca9f5b211401ae551aaf37` |

The installer also contains 32-bit variants and SAPI integration DLLs. The
voice database begins with `DDXVvox_ru_maxim`, while the language data begins
with `ISEF`. These are format observations, not decoded specifications.

## REA findings

REA's PE export inspection found 39 named engine exports, including `tts_create`,
`tts_init`, `tts_load_voice`, `tts_streamer_start`, `tts_streamer_synth`, and
`tts_streamer_synth0` (Evidence from `inspect_native_exports`, engine digest
above). The voice plugin exports `language_data`, `register_voice`, and
`unregister_voice` (same operation, plugin digest above).

Ghidra analysis of the engine found 2,497 procedures and 3,228 defined strings.
The static `tts_load_voice` pseudocode calls `LoadLibraryA` on the voice DLL,
looks up `register_voice` and `language_data`, and checks voice properties and
the database (REA Evidence `ev_859c499bc79668c6b2fb029c381877ac3495604f165b9c98be2a881626ba87c7`).
The `tts_streamer_synth0` pseudocode copies signed 16-bit sample data from an
internal wave buffer (Evidence
`ev_89d43d123f8a4a76cf327e76e06f2f1b77657bb3c5b842acebd8af58eb36846e`).
`tts_init` creates the engine and loads a certificate (Evidence
`ev_5a291cdf9640c353434243112d5701132712ac12a1624ae06d970c68c6a993a8`).
These are static observations; the binary was not executed.

The engine imports Windows file, thread, synchronization, encoding, and network
APIs (REA Evidence `ev_89b8dfca85b1de4b134d36bf13fa2ee9f0bf0c39f25e5daf6764501ac322f8dc`).
`rea-tts-engine.snapshot.json` retains this Ghidra analysis for continuation.
`rea-maxim-voice.snapshot.json` retains the voice-plugin analysis.

The voice plugin's database loader parses a 104-byte DDXV header and a table
of `(kind, offset, length)` 12-byte section entries (REA Evidence
`ev_0f9f2e2fa92eb54351f15e51de33dc255e572a175abceafb1522e554ab7da1cc`
and `ev_1991773ffb438683340e1a02f63d2d0b3c40f0912ef460158377905fc9a5099f`).
The plugin's codec-14 decoder calls a function with the exact standalone SILK
decoder signature and applies an XOR mask derived from the header checksum and
the packet's trailing bytes (REA decompilation of `FUN_18000c910`). The SILK
decoder state size returned by both the original plugin and `silk-v3-sys` is
13,728 bytes. The Rust implementation decodes a known corpus packet at
`0x20404fb` to 960 signed 16-bit samples at 24 kHz, with RMS 6059 and peak
21275. This validates actual audio decoding, though not exact equivalence to
the original output.

Section 3 has 1,612 20-byte diphone entries with contiguous candidate ranges.
Their last range ends at 565,950, exactly section 4's size divided by four.
Section 4's values are **unit indices**, not byte offsets: each index points to
an 8-byte entry in section 1 and a 36-byte acoustic feature record in section
5. Section 1 has 574,369 entries, of which 565,950 are valid and referenced
exactly once by section 4. The remaining 8,419 are zero/invalid entries.

Each valid section-1 entry points to a variable-length record near the end of
the database. Its layout is `u16 pitchmark_count`, that many `u16` pitchmark
positions, `u16 decoded_sample_skip`, `u32 encoded_audio_file_offset`, and
`u32 encoded_byte_count`. The record length equals `2 * pitchmark_count + 12`
for all 565,950 referenced units. The meaning is confirmed by engine record
parser `FUN_18002c420` (REA Evidence
`ev_19facc07ba47d6a302716d8354358a0ef6d1e4defb62514a1255abbaad495bb3`)
and decoder call `FUN_18002c020` (Evidence
`ev_83ce9e6b19a55353cd6e75c2a53ab47ff64f68b6d47abe4649954e18cbce7f0e`).
The audio offsets are monotonic, from `0x203cda4` to `0xad810a0`; 414,983 are
unique. Section 5's final `u32` points into variable-length section 6, likely
unit features relevant to selection.

The Rust `decode-unit` path reads exactly `encoded_byte_count` bytes, decodes
SILK packets, skips `decoded_sample_skip` samples, and extracts
`last_pitchmark` samples. The original decoder zero-fills short results; the
port does too. An earlier decoder bug repeated a shortened XOR key for packets
with trailing key bytes. Reinspection of `FUN_18000c910` (REA Evidence
`ev_b79eaa9d55ee565322e236594d5bccea3c3078d5bbb0736b554104da818e4807`)
shows that the key stride is always eight bytes. Correcting this made every
candidate in three checked diphone groups decode fully: `a-p` 1,847/1,847,
`p-a` 3,631/3,631, and `m-a` 2,280/2,280. The port now joins at pitchmark periods with a
raised-cosine blend, following original renderer `FUN_18002cb40` (Evidence
`ev_1c982fc86794a09129abc40cca74a67cce36018214e71eeb19fa0fe8a3b11f0d`)
and blend `FUN_18002f780` (Evidence
`ev_f033d3085f7584f3db0e7f531026ca2cab96eaa527b50468dae9b26225d3309d`).
Unit selection, pitch and duration modification, and linguistic accuracy remain
incomplete.

For every candidate record, section 5 bytes 4 and 5 are exactly the phoneme
codes of its diphone across all 1,612 diphone groups. Byte 3 is consistent
with the preceding phoneme and byte 6 with the following phoneme. The current
selector scores these two context bytes and runs a dynamic search over at most
64 decoded candidates per diphone. It rewards consecutive unit IDs, which
often occur for adjacent diphones from the same source utterance. These context
and continuity weights are inferred heuristics, not the original costs.

The original engine builds a candidate graph and runs Viterbi search with a
configured beam width of 50 (`FUN_18002e160`, Evidence
`ev_32575b3e2b8b57e28b791175721bd1f2478cd34ad499b49f21c7fb9d38cff45d`).
Its renderer then splits selected units at pitchmark positions and applies
duration and pitch adjustments (`FUN_18002eda0`, Evidence
`ev_9203a1fe940ca566f491a24f4a2a38ed496ce3f6b08d111b37f424fe7965eb35`).
The current CLI does not yet reproduce the original search costs or those
adjustments.

The corrected decoder changes which candidates the current heuristic selector
can choose. For the confirmed “Папа купил хлеб” reference, a 40-band log-spectrum
DTW comparison gave 2.232 for the corrected decoder's new selection versus
2.076 for the prior output (lower is closer). This is a diagnostic measure,
not a listening score. The original `FUN_18000d9f0` target cost includes many
context and stress terms that the Rust selector does not yet reproduce.

## Porting boundary

A native Rust CLI alone cannot load these PE DLLs. The current port replaces
the DDXV parser, part of compressed unit decoding, and a crude text-to-diphone
path. It still needs the original Russian text normalization, lexicon, stress
assignment, unit target and join costs, pitch and duration adjustment, and
reference validation. There is no native macOS or Linux implementation among
the files in this installer.

The next useful milestone is to resolve the SILK payload failures and map the
feature fields in sections 5–6 to the original candidate scoring code. The
supplied reference MP3s now support timing and acoustic comparison; exact text
mapping for the later numbers needs confirmation. Native output still needs
validation on Linux.

## Reference audio comparison

The user supplied MP3s in `samples/`, all 22.05 kHz mono at 128 kb/s. They
confirmed that `samples/1.mp3` says “Папа купил хлеб.”; the first comparison
had incorrectly paired it with “Мама мыла раму.” and any conclusions from that
pair are withdrawn. The text for the other numbered MP3s remains unverified.
The current path uses the corrected record layout and pitchmark joining. Its
22.05 kHz output also normalizes leading and trailing silence:

| File | Reference MP3 | Current native WAV |
| --- | ---: | ---: |
| `1`, “Папа купил хлеб.” | 1.254 s | 1.275 s in `comparison/1.wav` |
| `2`, text unverified | 1.750 s | assumed “Привет, Максим!” in `comparison/2.wav` |

The phoneme path now performs simple word-final obstruent devoicing and
regressive voicing assimilation. This resolves earlier missing diphones such
as `b-pau` in “хлеб” and `s-z` in “голос звучит”. These are approximation
rules, not a recovery of the original Russian language data. The current WAVs
for available reference numbers are in `comparison/`. These still need human
listening comparison; duration agreement alone does not establish quality.

## Waveform and phone-label audit

The corrected `analysis/compare-1.png` pairs “Папа купил хлеб.” with the
matching native WAV. `analysis/compare-2.png` still uses an unverified text
assignment. Individual native “Мама мыла раму.” units are plotted in
`analysis/selected-units-mama.png`. The `m-a` unit 224958 changes from nasal
low-frequency energy to a vowel formant pattern; `a-m` unit 390743 changes in
the opposite direction. This supports the corpus labels for those diphones.
It does not validate every letter-to-phone rule.

For the corrected sample 1 pair, the durations are close and the broad vowel
bursts occur in a similar order. The native waveform still differs in the
timing, strength, and spectral shape of individual phones. Duration agreement
alone does not establish pronunciation or voice similarity.

For the assumed sample 2 text, the selected `t-pau` unit 239944 contains about 0.2 s of
trailing silence and the following `pau-m` unit 272511 has about 0.18 s of
leading silence. Keeping both created a long artificial gap. The renderer now
keeps about 40 ms of recorded silence at each edge, cutting only at corpus
pitchmarks. It also inserts `j` before iotated vowels at word starts and after
vowels; `j-a`, `j-u`, `j-o`, and `j-e` all exist in the corpus. These are
specific corrections, but the comparison WAVs remain acoustically far from
the original because stress, vowel reduction, and original duration and unit
selection are not ported.

The decoder is also blocking better unit choices. For the `m-a` context in
“мама”, section 5 exposes four units matching both neighboring phone codes:
34111, 52751, 235861, and 300948. The first decodes only 2167 of 2663
requested samples; the other three fail with SILK `-12`. For the following
`a-m` context, both exact matches decode zero available samples. The selector
therefore uses context-mismatched units. The original voice plugin's packet
path in `FUN_18000c910` calls its internal SILK decoder and treats a nonzero
return as failure (REA Evidence
`ev_b79eaa9d55ee565322e236594d5bccea3c3078d5bbb0736b554104da818e4807`).
Its state and checksum-key setup are visible in `FUN_18000c860`. The remaining
work is to compare that internal decoder with `silk-v3-sys` on failing packets;
the context matches alone do not prove that the original engine would select
those units.

## Pitch, duration, and splice tracing

The original renderer `FUN_18002eda0` splits each selected diphone at a
pitchmark near half the first phoneme duration. It calls `FUN_18002dba0` for
each half, then `FUN_18002cb40` to append periods. The latter blends the
previous final period with the next first period through `FUN_18002f780` (REA
Evidence `ev_d8de886b35491f4f6bdcca4af253192e6bb819a807b6734feb348e45d0dc0cd8`,
`ev_d08f7003f7d2c08f68d20d345511731b06f6a0b78bbf002f0e5bce85c52f6d25`,
`ev_8c78003b4eb6c03cb79e0a19c9ecc1cf6de9b7dd9e9b23cf3aa1f048fb72597e`,
`ev_4b8b70c64ba94a92fea51ef90cf0e485176f9f1958593c854e73c4a394cbff4d`).
When `pitch_stretch` differs from 1, `FUN_180030760` changes samples per
period; `FUN_180030120` and `FUN_180030270` duplicate or remove whole periods
for duration (Evidence `ev_4d5dcad1e1b53d0f1b550067c04fa246e40f4976915fec8dbd53d16372931cb0`,
`ev_6bcecb4b1a6c2bb8f9ee5b2070c34dbb9627079cd6a6e2a0d70b4def8005f7da`,
`ev_3db13f8a421825ab96fb8dafb0ea873b664bada507b2caac85b56594a3aaec39`).
Those controls default to 1.0, so static analysis does not establish that
pitch or duration stretching was applied to the supplied sample 1.

In the selected native path for “Папа купил хлеб,” several consecutive unit
IDs have nearly identical boundary periods (correlation approximately 0.99–1.0),
while nonconsecutive joins have larger pitch, energy, or spectral differences.
The reference's measured voiced pitch varies more than the native output.
For the confirmed sample 1 pair, `analysis/pitch-1.png` shows an approximate
autocorrelation pitch track: the reference rises to about 135 Hz in the middle
and 170–180 Hz near the final voiced segment, while the native output mostly
stays near 110–135 Hz. The final high estimate could include tracking error,
but the sustained middle difference is clear. This supports a unit/prosody
mismatch, not a uniform time offset.
The original selector's `FUN_18000d9f0` scores extensive linguistic target
features, including stress and pitch-related fields, and `FUN_18000e110`
combines those costs (Evidence `ev_2b5315e0afd6bae71bbb8ab49884f2158af92c7b84ed3e3432fd988b060eafe1`,
`ev_0592e9ee18364ca43b35a5b900da4d0f75d83707b27e555b5cb534e510a379e8`).
The Rust selector has an optional heuristic decoded-boundary energy/spectrum
cost for nonconsecutive units (`--acoustic-join`). This changed two selected
units in sample 1; an
MFCC dynamic-time-warping distance to the reference changed only slightly
(2.551 to 2.529 under one analysis configuration), so listening is needed
to decide whether it helps. `comparison/1.wav` retains the previous path and
`comparison/papa-acoustic.wav` contains the experimental output.
