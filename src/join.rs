//! Original class-specific join costs from `tts_engine.dll`.
//! Inputs are the compact 36-byte section-5 rows: previous unit ID + 1 and
//! current unit ID. The lookup data is extracted reproducibly from that DLL.

const LOOKUPS: &[u8; 12508] = include_bytes!("../assets/join_luts.bin");
const REJECT: i32 = 1_000_000;
const WIDE: usize = 1025;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JoinClass {
    Vowel,
    Trill,
    Glide,
    Unvoiced,
    Voiced,
}

impl JoinClass {
    /// Reproduces the attribute checks in voice `FUN_18000d580` using the
    /// exact Russian phoneset attributes recorded in `docs/PHONES_RE.csv`.
    pub(crate) fn for_phones(left: &str, shared: &str) -> Self {
        if matches!(shared, "r" | "rj") {
            Self::Trill
        } else if matches!(shared, "a" | "ax" | "e" | "eh" | "i" | "o" | "u" | "y") {
            Self::Vowel
        } else if shared == "j" && left == "l" {
            Self::Glide
        } else if matches!(
            shared,
            "ch" | "f"
                | "fj"
                | "h"
                | "hj"
                | "k"
                | "kj"
                | "p"
                | "pj"
                | "s"
                | "sch"
                | "sh"
                | "sj"
                | "t"
                | "tj"
                | "ts"
        ) {
            Self::Unvoiced
        } else {
            Self::Voiced
        }
    }
}

fn lookup(table: usize, difference: i32) -> i32 {
    let index = if table == 0 {
        difference as usize
    } else {
        (difference + 512) as usize
    };
    let start = if table == 0 {
        4
    } else {
        4 + 51 * 4 + (table - 1) * WIDE * 4
    };
    let at = start + index * 4;
    i32::from_le_bytes(LOOKUPS[at..at + 4].try_into().unwrap())
}

fn u16_at(row: &[u8; 36], at: usize) -> i32 {
    u16::from_le_bytes([row[at], row[at + 1]]) as i32
}

fn i16_at(row: &[u8; 36], at: usize) -> i32 {
    i16::from_le_bytes([row[at], row[at + 1]]) as i32
}

fn spectral_distance(left: &[u8; 36], right: &[u8; 36]) -> i32 {
    let diff = |index: usize| (left[index] as i32 - right[index] as i32).abs();
    let group_a: i32 = (25..=31).map(diff).sum();
    let group_b: i32 = (19..=21).map(diff).sum::<i32>() * 3;
    let group_c: i32 = (22..=24).map(diff).sum();
    let group_d: i32 = (16..=18).map(diff).sum();
    ((group_a / 4 + group_b / 4 + group_c / 2 + group_d) * 3) / 2
}

/// Return the original join cost, or one million for a rejected join.
/// `budget` is the remaining Viterbi cost budget; pass `i32::MAX` when the
/// caller has not yet found an alternative for this candidate.
pub(crate) fn cost(class: JoinClass, left: &[u8; 36], right: &[u8; 36], budget: i32) -> i32 {
    debug_assert_eq!(&LOOKUPS[..4], b"MJL1");
    let a0 = u16_at(left, 0);
    let b0 = u16_at(right, 0);
    let duration_cost = if a0 + b0 == 0 {
        100
    } else {
        let difference = ((a0 - b0) * 50 / (a0 + b0)).unsigned_abs() as i32;
        lookup(0, difference)
    };

    let a8 = i16_at(left, 8);
    let b8 = i16_at(right, 8);
    if (a8 < 3 && b8 > 19) || (a8 > 19 && b8 < 3) {
        return REJECT;
    }
    let mut field8_cost = (a8 - b8).abs() / 5;
    if (a8 == 0 && b8 > 1) || (b8 == 0 && a8 > 1) {
        field8_cost += 200;
    }

    let field10 = lookup(1, u16_at(left, 10) - u16_at(right, 10));
    let field12 = lookup(3, i16_at(left, 12) - i16_at(right, 12));
    let field14 = lookup(2, u16_at(left, 14) - u16_at(right, 14));
    let vector = spectral_distance(left, right);
    let budget = budget.max(0) as i64;

    match class {
        JoinClass::Vowel => {
            let base = duration_cost + field8_cost + field10 + field12 + field14;
            if (base as i64) * 2 >= budget {
                REJECT
            } else {
                let vector = if vector < 36 {
                    vector
                } else {
                    vector * 10 - 315
                };
                (base + vector) * 2
            }
        }
        JoinClass::Trill => {
            let base = duration_cost + field8_cost + field12 + field14 / 2;
            if (base as i64) * 96 >= budget * 10 {
                REJECT
            } else {
                let vector = if vector < 61 {
                    vector
                } else {
                    vector * 3 - 120
                };
                (base + vector) * 96 / 10
            }
        }
        JoinClass::Glide => {
            let base = duration_cost + field8_cost + field10 + field12 + field14;
            if (base as i64) * 4 >= budget {
                REJECT
            } else {
                let vector = if vector <= 40 {
                    vector
                } else {
                    vector * 7 - 240
                };
                (base + vector) * 4
            }
        }
        JoinClass::Unvoiced => {
            let base = (field10 + duration_cost * 12) / 20 + field8_cost + field12;
            if (base as i64) * 4 >= budget * 5 {
                REJECT
            } else {
                let vector = if vector <= 90 {
                    vector
                } else {
                    vector * 5 - 360
                };
                (base + vector * 3 / 10) * 4 / 5
            }
        }
        JoinClass::Voiced => {
            let base = duration_cost + field8_cost + field10 + field12 + field14;
            if (base as i64) * 4 >= budget * 5 {
                REJECT
            } else {
                let vector = if vector < 41 {
                    vector
                } else {
                    vector * 5 - 160
                };
                (base + vector) * 4 / 5
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_tables_have_expected_center_values() {
        assert_eq!(&LOOKUPS[..4], b"MJL1");
        assert_eq!(lookup(0, 0), 0);
        assert_eq!(lookup(0, 6), 8);
        assert_eq!(lookup(1, 0), 0);
        assert_eq!(lookup(2, 0), 0);
        assert_eq!(lookup(3, 0), 0);
    }

    #[test]
    fn exact_match_is_free_and_large_offset_eight_mismatch_rejects() {
        let mut row = [0u8; 36];
        row[0..2].copy_from_slice(&100u16.to_le_bytes());
        row[8..10].copy_from_slice(&5u16.to_le_bytes());
        row[10..12].copy_from_slice(&100u16.to_le_bytes());
        row[12..14].copy_from_slice(&(-200i16).to_le_bytes());
        row[14..16].copy_from_slice(&50u16.to_le_bytes());
        row[16..32].fill(80);
        for class in [
            JoinClass::Vowel,
            JoinClass::Trill,
            JoinClass::Glide,
            JoinClass::Unvoiced,
            JoinClass::Voiced,
        ] {
            assert_eq!(cost(class, &row, &row, i32::MAX), 0);
        }
        let mut other = row;
        row[8..10].copy_from_slice(&0u16.to_le_bytes());
        other[8..10].copy_from_slice(&25u16.to_le_bytes());
        assert_eq!(cost(JoinClass::Vowel, &row, &other, i32::MAX), REJECT);
    }

    #[test]
    fn class_choice_follows_original_phone_attributes() {
        assert_eq!(JoinClass::for_phones("p", "a"), JoinClass::Vowel);
        assert_eq!(JoinClass::for_phones("a", "rj"), JoinClass::Trill);
        assert_eq!(JoinClass::for_phones("l", "j"), JoinClass::Glide);
        assert_eq!(JoinClass::for_phones("a", "p"), JoinClass::Unvoiced);
        assert_eq!(JoinClass::for_phones("a", "b"), JoinClass::Voiced);
    }
}
