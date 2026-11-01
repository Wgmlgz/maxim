# Maxim native prototype

This is an **offline native Rust/C prototype** using the IVONA Maxim voice data
from the supplied Speech2Go package. It turns Russian text into a WAV using
the original compressed lexicon, letter-to-sound rules, syllable decision tree,
postlex consonant tables, voice corpus, SILK decoder, and class-specific join
costs. It is **not yet a faithful port of the full IVONA synthesis engine**:
token normalization, POS, phrasing, intonation, duration targets, and complete
target-based unit selection remain incomplete.

## Build

```sh
cargo build --release
```

The `silk-v3-sys` crate builds the SILK C decoder locally. Its build requires a
C compiler and libclang. No Windows DLL or network service is used at runtime.

Place your supplied `247_S2G_Maxim.exe` in `sources/`, then extract only the
runtime files with `innoextract`:

```sh
python3 tools/prepare_voice.py
```

You can also pass the installer's path as an argument. The installer, extracted
voice data, sample recordings, and generated WAVs are local files excluded
from Git. The 209 MB VOX file exceeds GitHub's regular per-file limit. The
extractor keeps existing voice files intact.

Source is split into `lexicon.rs` (Russian lexicon and LTS), `corpus.rs` (DDXV
and SILK), `features.rs` (candidate metadata), `join.rs` (original join costs),
`target_cost.rs` (original target cost and active partial structural scoring),
`duration.rs` (original VOX duration CART),
`syllables.rs` (original syllable CART), `postlex.rs` (Russian consonant rules),
`text.rs` (text relations), `tone.rs` (phrase-end tone tables),
`synthesis.rs` (unit search and stitching),
`audio.rs` (WAV), and `main.rs` (CLI).

## Use

```sh
cd maxim
./target/release/maxim-native speak ./extracted/app/vox/vox_ru_maxim22i 'Привет, мама!' ./output.wav

# Compare the earlier letter mapper and join heuristic:
./target/release/maxim-native speak ./extracted/app/vox/vox_ru_maxim22i 'Папа купил хлеб.' ./papa-older.wav --simple-phones --basic-join

# Compare the earlier context-only selector:
./target/release/maxim-native speak ./extracted/app/vox/vox_ru_maxim22i 'Папа купил хлеб.' ./papa-heuristic.wav --heuristic-target
```

`TEXT` is the third argument; quote it so spaces and punctuation stay together.
The last argument is the output WAV path. The program supports Russian letters,
spaces, and common punctuation; unsupported characters return an error. The
`speak` output is mono 16-bit PCM at 22.05 kHz, matching the supplied reference
MP3s. SILK decoding internally requests 24 kHz, but the original renderer
plays those decoded pitchmark periods at the VOX header rate of 22.05 kHz.
Diagnostic `decode-unit` and `decode-packet` now use that playback rate too.
Keep `lang_ru_ru.dat` beside the VOX file and `x64/voice_ru_maxim.dll` in the
sibling `x64` directory. The DLL is read as data; it is not executed. Some
phone combinations may lack a diphone and fail rather than produce audio.
Internal hyphens are preserved for entries in the original lexicon.

Diagnostic commands:

```sh
VOX=./extracted/app/vox/vox_ru_maxim22i
./target/release/maxim-native inspect "$VOX"
./target/release/maxim-native phones "$VOX" 'Я люблю чай.'
./target/release/maxim-native syllables "$VOX" 'Папа купил хлеб.'
./target/release/maxim-native phones-simple "$VOX" 'Я люблю чай.'
./target/release/maxim-native diphone "$VOX" a-b
./target/release/maxim-native features "$VOX" 126192
./target/release/maxim-native duration-text "$VOX" 'Папа купил хлеб.'
./target/release/maxim-native lookup ./extracted/app/vox/lang_ru_ru.dat ./extracted/app/x64/voice_ru_maxim.dll автоматным
./target/release/maxim-native pronounce ./extracted/app/vox/lang_ru_ru.dat ./extracted/app/x64/voice_ru_maxim.dll папа
./target/release/maxim-native decode-unit "$VOX" 4 ./unit.wav
./target/release/maxim-native decode-packet "$VOX" 0x20404fb ./packet.wav
./target/release/maxim-native dump-diphone "$VOX" a-p ./a-p.mdu
```

## Port boundary

The voice database contains 1,612 diphones and 565,950 candidate unit IDs.
`lookup` reads only exact lexicon entries; `pronounce` uses the same lexicon and
the original LTS fallback. `speak` now uses that pronunciation path and the
five original class-specific join costs by default. The default selector uses
the original target-cost terms whose
text inputs are known, estimates from the original duration CART, phrase end
tone, stress-first
candidate filtering, context retention, and the original 50-state beam and join
costs. It is still incomplete because word class and phrase features are
approximated. `--heuristic-target`, `--basic-join`, and `--acoustic-join`
retain earlier experiments.
Recorded silence in `pau` units is trimmed at pitchmark boundaries, shortening
the excessive gaps that occurred after punctuation.
A complete port still needs the original tokenizer, POS and phrasing rules,
full postlex phrase handling, exact duration and intonation targets, and full target cost integration. See
[GENERATION_PATHS.md](GENERATION_PATHS.md) for the step-by-step original
pipeline, and [RESEARCH.md](RESEARCH.md) for binary evidence and data-layout
notes.

If you have the local reference and comparison WAVs, a rough spectral
comparison is available with:

```sh
python3 tools/compare_waves.py analysis/ref-1.wav comparison/papa-heuristic.wav comparison/papa-current.wav
```

This metric compares spectral trajectories. It does not measure pronunciation
or listening quality.
