use super::*;
use std::cell::Cell;

thread_local! { static CHECKS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn coverage_check() {
    CHECKS.with(|count| count.set(count.get() + 1));
}
fn reset_checks() {
    CHECKS.with(|count| count.set(0));
}
fn checks() -> usize {
    CHECKS.with(Cell::get)
}

// Denotation oracle: empty and one-character prefixes witness every ASCII
// exclusion difference; non-ASCII prefixes witness the non-singleton language.
fn contains(part: &StringPart, word: &str) -> bool {
    match part {
        Ok(exact) => exact == word,
        Err((excluded, suffix)) => word.strip_suffix(suffix).is_some_and(|prefix| {
            prefix
                .chars()
                .all(|c| !c.is_ascii() || excluded & (1u128 << c as u32) == 0)
        }),
    }
}
fn language_covers(a: &StringPart, b: &StringPart) -> bool {
    match b {
        Ok(word) => contains(a, word),
        Err((_, suffix)) => std::iter::once(String::new())
            .chain((0..128u8).map(|c| (c as char).to_string()))
            .chain(["é".to_string(), "😀".to_string()])
            .map(|prefix| prefix + suffix)
            .all(|word| !contains(b, &word) || contains(a, &word)),
    }
}
fn reference(parts: &[StringPart]) -> Vec<StringPart> {
    parts
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            !parts.iter().enumerate().any(|(j, a)| {
                i != &j && language_covers(a, b) && (j < *i || !language_covers(b, a))
            })
        })
        .map(|(_, part)| part.clone())
        .collect()
}
fn sample_parts() -> Vec<StringPart> {
    vec![
        Ok("".into()),
        Ok("a".into()),
        Ok("é".into()),
        Ok("e\u{301}".into()),
        Err((0, "".into())),
        Err((1 << b'a', "".into())),
        Err((1 << b'b', "a".into())),
        Err(((1 << b'a') | (1 << b'b'), "é".into())),
        Err((0, "😀".into())),
        Ok("a😀".into()),
    ]
}
#[test]
fn coverage_matches_independent_language_oracle() {
    let universe = sample_parts();
    let mut digest = 0xcbf29ce484222325u64;
    for bits in 0..(1usize << universe.len()) {
        let mut parts: Vec<_> = universe
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & (1 << i) != 0)
            .map(|(_, p)| p.clone())
            .collect();
        if bits & 1 != 0 {
            parts.reverse();
        }
        if let Some(part) = parts.first().cloned() {
            parts.push(part);
        }
        let actual = canonical_parts(parts.clone());
        assert_eq!(actual, reference(&parts), "subset {bits}");
        assert_eq!(
            canonical_parts(actual.clone()),
            actual,
            "idempotence {bits}"
        );
        for byte in format!("{actual:?}").bytes() {
            digest = (digest ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    println!("COVERAGE_ORACLE cases=1024 digest={digest:016x}");
}
fn model_node(m: &mut Model, atoms: Vec<Atom>, primitive: u16) -> NodeId {
    let n = m.node_l("coverage-repro");
    let mut atoms = atoms;
    atoms.sort_unstable();
    atoms.dedup();
    let strings = m.str_set(&atoms);
    m.nodes[n as usize].strs = strings;
    m.nodes[n as usize].prim = primitive;
    n
}
fn ranked_case(suffix_count: usize) -> (usize, usize) {
    let mut m = Model::default();
    let chars: Vec<u8> = (b'A'..b'I').collect();
    let mut masks = Vec::new();
    for (i, &a) in chars.iter().enumerate() {
        masks.push(!(1u128 << a));
        for &b in &chars[i + 1..] {
            masks.push(!((1u128 << a) | (1u128 << b)));
        }
    }
    let mut atoms = Vec::new();
    for c in b'A'..b'A' + suffix_count as u8 {
        for &mask in &masks {
            atoms.push(m.excl_atom(mask, &(c as char).to_string()).unwrap());
        }
    }
    let count = atoms.len();
    let right = model_node(&mut m, atoms, prim::STR);
    let left_atom = m.atom("fixed");
    let left = model_node(&mut m, vec![left_atom], prim::STR);
    reset_checks();
    let (result, gated) = m.plus_strs(left, right);
    let work = checks();
    assert!(!gated);
    let result_atoms = m.str_atoms(result);
    assert_eq!(result_atoms.len(), 1);
    let (prefix, excluded, suffix) = m.atom_pfx_excl(result_atoms[0]).unwrap();
    let suffix_mask = (b'A'..b'A' + suffix_count as u8).fold(0u128, |a, b| a | (1u128 << b));
    let alphabet_mask = chars.iter().fold(0u128, |a, &b| a | (1u128 << b));
    assert_eq!(
        (prefix, excluded, suffix),
        ("fixed", !(alphabet_mask | suffix_mask), "")
    );
    println!("COVERAGE_FACTS parts={count} prefix={prefix} excluded={excluded:032x} suffix={suffix:?} checks={work}");
    (count, work)
}
#[test]
fn base64_parts_avoid_quadratic_coverage() {
    let results = [ranked_case(8), ranked_case(16)];
    for (parts, work) in results {
        assert!(
            work <= parts * 8,
            "{parts} parts perform {work} coverage checks"
        );
    }
}
#[test]
fn coverage_preserves_top_empty_and_primitive_duplicate() {
    let mut m = Model::default();
    let empty = m.node_l("empty");
    assert_eq!(m.excl_parts(empty), Some((vec![], false)));
    m.nodes[empty as usize].prim = prim::STR;
    m.nodes[empty as usize].strs = STRS_TOP;
    assert_eq!(m.excl_parts(empty), None);
    let atom = m.atom("undefined");
    let duplicate = model_node(&mut m, vec![atom], prim::STR | prim::UNDEF);
    assert_eq!(
        m.excl_parts(duplicate),
        Some((vec![Ok("undefined".into())], false))
    );
}

#[test]
fn coverage_keeps_unicode_boundaries_and_extreme_masks() {
    let parts = vec![
        Err((u128::MAX, "é".into())),
        Ok("😀é".into()),
        Ok("aé".into()),
        Err((0, "\u{301}".into())),
        Ok("e\u{301}".into()),
        Err((u128::MAX, "".into())),
        Ok("".into()),
    ];
    assert_eq!(canonical_parts(parts.clone()), reference(&parts));
}

fn base64_shape(suffixes: &[u8], alphabet: &[u8], pairs: bool) -> Vec<StringPart> {
    let mut masks = Vec::new();
    for (i, &a) in alphabet.iter().enumerate() {
        masks.push(!(1u128 << a));
        if pairs {
            for &b in &alphabet[i + 1..] {
                masks.push(!((1u128 << a) | (1u128 << b)));
            }
        }
    }
    suffixes
        .iter()
        .flat_map(|&c| {
            masks
                .iter()
                .map(move |&mask| Err((mask, (c as char).to_string())))
        })
        .collect()
}

#[test]
fn observed_base64_dimensions_keep_every_incomparable_pair() {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let parts = base64_shape(alphabet, alphabet, true);
    assert_eq!(parts.len(), 133_120);
    // The two-character allowed alphabets strictly contain the one-character
    // alphabets. Distinct pairs have equal rank and are incomparable; different
    // one-character suffixes cannot cover each other.
    let expected: Vec<_> = parts
        .iter()
        .filter(|p| p.as_ref().err().unwrap().0.count_ones() == 126)
        .cloned()
        .collect();
    reset_checks();
    let actual = canonical_parts(parts);
    let work = checks();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 129_024);
    assert!(
        work <= 133_120 * 64,
        "observed shape uses {work} comparisons"
    );
    println!(
        "COVERAGE_REAL_SHAPE parts=133120 kept={} checks={work}",
        actual.len()
    );
}

// Frozen pre-change relation and all-pairs algorithm, used only for bounded
// timing comparison. Semantic assertions use the denotation oracle above.
fn quadratic_reference(parts: Vec<StringPart>) -> Vec<StringPart> {
    let covers = |a: &StringPart, b: &StringPart| match (a, b) {
        (Err((e, s)), Err((e2, s2))) => {
            s2.ends_with(s.as_str())
                && e2 & e == *e
                && ascii_mask(&s2[..s2.len() - s.len()]) & e == 0
        }
        (Err((e, s)), Ok(c)) => {
            c.ends_with(s.as_str()) && ascii_mask(&c[..c.len() - s.len()]) & e == 0
        }
        (Ok(c), Ok(c2)) => c == c2,
        (Ok(_), Err(_)) => false,
    };
    let keep: Vec<_> = (0..parts.len())
        .map(|i| {
            !(0..parts.len()).any(|j| {
                j != i && covers(&parts[j], &parts[i]) && (j < i || !covers(&parts[i], &parts[j]))
            })
        })
        .collect();
    parts
        .into_iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(p))
        .collect()
}

#[test]
fn coverage_timing_report_has_no_wall_clock_gate() {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for suffix_count in [64, 128] {
        let suffixes: Vec<_> = (0..suffix_count).map(|c| c as u8).collect();
        let parts = base64_shape(&suffixes, alphabet, false);
        let mut old = Vec::new();
        let mut new = Vec::new();
        for iteration in 0..5 {
            let mut outputs = Vec::new();
            for baseline in if iteration % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let input = parts.clone();
                let start = std::time::Instant::now();
                let result = if baseline {
                    quadratic_reference(input)
                } else {
                    canonical_parts(input)
                };
                let ns = start.elapsed().as_nanos();
                if baseline {
                    old.push(ns);
                } else {
                    new.push(ns);
                }
                outputs.push(std::hint::black_box(result));
            }
            assert_eq!(outputs[0], outputs[1]);
        }
        old.sort_unstable();
        new.sort_unstable();
        println!(
            "COVERAGE_TIMING parts={} original_reference_median_ns={} candidate_median_ns={}",
            parts.len(),
            old[2],
            new[2]
        );
    }
}
