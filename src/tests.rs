use super::*;
use crate::audio::normalize_leading_silence;
use crate::corpus::{deobfuscate, silk_rate, DecodedUnit};
use crate::features::FeatureDb;
use crate::lexicon::Lexicon;
use crate::synthesis::{trim_pause_silence, Stitcher};

#[test]
fn spaces_join_words_and_final_obstruents_devoice() {
    let phones = phonemes("Папа купил хлеб.").unwrap();
    assert_eq!(phones.iter().filter(|p| *p == "pau").count(), 2);
    assert_eq!(&phones[phones.len() - 2..], ["p", "pau"]);
}

#[test]
fn consonants_assimilate_before_voiced_obstruents() {
    let phones = phonemes("голос звучит").unwrap();
    assert!(phones.windows(2).any(|p| p == ["z", "z"]));
}

#[test]
fn iotated_vowels_keep_their_initial_y_sound() {
    assert_eq!(phonemes("Я").unwrap(), ["pau", "j", "a", "pau"]);
    assert_eq!(
        phonemes("поёт").unwrap(),
        ["pau", "p", "o", "j", "o", "t", "pau"]
    );
    assert_eq!(phonemes("мяч").unwrap(), ["pau", "mj", "a", "ch", "pau"]);
}

#[test]
fn original_lts_supplies_reduced_vowel_for_reference_phrase() {
    let language = Path::new("extracted/app/vox/lang_ru_ru.dat");
    let plugin = Path::new("extracted/app/x64/voice_ru_maxim.dll");
    let vox = Path::new("extracted/app/vox/vox_ru_maxim22i");
    if !language.exists() || !plugin.exists() || !vox.exists() {
        return;
    }
    let lexicon = Lexicon::open(language, plugin).unwrap();
    assert_eq!(
        crate::text::phonemes_original("Папа купил хлеб.", &lexicon).unwrap(),
        ["pau", "p", "a", "p", "ax", "k", "u", "pj", "i", "l", "h", "lj", "e", "p", "pau"]
    );
    let pronounced = crate::text::pronounced_original("Папа купил хлеб.", &lexicon).unwrap();
    assert_eq!(pronounced.stress[2], Some(1));
    assert_eq!(pronounced.stress[4], Some(0));
    assert_eq!(pronounced.punctuation.last().copied(), Some(Some('.')));
    assert_eq!(
        pronounced
            .syllables
            .iter()
            .map(|s| (s.word, s.index_in_word, s.start, s.end, s.stress))
            .collect::<Vec<_>>(),
        [
            (0, 0, 1, 3, 1),
            (0, 1, 3, 5, 0),
            (1, 0, 5, 7, 0),
            (1, 1, 7, 10, 1),
            (2, 0, 10, 14, 1)
        ]
    );
    let mut db = VoiceDb::open(vox).unwrap();
    let features = FeatureDb::load(&mut db).unwrap();
    assert_eq!(features.stress(&mut db, 66788).unwrap(), 1);
    assert_eq!(features.stress(&mut db, 145126).unwrap(), 0);
}

#[test]
fn mixed_text_reaches_the_original_pronunciation_path() {
    let language = Path::new("extracted/app/vox/lang_ru_ru.dat");
    let plugin = Path::new("extracted/app/x64/voice_ru_maxim.dll");
    if !language.exists() || !plugin.exists() {
        return;
    }
    let lexicon = Lexicon::open(language, plugin).unwrap();
    let pronounced = crate::text::pronounced_original("Hello, мир! №42 😊", &lexicon).unwrap();
    let words: Vec<_> = pronounced
        .words
        .iter()
        .map(|word| word.name.as_str())
        .collect();
    assert_eq!(words, ["hello", "мир", "номер", "сорок", "два"]);
    assert!(pronounced.names.len() > 10);
    assert_eq!(
        crate::text::pronounced_original("h", &lexicon)
            .unwrap()
            .words[0]
            .name,
        "h"
    );
}

#[test]
fn original_clitic_reduction_uses_extracted_phrase_tables() {
    let language = Path::new("extracted/app/vox/lang_ru_ru.dat");
    let plugin = Path::new("extracted/app/x64/voice_ru_maxim.dll");
    if !language.exists() || !plugin.exists() {
        return;
    }
    let lexicon = Lexicon::open(language, plugin).unwrap();
    let ordinary = crate::text::pronounced_original("Не дома.", &lexicon).unwrap();
    assert_eq!(&ordinary.names[1..3], ["nj", "i"]);
    assert_eq!(ordinary.syllables[0].stress, 0);
    let idiom = crate::text::pronounced_original("Не был.", &lexicon).unwrap();
    assert_eq!(&idiom.names[1..3], ["nj", "e"]);
    assert_eq!(idiom.syllables[0].stress, 1);
    assert_eq!(idiom.syllables[1].stress, 0);
    let compound = crate::text::pronounced_original("Из-под стола.", &lexicon).unwrap();
    assert_eq!(compound.words[0].name, "из-под");
    assert_eq!(
        compound.names.iter().filter(|name| *name == "pau").count(),
        2
    );
}

#[test]
fn recorded_pause_edges_are_shortened_without_losing_speech() {
    let mut samples = vec![0; 4800];
    samples.extend(vec![1000; 2400]);
    samples.extend(vec![0; 4800]);
    let unit = DecodedUnit {
        samples,
        pitchmarks: (240..=12000).step_by(240).collect(),
        available: 12000,
    };
    let trimmed = trim_pause_silence(&unit, true, true, 24000);
    assert!(trimmed.samples.len() < 6000);
    assert_eq!(trimmed.samples.iter().filter(|&&x| x == 1000).count(), 2400);
    assert_eq!(
        trimmed.pitchmarks.last().copied().map(usize::from),
        Some(trimmed.samples.len())
    );
}

#[test]
fn silk_decoder_rate_differs_from_vox_playback_rate() {
    let path = std::path::Path::new("extracted/app/vox/vox_ru_maxim22i");
    if !path.exists() {
        return;
    }
    let db = VoiceDb::open(path).unwrap();
    assert_eq!(db.sample_rate, 22050);
    assert_eq!(silk_rate(&db), 24000);
}

#[test]
fn leading_silence_matches_reference_onset() {
    let mut samples = vec![0; 4400];
    samples.extend(vec![1000; 2200]);
    normalize_leading_silence(&mut samples, 22050);
    let onset = samples.iter().position(|&x| x != 0).unwrap();
    assert!((onset as i32 - 1984).abs() < 220);
}

#[test]
fn pitchmark_join_overlaps_one_period() {
    let first = DecodedUnit {
        samples: vec![1000; 12],
        pitchmarks: vec![4, 8, 12],
        available: 12,
    };
    let second = DecodedUnit {
        samples: vec![-1000; 12],
        pitchmarks: vec![4, 8, 12],
        available: 12,
    };
    let mut stitcher = Stitcher::default();
    stitcher.append(&first).unwrap();
    stitcher.append(&second).unwrap();
    stitcher.flush();
    assert_eq!(stitcher.output.len(), 20);
    assert_eq!(&stitcher.output[..8], &[1000; 8]);
    assert_eq!(&stitcher.output[16..], &[-1000; 4]);
}

#[test]
fn supplied_unit_record_uses_encoded_byte_count() {
    let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
    if !path.exists() {
        return;
    }
    let mut db = VoiceDb::open(path).unwrap();
    let unit = db.unit_record(277).unwrap();
    assert_eq!(unit.sample_skip, 4775);
    assert_eq!(unit.audio_offset, 0x204c7be);
    assert_eq!(unit.encoded_bytes, 897);
    assert_eq!(unit.pitchmarks.last(), Some(&1534));
}

#[test]
fn packet_key_keeps_eight_byte_stride_with_trailing_key_bytes() {
    let mut key = [1, 2, 3, 4, 0, 0, 0, 0];
    let mut packet = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    deobfuscate(&mut packet, &mut key);
    assert_eq!(key, [1, 2, 3, 4, 90, 100, 0, 0]);
    assert_eq!(packet, [11, 22, 29, 44, 104, 88, 70, 80, 90, 100]);
}

#[test]
fn formerly_rejected_corpus_unit_decodes_fully() {
    let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
    if !path.exists() {
        return;
    }
    let mut db = VoiceDb::open(path).unwrap();
    let unit = decode_unit(&mut db, 305).unwrap();
    assert_eq!(unit.available, unit.samples.len());
    assert!(!unit.samples.is_empty());
}

#[test]
fn original_russian_lexicon_decodes_stress_and_reduced_vowels() {
    let language = Path::new("extracted/app/vox/lang_ru_ru.dat");
    let plugin = Path::new("extracted/app/x64/voice_ru_maxim.dll");
    if !language.exists() || !plugin.exists() {
        return;
    }
    let lexicon = Lexicon::open(language, plugin).unwrap();
    assert_eq!(lexicon.len(), 112_699);
    assert_eq!(
        lexicon.lookup("блесна", None).unwrap(),
        ["b", "lj", "i", "s", "n", "a1"]
    );
    assert_eq!(
        lexicon.lookup("хлебопечения", None).unwrap(),
        ["h", "lj", "i", "b", "ax", "pj", "i", "ch", "e1", "nj", "i", "j", "a"]
    );
    for (word, expected) in [
        ("папа", vec!["p", "a1", "p", "ax"]),
        ("купил", vec!["k", "u", "pj", "i1", "l"]),
        ("хлеб", vec!["h", "lj", "e1", "p"]),
        ("мама", vec!["m", "a1", "m", "ax"]),
        ("мыла", vec!["m", "y1", "l", "ax"]),
        ("раму", vec!["r", "a1", "m", "u"]),
    ] {
        assert_eq!(lexicon.pronounce(word, None).unwrap(), expected, "{word}");
    }
}

#[test]
fn compact_candidate_features_expand_from_voice_sections() {
    let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
    if !path.exists() {
        return;
    }
    let mut db = VoiceDb::open(path).unwrap();
    let feature_db = FeatureDb::load(&mut db).unwrap();
    let feature = feature_db.decode(&mut db, 191295).unwrap();
    assert_eq!(feature.context_names, ["0", "pau", "p", "a", "eh"]);
    assert_eq!(feature.section6_offset, 0x20103e);
    assert!((feature.duration_seconds - 0.038).abs() < 0.00001);
    assert_eq!(feature.tail_name, "L*");
    assert_eq!(feature.content_flags, [false; 3]);
    for id in [0, 277, 126192, 451463] {
        assert_eq!(feature_db.decode(&mut db, id).unwrap().unit_id, id);
    }
}

#[test]
fn corpus_context_codes_match_diphone_names() {
    let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
    if !path.exists() {
        return;
    }
    let mut db = VoiceDb::open(path).unwrap();
    assert_eq!(db.phone_code("m").unwrap(), 32);
    assert_eq!(db.phone_code("a").unwrap(), 26);
    let meta = db.unit_metadata(2989).unwrap();
    assert_eq!(&meta[4..6], &[32, 26]);
}
