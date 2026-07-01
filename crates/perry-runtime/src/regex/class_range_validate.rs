//! JS-spec character-class range validation the underlying `regex` /
//! `fancy-regex` crates don't perform themselves. Split out of `grammar.rs`
//! to keep that file under the 2000-line size gate.

/// Detect a character-class range made out of order by a doubled hyphen
/// (`[a--z]`): JS parses the class contents "a--z" as ClassAtom `a`, a `-`
/// range operator, ClassAtom `-` (a literal hyphen is itself a valid
/// ClassAtom) — i.e. the range `a`..`-`. Since `a` (U+0061) is greater than
/// `-` (U+002D), `CharacterRange` (22.2.1.1) requires the low bound's code
/// point to be no greater than the high bound's, so this is a SyntaxError.
/// The Rust `regex` crate parses the doubled hyphen differently and
/// silently accepts patterns like this, so Perry must catch it before
/// delegating to the crate (test262 `S15.10.4.1_A9_T3`).
pub(super) fn has_out_of_order_double_dash_class_range(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    // True for the first ClassAtom position of the current class — `^` is
    // only a negation marker there (`[^a]`); a `^` anywhere else (`[x^--z]`)
    // is an ordinary ClassAtom and must still be range-order-checked.
    let mut at_class_start = false;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 2;
                at_class_start = false;
            }
            '[' if !in_class => {
                in_class = true;
                at_class_start = true;
                i += 1;
            }
            ']' if in_class => {
                in_class = false;
                i += 1;
            }
            '^' if in_class && at_class_start => {
                at_class_start = false;
                i += 1;
            }
            c if in_class
                && chars.get(i + 1) == Some(&'-')
                && chars.get(i + 2) == Some(&'-')
                && !matches!(chars.get(i + 3), None | Some(']')) =>
            {
                if (c as u32) > ('-' as u32) {
                    return true;
                }
                at_class_start = false;
                i += 1;
            }
            _ => {
                at_class_start = false;
                i += 1;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::has_out_of_order_double_dash_class_range;

    #[test]
    fn rejects_descending_double_dash_ranges() {
        assert!(has_out_of_order_double_dash_class_range("[a--z]"));
        assert!(has_out_of_order_double_dash_class_range("[a---z]"));
    }

    #[test]
    fn accepts_ascending_double_dash_ranges() {
        assert!(!has_out_of_order_double_dash_class_range("[!--z]"));
        assert!(!has_out_of_order_double_dash_class_range("[+--z]"));
    }

    #[test]
    fn ignores_unrelated_class_hyphens() {
        assert!(!has_out_of_order_double_dash_class_range("[a-z-]"));
        assert!(!has_out_of_order_double_dash_class_range("[a-z]"));
        assert!(!has_out_of_order_double_dash_class_range("abc"));
    }

    #[test]
    fn caret_is_only_a_negation_marker_at_class_start() {
        // A leading `^` negates the class and is not itself a range atom.
        assert!(!has_out_of_order_double_dash_class_range("[^--z]"));
        // A non-leading `^` is an ordinary ClassAtom (U+005E > U+002D `-`),
        // so `^--z` is the same out-of-order range as `a--z`.
        assert!(has_out_of_order_double_dash_class_range("[x^--z]"));
    }
}
