//! Russian consonant postlexical substitutions from voice_ru_maxim.dll.
//! The tables are extracted by tools/extract_postlex_tables.py. The conditions
//! follow FUN_18000eca0, FUN_18000ef10, and FUN_18000f070.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::syllables::phone_feature;
use crate::text::{Syllable, Word};

struct Tables {
    devoice: HashMap<&'static str, &'static str>,
    voice: HashMap<&'static str, &'static str>,
    sonorants: HashSet<&'static str>,
    exceptions: HashSet<&'static str>,
    reduction_anchors: HashSet<&'static str>,
    reduction_pairs: HashSet<(&'static str, &'static str)>,
    reduction_triples: Vec<[&'static str; 3]>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut result = Tables {
            devoice: HashMap::new(),
            voice: HashMap::new(),
            sonorants: HashSet::new(),
            exceptions: HashSet::new(),
            reduction_anchors: HashSet::new(),
            reduction_pairs: HashSet::new(),
            reduction_triples: Vec::new(),
        };
        for line in include_str!("../assets/postlex_tables.txt").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            match parts.as_slice() {
                ["devoice", from, to] => {
                    result.devoice.insert(from, to);
                }
                ["voice", from, to] => {
                    result.voice.insert(from, to);
                }
                ["sonorant", name] => {
                    result.sonorants.insert(name);
                }
                ["exception", name] => {
                    result.exceptions.insert(name);
                }
                ["reduction_anchor", name] => {
                    result.reduction_anchors.insert(name);
                }
                ["reduction_pair", first, second] => {
                    result.reduction_pairs.insert((first, second));
                }
                ["reduction_triple", first, second, third] => {
                    result.reduction_triples.push([first, second, third]);
                }
                _ if line.starts_with('#') => {}
                _ => panic!("invalid extracted postlex table"),
            }
        }
        result
    })
}

pub(crate) fn is_listed_function_word(word: &str) -> bool {
    let data = tables();
    data.reduction_anchors.contains(word) || data.exceptions.contains(word)
}

fn reduce_word(
    phones: &mut [String],
    stress: &mut [Option<u8>],
    syllables: &mut [Syllable],
    word: &Word,
) {
    for index in word.start..word.end {
        stress[index] = stress[index].map(|_| 0);
        phones[index] = match phones[index].as_str() {
            "e" => "i".to_string(),
            "o" => "a".to_string(),
            _ => continue,
        };
    }
    for syllable in syllables.iter_mut() {
        if syllable.start >= word.start && syllable.end <= word.end {
            syllable.stress = 0;
        }
    }
}

/// Phrase-context clitic reduction from FUN_18000e950/FUN_18000ee20.
/// A listed preceding function word loses stress unless an idiom pair shifts
/// the reduction to the following word. The DLL also has three triple cases.
pub(crate) fn reduce_clitics(
    phones: &mut [String],
    stress: &mut [Option<u8>],
    syllables: &mut [Syllable],
    words: &[Word],
) {
    let data = tables();
    for current in 1..words.len() {
        let previous = current - 1;
        if words[previous].end != words[current].start
            || !data
                .reduction_anchors
                .contains(words[previous].name.as_str())
        {
            continue;
        }
        let preceding_name = words[previous].name.as_str();
        let current_name = words[current].name.as_str();
        if data
            .reduction_pairs
            .contains(&(preceding_name, current_name))
        {
            reduce_word(phones, stress, syllables, &words[current]);
            continue;
        }
        let next = words
            .get(current + 1)
            .filter(|next| words[current].end == next.start);
        if let Some(next) = next {
            let triple = [preceding_name, current_name, next.name.as_str()];
            if data
                .reduction_triples
                .first()
                .is_some_and(|pattern| *pattern == triple)
            {
                reduce_word(phones, stress, syllables, &words[previous]);
                reduce_word(phones, stress, syllables, &words[current]);
                continue;
            }
            if data
                .reduction_triples
                .iter()
                .skip(1)
                .any(|pattern| *pattern == triple)
            {
                reduce_word(phones, stress, syllables, &words[previous]);
                reduce_word(phones, stress, syllables, next);
                continue;
            }
        }
        reduce_word(phones, stress, syllables, &words[previous]);
    }
}

pub(crate) fn apply(phones: &mut [String], words: &[Word]) {
    let data = tables();
    let mut word_by_phone = vec![None; phones.len()];
    for (index, word) in words.iter().enumerate() {
        for slot in &mut word_by_phone[word.start..word.end] {
            *slot = Some(index);
        }
    }
    // The original callback walks the Segment relation in forward order and
    // changes each name in place. A later segment therefore sees earlier edits.
    for index in 0..phones.len() {
        let Some(word_index) = word_by_phone[index] else {
            continue;
        };
        let current = phones[index].as_str();
        if !data.devoice.contains_key(current) && !data.voice.contains_key(current) {
            continue;
        }
        let next = phones.get(index + 1).map(String::as_str);
        let word_final = index + 1 == words[word_index].end;
        let exception = word_final && data.exceptions.contains(words[word_index].name.as_str());
        let next_cvox = next.map(|phone| phone_feature(phone, 1));
        let next_vowel = next.is_some_and(|phone| phone_feature(phone, 0) == b'+');
        let next_sonorant = next.is_some_and(|phone| data.sonorants.contains(phone));
        if let Some(replacement) = data.devoice.get(current) {
            let should_devoice = if next.is_none() || next == Some("pau") {
                true
            } else if exception {
                next_cvox == Some(b'-')
            } else if word_final {
                next_cvox != Some(b'+') || next_sonorant
            } else {
                next_cvox == Some(b'-')
            };
            if should_devoice {
                phones[index] = (*replacement).to_string();
            }
        } else if let Some(replacement) = data.voice.get(current) {
            let should_voice = if next.is_none() || next == Some("pau") {
                false
            } else if exception {
                next_cvox == Some(b'+') || next_vowel
            } else {
                next_cvox == Some(b'+') && !next_sonorant
            };
            if should_voice {
                phones[index] = (*replacement).to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_case(raw: &str, words: &[(&str, usize, usize)]) -> String {
        let mut phones: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
        let records: Vec<Word> = words
            .iter()
            .map(|&(name, start, end)| Word {
                name: name.to_string(),
                start,
                end,
            })
            .collect();
        apply(&mut phones, &records);
        phones.join(" ")
    }

    #[test]
    fn word_end_and_preposition_paths() {
        assert_eq!(apply_case("b pau", &[("хлеб", 0, 1)]), "p pau");
        assert_eq!(apply_case("b d", &[("хлеб", 0, 1), ("дом", 1, 2)]), "b t");
        assert_eq!(apply_case("p b", &[("суп", 0, 1), ("был", 1, 2)]), "b p");
        assert_eq!(apply_case("v m", &[("в", 0, 1), ("мир", 1, 2)]), "v m");
        assert_eq!(apply_case("v m", &[("лев", 0, 1), ("мир", 1, 2)]), "f m");
    }
}
