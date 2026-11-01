//! Reader for the Russian `lex_data` section shipped in `lang_ru_ru.dat`.
//! The byte-to-string tables are embedded in `voice_ru_maxim.dll`; reading
//! them as PE data does not execute any Windows code.

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

const KEY_TABLE_RVA: usize = 0x531d0;
const PHONE_TABLE_RVA: usize = 0x539e0;
const EXPANSION_TABLE_RVA: usize = 0x53b90;
const EXPECTED_WORDS: usize = 0x1b83b;
const LTS_LETTERS_RVA: usize = 0x54a50;
const LTS_ROOTS_RVA: usize = 0x54c50;
const LTS_RULES_RVA: usize = 0x54e10;
const LTS_OUTPUTS_RVA: usize = 0x54800;

const ISO_HIGH: [char; 96] = [
    '\u{a0}', '\u{401}', '\u{402}', '\u{403}', '\u{404}', '\u{405}', '\u{406}', '\u{407}',
    '\u{408}', '\u{409}', '\u{40a}', '\u{40b}', '\u{40c}', '\u{ad}', '\u{40e}', '\u{40f}',
    '\u{410}', '\u{411}', '\u{412}', '\u{413}', '\u{414}', '\u{415}', '\u{416}', '\u{417}',
    '\u{418}', '\u{419}', '\u{41a}', '\u{41b}', '\u{41c}', '\u{41d}', '\u{41e}', '\u{41f}',
    '\u{420}', '\u{421}', '\u{422}', '\u{423}', '\u{424}', '\u{425}', '\u{426}', '\u{427}',
    '\u{428}', '\u{429}', '\u{42a}', '\u{42b}', '\u{42c}', '\u{42d}', '\u{42e}', '\u{42f}',
    '\u{430}', '\u{431}', '\u{432}', '\u{433}', '\u{434}', '\u{435}', '\u{436}', '\u{437}',
    '\u{438}', '\u{439}', '\u{43a}', '\u{43b}', '\u{43c}', '\u{43d}', '\u{43e}', '\u{43f}',
    '\u{440}', '\u{441}', '\u{442}', '\u{443}', '\u{444}', '\u{445}', '\u{446}', '\u{447}',
    '\u{448}', '\u{449}', '\u{44a}', '\u{44b}', '\u{44c}', '\u{44d}', '\u{44e}', '\u{44f}',
    '\u{2116}', '\u{451}', '\u{452}', '\u{453}', '\u{454}', '\u{455}', '\u{456}', '\u{457}',
    '\u{458}', '\u{459}', '\u{45a}', '\u{45b}', '\u{45c}', '\u{a7}', '\u{45e}', '\u{45f}',
];

fn iso_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0xa0 {
                b as char
            } else {
                ISO_HIGH[(b - 0xa0) as usize]
            }
        })
        .collect()
}

fn u16_at(data: &[u8], pos: usize) -> Result<usize, Box<dyn Error>> {
    Ok(u16::from_le_bytes(
        data.get(pos..pos + 2)
            .ok_or("short PE header")?
            .try_into()?,
    ) as usize)
}

fn u32_at(data: &[u8], pos: usize) -> Result<usize, Box<dyn Error>> {
    Ok(u32::from_le_bytes(
        data.get(pos..pos + 4)
            .ok_or("short PE header")?
            .try_into()?,
    ) as usize)
}

fn u64_at(data: &[u8], pos: usize) -> Result<usize, Box<dyn Error>> {
    Ok(u64::from_le_bytes(data.get(pos..pos + 8).ok_or("short PE data")?.try_into()?) as usize)
}

struct PeData {
    bytes: Vec<u8>,
    image_base: usize,
    sections: Vec<(usize, usize, usize)>, // RVA, virtual size, file offset
}

impl PeData {
    fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        let bytes = fs::read(path)?;
        if bytes.get(..2) != Some(b"MZ") {
            return Err("not a PE file".into());
        }
        let pe = u32_at(&bytes, 0x3c)?;
        if bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err("invalid PE signature".into());
        }
        let section_count = u16_at(&bytes, pe + 6)?;
        let optional_len = u16_at(&bytes, pe + 20)?;
        let image_base = u64_at(&bytes, pe + 24 + 24)?;
        let section_table = pe + 24 + optional_len;
        let mut sections = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let entry = section_table + 40 * i;
            let virtual_size = u32_at(&bytes, entry + 8)?;
            let rva = u32_at(&bytes, entry + 12)?;
            let file_offset = u32_at(&bytes, entry + 20)?;
            sections.push((rva, virtual_size, file_offset));
        }
        Ok(Self {
            bytes,
            image_base,
            sections,
        })
    }

    fn offset(&self, rva: usize) -> Result<usize, Box<dyn Error>> {
        self.sections
            .iter()
            .find(|&&(start, size, _)| rva >= start && rva - start < size)
            .map(|&(start, _, offset)| offset + rva - start)
            .ok_or_else(|| format!("PE RVA {rva:#x} is outside sections").into())
    }

    fn cstring(&self, va: usize) -> Result<Vec<u8>, Box<dyn Error>> {
        if va < self.image_base {
            return Err("PE pointer below image base".into());
        }
        let start = self.offset(va - self.image_base)?;
        let end = self.bytes[start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or("unterminated PE string")?
            + start;
        Ok(self.bytes[start..end].to_vec())
    }

    fn pointer_table(&self, rva: usize, count: usize) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
        let start = self.offset(rva)?;
        (0..count)
            .map(|i| {
                let va = u64_at(&self.bytes, start + i * 8)?;
                if va == 0 {
                    Ok(Vec::new())
                } else {
                    self.cstring(va)
                }
            })
            .collect()
    }
}

pub(crate) struct Lexicon {
    entries: HashMap<String, Vec<String>>,
    lts: Lts,
}

struct Lts {
    pe: PeData,
    letters: HashMap<u8, usize>,
    roots: Vec<usize>,
    outputs: Vec<Vec<u8>>,
    rules_offset: usize,
}

impl Lts {
    fn new(pe: PeData) -> Result<Self, Box<dyn Error>> {
        let letter_start = pe.offset(LTS_LETTERS_RVA)?;
        let mut letters = HashMap::new();
        for i in 3..62 {
            let va = u64_at(&pe.bytes, letter_start + 8 * i)?;
            let name = pe.cstring(va)?;
            if name.len() != 1 {
                return Err("invalid LTS alphabet".into());
            }
            letters.insert(name[0], i - 3);
        }
        let roots_start = pe.offset(LTS_ROOTS_RVA)?;
        let roots = (0..59)
            .map(|i| u32_at(&pe.bytes, roots_start + i * 4))
            .collect::<Result<Vec<_>, _>>()?;
        let outputs = pe.pointer_table(LTS_OUTPUTS_RVA, (LTS_LETTERS_RVA - LTS_OUTPUTS_RVA) / 8)?;
        let rules_offset = pe.offset(LTS_RULES_RVA)?;
        Ok(Self {
            pe,
            letters,
            roots,
            outputs,
            rules_offset,
        })
    }

    fn pronounce(&self, word: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let mut bytes = Vec::new();
        for ch in word.to_lowercase().chars() {
            let code = if (ch as u32) < 0xa0 {
                ch as u8
            } else {
                ISO_HIGH
                    .iter()
                    .position(|&candidate| candidate == ch)
                    .map(|index| index as u8 + 0xa0)
                    .ok_or_else(|| format!("LTS cannot encode {ch:?}"))?
            };
            bytes.push(code);
        }
        let mut padded = b"000#".to_vec();
        padded.extend_from_slice(&bytes);
        padded.extend_from_slice(b"#000");
        let mut result = Vec::new();
        for (index, &letter) in bytes.iter().enumerate() {
            let Some(&letter_index) = self.letters.get(&letter) else {
                return Err(format!("LTS has no rule for {letter:#x}").into());
            };
            let root = self.roots[letter_index];
            let mut node = root;
            let center = index + 4;
            let mut context = [0u8; 8];
            context[..4].copy_from_slice(&padded[center - 4..center]);
            context[4..].copy_from_slice(&padded[center + 1..center + 5]);
            let output_code = loop {
                let offset = self.rules_offset + node * 6;
                let rule = self
                    .pe
                    .bytes
                    .get(offset..offset + 6)
                    .ok_or("LTS rule outside PE data")?;
                if rule[0] == 0xff {
                    break rule[1] as usize;
                }
                let position = rule[0] as usize;
                let value = *context
                    .get(position)
                    .ok_or("invalid LTS context position")?;
                let branch = if value == rule[1] { 2 } else { 4 };
                node = root + u16::from_le_bytes([rule[branch], rule[branch + 1]]) as usize;
            };
            let output = self.outputs[output_code].as_slice();
            let output = std::str::from_utf8(output)?;
            if output == "epsilon" || output == "nothing" {
                continue;
            }
            for part in output.split('-') {
                let phone = part.strip_suffix('0').unwrap_or(part);
                if !phone.is_empty() {
                    result.push(phone.to_string());
                }
            }
        }
        Ok(result)
    }
}

impl Lexicon {
    pub(crate) fn open(language_data: &Path, voice_plugin: &Path) -> Result<Self, Box<dyn Error>> {
        let pe = PeData::open(voice_plugin)?;
        let keys = pe.pointer_table(KEY_TABLE_RVA, 256)?;
        let phones = pe.pointer_table(PHONE_TABLE_RVA, 256)?;
        let expansions = pe.pointer_table(EXPANSION_TABLE_RVA, 256)?;
        let data = fs::read(language_data)?;
        if data.get(..4) != Some(b"ISEF") {
            return Err("not an ISEF language data file".into());
        }
        let label = b"lex_data\0";
        let section = data
            .windows(label.len())
            .position(|window| window == label)
            .ok_or("language file has no lex_data section")?;
        let first_marker = data[section + label.len()..]
            .iter()
            .position(|&b| b == 0xff)
            .ok_or("lex_data has no records")?
            + section
            + label.len();
        let mut entries = HashMap::with_capacity(EXPECTED_WORDS);
        let mut previous_end = data[..first_marker]
            .iter()
            .rposition(|&b| b == 0)
            .ok_or("first lexicon record lacks delimiter")?;
        let mut marker = first_marker;
        let mut record_count = 0;
        loop {
            let key_end = data[marker + 1..]
                .iter()
                .position(|&b| b == 0)
                .ok_or("unterminated lexicon key")?
                + marker
                + 1;
            let key = data[marker + 1..key_end]
                .iter()
                .flat_map(|&code| keys[code as usize].iter().copied())
                .collect::<Vec<_>>();
            let key = iso_string(&key);
            let mut sequence = Vec::new();
            for &code in &data[previous_end + 1..marker] {
                let expansion = &expansions[code as usize];
                if expansion.is_empty() {
                    return Err(format!("missing pronunciation code {code:#x}").into());
                }
                for &phone in expansion {
                    let name = phones[phone as usize].as_slice();
                    if name.is_empty() {
                        return Err(format!("missing phone code {phone:#x}").into());
                    }
                    sequence.push(String::from_utf8(name.to_vec())?);
                }
            }
            entries.insert(key, sequence);
            record_count += 1;
            previous_end = key_end;
            let Some(next) = data[key_end + 1..].iter().position(|&b| b == 0xff) else {
                break;
            };
            marker = key_end + 1 + next;
        }
        if record_count != EXPECTED_WORDS {
            return Err(
                format!("lexicon has {record_count} records, expected {EXPECTED_WORDS}").into(),
            );
        }
        Ok(Self {
            entries,
            lts: Lts::new(pe)?,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The original lexicon uses a one-character part-of-speech prefix.
    /// `0` is the default when the word has no part-of-speech annotation.
    pub(crate) fn lookup(&self, word: &str, pos: Option<char>) -> Option<&[String]> {
        let key = format!("{}{}", pos.unwrap_or('0'), word.to_lowercase());
        self.entries.get(&key).map(Vec::as_slice)
    }

    /// Resolve a word from the original lexicon, then use its original
    /// context-sensitive letter-to-sound decision trees for unknown words.
    pub(crate) fn pronounce(
        &self,
        word: &str,
        pos: Option<char>,
    ) -> Result<Vec<String>, Box<dyn Error>> {
        if let Some(phones) = self.lookup(word, pos) {
            return Ok(phones.to_vec());
        }
        self.lts.pronounce(word)
    }
}
