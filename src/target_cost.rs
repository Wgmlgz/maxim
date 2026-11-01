//! Target-cost functions recovered from Maxim `voice_ru_maxim.dll`.
//! The default selector uses the terms with available text features; the full
//! scorer remains available for a complete relation graph. Offsets refer to
//! the original 0xb8-byte expanded feature record.

use crate::features::CandidateFeatures;
use crate::postlex;
use crate::syllables::phone_feature;
use crate::text::Pronounced;
use crate::tone;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct FeatureRecord {
    bytes: [u8; 0xb8],
    strings: [Option<Arc<str>>; 12],
}

fn name_slot(offset: usize) -> usize {
    match offset {
        0x08 => 0,
        0x10 => 1,
        0x18 => 2,
        0x20 => 3,
        0x28 => 4,
        0x48 => 5,
        0x58 => 6,
        0x68 => 7,
        0x78 => 8,
        0x88 => 9,
        0x98 => 10,
        0xa0 => 11,
        _ => panic!("unsupported feature name offset {offset:#x}"),
    }
}

impl FeatureRecord {
    pub(crate) fn empty() -> Self {
        Self {
            bytes: [0; 0xb8],
            strings: std::array::from_fn(|_| None),
        }
    }

    pub(crate) fn from_candidate(feature: &CandidateFeatures) -> Self {
        let mut record = Self::empty();
        record.bytes[0..4].copy_from_slice(&feature.duration_seconds.to_le_bytes());
        for (offset, name) in [0x08, 0x10, 0x18, 0x20, 0x28]
            .into_iter()
            .zip(&feature.context_names)
        {
            record.strings[name_slot(offset)] = Some(Arc::clone(name));
        }
        for (offset, name) in [0x48, 0x58, 0x68, 0x78]
            .into_iter()
            .zip(&feature.middle_names)
        {
            record.strings[name_slot(offset)] = Some(Arc::clone(name));
        }
        record.strings[name_slot(0x88)] = Some(Arc::clone(&feature.tail_name));
        record.bytes[0x30] = feature.raw_row[7] & 15;
        record.bytes[0x31] = feature.raw_row[7] >> 4;
        let head = &feature.head_bytes;
        record.bytes[0x32] = head[0] & 15;
        record.bytes[0x33] = head[0] >> 4;
        record.bytes[0x34] = head[1];
        record.bytes[0x35] = head[2] & 15;
        record.bytes[0x36] = head[2] >> 4;
        record.bytes[0x37..0x40].copy_from_slice(&head[3..12]);
        if let Some(value) = feature.optional_float {
            record.bytes[0x40..0x44].copy_from_slice(&value.to_le_bytes());
        }
        if let Some(value) = feature.optional_byte {
            record.bytes[0x44] = value;
        }
        record.bytes[0x50] = feature.middle_flags[5];
        record.bytes[0x51] = feature.middle_flags[6];
        record.bytes[0x52] = head[14];
        for (offset, value) in [0x60, 0x70, 0x71, 0x72, 0x80]
            .into_iter()
            .zip(feature.middle_flags)
        {
            record.bytes[offset] = value;
        }
        record.bytes[0x81..0x87].copy_from_slice(&feature.tail_flags[0..6]);
        record.bytes[0x90] = feature.tail_flags[6];
        record.bytes[0x91] = feature.tail_flags[7];
        for (offset, value) in (0x92..=0x94).zip(feature.content_flags) {
            record.bytes[offset] = u8::from(value);
        }
        record
    }

    pub(crate) fn set_name(&mut self, offset: usize, value: &str) {
        self.strings[name_slot(offset)] = Some(Arc::from(value));
    }

    pub(crate) fn set_byte(&mut self, offset: usize, value: u8) {
        self.bytes[offset] = value;
    }

    pub(crate) fn set_duration(&mut self, seconds: f32) {
        self.bytes[0..4].copy_from_slice(&seconds.to_le_bytes());
    }

    fn name(&self, offset: usize) -> &str {
        self.strings[name_slot(offset)].as_deref().unwrap_or("")
    }

    fn byte(&self, offset: usize) -> u8 {
        self.bytes[offset]
    }

    fn duration(&self) -> f32 {
        f32::from_le_bytes(self.bytes[0..4].try_into().unwrap())
    }
}

/// Fields derived from the original Segment/SylStructure paths in
/// `tts_engine.dll` FUN_180034000. The duration stage fills the duration field
/// separately. Phrase intonation, word POS, and glottal-stop fields await
/// their complete upstream stages.
pub(crate) fn text_records(pronounced: &Pronounced) -> Vec<FeatureRecord> {
    let mut phone_syllable = vec![None; pronounced.names.len()];
    for (index, syllable) in pronounced.syllables.iter().enumerate() {
        for slot in &mut phone_syllable[syllable.start..syllable.end] {
            *slot = Some(index);
        }
    }
    let mut word_syllables = Vec::new();
    for syllable in &pronounced.syllables {
        if word_syllables.len() <= syllable.word {
            word_syllables.resize(syllable.word + 1, 0);
        }
        word_syllables[syllable.word] += 1;
    }
    let mut phrase_for_word = vec![0usize; pronounced.words.len()];
    for word in 1..pronounced.words.len() {
        phrase_for_word[word] = phrase_for_word[word - 1]
            + usize::from(pronounced.words[word - 1].end < pronounced.words[word].start);
    }
    let phrase_count = phrase_for_word.last().copied().map_or(0, |last| last + 1);
    let mut phrase_words = vec![0usize; phrase_count];
    let mut phrase_syllables = vec![0usize; phrase_count];
    for (word, &phrase) in phrase_for_word.iter().enumerate() {
        phrase_words[phrase] += 1;
        phrase_syllables[phrase] += word_syllables[word];
    }
    let phrase_tone = tone::phrase_tones(pronounced, &phrase_for_word);
    let is_content = |word: usize| !postlex::is_listed_function_word(&pronounced.words[word].name);
    pronounced
        .names
        .iter()
        .enumerate()
        .map(|(index, _name)| {
            let mut target = FeatureRecord::empty();
            for (offset, relative) in [(0x08, -2isize), (0x10, -1), (0x18, 0), (0x20, 1), (0x28, 2)]
            {
                let context = index
                    .checked_add_signed(relative)
                    .and_then(|at| pronounced.names.get(at))
                    .map(String::as_str)
                    .unwrap_or("0");
                target.set_name(offset, context);
            }
            let Some(syllable_index) = phone_syllable[index] else {
                return target;
            };
            let syllable = &pronounced.syllables[syllable_index];
            let word = syllable.word;
            let phrase = phrase_for_word[word];
            let phrase_start_word = phrase_for_word
                .iter()
                .position(|&other| other == phrase)
                .unwrap();
            let phrase_start_syllable = pronounced
                .syllables
                .iter()
                .position(|other| phrase_for_word[other.word] == phrase)
                .unwrap();
            let local_syllable = syllable_index - phrase_start_syllable;
            let position = index - syllable.start;
            let length = syllable.end - syllable.start;
            target.set_byte(0x30, (position + 1).min(255) as u8);
            target.set_byte(0x31, (length - position).min(255) as u8);
            target.set_byte(
                0x32,
                syllable_index
                    .checked_sub(1)
                    .and_then(|previous| pronounced.syllables.get(previous))
                    .filter(|previous| previous.word == syllable.word)
                    .map_or(0, |previous| previous.stress),
            );
            target.set_byte(0x35, syllable.stress);
            target.set_byte(0x37, length.min(255) as u8);
            target.set_byte(0x38, (syllable.index_in_word + 1).min(255) as u8);
            target.set_byte(
                0x39,
                (word_syllables[syllable.word] - syllable.index_in_word).min(255) as u8,
            );
            target.set_byte(0x3a, (local_syllable + 1).min(255) as u8);
            target.set_byte(
                0x3b,
                (phrase_syllables[phrase] - local_syllable).min(255) as u8,
            );
            let phrase_end_syllable = phrase_start_syllable + phrase_syllables[phrase];
            let previous_stressed = pronounced.syllables[phrase_start_syllable..syllable_index]
                .iter()
                .filter(|other| other.stress > 0)
                .count();
            let following_stressed = pronounced.syllables[syllable_index + 1..phrase_end_syllable]
                .iter()
                .filter(|other| other.stress > 0)
                .count();
            target.set_byte(0x3c, previous_stressed.min(255) as u8);
            target.set_byte(0x3d, following_stressed.min(255) as u8);
            target.set_byte(
                0x3e,
                pronounced.syllables[phrase_start_syllable..syllable_index]
                    .iter()
                    .rposition(|other| other.stress > 0)
                    .map_or(0, |at| syllable_index - phrase_start_syllable - at)
                    .min(255) as u8,
            );
            target.set_byte(
                0x3f,
                pronounced.syllables[syllable_index + 1..phrase_end_syllable]
                    .iter()
                    .position(|other| other.stress > 0)
                    .map_or(0, |at| at + 1)
                    .min(255) as u8,
            );
            target.set_byte(
                0x50,
                pronounced
                    .syllables
                    .get(syllable_index + 1)
                    .filter(|following| following.word == syllable.word)
                    .map_or(0, |following| following.stress),
            );
            let vowel = pronounced.names[syllable.start..syllable.end]
                .iter()
                .find(|name| {
                    matches!(
                        name.as_str(),
                        "a" | "ax" | "e" | "eh" | "i" | "o" | "u" | "y"
                    )
                })
                .map(String::as_str)
                .unwrap_or("0");
            target.set_name(0x48, vowel);
            target.set_name(0x88, phrase_tone[phrase]);
            for (offset, adjacent) in [
                (0x58, word.checked_sub(1)),
                (0x68, Some(word)),
                (0x78, pronounced.words.get(word + 1).map(|_| word + 1)),
            ] {
                let value = adjacent.map_or("0", |neighbor| {
                    if is_content(neighbor) {
                        "content"
                    } else {
                        "function"
                    }
                });
                target.set_name(offset, value);
            }
            target.set_byte(
                0x60,
                word.checked_sub(1)
                    .map_or(0, |previous| word_syllables[previous].min(255) as u8),
            );
            target.set_byte(0x70, word_syllables[word].min(255) as u8);
            target.set_byte(0x71, (word - phrase_start_word + 1).min(255) as u8);
            target.set_byte(
                0x72,
                (phrase_words[phrase] - (word - phrase_start_word)).min(255) as u8,
            );
            target.set_byte(
                0x80,
                word_syllables.get(word + 1).copied().unwrap_or(0).min(255) as u8,
            );
            target.set_byte(0x83, phrase_syllables[phrase].min(255) as u8);
            target.set_byte(0x84, phrase_words[phrase].min(255) as u8);
            target.set_byte(0x85, (phrase + 1).min(255) as u8);
            target.set_byte(0x86, (phrase_count - phrase).min(255) as u8);
            target.set_byte(0x92, word.checked_sub(1).is_some_and(is_content) as u8);
            target.set_byte(0x93, is_content(word) as u8);
            target.set_byte(
                0x94,
                pronounced
                    .words
                    .get(word + 1)
                    .is_some_and(|_| is_content(word + 1)) as u8,
            );
            target.set_name(0x98, &(phone_feature(_name, 0) as char).to_string());
            target.set_name(0xa0, &(phone_feature(_name, 1) as char).to_string());
            target
        })
        .collect()
}

/// Original target-cost terms whose target inputs are known from the ported
/// word-to-segment and syllable stages. Used only by the opt-in selector mode.
pub(crate) fn known_single(candidate: &FeatureRecord, target: &FeatureRecord) -> i32 {
    let mut cost = 3
        * (i32::from(candidate.name(0x08) != target.name(0x08))
            + i32::from(candidate.name(0x28) != target.name(0x28)))
        + 7 * (i32::from(candidate.name(0x10) != target.name(0x10))
            + i32::from(candidate.name(0x20) != target.name(0x20)));
    if target.name(0x18) == "r" {
        cost *= 3;
    }
    for offset in [0x30, 0x31, 0x32] {
        cost += 3 * difference(candidate.byte(offset), target.byte(offset));
    }
    if candidate.byte(0x35) != target.byte(0x35) {
        cost += 5;
        if target.name(0x98).starts_with('+') {
            cost += 1000;
        }
    }
    cost += 2
        * (difference(candidate.byte(0x38), target.byte(0x38))
            + difference(candidate.byte(0x39), target.byte(0x39)));
    cost += one_to_many_penalty(candidate.byte(0x3a), target.byte(0x3a), 2, 2);
    cost += one_to_many_penalty(candidate.byte(0x3b), target.byte(0x3b), 10, 3);
    cost += 3 * i32::from(candidate.name(0x48) != target.name(0x48));
    cost += 3 * difference(candidate.byte(0x50), target.byte(0x50));
    for offset in [0x3e, 0x3f] {
        cost += i32::from(candidate.byte(offset) != target.byte(offset));
    }
    for offset in [0x70, 0x80, 0x84] {
        cost += i32::from(candidate.byte(offset) != target.byte(offset));
    }
    for offset in [0x71, 0x72] {
        cost += one_to_many_penalty(candidate.byte(offset), target.byte(offset), 2, 1);
    }
    if candidate.byte(0x93) == 0 && target.byte(0x93) != 0 {
        cost += 75;
    } else if candidate.byte(0x93) != 0 && target.byte(0x93) == 0 {
        cost += 50;
    }
    cost += 3 * i32::from(candidate.byte(0x92) != target.byte(0x92));
    cost += 3 * i32::from(candidate.byte(0x94) != target.byte(0x94));
    if target.byte(0x93) == 0 && candidate.name(0x68) != target.name(0x68) {
        cost += 10;
    }
    for offset in [0x85, 0x86] {
        cost += one_to_many_penalty(candidate.byte(offset), target.byte(offset), 1, 1);
    }
    if target.byte(0x72) == 1 {
        let tone_cost = if target.name(0x88) == "L*" && candidate.name(0x88) == "L" {
            0
        } else if target.name(0x88) == "L" && candidate.name(0x88) == "L*" {
            8
        } else {
            10 * i32::from(candidate.name(0x88) != target.name(0x88))
        };
        let boundary_cost = if candidate.byte(0x72) == 1 { 0 } else { 2 };
        if target.byte(0x3b) == 1 {
            let stress_cost = if candidate.byte(0x3b) == 1 { 0 } else { 10 };
            if target.name(0x98).starts_with('+') || !target.name(0xa0).starts_with('-') {
                cost += stress_cost * 150 + tone_cost * 250;
            } else {
                cost += (stress_cost + tone_cost * 2) * 10;
            }
        } else if target.byte(0x35) != 0 {
            let stress_cost =
                if candidate.byte(0x72) > 1 || candidate.byte(0x35) != target.byte(0x35) {
                    10
                } else {
                    0
                };
            cost += (stress_cost + tone_cost)
                * if target.name(0x98).starts_with('+') {
                    25
                } else if target.name(0xa0).starts_with('-') {
                    10
                } else {
                    15
                };
        }
        if target.byte(0x31) == 1 {
            cost += tone_cost
                * if target.name(0xa0).starts_with('-') {
                    20
                } else {
                    40
                };
        }
        cost += tone_cost + boundary_cost * 2;
    }
    if target.duration() > 0.0 {
        cost += duration_penalty(candidate.duration(), target.duration());
    }
    (cost as f32 * 0.4) as i32
}

/// The original pair scorer's cross-half terms depend only on the segment,
/// word, and syllable positions already present in `text_records`.
pub(crate) fn known_pair(
    candidate_left: &FeatureRecord,
    candidate_right: &FeatureRecord,
    target_left: &FeatureRecord,
    target_right: &FeatureRecord,
) -> i32 {
    let mut cost =
        known_single(candidate_left, target_left) + known_single(candidate_right, target_right);
    let sequence = |left: &FeatureRecord, right: &FeatureRecord| {
        left.byte(0x31) == 1
            && left.byte(0x39) == 1
            && right.byte(0x30) == 1
            && right.byte(0x38) == 1
    };
    let candidate_sequence = sequence(candidate_left, candidate_right);
    let target_sequence = sequence(target_left, target_right);
    if candidate_sequence && !target_sequence {
        cost += if target_right.name(0x98).starts_with('+') {
            500
        } else {
            350
        };
    } else if !candidate_sequence && target_sequence {
        cost += 5;
    }
    let left_flags = |record: &FeatureRecord| record.byte(0x30) == 1 && record.byte(0x38) == 1;
    if left_flags(candidate_left) != left_flags(target_left) {
        cost += if target_left.name(0x98).starts_with('+') {
            10
        } else {
            5
        };
    }
    let right_flags = |record: &FeatureRecord| record.byte(0x31) == 1 && record.byte(0x39) == 1;
    if right_flags(candidate_right) != right_flags(target_right) {
        cost += 5;
    }
    cost
}

fn duration_penalty(actual: f32, expected: f32) -> i32 {
    if actual + expected <= 0.0 {
        return 0;
    }
    let ratio = ((actual - expected) / (actual + expected)).abs();
    let mut cost = 0;
    if actual < expected {
        cost += (ratio * 250.0) as i32;
        if ratio > 0.2 {
            cost += (ratio * 120.0 - 24.0) as i32;
        }
        if ratio > 0.3 {
            cost += (ratio * 150.0 - 45.0) as i32;
        }
        if ratio > 0.4 {
            cost += (ratio * 300.0 - 120.0) as i32;
        }
    } else {
        cost += (ratio * 150.0) as i32;
        if ratio > 0.15 {
            cost += (ratio * 150.0 - 22.5) as i32;
        }
        if ratio > 0.35 {
            cost += (ratio * 360.0 - 126.0) as i32;
        }
    }
    cost
}

fn difference(a: u8, b: u8) -> i32 {
    (a as i32 - b as i32).abs()
}

fn one_to_many_penalty(a: u8, b: u8, cost_if_a_one: i32, cost_if_b_one: i32) -> i32 {
    if a == 1 && b > 1 {
        cost_if_a_one
    } else if a > 1 && b == 1 {
        cost_if_b_one
    } else {
        0
    }
}

/// `FUN_18000d9f0(candidate, target)`; exact byte and string comparisons,
/// with target feature names supplied by the caller.
pub(crate) fn single(candidate: &FeatureRecord, target: &FeatureRecord) -> i32 {
    let mut cost = 3
        * (i32::from(candidate.name(0x28) != target.name(0x28))
            + i32::from(candidate.name(0x08) != target.name(0x08)))
        + 7 * (i32::from(candidate.name(0x10) != target.name(0x10))
            + i32::from(candidate.name(0x20) != target.name(0x20)));
    if target.name(0x18) == "r" {
        cost *= 3;
    }
    if target.name(0x10) == "r" || target.name(0x20) == "r" {
        cost *= if target.name(0x98).starts_with('+') {
            6
        } else {
            3
        };
    }
    for offset in [0x30, 0x31, 0x32] {
        cost += 3 * difference(candidate.byte(offset), target.byte(offset));
    }
    if candidate.byte(0x35) != target.byte(0x35) {
        cost += 5;
        if target.name(0x98).starts_with('+') {
            cost += 1000;
        }
    }
    cost += 2
        * (difference(candidate.byte(0x39), target.byte(0x39))
            + difference(candidate.byte(0x38), target.byte(0x38)));
    cost += one_to_many_penalty(candidate.byte(0x3a), target.byte(0x3a), 2, 2);
    cost += one_to_many_penalty(candidate.byte(0x3b), target.byte(0x3b), 10, 3);
    for offset in [0x3e, 0x3f] {
        cost += i32::from(candidate.byte(offset) != target.byte(offset));
    }
    cost += 3
        * (i32::from(candidate.name(0x48) != target.name(0x48))
            + difference(candidate.byte(0x50), target.byte(0x50)));
    for offset in [0x70, 0x80, 0x84] {
        cost += i32::from(candidate.byte(offset) != target.byte(offset));
    }
    for offset in [0x71, 0x72] {
        cost += one_to_many_penalty(candidate.byte(offset), target.byte(offset), 2, 1);
    }
    if candidate.byte(0x93) == 0 && target.byte(0x93) != 0 {
        cost += 75;
    } else if candidate.byte(0x93) != 0 && target.byte(0x93) == 0 {
        cost += 50;
    }
    cost += 3 * i32::from(candidate.byte(0x92) != target.byte(0x92));
    cost += 3 * i32::from(candidate.byte(0x94) != target.byte(0x94));
    if target.byte(0x93) == 0 && candidate.name(0x68) != target.name(0x68) {
        cost += 10;
    }
    for offset in [0x85, 0x86] {
        cost += one_to_many_penalty(candidate.byte(offset), target.byte(offset), 1, 1);
    }
    if target.byte(0x72) == 1 {
        let candidate_tone = candidate.name(0x88);
        let target_tone = target.name(0x88);
        let tone_cost = if target_tone == "L*" && candidate_tone == "L" {
            0
        } else if target_tone == "L" && candidate_tone == "L*" {
            8
        } else {
            10 * i32::from(candidate_tone != target_tone)
        };
        let boundary_cost = if candidate.byte(0x72) == 1 { 0 } else { 2 };
        if target.byte(0x3b) == 1 {
            let stress_cost = if candidate.byte(0x3b) == 1 { 0 } else { 10 };
            if target.name(0x98).starts_with('+') || !target.name(0xa0).starts_with('-') {
                cost += stress_cost * 150 + tone_cost * 250;
            } else {
                cost += (stress_cost + tone_cost * 2) * 10;
            }
        } else if target.byte(0x35) != 0 {
            let stress_cost =
                if candidate.byte(0x72) > 1 || candidate.byte(0x35) != target.byte(0x35) {
                    10
                } else {
                    0
                };
            cost += (stress_cost + tone_cost)
                * if target.name(0x98).starts_with('+') {
                    25
                } else if target.name(0xa0).starts_with('-') {
                    10
                } else {
                    15
                };
        }
        if target.byte(0x31) == 1 {
            cost += tone_cost
                * if target.name(0xa0).starts_with('-') {
                    20
                } else {
                    40
                };
        }
        cost += tone_cost + boundary_cost * 2;
    }
    if (candidate.byte(0x72) == 1 || candidate.byte(0x71) == 1)
        && target.byte(0x3b) > 1
        && target.byte(0x3a) > 1
        && candidate.name(0x88) == "H"
    {
        cost += 1000;
    }
    if candidate.name(0x18) != "pau" {
        let actual = candidate.duration();
        let expected = target.duration();
        cost += duration_penalty(actual, expected);
    }
    (cost as f32 * 0.4) as i32
}

/// `FUN_18000e110`: pair cost plus cross-half boundary penalties.
pub(crate) fn pair(
    candidate_left: &FeatureRecord,
    candidate_right: &FeatureRecord,
    target_left: &FeatureRecord,
    target_right: &FeatureRecord,
) -> i32 {
    let mut cost = single(candidate_left, target_left) + single(candidate_right, target_right);
    let candidate_sequence = candidate_left.byte(0x31) == 1
        && candidate_left.byte(0x39) == 1
        && candidate_right.byte(0x30) == 1
        && candidate_right.byte(0x38) == 1;
    let target_sequence = target_left.byte(0x31) == 1
        && target_left.byte(0x39) == 1
        && target_right.byte(0x30) == 1
        && target_right.byte(0x38) == 1;
    if candidate_sequence && !target_sequence {
        cost += if target_right.name(0x98).starts_with('+') {
            500
        } else {
            350
        };
    } else if !candidate_sequence && target_sequence {
        cost += 5;
    }
    let candidate_left_flags = candidate_left.byte(0x30) == 1 && candidate_left.byte(0x38) == 1;
    let target_left_flags = target_left.byte(0x30) == 1 && target_left.byte(0x38) == 1;
    if candidate_left_flags != target_left_flags {
        cost += if target_left.name(0x98).starts_with('+') {
            10
        } else {
            5
        };
    }
    let candidate_right_flags = candidate_right.byte(0x31) == 1 && candidate_right.byte(0x39) == 1;
    let target_right_flags = target_right.byte(0x31) == 1 && target_right.byte(0x39) == 1;
    if candidate_right_flags != target_right_flags {
        cost += 5;
    }
    cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::VoiceDb;
    use crate::features::FeatureDb;
    use crate::lexicon::Lexicon;
    use std::path::Path;

    #[test]
    fn equal_record_has_zero_target_cost() {
        let mut record = FeatureRecord::empty();
        record.set_name(0x18, "a");
        record.set_duration(0.1);
        assert_eq!(single(&record, &record), 0);
        assert_eq!(pair(&record, &record, &record, &record), 0);
    }

    #[test]
    fn stressed_vowel_mismatch_has_original_large_penalty() {
        let candidate = FeatureRecord::empty();
        let mut target = FeatureRecord::empty();
        target.set_name(0x98, "+");
        target.set_byte(0x35, 1);
        assert_eq!(single(&candidate, &target), 402);
    }

    #[test]
    fn pair_penalty_uses_cross_half_flags() {
        let mut candidate_left = FeatureRecord::empty();
        let mut candidate_right = FeatureRecord::empty();
        let target_left = FeatureRecord::empty();
        let mut target_right = FeatureRecord::empty();
        candidate_left.set_byte(0x31, 1);
        candidate_left.set_byte(0x39, 1);
        candidate_right.set_byte(0x30, 1);
        candidate_right.set_byte(0x38, 1);
        target_right.set_name(0x98, "+");
        let separate =
            single(&candidate_left, &target_left) + single(&candidate_right, &target_right);
        let combined = pair(
            &candidate_left,
            &candidate_right,
            &target_left,
            &target_right,
        );
        assert_eq!(combined - separate, 500);
    }

    #[test]
    fn expanded_record_matches_known_corpus_fields() {
        let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
        if !path.exists() {
            return;
        }
        let mut db = VoiceDb::open(path).unwrap();
        let feature_db = FeatureDb::load(&mut db).unwrap();
        let feature = feature_db.decode(&mut db, 191295).unwrap();
        let record = FeatureRecord::from_candidate(&feature);
        assert_eq!(record.name(0x18), "p");
        assert_eq!(record.name(0x88), "L*");
        assert_eq!(record.byte(0x30), 1);
        assert_eq!(record.byte(0x31), 2);
        assert_eq!(record.byte(0x35), 0);
        assert!((record.duration() - 0.038).abs() < 0.00001);
    }

    #[test]
    fn text_record_uses_ported_syllable_relation_fields() {
        let language = Path::new("extracted/app/vox/lang_ru_ru.dat");
        let plugin = Path::new("extracted/app/x64/voice_ru_maxim.dll");
        if !language.exists() || !plugin.exists() {
            return;
        }
        let lexicon = Lexicon::open(language, plugin).unwrap();
        let pronounced = crate::text::pronounced_original("Папа купил хлеб.", &lexicon).unwrap();
        let records = text_records(&pronounced);
        assert_eq!(records[2].name(0x18), "a");
        assert_eq!(records[2].name(0x48), "a");
        assert_eq!(records[2].byte(0x30), 2);
        assert_eq!(records[2].byte(0x31), 1);
        assert_eq!(records[2].byte(0x35), 1);
        assert_eq!(records[3].byte(0x38), 2);
        assert_eq!(records[3].byte(0x39), 1);
        assert_eq!(records[3].name(0x48), "ax");
        assert_eq!(records[12].name(0x88), "L");
        assert_eq!(records[12].byte(0x72), 1);
    }

    #[test]
    fn known_pair_keeps_original_cross_half_boundary_penalty() {
        let mut candidate_left = FeatureRecord::empty();
        let mut candidate_right = FeatureRecord::empty();
        let target_left = FeatureRecord::empty();
        let mut target_right = FeatureRecord::empty();
        candidate_left.set_byte(0x31, 1);
        candidate_left.set_byte(0x39, 1);
        candidate_right.set_byte(0x30, 1);
        candidate_right.set_byte(0x38, 1);
        target_right.set_name(0x98, "+");
        let separate = known_single(&candidate_left, &target_left)
            + known_single(&candidate_right, &target_right);
        assert_eq!(
            known_pair(
                &candidate_left,
                &candidate_right,
                &target_left,
                &target_right
            ) - separate,
            500
        );
    }
}
