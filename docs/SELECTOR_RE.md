# Original unit selection: current evidence

This is a static reverse-engineering map of the shipped `voice_ru_maxim.dll`
(`6c1bb6badc0d6aa3a2b6dbffeb721e027c00fd0eab24b021140e372bf10d5d56`)
and `tts_engine.dll`. It does not claim to observe a particular rendered
utterance at runtime.

## Data path

1. The voice loader `FUN_18000a060` reads DDXV sections and stores pointers for
   section 3 (20-byte diphone index), 4 (candidate unit IDs), 5 (36-byte per-unit
   records), 6 (variable data), and 7 (32-byte records). The wrapper
   `FUN_18000a4b0` requires these sections, exposes the database object as
   `diphonedb`, and registers a duration CART as `zdur_cart` when present.
   Evidence: `ev_1991773ffb438683340e1a02f63d2d0b3c40f0912ef460158377905fc9a5099f`,
   `ev_ada604da9d43bc758d0d0c1bbf652c085bbcfa169a67b68a1280ac72c8665671`.
   Corpus inspection identifies section 7 as a 57-entry, 32-byte phoneme name
   table (`13=ax`, `25=p`, `26=a`). Section 8 is the duration model's
   142-entry, 64-byte feature-string table (first entry begins `=n.name`).
2. The core `FUN_18002e160` iterates the `Unit` relation, creates one
   0xb8-byte feature record for each `Segment` through `FUN_180034000`, invokes
   the configured `diphone_graph_func`, then runs `FUN_1800314b0` with the
   `viterbi_beam` setting. It writes selected diphone indices back to `Unit`
   nodes. Evidence: `ev_32575b3e2b8b57e28b791175721bd1f2478cd34ad499b49f21c7fb9d38cff45d`.
3. The Russian plugin supplies `diphone_graph_func` as `FUN_18000d580`.
   For each adjacent segment pair, it formats a `phone-phone` key, binary
   searches the diphone index via `FUN_180009d00`, copies both 0xb8-byte
   target records into a graph node, and sets a class-dependent concentration
   cost from `conc_cost_trill`, `conc_cost_vowel`, `conc_cost_voiced`,
   `conc_cost_glide`, or `conc_cost_unvoiced`. It also uses `tb_min_distance`.
   Evidence: `ev_53050c92729446f4f59ededcbdacff4e079f560f076910d6a0ded87f9e6e5590`,
   `ev_d47c5b5473ed4e41da74891ffd158fedb81056d7a781faeff89c5542f9eef174`.
   Core `FUN_18002bdc0` expands each compact 36-byte section-5 candidate row
   into a 0xb8-byte feature record. It maps row bytes 2–6 through the section-7
   name table, and uses the row's final `u32` as an offset into section 6.
   Evidence: `ev_a58ec7498643bf5f5a5316b73e44224170df5c200c295fc20dc534ee71af8538`.
4. The graph's target-cost callback is `FUN_18000e110`. It sums
   `FUN_18000d9f0` for each half of a candidate diphone and adds extra
   boundary/continuity penalties, including a large 350- or 500-point penalty
   for particular mismatched boundary flags. Evidence:
   `ev_0592e9ee18364ca43b35a5b900da4d0f75d83707b27e555b5cb534e510a379e8`.
5. `FUN_18000d9f0` compares a large feature vector: neighboring phoneme
   strings, multiple ordinal/flag bytes, stress-related flags, syllable and
   phrase context, and a floating-point target value. Some mismatches cost
   1000 or more before final 0.4 scaling. The context differences are weighted
   differently by phoneme class. Evidence:
   `ev_2b5315e0afd6bae71bbb8ab49884f2158af92c7b84ed3e3432fd988b060eafe1`.
6. Core `FUN_180031120` expands candidate layers and `FUN_1800314b0` runs the
   Viterbi search. Core `FUN_180031ad0` hard-filters vowel candidates by
   comparing section-6 byte 2's low nibble with target feature byte `0x35`;
   the candidate blacklist check is `FUN_180029400`. Evidence:
   `ev_18c46c024ac806eea8838e3a88c47ceb654429b41ceb387413a9bfd10a8b49d0`,
   `ev_40a666d59fc40f3e0164f64031164c052b7318b1e2887ee873bb7940f386ca2a`,
   `ev_da5053ce77ef80914f97a199fcb66ea71c7abf7cc59fe349befb6c313de9c239`,
   `ev_c43c5e3a8ae501a2eba5ef354c891e263ccecb1a0ddee01f9a1853d94120402d`.
   Candidate expansion makes a first pass over candidates that satisfy the
   hard filter. If it collects fewer than 51, a second pass adds candidates
   that failed the filter. Every admitted candidate gets the two expanded
   0xb8-byte feature records and the pair target cost. The candidates are
   sorted by that cost. Viterbi accumulates target and transition costs and
   stores backpointers. Consecutive unit IDs take a zero-cost continuation;
   other transitions call the class-specific cost function with the two raw
   36-byte section-5 records. This is a material difference from the Rust
   selector's fixed `-80` continuity bonus.

## Current native selector status

The CLI default now builds partial target records from the ported word,
syllable, postlex, duration, and phrase-tone stages. `known_pair` evaluates the original
single-half terms with known text inputs plus all three cross-half boundary
terms. The candidate pass admits stress-compatible vowels first; it uses the
second pass when fewer than 51 survive. It retains a target-cost band and
recorded-context matches, then uses a 50-state Viterbi beam with original
class-specific join costs and zero-cost adjacent-ID continuation. The prior
context-only selector is available with `--heuristic-target`.

This is still partial: `gpos`, some phrase-tone decisions, glottal-stop, and
some other text features are inferred or missing. The phrase-end tone table
and `или`/`либо` continuation branch from `FUN_180010db0` are now active in
the default text target and duration feature paths. The original
`FUN_180030d90` also has graph
flags and context-sensitive pruning not fully represented in Rust. These gaps
can change the selected units even though the component cost functions are
ported. REA Evidence for the original pruning and beam:
`FUN_180030d90`, `FUN_1800314b0`, `FUN_180031120` in `tts_engine.dll`.

## Gaps to resolve before porting faithfully

- `FUN_180034000` is now traced (Evidence
  `ev_7615e6d0756a6a3bd39db5991b673e81d741d746b89bc3e64df133ddcf647f72`).
  It builds the 0xb8-byte target record from Segment, SylStructure, Word,
  Phrase, and phonetic feature paths: phone context, stress, syllable and word
  positions/counts, phrase counts/position, word class, end tone, and duration.
  The unresolved part is producing those source relations from arbitrary text.
- The seven class-specific join-cost functions are traced below. The numeric
  feature meanings and the target-side linguistic values remain unresolved.
- The original linguistic stages must populate the target features, especially
  stress and duration, before target-cost parity is meaningful.

The earlier Rust selector used neighboring phoneme codes and an ID-continuity
bonus only. That path remains behind `--heuristic-target` for comparison.

## Original join-cost path

Core Viterbi `FUN_1800314b0` bypasses the join callback when the previous unit
ID plus one equals the current ID. Otherwise it passes two *compact* 36-byte
section-5 rows to the class-specific callback: row `previous_id + 1` and row
`current_id`. The third argument is the remaining cost budget, allowing an
early rejection. A callback returns `1,000,000` for an impossible or too-costly
join. Evidence: `ev_40a666d59fc40f3e0164f64031164c052b7318b1e2887ee873bb7940f386ca2a`.

Voice `FUN_180031460` unwraps a type-`0x3b` function resource. Core setup
`FUN_1800246c0` registers: vowel `FUN_180024400`, trill `FUN_180023b70`,
glide `FUN_1800232c0`, unvoiced `FUN_180023e30`, voiced `FUN_180024110`,
plus plosive-unvoiced `FUN_180023580` and plosive-voiced `FUN_180023880`.
The Russian graph builder currently requests the first five categories.
Evidence: `ev_470741e2d3b5ebb33209666e9de9467201f00e041cf8313ef7ff08c8ebc1d1cc`,
`ev_3ec483501c635ce44964a84d9fb3aae0fa19184f217183900092ca1a1d48aef8`.

All seven callbacks use section-5 bytes 0–1, 8–15, and 16–31. The first term
is a piecewise cost for the normalized difference between the two little-endian
`u16` values at offset 0. Offset 8 is another `u16` with a large penalty or
rejection for some extreme pairs. Offsets 10, 12, and 14 are `u16` values fed
to three centered difference lookup tables. Bytes 16–31 are compared as a
weighted absolute-difference vector, with a class-specific knee and scale.
The functions differ substantially in how they weight these terms. The
lookup tables are static data pointed to by `PTR_DAT_18012c2d8`,
`PTR_DAT_18012c2e0`, `PTR_DAT_18012c2e8`, and `PTR_DAT_18012c2f0`. Evidence:
`ev_44ee336301e6a06e52ea823c1c8ec149ea42ce4c4a2a881ffef17cc0c464c8b2`,
`ev_c10b3e437f14443a77f894b321a297116b2ff74c8d225516a82820f3fc09e9f6`,
`ev_997ceaf437b0952f99da260aaf4702488da92b0fb2cc7943662544dddf8523a9`,
`ev_9b375e3c7c7f98b89b08788c81075dbd5713d752daf88bd9fedfd72808bab535`,
`ev_ab418d02f7a830ff57f48c7f0087cf5716556ab9b90f533d78952c449a3c92ce`,
`ev_030a21903c076ea0751e5278be52d1f91466c3f0151d2415eac71c143ef1ea71`,
`ev_58572bb23d48ce033d587e167767bd900b9acea8cb8a7bf81f5025940f1fa6ab`.

These are static code and data observations. The names of the numeric and
vector features remain unknown; the call pattern establishes how they are
used, not their physical meaning. The Rust `--acoustic-join` experiment used
decoded-wave RMS and six coarse bands, which is not the original cost.

`src/join.rs` now implements the five classes used by Maxim's graph builder.
`tools/extract_join_luts.py` reproduces the embedded, 12,508-byte
`assets/join_luts.bin` from the supplied `tts_engine.dll` after checking its
SHA-256. The selector exposes the cost as `--original-join` for comparison.
Class choice follows the exact Russian phoneset attributes recorded in
`PHONES_RE.csv`. The existing heuristic target cost and 64-candidate cap still
precede this join function, so the mode is not the original selector as a
whole. On “Папа купил хлеб,” this join mode gives 2.326 against the reference
in a 40-band log-spectrum DTW diagnostic, versus 2.232 for the current
default; lower is closer. Thus it remains opt-in while the target cost and
candidate filtering are incomplete.

`src/features.rs` now decodes a section-5 row through the section-6 variable
record and section-7 name table, retaining raw unidentified fields. The
`features VOX UNIT_ID` command prints the result. This prepares a faithful
selector without injecting guessed feature meanings into the current output.

`src/target_cost.rs` ports the byte/string comparisons, asymmetric stress and
tone penalties, duration penalty, and two-half combination from
`FUN_18000d9f0` and `FUN_18000e110` into an isolated `FeatureRecord` scorer.
The corpus feature decoder builds candidate records. The default `speak`
path uses the subset of this scorer whose target features are currently
available; the full scorer waits for all linguistic target records. Evidence:
`ev_2b5315e0afd6bae71bbb8ab49884f2158af92c7b84ed3e3432fd988b060eafe1`,
`ev_0592e9ee18364ca43b35a5b900da4d0f75d83707b27e555b5cb534e510a379e8`.
