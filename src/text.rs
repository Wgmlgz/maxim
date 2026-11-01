use std::error::Error;

use crate::lexicon::Lexicon;
use crate::postlex;
use crate::syllables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) name: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Syllable {
    pub(crate) word: usize,
    pub(crate) index_in_word: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) stress: u8,
}

pub(crate) struct Pronounced {
    pub(crate) names: Vec<String>,
    pub(crate) stress: Vec<Option<u8>>,
    pub(crate) syllables: Vec<Syllable>,
    pub(crate) words: Vec<Word>,
    pub(crate) punctuation: Vec<Option<char>>,
}

pub(crate) fn phonemes_original(
    text: &str,
    lexicon: &Lexicon,
) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(pronounced_original(text, lexicon)?.names)
}

pub(crate) fn pronounced_original(
    text: &str,
    lexicon: &Lexicon,
) -> Result<Pronounced, Box<dyn Error>> {
    let mut result = vec!["pau".to_string()];
    let mut stress = vec![None];
    let mut punctuation = vec![None];
    let mut syllable_records = Vec::new();
    let mut words = Vec::new();
    let mut word_index = 0;
    let mut word = String::new();
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    for (index, ch) in chars
        .iter()
        .copied()
        .chain(std::iter::once(' '))
        .enumerate()
    {
        let internal_hyphen = ch == '-'
            && !word.is_empty()
            && chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphabetic());
        if ch.is_alphabetic() || internal_hyphen {
            word.push(ch);
            continue;
        }
        if !word.is_empty() {
            let raw_phones = lexicon.pronounce(&word, None)?;
            let word_start = result.len();
            let syllable_breaks = syllables::breaks(&raw_phones);
            for phone in &raw_phones {
                let name = phone.trim_end_matches(|c: char| c.is_ascii_digit());
                if name.is_empty() {
                    return Err(format!("invalid phone {phone:?} for {word}").into());
                }
                let marked = phone[name.len()..]
                    .chars()
                    .last()
                    .and_then(|c| c.to_digit(10));
                let vowel = matches!(name, "a" | "ax" | "e" | "eh" | "i" | "o" | "u" | "y");
                result.push(name.to_string());
                punctuation.push(None);
                stress.push(if vowel {
                    Some(marked.unwrap_or(0) as u8)
                } else {
                    None
                });
            }
            words.push(Word {
                name: word.clone(),
                start: word_start,
                end: result.len(),
            });
            let mut first = 0;
            for (index_in_word, last) in syllable_breaks
                .into_iter()
                .chain(std::iter::once(raw_phones.len().saturating_sub(1)))
                .enumerate()
            {
                if last < first || raw_phones.is_empty() {
                    continue;
                }
                let start = word_start + first;
                let end = word_start + last + 1;
                let syllable_stress = stress[start..end]
                    .iter()
                    .flatten()
                    .copied()
                    .max()
                    .unwrap_or(0);
                syllable_records.push(Syllable {
                    word: word_index,
                    index_in_word,
                    start,
                    end,
                    stress: syllable_stress,
                });
                first = last + 1;
            }
            word_index += 1;
            word.clear();
        }
        if ch.is_whitespace() {
        } else if ",.!?;:—-«»\"".contains(ch) {
            if result.last().is_some_and(|x| x != "pau") {
                result.push("pau".to_string());
                stress.push(None);
                punctuation.push(Some(ch));
            } else if let Some(last) = punctuation.last_mut() {
                *last = Some(ch);
            }
        } else {
            return Err(format!("unsupported character {ch:?}").into());
        }
    }
    if result.last().is_some_and(|x| x != "pau") {
        result.push("pau".to_string());
        stress.push(None);
        punctuation.push(None);
    }
    postlex::apply(&mut result, &words);
    postlex::reduce_clitics(&mut result, &mut stress, &mut syllable_records, &words);
    if result.len() < 3 {
        return Err("input contains no Russian letters".into());
    }
    Ok(Pronounced {
        names: result,
        stress,
        syllables: syllable_records,
        words,
        punctuation,
    })
}

pub(crate) fn phonemes(text: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut result = vec!["pau".to_string()];
    for (i, &ch) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let previous = i.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let needs_j = matches!(ch, 'е' | 'ё' | 'ю' | 'я')
            && (previous.is_none()
                || matches!(
                    previous,
                    Some('а' | 'е' | 'ё' | 'и' | 'о' | 'у' | 'ы' | 'э' | 'ю' | 'я' | 'ь' | 'ъ')
                )
                || previous.is_some_and(|c| c.is_whitespace() || ",.!?;:—-«»\"".contains(c)));
        if needs_j {
            result.push("j".to_string());
        }
        let soft = matches!(next, Some('е' | 'ё' | 'и' | 'ю' | 'я' | 'ь'));
        let phone = match ch {
            'а' | 'я' => "a",
            'б' => {
                if soft {
                    "bj"
                } else {
                    "b"
                }
            }
            'в' => {
                if soft {
                    "vj"
                } else {
                    "v"
                }
            }
            'г' => {
                if soft {
                    "gj"
                } else {
                    "g"
                }
            }
            'д' => {
                if soft {
                    "dj"
                } else {
                    "d"
                }
            }
            'е' => {
                if needs_j
                    || matches!(
                        result.last().map(String::as_str),
                        Some(
                            "bj" | "vj"
                                | "gj"
                                | "dj"
                                | "zj"
                                | "kj"
                                | "lj"
                                | "mj"
                                | "nj"
                                | "pj"
                                | "rj"
                                | "sj"
                                | "tj"
                                | "fj"
                                | "hj"
                        )
                    )
                {
                    "e"
                } else {
                    "eh"
                }
            }
            'э' => "eh",
            'ё' | 'о' => "o",
            'ж' => "zh",
            'з' => {
                if soft {
                    "zj"
                } else {
                    "z"
                }
            }
            'и' => "i",
            'й' => "j",
            'к' => {
                if soft {
                    "kj"
                } else {
                    "k"
                }
            }
            'л' => {
                if soft {
                    "lj"
                } else {
                    "l"
                }
            }
            'м' => {
                if soft {
                    "mj"
                } else {
                    "m"
                }
            }
            'н' => {
                if soft {
                    "nj"
                } else {
                    "n"
                }
            }
            'п' => {
                if soft {
                    "pj"
                } else {
                    "p"
                }
            }
            'р' => {
                if soft {
                    "rj"
                } else {
                    "r"
                }
            }
            'с' => {
                if soft {
                    "sj"
                } else {
                    "s"
                }
            }
            'т' => {
                if soft {
                    "tj"
                } else {
                    "t"
                }
            }
            'у' | 'ю' => "u",
            'ф' => {
                if soft {
                    "fj"
                } else {
                    "f"
                }
            }
            'х' => {
                if soft {
                    "hj"
                } else {
                    "h"
                }
            }
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "sch",
            'ы' => "y",
            'ь' | 'ъ' => continue,
            c if c.is_whitespace() => {
                devoice_last(&mut result);
                continue;
            }
            c if ",.!?;:—-«»\"".contains(c) => {
                devoice_last(&mut result);
                if result.last().is_some_and(|x| x != "pau") {
                    result.push("pau".to_string());
                }
                continue;
            }
            _ => return Err(format!("unsupported character {ch:?}").into()),
        };
        result.push(phone.to_string());
    }
    devoice_last(&mut result);
    if result.last().is_some_and(|x| x != "pau") {
        result.push("pau".to_string());
    }
    assimilate(&mut result);
    if result.len() < 3 {
        return Err("input contains no Russian letters".into());
    }
    Ok(result)
}

fn assimilate(result: &mut [String]) {
    for i in 0..result.len().saturating_sub(1) {
        let next = result[i + 1].as_str();
        if matches!(
            next,
            "p" | "pj"
                | "t"
                | "tj"
                | "k"
                | "kj"
                | "f"
                | "fj"
                | "s"
                | "sj"
                | "sh"
                | "sch"
                | "ch"
                | "ts"
        ) {
            let replacement = match result[i].as_str() {
                "b" => Some("p"),
                "bj" => Some("pj"),
                "d" => Some("t"),
                "dj" => Some("tj"),
                "g" => Some("k"),
                "gj" => Some("kj"),
                "v" => Some("f"),
                "vj" => Some("fj"),
                "z" => Some("s"),
                "zj" => Some("sj"),
                "zh" => Some("sh"),
                _ => None,
            };
            if let Some(p) = replacement {
                result[i] = p.to_string();
            }
        } else if matches!(
            next,
            "b" | "bj" | "d" | "dj" | "g" | "gj" | "z" | "zj" | "zh"
        ) {
            let replacement = match result[i].as_str() {
                "p" => Some("b"),
                "pj" => Some("bj"),
                "t" => Some("d"),
                "tj" => Some("dj"),
                "k" => Some("g"),
                "kj" => Some("gj"),
                "f" => Some("v"),
                "fj" => Some("vj"),
                "s" => Some("z"),
                "sj" => Some("zj"),
                "sh" => Some("zh"),
                _ => None,
            };
            if let Some(p) = replacement {
                result[i] = p.to_string();
            }
        }
    }
}

fn devoice_last(phones: &mut [String]) {
    if let Some(last) = phones.last_mut() {
        let replacement = match last.as_str() {
            "b" => Some("p"),
            "bj" => Some("pj"),
            "d" => Some("t"),
            "dj" => Some("tj"),
            "g" => Some("k"),
            "gj" => Some("kj"),
            "v" => Some("f"),
            "vj" => Some("fj"),
            "z" => Some("s"),
            "zj" => Some("sj"),
            "zh" => Some("sh"),
            _ => None,
        };
        if let Some(p) = replacement {
            *last = p.to_string();
        }
    }
}
