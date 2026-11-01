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

Pushes to `main` run [GitHub Actions](.github/workflows/build.yml) builds for
Linux x86-64 and ARM64, macOS Intel and Apple Silicon, and Windows x86-64.
Pushes to the `release` branch build the same five archives and publish them as
assets on the GitHub Release tagged with the version from `Cargo.toml` (for
example, `v0.1.0`). A later push with the same version replaces those assets;
bump the package version for a new release. Each `.tar.gz` or `.zip` archive
contains the executable and voice data in the required directory layout.
Extract it and run the executable from the extracted top-level directory.

The `silk-v3-sys` crate builds the SILK C decoder locally. Its build requires a
C compiler and libclang. No Windows DLL or network service is used at runtime.

The repository includes the required voice data. The VOX corpus is tracked as
seven numbered parts of at most 32 MiB, with a manifest and SHA-256 hashes.
The program reads these parts directly; a fresh clone needs no extraction or
assembly script. If you already have the original unsplit VOX file at the
default path, the program can read that too. The supplied installer remains
excluded from Git. `tools/prepare_voice.py` is optional for working with the
installer, and recognizes the checked-in parts as ready.

Source is split into `lexicon.rs` (Russian lexicon and LTS), `corpus.rs` (DDXV
and SILK), `features.rs` (candidate metadata), `join.rs` (original join costs),
`target_cost.rs` (original target cost and active partial structural scoring),
`duration.rs` (original VOX duration CART),
`syllables.rs` (original syllable CART), `postlex.rs` (Russian consonant rules),
`text.rs` (text relations), `normalize.rs` (mixed-text normalization),
`tone.rs` (phrase-end tone tables),
`cli.rs` (argument parsing),
`synthesis.rs` (unit search and stitching),
`audio.rs` (WAV), and `main.rs` (CLI).

## Use

```sh
cd maxim
./target/release/maxim-native 'Привет, мама!'
./target/release/maxim-native 'Привет, мама!' --play
./target/release/maxim-native 'Цена 12,50 ₽. Hello, world!' -o mixed.wav
printf 'Папа купил хлеб.\n' | ./target/release/maxim-native - -o papa.wav

# Compare the earlier letter mapper and join heuristic:
./target/release/maxim-native 'Папа купил хлеб.' -o papa-older.wav --simple-phones --join basic

# Compare the earlier context-only selector:
./target/release/maxim-native 'Папа купил хлеб.' -o papa-heuristic.wav --heuristic-target
```

The first argument is text; quote it so spaces and punctuation stay together.
The default output is `output.wav`; use `-o` to choose another path. Explicit
`speak` is optional. Use `--voice PATH` if the VOX database is elsewhere.
`--play` plays the generated WAV after writing it, using `afplay` on macOS, or
an installed `pw-play`, `paplay`, `aplay`, or `ffplay` on Linux. On Windows it
uses PowerShell's WAV player. The WAV remains at the selected output path.
An interactive terminal shows progress through unit selection and rendering.
Redirected output gets only the final output path. Use `--verbose` for the
phone sequence, selected units, costs, and sample counts; it hides the progress
bar while those details print.
Numbers, common symbols, English letters, and punctuation are accepted; the
number and symbol wording is still a partial port of the original token
normalizer, especially for dates and currency inflection. Unsupported symbols
delimit tokens. The
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
./target/release/maxim-native inspect
./target/release/maxim-native phones 'Я люблю чай.'
./target/release/maxim-native syllables 'Папа купил хлеб.'
./target/release/maxim-native phones-simple 'Я люблю чай.'
./target/release/maxim-native diphone a-b
./target/release/maxim-native features 126192
./target/release/maxim-native duration-text 'Папа купил хлеб.'
./target/release/maxim-native lookup автоматным
./target/release/maxim-native pronounce папа
./target/release/maxim-native decode-unit 4 -o unit.wav
./target/release/maxim-native decode-packet 0x20404fb -o packet.wav
./target/release/maxim-native dump-diphone a-p -o a-p.mdu
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
[GENERATION_PATHS.md](docs/GENERATION_PATHS.md) for the step-by-step original
pipeline, and [RESEARCH.md](docs/RESEARCH.md) for binary evidence and data-layout
notes.

If you have the local reference and comparison WAVs, a rough spectral
comparison is available with:

```sh
python3 tools/compare_waves.py analysis/ref-1.wav comparison/papa-heuristic.wav comparison/papa-current.wav
```

This metric compares spectral trajectories. It does not measure pronunciation
or listening quality.
