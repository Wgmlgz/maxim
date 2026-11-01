//! Original `zdur_cart` duration model in DDXV sections 8, 9, and 10.
//! The interpreter follows voice_ru_maxim.dll FUN_18000ba40/FUN_18000b900.

use std::error::Error;
use std::io::{Read, Seek, SeekFrom};

use crate::corpus::VoiceDb;
use crate::postlex;
use crate::syllables::phone_feature;
use crate::text::Pronounced;
use crate::tone;

pub(crate) struct DurationDb {
    names: Vec<String>,
    stats: Vec<[u8; 6]>,
    nodes: Vec<[u8; 4]>,
}

#[derive(Debug)]
pub(crate) struct DurationEstimate {
    pub(crate) base_ms: i16,
    pub(crate) scale_ms: i16,
    pub(crate) cart_value: i16,
    pub(crate) duration_ms: i32,
    pub(crate) missing_features: Vec<String>,
}

impl DurationDb {
    pub(crate) fn load(db: &mut VoiceDb) -> Result<Self, Box<dyn Error>> {
        let mut sections = Vec::new();
        for kind in [8, 9, 10] {
            let section = db.section(kind)?;
            db.file.seek(SeekFrom::Start(section.offset as u64))?;
            let mut bytes = vec![0u8; section.len as usize];
            db.file.read_exact(&mut bytes)?;
            sections.push(bytes);
        }
        if sections[0].len() % 64 != 0 || sections[1].len() % 6 != 0 || sections[2].len() % 4 != 0 {
            return Err("invalid original duration section dimensions".into());
        }
        let names = sections[0]
            .chunks_exact(64)
            .map(|bytes| {
                let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(64);
                String::from_utf8_lossy(&bytes[..end]).to_string()
            })
            .collect();
        let stats = sections[1]
            .chunks_exact(6)
            .map(|row| row.try_into().unwrap())
            .collect();
        let nodes = sections[2]
            .chunks_exact(4)
            .map(|row| row.try_into().unwrap())
            .collect();
        Ok(Self {
            names,
            stats,
            nodes,
        })
    }

    pub(crate) fn stats_for(&self, phone: &str) -> Option<(i16, i16)> {
        let matched = self.stats.iter().find_map(|row| {
            if self.names.get(row[0] as usize).map(String::as_str) == Some(phone) {
                Some((
                    i16::from_le_bytes([row[2], row[3]]),
                    i16::from_le_bytes([row[4], row[5]]),
                ))
            } else {
                None
            }
        });
        matched.or_else(|| {
            // FUN_18000ba40 keeps the first six-byte row when no name matches.
            self.stats.first().map(|row| {
                (
                    i16::from_le_bytes([row[2], row[3]]),
                    i16::from_le_bytes([row[4], row[5]]),
                )
            })
        })
    }

    /// Evaluate the original tree with feature values supplied by the text
    /// relation builder. Missing features use the engine's numeric zero/string
    /// "0" fallback and are returned for diagnostics.
    pub(crate) fn estimate(
        &self,
        phone: &str,
        mut feature: impl FnMut(&str) -> Option<String>,
    ) -> Result<DurationEstimate, Box<dyn Error>> {
        let (base_ms, scale_ms) = self
            .stats_for(phone)
            .ok_or_else(|| format!("no original duration stats for phone {phone}"))?;
        let mut node_index = 0usize;
        let mut missing_features = Vec::new();
        let cart_value = loop {
            let node = self
                .nodes
                .get(node_index)
                .ok_or("duration CART jump out of bounds")?;
            if node[0] == 0xff {
                break i16::from_le_bytes([node[2], node[3]]);
            }
            let query = self
                .names
                .get(node[0] as usize)
                .ok_or("duration CART feature index out of bounds")?;
            let expected = self
                .names
                .get(node[1] as usize)
                .ok_or("duration CART value index out of bounds")?;
            let value = feature(query).unwrap_or_else(|| {
                if !missing_features.contains(query) {
                    missing_features.push(query.clone());
                }
                "0".to_string()
            });
            let equal = if query.starts_with('<') {
                value.parse::<f64>().unwrap_or(0.0) < expected.parse::<f64>().unwrap_or(0.0)
            } else if query.starts_with('>') {
                value.parse::<f64>().unwrap_or(0.0) > expected.parse::<f64>().unwrap_or(0.0)
            } else {
                value == *expected
            };
            node_index = if equal {
                node_index + 1
            } else {
                u16::from_le_bytes([node[2], node[3]]) as usize
            };
        };
        let product = scale_ms as i32 * cart_value as i32;
        let duration_ms =
            base_ms as i32 + (product + if cart_value > 0 { 500 } else { -500 }) / 1000;
        Ok(DurationEstimate {
            base_ms,
            scale_ms,
            cart_value,
            duration_ms,
            missing_features,
        })
    }

    pub(crate) fn estimate_text(
        &self,
        pronounced: &Pronounced,
        position: usize,
    ) -> Result<DurationEstimate, Box<dyn Error>> {
        let name = pronounced
            .names
            .get(position)
            .ok_or("duration phone index out of bounds")?;
        self.estimate(name, |query| text_feature(pronounced, position, query))
    }
}

fn text_feature(text: &Pronounced, position: usize, query: &str) -> Option<String> {
    let path = query.trim_start_matches(['=', '<', '>']);
    let context = if let Some(rest) = path.strip_prefix("pp.") {
        Some((position.checked_sub(2)?, rest))
    } else if let Some(rest) = path.strip_prefix("nn.") {
        Some((position + 2, rest))
    } else if let Some(rest) = path.strip_prefix("p.") {
        Some((position.checked_sub(1)?, rest))
    } else if let Some(rest) = path.strip_prefix("n.") {
        Some((position + 1, rest))
    } else {
        Some((position, path))
    };
    if let Some((index, attribute)) = context {
        if attribute == "name" {
            return Some(
                text.names
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("0")
                    .to_string(),
            );
        }
        let phone = text.names.get(index).map(String::as_str).unwrap_or("pau");
        let number = match attribute {
            "ph_vc" => Some(0),
            "ph_cvox" => Some(1),
            "ph_vheight" => Some(2),
            "ph_vfront" => Some(3),
            "ph_vrnd" => Some(4),
            "ph_ctype" => Some(5),
            "ph_cplace" => Some(6),
            _ => None,
        };
        if let Some(number) = number {
            return Some((phone_feature(phone, number) as char).to_string());
        }
        if attribute == "seg_onsetcoda" {
            let syllable = text
                .syllables
                .iter()
                .find(|s| s.start <= index && index < s.end)?;
            let vowel = (syllable.start..syllable.end)
                .find(|&at| phone_feature(&text.names[at], 0) == b'+')?;
            return Some(if index >= vowel { "coda" } else { "onset" }.to_string());
        }
    }
    let syllable_index = text
        .syllables
        .iter()
        .position(|s| s.start <= position && position < s.end)?;
    let syllable = &text.syllables[syllable_index];
    let same_word_neighbor = |offset: isize| {
        syllable_index
            .checked_add_signed(offset)
            .and_then(|index| text.syllables.get(index))
            .filter(|neighbor| neighbor.word == syllable.word)
    };
    let value = match path {
        "pos_in_syl" => (position - syllable.start).to_string(),
        "syl_final" => u8::from(position + 1 == syllable.end).to_string(),
        "R:SylStructure.parent.stress" => syllable.stress.to_string(),
        "R:SylStructure.parent.parent.word_num_syls" => text
            .syllables
            .iter()
            .filter(|other| other.word == syllable.word)
            .count()
            .to_string(),
        "R:SylStructure.parent.parent.gpos" => {
            if postlex::is_listed_function_word(&text.words[syllable.word].name) {
                "function".to_string()
            } else {
                "content".to_string()
            }
        }
        "R:SylStructure.parent.pos_in_word" => syllable.index_in_word.to_string(),
        "R:SylStructure.parent.R:Syllable.p.stress" => same_word_neighbor(-1)
            .map_or(0, |previous| previous.stress)
            .to_string(),
        "R:SylStructure.parent.R:Syllable.n.stress" => same_word_neighbor(1)
            .map_or(0, |next| next.stress)
            .to_string(),
        "R:SylStructure.parent.syl_break" => syllable_break(text, syllable_index).to_string(),
        "R:SylStructure.parent.R:Syllable.p.syl_break" => same_word_neighbor(-1)
            .map_or(0, |previous| {
                let previous_index = text
                    .syllables
                    .iter()
                    .position(|item| item.start == previous.start)
                    .unwrap();
                syllable_break(text, previous_index)
            })
            .to_string(),
        "R:SylStructure.parent.ssyl_to" => text.syllables[syllable_index + 1..]
            .iter()
            .position(|following| following.stress > 0)
            .map_or(0, |distance| distance + 1)
            .to_string(),
        "R:SylStructure.parent.R:Syllable.n.endtone" => {
            let next = text.syllables.get(syllable_index + 1)?;
            match text.names.get(next.end).map(String::as_str) {
                Some("pau") => {
                    let mut phrase_for_word = vec![0usize; text.words.len()];
                    for word in 1..text.words.len() {
                        phrase_for_word[word] = phrase_for_word[word - 1]
                            + usize::from(text.words[word - 1].end < text.words[word].start);
                    }
                    let phrase = phrase_for_word[next.word];
                    tone::phrase_tones(text, &phrase_for_word)[phrase].to_string()
                }
                _ => "0".to_string(),
            }
        }
        "R:SylStructure.parent.syl_codasize" => {
            let vowel = (syllable.start..syllable.end)
                .find(|&at| phone_feature(&text.names[at], 0) == b'+');
            vowel.map_or(0, |at| syllable.end - at - 1).to_string()
        }
        _ => return None,
    };
    Some(value)
}

fn syllable_break(text: &Pronounced, syllable_index: usize) -> u8 {
    let syllable = &text.syllables[syllable_index];
    if text
        .syllables
        .get(syllable_index + 1)
        .is_some_and(|next| next.word == syllable.word)
    {
        return 0;
    }
    let following = text.names.get(syllable.end).map(String::as_str);
    if following != Some("pau") {
        return 1;
    }
    if syllable.end + 1 == text.names.len()
        || matches!(text.punctuation[syllable.end], Some('.' | '!' | '?'))
    {
        4
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Syllable, Word};
    use std::path::Path;

    #[test]
    fn supplied_duration_sections_have_expected_shape_and_stats() {
        let path = Path::new("extracted/app/vox/vox_ru_maxim22i");
        if !path.exists() {
            return;
        }
        let mut db = VoiceDb::open(path).unwrap();
        let duration = DurationDb::load(&mut db).unwrap();
        assert_eq!(duration.names.len(), 142);
        assert_eq!(duration.stats.len(), 46);
        assert_eq!(duration.nodes.len(), 10823);
        assert_eq!(duration.stats_for("a"), Some((79, 39)));
        assert_eq!(duration.stats_for("p"), Some((87, 31)));
        assert_eq!(duration.stats_for("pau"), Some((743, 396)));
        assert_eq!(duration.stats_for("gj"), Some((109, 27)));
        assert!(duration.estimate("a", |_| None).unwrap().duration_ms > 0);
    }

    #[test]
    fn duration_endtone_uses_phrase_tone() {
        let text = Pronounced {
            names: vec![
                "pau".into(),
                "d".into(),
                "a".into(),
                "pau".into(),
                "i".into(),
                "pau".into(),
            ],
            stress: vec![None; 6],
            syllables: vec![
                Syllable {
                    word: 0,
                    index_in_word: 0,
                    start: 1,
                    end: 3,
                    stress: 1,
                },
                Syllable {
                    word: 1,
                    index_in_word: 0,
                    start: 4,
                    end: 5,
                    stress: 1,
                },
            ],
            words: vec![
                Word {
                    name: "да".into(),
                    start: 1,
                    end: 3,
                },
                Word {
                    name: "или".into(),
                    start: 4,
                    end: 5,
                },
            ],
            punctuation: vec![None, None, None, Some(','), None, Some('?')],
        };
        assert_eq!(
            text_feature(&text, 1, "R:SylStructure.parent.R:Syllable.n.endtone"),
            Some("L".into())
        );
    }
}
