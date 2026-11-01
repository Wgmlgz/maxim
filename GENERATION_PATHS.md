# Maxim generation paths in the supplied binaries

This is a step-by-step map of the shipped `tts_engine.dll`,
`voice_ru_maxim.dll`, and DDXV voice database. The evidence is static REA
decompilation and database inspection. It does not establish the exact runtime
settings used for any supplied MP3. See `LINGUISTIC_RE.md`, `SELECTOR_RE.md`,
and `RENDERER_RE.md` for function-level details and Evidence IDs.

| Step | Original path | Native Rust status |
| --- | --- | --- |
| 1 | `tts_streamer_synth` constructs and runs the utterance pipeline. The registered order is `tokenstotokens → text_analysis → pos_tagger → phrasing → lettertosound → change_language → pause_insertion → postlex → intonation → duration`. | Word segmentation and pronunciation are ported for ordinary Russian text. Token normalization and several stages remain. |
| 2 | Russian voice callbacks supply token-to-word, sentence-boundary, word-to-segment, postlex, syllable-boundary, xSampa-to-xIvona, and phrase-end-tone behavior. `lang_ru_ru.dat` contains a compressed lexicon and a letter-to-sound fallback. | The 112,699-entry lexicon, LTS trees, syllable CART, consonant postlex, and phrase-context clitic reduction tables are ported. Token normalization, POS, compound rules, and full phrase behavior remain. |
| 3 | The engine creates `Segment`, `SylStructure`, `Word`, `Phrase`, `Token`, and `Unit` relations. Intonation and duration stages write context and timing features into them. | Rust retains word and syllable spans, stress, and estimated durations from the original VOX CART. Phrase, Token, Unit, and exact intonation relations remain. |
| 4 | `FUN_180034000` makes a 184-byte target feature record for each segment from the relation graph. `FUN_18002e160` passes adjacent record pairs to the voice plugin. | Segment context, syllable fields, CART duration, and phrase end tone populate partial target records. Other fields still require their upstream stages. |
| 5 | The voice plugin looks up each phone pair in section 3, obtains candidate unit IDs from section 4, expands section 5/6/7 compact candidate features with `FUN_18002bdc0`, filters stress-incompatible vowel units, and calculates target cost with `FUN_18000e110`/`FUN_18000d9f0`. | Compact candidate features decode in Rust. The default scores recovered target terms from available relation fields, tries matching stress first, and retains cheap candidates plus exact recorded contexts. The remaining target fields are absent or approximated. |
| 6 | `FUN_1800314b0` searches the candidate graph with a beam and class-specific join cost. Adjacent IDs from the same source recording continue at zero transition cost. | The CLI uses the original five class-specific join costs, zero-cost adjacent unit continuation, and a 50-state beam by default. Search still lacks some original graph flags and special pruning. |
| 7 | The selected unit records provide pitchmarks and offsets into SILK audio. The voice plugin deobfuscates packets with an 8-byte XOR key and decodes them. | Ported. Checked diphone groups now decode all candidates. |
| 8 | `FUN_18002eda0` splits selected diphones near the first segment's midpoint, applies optional pitch, duration, compression, and amplitude controls to each half, then `FUN_18002cb40` blends adjacent pitchmark periods. `silence` and `wave` features take separate renderer branches. | Unit PCM and period joins are ported. The 16-step original blend curve is used. Per-half adjustments and special branches remain. |
| 9 | The decoded unit is tagged with the VOX header rate and its pitchmark periods are copied directly to the output. Wave tokens have a separate resampling branch. | The CLI writes the decoded periods directly as mono 16-bit WAV at 22.05 kHz. |

## Exact target feature paths

`FUN_180034000` reads the following paths when constructing the target record
(Evidence `ev_7615e6d0756a6a3bd39db5991b673e81d741d746b89bc3e64df133ddcf647f72`).
The list preserves the engine's feature names and relation levels.

- **Segment context:** `end`, `p.end`, `p.p.name`, `p.name`, `name`,
  `n.name`, `n.n.name`, `pos_in_syl`, `ph_vc`, `ph_cvox`, `ph_ctype`,
  `ph_cplace`.
- **Syllable context:** `syl_vowel`, `p.stress`, `p.glottal_stop_pos`,
  `p.syl_num_phones`, `stress`, `glottal_stop_pos`, `syl_num_phones`,
  `pos_in_word`, `syl_in`, `syl_out`, `ssyl_in`, `ssyl_out`, `ssyl_from`,
  `ssyl_to`, `n.stress`, `n.glottal_stop_pos`, `n.syl_num_phones`.
- **Word context:** `p.gpos`, `p.word_num_syls`, `gpos`, `word_num_syls`,
  `n.gpos`, `n.word_num_syls`, plus previous/current/next `gpos == content`
  flags and word position in the phrase derived by traversing `Phrase` links.
- **Phrase context:** `p.phrase_num_syls`, `p.phrase_num_words`,
  `phrase_num_syls`, `phrase_num_words`, `pos_in_utt`, `num_phrases`,
  `n.phrase_num_syls`, `n.phrase_num_words`, and the last syllable's
  `endtone`.

The voice graph builder chooses the join-cost family from `ph_ctype`,
`ph_vc`, and `ph_cvox` on the second segment: trill, vowel, glide,
unvoiced, or voiced. The glide case also checks the first segment's `name`.

Some paths are traversed differently for pause segments, but populate the
same target record fields. The original target-cost code does not weigh every
field equally; stress and certain boundary mismatches carry large penalties.

The exact byte assignments for the copied subset are in `target_cost::text_records`.
For example, `0x30` is one-based phone position in the syllable, `0x31` is
remaining phones, `0x35` is syllable stress, `0x38`/`0x39` are syllable position
from the start/end of the word, and `0x48` names the syllable vowel. This
mapping is from the `tts_engine.dll` decompilation (REA Evidence
`ev_7615e6d0756a6a3bd39db5991b673e81d741d746b89bc3e64df133ddcf647f72`).

## Current fidelity boundary

The dominant unresolved path is creating the full original relation graph and
its exact word class, phrase, duration, and intonation features from arbitrary text.
Without those target values, using the full original selector cost would
compare corpus candidates against fabricated inputs. The default now scores
only target terms supported by the ported stages; `--heuristic-target` retains
the prior context-only selector for comparison. Listening quality of the new
default remains unverified. On the confirmed “Папа купил хлеб.” recording,
a rough 20-band log-spectrum DTW score dropped from 0.303 for the earlier
selector to 0.0858 after phrase tone, candidate pruning, and removal of an
extra output resampling step (lower
is closer; this is a diagnostic, not a listening judgment).

The supplied `ISEF` language file contains 112,699 lexicon records. Each
carries a compressed word key and phone sequence. The original LTS decision
trees handle missing entries, including every word in the confirmed reference
phrase. The remaining pronunciation differences are in token normalization,
POS selection, compound handling, and postlex context.

The renderer's pitch and duration stretch controls default to 1.0. The
original's ordinary pitch and timing can therefore depend chiefly on the
selected recorded units and the duration/intonation target features. That is
an inference from static code, not a measurement of which controls were set
for the supplied MP3.

The original voice registers `FUN_1800304d0` as `duration_func`. For speech
segments it evaluates `zdur_cart` or `dur_cart` plus `dur_stats`; it sums each
result into the segment `end` feature. Pause segments take separate paths:
the initial pause is clamped to 10–50 ms, `SB` is 250 ms, an ordinary break is
200 ms, and `BB` uses the pause base plus a clamped end component (with another
100 ms when followed by a segment). Token `rvd_pause` or a configured
`break_dur_cart` may override some cases. These observations come from REA
decompilation of `voice_ru_maxim.dll` `FUN_1800304d0` and `FUN_180030450`.
The supplied VOX has `zdur_cart` in sections 8 (142 feature/value strings),
9 (46 phone statistics), and 10 (10,823 tree nodes). `src/duration.rs`
interprets the original nodes and base/scale formula. Its relation-feature
provider now covers the checked reference phrase. Its phrase-end tone input
uses the extracted Russian question-pattern table, but `gpos` and the
question-word branch remain approximations for arbitrary text. The diagnostic
`duration-text` reports any feature path that fell back to zero. Four rare
phones have no matching name in the 142-entry duration string table; the
original search retains its first statistics row as a fallback, and Rust does
the same.
