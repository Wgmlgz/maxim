//! Token normalization before the Russian lexicon/LTS stage.
//!
//! The supplied voice routes tokens through `tokentowords_func` before
//! `wordtosegments_func`. This approximates its numeric and symbol categories
//! for the CLI; exact date, currency inflection, and abbreviation contexts
//! remain unported.

const ONES_M: [&str; 10] = [
    "ноль",
    "один",
    "два",
    "три",
    "четыре",
    "пять",
    "шесть",
    "семь",
    "восемь",
    "девять",
];
const ONES_F: [&str; 10] = [
    "ноль",
    "одна",
    "две",
    "три",
    "четыре",
    "пять",
    "шесть",
    "семь",
    "восемь",
    "девять",
];
const TEENS: [&str; 10] = [
    "десять",
    "одиннадцать",
    "двенадцать",
    "тринадцать",
    "четырнадцать",
    "пятнадцать",
    "шестнадцать",
    "семнадцать",
    "восемнадцать",
    "девятнадцать",
];
const TENS: [&str; 10] = [
    "",
    "",
    "двадцать",
    "тридцать",
    "сорок",
    "пятьдесят",
    "шестьдесят",
    "семьдесят",
    "восемьдесят",
    "девяносто",
];
const HUNDREDS: [&str; 10] = [
    "",
    "сто",
    "двести",
    "триста",
    "четыреста",
    "пятьсот",
    "шестьсот",
    "семьсот",
    "восемьсот",
    "девятьсот",
];
const MAGNITUDES: [[&str; 3]; 5] = [
    ["", "", ""],
    ["тысяча", "тысячи", "тысяч"],
    ["миллион", "миллиона", "миллионов"],
    ["миллиард", "миллиарда", "миллиардов"],
    ["триллион", "триллиона", "триллионов"],
];
const FRACTIONS: [[&str; 2]; 6] = [
    ["десятая", "десятых"],
    ["сотая", "сотых"],
    ["тысячная", "тысячных"],
    ["десятитысячная", "десятитысячных"],
    ["стотысячная", "стотысячных"],
    ["миллионная", "миллионных"],
];

fn push_hundreds(mut value: u16, feminine: bool, words: &mut Vec<&'static str>) {
    if value >= 100 {
        words.push(HUNDREDS[(value / 100) as usize]);
        value %= 100;
    }
    if value >= 20 {
        words.push(TENS[(value / 10) as usize]);
        value %= 10;
    } else if value >= 10 {
        words.push(TEENS[(value - 10) as usize]);
        return;
    }
    if value > 0 {
        words.push(if feminine {
            ONES_F[value as usize]
        } else {
            ONES_M[value as usize]
        });
    }
}

fn magnitude_form(value: u16) -> usize {
    let last_two = value % 100;
    if (11..=14).contains(&last_two) {
        2
    } else {
        match value % 10 {
            1 => 0,
            2..=4 => 1,
            _ => 2,
        }
    }
}

fn cardinal(value: u64, feminine: bool) -> String {
    if value == 0 {
        return "ноль".to_string();
    }
    let mut groups = Vec::new();
    let mut remaining = value;
    while remaining > 0 {
        groups.push((remaining % 1000) as u16);
        remaining /= 1000;
    }
    if groups.len() > MAGNITUDES.len() {
        return digit_words(&value.to_string());
    }
    let mut words = Vec::new();
    for index in (0..groups.len()).rev() {
        let group = groups[index];
        if group == 0 {
            continue;
        }
        push_hundreds(group, index == 1 || (index == 0 && feminine), &mut words);
        if index > 0 {
            words.push(MAGNITUDES[index][magnitude_form(group)]);
        }
    }
    words.join(" ")
}

fn digit_words(digits: &str) -> String {
    digits
        .bytes()
        .filter(|digit| digit.is_ascii_digit())
        .map(|digit| ONES_M[(digit - b'0') as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

fn number_words(integer: &str, fraction: Option<&str>) -> String {
    let whole = integer.parse::<u64>().ok();
    if let Some(frac) = fraction {
        if let (Some(value), 1..=6) = (whole, frac.len()) {
            if let Ok(part) = frac.parse::<u64>() {
                let whole_words = cardinal(value, true);
                let separator = if value % 10 == 1 && value % 100 != 11 {
                    "целая"
                } else {
                    "целых"
                };
                let fraction_words = cardinal(part, true);
                let form = if part % 10 == 1 && part % 100 != 11 {
                    0
                } else {
                    1
                };
                return format!(
                    "{whole_words} {separator} {fraction_words} {}",
                    FRACTIONS[frac.len() - 1][form]
                );
            }
        }
        return format!(
            "{} запятая {}",
            whole.map_or_else(|| digit_words(integer), |v| cardinal(v, false)),
            digit_words(frac)
        );
    }
    whole.map_or_else(|| digit_words(integer), |value| cardinal(value, false))
}

fn supported_letter(ch: char) -> Option<char> {
    match ch {
        'a'..='z' | 'а'..='я' | 'ё' => Some(ch),
        'і' | 'ї' => Some('и'),
        'є' => Some('е'),
        'ґ' => Some('г'),
        'ў' => Some('у'),
        _ => None,
    }
}

fn symbol_word(ch: char) -> Option<&'static str> {
    match ch {
        '+' => Some("плюс"),
        '=' => Some("равно"),
        '%' => Some("процентов"),
        '&' => Some("и"),
        '@' => Some("собака"),
        '#' | '№' => Some("номер"),
        '$' => Some("долларов"),
        '€' => Some("евро"),
        '₽' => Some("рублей"),
        '/' => Some("дробь"),
        '*' | '×' => Some("умножить на"),
        _ => None,
    }
}

/// Produce only characters the original lexicon/phoneset path can consume.
/// Unsupported characters delimit tokens instead of aborting synthesis.
pub(crate) fn input(text: &str) -> String {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_digit() {
            let start = index;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            let mut integer: String = chars[start..index].iter().collect();
            loop {
                let mut next = index;
                while next < chars.len() && matches!(chars[next], ' ' | '\u{a0}') {
                    next += 1;
                }
                if next == index
                    || next + 3 > chars.len()
                    || !chars[next..next + 3].iter().all(|ch| ch.is_ascii_digit())
                    || chars.get(next + 3).is_some_and(|ch| ch.is_ascii_digit())
                {
                    break;
                }
                integer.extend(chars[next..next + 3].iter());
                index = next + 3;
            }
            let fraction = if index + 1 < chars.len()
                && matches!(chars[index], '.' | ',')
                && chars[index + 1].is_ascii_digit()
            {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index].is_ascii_digit() {
                    index += 1;
                }
                Some(chars[start..index].iter().collect::<String>())
            } else {
                None
            };
            result.push(' ');
            result.push_str(&number_words(&integer, fraction.as_deref()));
            result.push(' ');
            continue;
        }
        if let Some(letter) = supported_letter(ch) {
            result.push(letter);
        } else if ch == '\'' || ch == '’' {
            // Original word-to-segments strips apostrophes in unknown words.
        } else if ch == '-'
            && index > 0
            && index + 1 < chars.len()
            && supported_letter(chars[index - 1]).is_some()
            && supported_letter(chars[index + 1]).is_some()
        {
            result.push('-');
        } else if matches!(ch, '.' | ',' | '!' | '?' | ';' | ':' | '—') {
            result.push(ch);
        } else if ch == '…' {
            result.push('.');
        } else if let Some(word) = symbol_word(ch) {
            result.push(' ');
            result.push_str(word);
            result.push(' ');
        } else if ch == '−'
            || ch == '-' && index + 1 < chars.len() && chars[index + 1].is_ascii_digit()
        {
            result.push_str(" минус ");
        } else {
            result.push(' ');
        }
        index += 1;
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_decimals_become_russian_words() {
        assert_eq!(input("Цена 123 ₽"), "цена сто двадцать три рублей");
        assert_eq!(input("1 002"), "одна тысяча два");
        assert_eq!(number_words("2024", None), "две тысячи двадцать четыре");
        assert_eq!(
            number_words("3", Some("14")),
            "три целых четырнадцать сотых"
        );
        assert_eq!(number_words("1", Some("5")), "одна целая пять десятых");
    }

    #[test]
    fn mixed_text_does_not_abort_on_symbols() {
        let normalized = input("Hello, мир! №42 — 5% 😊");
        assert!(normalized.contains("hello, мир!"));
        assert!(normalized.contains("номер сорок два"));
        assert!(normalized.contains("пять процентов"));
        assert!(!normalized.contains('😊'));
    }
}
