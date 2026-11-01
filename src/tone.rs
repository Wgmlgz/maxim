//! Phrase-end tone rules recovered from voice_ru_maxim.dll FUN_180010db0.
//! The table entries are extracted from the supplied DLL.

use std::sync::OnceLock;

use crate::text::Pronounced;

struct ToneTables {
    high_questions: Vec<Vec<&'static str>>,
    alternatives: Vec<&'static str>,
}

fn tables() -> &'static ToneTables {
    static TABLES: OnceLock<ToneTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut result = ToneTables {
            high_questions: Vec::new(),
            alternatives: Vec::new(),
        };
        for line in include_str!("../assets/tone_tables.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            match fields.next() {
                Some("question_h") => result.high_questions.push(fields.collect()),
                Some("alternative") => result.alternatives.extend(fields),
                _ => panic!("invalid extracted tone table"),
            }
        }
        result
    })
}

pub(crate) fn phrase_tones(
    pronounced: &Pronounced,
    phrase_for_word: &[usize],
) -> Vec<&'static str> {
    let phrase_count = phrase_for_word.last().copied().map_or(0, |last| last + 1);
    let mut tones = vec!["L"; phrase_count];
    let data = tables();
    for phrase in 0..phrase_count {
        let words: Vec<_> = pronounced
            .words
            .iter()
            .enumerate()
            .filter(|(index, _)| phrase_for_word[*index] == phrase)
            .map(|(_, word)| word)
            .collect();
        let Some(last) = words.last() else { continue };
        tones[phrase] = match pronounced.punctuation[last.end] {
            Some('?') => {
                let special_high = data.high_questions.iter().any(|pattern| {
                    pattern.len() == words.len()
                        && pattern
                            .iter()
                            .zip(&words)
                            .all(|(expected, word)| *expected == word.name)
                });
                let alternative_continuation = phrase > 0
                    && words
                        .first()
                        .is_some_and(|word| data.alternatives.contains(&word.name.as_str()));
                if special_high || !alternative_continuation {
                    "H"
                } else {
                    "L"
                }
            }
            Some(',' | ';' | ':' | '—' | '-') => "L*",
            _ => "L",
        };
    }
    tones
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Word;

    #[test]
    fn extracted_patterns_and_alternatives() {
        assert!(tables()
            .high_questions
            .iter()
            .any(|p| p == &["не", "правда", "ли"]));
        assert!(tables().alternatives.contains(&"или"));
        let text = Pronounced {
            names: vec![
                "pau".into(),
                "a".into(),
                "pau".into(),
                "i".into(),
                "pau".into(),
            ],
            stress: vec![None; 5],
            syllables: Vec::new(),
            words: vec![
                Word {
                    name: "да".into(),
                    start: 1,
                    end: 2,
                },
                Word {
                    name: "или".into(),
                    start: 3,
                    end: 4,
                },
            ],
            punctuation: vec![None, None, Some(','), None, Some('?')],
        };
        assert_eq!(phrase_tones(&text, &[0, 1]), vec!["L*", "L"]);
    }
}
