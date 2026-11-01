//! Russian syllable-boundary decision tree from voice_ru_maxim.dll.
//! `assets/syllable_cart.bin` is reproducibly extracted by
//! `tools/extract_syllable_cart.py` from the supplied DLL.

use std::collections::HashMap;
use std::sync::OnceLock;

const CART: &[u8; 1893] = include_bytes!("../assets/syllable_cart.bin");
const VALUES: [u8; 20] = [
    b'+', b'-', b'0', b'1', b'2', b'3', b'4', b'5', b'a', b'b', b'd', b'f', b'g', b'l', b'n', b'p',
    b'r', b's', b'v', b'x',
];
// Strings in the original phoneset value table at voice DLL VA 0x180048c70.
const PHONE_VALUES: [u8; 20] = [
    b'+', b'0', b'5', b'1', b'-', b'3', b'2', b'p', b'l', b'a', b'd', b'4', b'f', b'b', b'v', b'g',
    b'x', b'n', b'r', b's',
];

fn phoneset() -> &'static HashMap<&'static str, [u8; 8]> {
    static TABLE: OnceLock<HashMap<&'static str, [u8; 8]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut result = HashMap::new();
        for line in include_str!("../PHONES_RE.csv").lines().skip(1) {
            let columns: Vec<&'static str> = line.split(',').collect();
            if columns.len() != 9 {
                continue;
            }
            let mut attributes = [0u8; 8];
            for (slot, value) in attributes.iter_mut().zip(columns.iter().skip(1)) {
                *slot = value
                    .trim()
                    .parse()
                    .expect("invalid extracted phone attribute");
            }
            result.insert(columns[0], attributes);
        }
        result
    })
}

fn plain_name(phone: &str) -> &str {
    phone.trim_end_matches(|c: char| c.is_ascii_digit())
}

pub(crate) fn phone_feature(phone: &str, attribute: u8) -> u8 {
    let base = plain_name(phone);
    if attribute == 8 {
        if let Some(last) = phone.as_bytes().last().copied() {
            if last.is_ascii_digit() {
                return last;
            }
        }
        let is_vowel = phoneset()
            .get(base)
            .is_some_and(|features| features[0] == 0);
        return if is_vowel { b'0' } else { b'-' };
    }
    let code = phoneset()
        .get(base)
        .and_then(|features| features.get(attribute as usize))
        .copied()
        .unwrap_or(1);
    PHONE_VALUES[code as usize]
}

fn descriptor(index: usize) -> (u8, u8, u8) {
    let start = 10 + index * 3;
    (CART[start], CART[start + 1], CART[start + 2])
}

fn node(index: usize) -> (u8, u8, u16, u16) {
    let start = 10 + 43 * 3 + 20 + index * 6;
    (
        CART[start],
        CART[start + 1],
        u16::from_le_bytes([CART[start + 2], CART[start + 3]]),
        u16::from_le_bytes([CART[start + 4], CART[start + 5]]),
    )
}

fn feature_value(phones: &[String], first: usize, next: usize, feature: usize) -> u8 {
    let (kind, depth, attribute) = descriptor(feature);
    let depth = depth as usize;
    let position = match kind {
        0 => first.checked_sub(depth),
        1 => next.checked_sub(depth).filter(|&index| index >= first),
        2 => next
            .checked_add(depth)
            .filter(|&index| index < phones.len()),
        3 => match depth {
            1 => Some(first),
            2 => Some(next),
            _ => None,
        },
        _ => None,
    };
    phone_feature(
        position
            .and_then(|index| phones.get(index))
            .map(String::as_str)
            .unwrap_or("pau"),
        attribute,
    )
}

fn onset_size(phones: &[String], first: usize, next: usize) -> usize {
    if next - first < 2 {
        return 0;
    }
    let mut index = 0usize;
    loop {
        let (feature, expected, yes, no) = node(index);
        if yes == 0 && no == 0 {
            return (expected as usize).min(next - first - 1);
        }
        index = if feature_value(phones, first, next, feature as usize) == VALUES[expected as usize]
        {
            yes as usize
        } else {
            no as usize
        };
    }
}

/// Return indices after which the original syllable callback inserts a break.
/// Input is one word's original LTS/lexicon segment names, including stress
/// suffixes such as `a1`.
pub(crate) fn breaks(phones: &[String]) -> Vec<usize> {
    debug_assert_eq!(&CART[..4], b"MSC1");
    let vowels: Vec<usize> = phones
        .iter()
        .enumerate()
        .filter(|(_, phone)| phone_feature(phone, 0) == b'+')
        .map(|(index, _)| index)
        .collect();
    vowels
        .windows(2)
        .map(|pair| pair[1] - onset_size(phones, pair[0], pair[1]) - 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_cart_has_expected_shape_and_values() {
        assert_eq!(&CART[..4], b"MSC1");
        assert_eq!(u16::from_le_bytes([CART[4], CART[5]]), 43);
        assert_eq!(u16::from_le_bytes([CART[8], CART[9]]), 289);
        assert_eq!(phone_feature("a1", 8), b'1');
        assert_eq!(phone_feature("ax", 8), b'0');
        assert_eq!(phone_feature("p", 1), b'-');
    }

    #[test]
    fn syllable_tree_splits_checked_words() {
        let phones = |tokens: &str| {
            tokens
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(breaks(&phones("p a1 p ax")), vec![1]);
        assert_eq!(breaks(&phones("k u pj i1 l")), vec![1]);
        assert_eq!(breaks(&phones("h lj e1 p")), Vec::<usize>::new());
    }
}
