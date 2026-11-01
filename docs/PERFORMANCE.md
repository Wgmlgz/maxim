# Output-preserving performance investigation

All timings below are local release builds on an Apple Silicon Mac with ten
logical CPUs and a warm filesystem cache. They are wall-clock times for one
CLI invocation, including startup. They are useful for comparing changes on
this machine, not guarantees for other platforms.

## Findings and changes retained

1. The original default path decoded every candidate before choosing a unit.
   A macOS CPU sample placed about 95% of its synthesis samples inside SILK
   decoding. The checked-in `vox_ru_maxim22i.unitidx` stores the exact
   decoder-validity and sample-coverage results for all 574,369 unit slots.
   Selection uses this table, then decodes the chosen units normally. If the
   index is absent for another voice, the previous decoding path remains.
2. After indexing, a second CPU sample showed that writing the WAV one 16-bit
   sample at a time dominated a long run. A buffered writer emits the same
   bytes with far fewer system calls.
3. The default selector no longer computes acoustic boundary spectra that
   only the experimental join modes use. Candidate feature names are shared
   immutable strings, and a fixed array replaces the per-candidate tree map.
   Single-phone costs are calculated only for pause phones, where their values
   affect the existing score.
4. The final selected units decode on several threads and are stitched in
   their original path order. `--jobs 1` disables this; `--jobs 0` (the default)
   chooses up to eight workers. For the 12-times-repeated long phrase,
   decode/render/write fell from about 168 ms with one worker to about 31 ms
   with eight after buffering. The total fell from about 1.34 to 1.20 seconds.
5. Opening the Git-tracked VOX parts verifies 207 MB of data and costs about
   0.21 seconds per process start on this machine. Release archives now
   contain the byte-identical assembled VOX, avoiding this startup cost.
   Source checkouts still read the parts directly with no script.

## End-to-end measurements

| Phrase | Current code without index, split VOX | Split VOX with index | Assembled VOX with index |
| --- | ---: | ---: | ---: |
| `Папа купил хлеб.` | 0.94 s | 0.34 s | 0.15 s |
| Four short sentences | 5.61 s | 0.41 s | 0.21 s |

Each number is the median of three warm runs. All outputs in these comparisons
had identical SHA-256 hashes. Separate checks of mixed English/numeric text and
repeated text also matched the previous WAV byte for byte. The four baseline
cases retained the same selected unit IDs with `--verbose`; experimental
heuristic and basic-join modes also retained their previous WAV bytes.

## Experiments discarded

- Parallel candidate selection increased the long phrase's selection time
  from about 0.9 seconds to 1.5–1.8 seconds with four to eight workers. It
  competed for memory and file bandwidth, so selection remains sequential.
- Caching whole candidate lists by exact phone context, stress, and target
  features produced **zero hits** across 768 pairs in a repeated long phrase.
  The target records include phrase-position features; the cache was removed.
- Skipping unused boundary spectra alone produced only a small improvement.
  It remains because it removes unnecessary work without affecting the
  default join scorer.

## Reproducibility and limits

`cargo test --locked` checks 128 deterministic corpus unit IDs against the
tracked index. Rebuilding the full index with two workers produced the same
SHA-256 as the eight-worker build:
`7b8a08c5985797a37fcb8c97a4026632a01a36e82fb00aa22821b6d1823f3b6b`.
The index is bound to the VOX size, header checksum, sample rate, codec, unit
count, and its own record hash. The chosen unit's decoded coverage is checked
again before rendering. Cross-platform GitHub builds have not yet run for
these uncommitted changes.

For further speed, the next target is the remaining candidate feature decoding
and scoring, which takes roughly 70–90 ms for the four-sentence phrase and
about 0.9 seconds for a 12-times-repeated phrase. Any rewrite must preserve
ranking, tie breaks, selected unit IDs, and WAV hashes. A persistent process
could also avoid reopening the voice for every short request.
Set `MAXIM_PROFILE=1` for a local phase-timing line on standard error.
