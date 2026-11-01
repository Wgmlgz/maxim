# Original Russian text-to-phone path

This note records static observations from the supplied `tts_engine.dll`,
`voice_ru_maxim.dll`, and `lang_ru_ru.dat`. Pseudocode is Ghidra's
reconstruction, not source code or an observation of a live Windows run.

## Ordered generation stages

The engine's `tts_streamer_synth` constructs an utterance pipeline with
`FUN_180029a70`, runs it with `FUN_1800298f0`, and passes processed data to
`FUN_180016c20` for audio synthesis. It copies `duration_stretch`, `amplify`,
and `num_parallel_threads` from voice properties before processing (REA
Evidence `ev_e382b1f6069935754a26d816db9f7d1275dded691c847af373838350242738d3`,
`ev_fcacbb7163da885bf4fa4a86e5e9be0cb255d610e6f818ad5eb2f5fcda6f38a1`).

`FUN_18002d520` walks a static triplet table. Its exact stage order is:

1. `tokenstotokens_func`
2. `text_analysis_func`
3. `pos_tagger_func`
4. `phrasing_func`
5. `lettertosound_func`
6. `change_language_func`
7. `pause_insertion_func`
8. `postlex_func`
9. `intonation_func`
10. `duration_func`

REA Evidence `ev_38a8c430a2b5e72f086aef0552a54234652466da97647b7ee08a0466c556a12d`
and the table read `ev_76f00ba05e2139624dfe5f4f1220f4ca97b9ad548b170c521346a282c421bbda`
support that sequence. Six stages have engine defaults; the other stages
require a voice callback or prior configuration. The Russian voice registers
token-to-words, sentence boundary, word-to-segments, postlex, syllable
boundaries, xSampa-to-xIvona, and phrase-syllable end tone callbacks
in `FUN_18000e250` (Evidence
`ev_c027d421476c760045f6dbb223a5d03cdf344b0eb10400168344f038b4dc1a80`).
That setup also chooses `ISO-8859-5` text encoding, the `x-ivona-ru`
phoneset, and punctuation/whitespace classes.

## Original pronunciation data

`wordtosegments` (`FUN_18000e8c0`, Evidence
`ev_89a2fda84b89472923c67ed595222da663d48fe64d67cc0966f2b86690d23c05`)
reads word `name` and `pos` and calls `FUN_18000e5f0` (Evidence
`ev_3ea0c6d4e7c66725e97a4fe07869a24f4b313de71926f01010495394630a1b3f`).
The latter consults the compressed `ru_lex` dictionary and has special
handling for punctuation and compound words. Dictionary lookup and LTS
fallback pass through `FUN_1800110f0` and `FUN_180031020` (Evidence
`ev_6b1dc2f8a7bf02764f2eb0736c435f527ba7325ecf0755565967f9ef26255a95`,
`ev_4a0026c682e9e6f18a657bdb9d7b9ec3aed1bd12c4491992c6960606e345232e`).

`FUN_180011000` binds the `ru_lex` struct to language-data bytes supplied by
the engine and static codebooks in the voice plugin (Evidence
`ev_20bec707ec5f1c09c19bb4fad61bb3ec660f06f2b353f38fdd97592e5f4f2df2`).
The engine chooses a locale-specific language data path, falling back to
`lang_<locale>.dat` (`FUN_1800184a0`, Evidence
`ev_80b5035397f186f0f55b1153b61d64656758e0a9816db0a950d1daecb022bc50`).

The supplied ISEF file has a `lex_data` label at offset `0xA4A4`. Its
records contain pronunciation codes, `0xFF`, compressed key codes, and a
NUL terminator. The key codebook is at voice DLL RVA `0x531D0`; phone and
phone-sequence codebooks are at RVAs `0x539E0` and `0x53B90`. Decoding keys
as ISO-8859-5 yields 112,699 entries, matching the count `0x1B83B` set for
`ru_lex`. Each key has a one-character part-of-speech prefix; `0` is used
when no POS is given. `src/lexicon.rs` implements this parser and returns
original segment names, including stress suffix `1` and reduced vowel `ax`.
For example, `блесна` maps to `b lj i s n a1` and `хлебопечения` maps to
`h lj i b ax pj i ch e1 nj i j a`.

Words missing from the dictionary use `ru_lex_lts`, a per-letter decision
tree embedded in the voice DLL. The interpreter (`FUN_1800354e0`, Evidence
`ev_85266c8df40730ad0f261b0f80cdfdf4234a886b45911b28a92a64bd51b30acc`)
pads the word with `000#` and `#000`, builds four-character left and right
contexts for each letter, then traverses six-byte tree nodes with
`FUN_180035420` (Evidence
`ev_150f0776e718ca5709c7cf8c983722acf70020086f45451337b1e0b84d20dee0`).
The tree roots are at RVA `0x54C50`, nodes at `0x54E10`, the alphabet at
`0x54A50`, and output strings at `0x54800`. `src/lexicon.rs` ports this
interpreter. Its original rules produce:

| Word | Original LTS segments |
| --- | --- |
| папа | `p a1 p ax` |
| купил | `k u pj i1 l` |
| хлеб | `h lj e1 p` |
| мама | `m a1 m ax` |
| мыла | `m y1 l ax` |
| раму | `r a1 m u` |

The trailing `0` on LTS output symbols is removed by the original
interpreter; `1` denotes stress and remains. The Rust `pronounce` diagnostic
uses the dictionary first and the original LTS for missing words. This port
does not yet implement original POS tagging or special compound-word logic.

## Postlexical and phone features

The Russian `postlex_func` calls `FUN_18000eca0` and `FUN_18000e950`
(Evidence `ev_814cb2ac72c45b3c17b67e338ae0fa59652807fa03efefe94ce1f713154f874a`).
The first contains voiced/unvoiced consonant substitution tables, governed by
syllable position and neighboring `ph_cvox` (Evidence
`ev_3b86ae4395770171fcc779c495bf5736fa833fe401d9aedc66df4dd37d386759`,
`ev_96a94b3095aa88f0cc5f50e493bd8f4f5d89b1c427a7bf8003429a33bc5463ca`).
The second applies phrase-context cases; `FUN_18000ee20` rewrites `e` to `i`
and `o` to `a` in selected unstressed syllables, and does **not** rewrite
`a` to `ax` (Evidence
`ev_3d716fd1ed560ec3f52704537ea320f47ce51998e9478503c9816e7981bbe2d7`,
`ev_b4e9247e0f92662652dcc0a9eaef6db81fa6940fad8de5782dc4e0d6a8b996f0`).
The `ax` in `папа` comes from LTS.

The `x-ivona-ru` phoneset at voice DLL RVA `0x48D20` has 46 phones. Each has
eight integer attributes: `vc`, `cvox`, `vheight`, `vfront`, `vrnd`, `ctype`,
`cplace`, and `csoft`. `PHONES_RE.csv` records all 46 rows directly from the
name table at RVA `0x48130` and attribute-pointer table at `0x489B0`.

The original syllable-boundary callback `FUN_18000e8a0` calls a 289-node
decision tree rooted at voice DLL VA `0x180049a60`. Its 43 tests inspect nearby
phone attributes and stress; the result chooses the onset size between vowels.
`tools/extract_syllable_cart.py` extracts all descriptors, values, and nodes to
`assets/syllable_cart.bin`. `src/syllables.rs` interprets them. For the checked
LTS words it splits `p a1 | p ax`, `k u | pj i1 l`, and `h lj e1 p` (REA
Evidence `ev_c6d54e597d79bba0cce82f22c689bd32a4fd8abc45dc57907e6fff122d5428cc`,
`ev_931c2d34d223e2fe859901de3b4d29609654433aa0b0c273bd5f1bbcadb1409d`).

The two consonant conversion arrays in `FUN_18000eca0` contain 12 paired
voiced/unvoiced names, including `zzh ↔ sch`. The source DLL also supplies 11
sonorant names and 11 exceptional prepositions. The extraction script writes
these original tables to `assets/postlex_tables.txt`; `src/postlex.rs` applies
the word-final and next-phone conditions traced in `FUN_18000ef10` and
`FUN_18000f070` (REA Evidence
`ev_1b829be8aa02804c1ccb4a9a8d9b1d151a781429d49d1f1f29419f04e1e5f2c3`,
`ev_8fc5d269426b8a4f2d8fab511c491ad76513b8976fa65bf4b4e5e82bd4961d2c`,
`ev_03615bc91e2d6935b8321bbef67b8e5a68a83f27be090811f88a0fdaa6a67613`).
The phrase-context path `FUN_18000e950` checks a DLL table of 31 function
words, 68 idiom pairs, and three triple patterns before calling
`FUN_18000ee20`. That routine clears word syllable stress and changes `e → i`
and `o → a`; `src/postlex.rs` now applies the extracted table to adjacent
words in the same phrase. For example, `не дома` becomes `nj i | d o1 m ax`,
while the table's `не был` pair preserves `не` and reduces `был` (REA Evidence
`ev_457962d7de7a920df4d063d21294a9bd4d42a146519c33a5482b8ed797175e2f`,
`ev_a5bfc83e4adf3c25063156bffac8b56df866ccb99562064a59f1353ad13f145c`).
The exact relationship of punctuation to the original `Phrase` relation is
still being reconstructed; this code treats adjacent words without an inserted
pause as one phrase.

Other registered feature names in the voice plugin include `word_num_syls`,
`word_break`, `word_punc`, `ssyl_in/out`, `syl_in/out`, punctuated-syllable
positions, `syl_break`, onset/coda sizes, `accented`, accent distances,
onset/coda fricative/stop/nasal/glide flags, `position_type`, `syl_final`,
`syl_vowel`, `segment_duration`, phrase counts, and position in word,
phrase, and utterance. These names occur in the shipped plugin's `.rdata`
around file offsets `0xEA1F0`–`0xEA490`; the static inventory alone does not
prove each feature was evaluated for every utterance.

## Remaining porting boundary

The CLI now performs a tolerant normalization pass before the original
lexicon/LTS path. It expands ordinary integers and short decimals into
Russian words, maps common symbols, preserves Latin letters for the supplied
LTS, and treats unsupported characters as token boundaries. The original
`FUN_18000f620` token-to-words callback and `FUN_18000fd10` fallback cover
many more date, currency, abbreviation, and context cases; `FUN_180012370`
dispatches number forms. The current Rust rules are an approximation of those
paths, not a byte-for-byte port of their tables or relation features.

The lexicon, LTS, syllable CART, and consonant postlex now cover their checked
paths. Ordinary text generation still needs the exact original tokenizer and
normalizer (especially dates, currencies, and abbreviations), POS annotation,
full phrasing, intonation,
duration model, and original candidate search/rendering.
These paths are registered and ordered above,
but their complete behavior has not yet been ported or validated against a
live Windows execution.
