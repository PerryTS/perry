use super::*;

fn compile(pattern: &[u8], flags: &str) -> Regex {
    Regex::compile(
        pattern,
        flags,
        Limits::default(),
        &mut Budget::new(100_000_000),
    )
    .unwrap()
}

#[test]
fn captures_preserve_original_bytes_utf16_offsets_and_unset_groups() {
    let re = compile(br"(?<=(a+))(b)(c)?", "");
    let subject = "😀aaab".as_bytes();
    let original = subject.as_ptr();
    let mut captures = vec![None; re.capture_count()];
    let mut stats = SearchStats::default();
    assert!(re
        .find(
            subject,
            0,
            Some(&mut captures),
            Limits::default(),
            &mut Budget::new(100_000),
            &mut stats
        )
        .unwrap());
    let spans: Vec<_> = captures
        .iter()
        .map(|s| s.map(|s| (s.start(), s.end())))
        .collect();
    assert_eq!(spans, [Some((5, 6)), Some((2, 5)), Some((5, 6)), None]);
    assert_eq!(subject.as_ptr(), original);
    assert_eq!(subject, "😀aaab".as_bytes());
    assert!(stats.growths > 0);
    assert_eq!(stats.scratch_final_bytes, 0);
    assert!(stats.scratch_peak_bytes < 64 * 1024);
    // Capture one surrogate half directly from the original four-byte scalar.
    let re = compile(b"(.)", "");
    let mut captures = [None; 2];
    assert!(re
        .find(
            "😀".as_bytes(),
            1,
            Some(&mut captures),
            Limits::default(),
            &mut Budget::new(100_000),
            &mut stats
        )
        .unwrap());
    assert_eq!(captures[0].map(|s| (s.start(), s.end())), Some((1, 2)));
    // Original lone-surrogate WTF-8 is accepted, with no UTF-8 repair step.
    assert!(re
        .is_match(
            b"\xed\xa0\x80",
            Limits::default(),
            &mut Budget::new(100_000)
        )
        .unwrap());
}

#[test]
fn work_memory_encoding_and_capacity_errors_are_distinct_from_no_match() {
    let re = compile(br"^(a|aa)+b$", "");
    let subject = b"aaaaaaaaaaaaaaaaac";
    let mut stats = SearchStats::default();
    let mut budget = Budget::new(0);
    assert!(matches!(
        re.find(subject, 0, None, Limits::default(), &mut budget, &mut stats),
        Err(Error::WorkLimit)
    ));
    assert_eq!(stats.scratch_final_bytes, 0);
    let limits = Limits {
        scratch_bytes: 1,
        ..Limits::default()
    };
    assert!(matches!(
        re.find(
            subject,
            0,
            None,
            limits,
            &mut Budget::new(100_000),
            &mut stats
        ),
        Err(Error::MemoryLimit)
    ));
    assert_eq!(stats.scratch_final_bytes, 0);
    assert!(matches!(
        re.is_match(b"\xff", Limits::default(), &mut Budget::new(100_000)),
        Err(Error::Encoding(_))
    ));
    let limits = Limits {
        input_bytes: 1,
        ..Limits::default()
    };
    assert!(matches!(
        re.is_match(subject, limits, &mut Budget::new(100_000)),
        Err(Error::InputLimit)
    ));
    let re = compile(br"(a)+", "");
    let mut too_short = [];
    assert!(matches!(
        re.find(
            b"aa",
            0,
            Some(&mut too_short),
            Limits::default(),
            &mut Budget::new(100_000),
            &mut stats
        ),
        Err(Error::Execution(ExecError::Captures))
    ));
    assert_eq!(stats.scratch_final_bytes, 0);
    assert!(!re
        .is_match(b"bbb", Limits::default(), &mut Budget::new(100_000))
        .unwrap());
}

#[test]
fn scratch_growth_failure_releases_old_and_partial_new_owners() {
    let re = compile(br"(a|b)+", "");
    let mut stats = SearchStats::default();
    let limits = Limits {
        scratch_bytes: 1024,
        ..Limits::default()
    };
    let subject = vec![b'a'; 1024];
    let mut budget = Budget::new(100_000);
    assert!(matches!(
        re.find(&subject, 0, None, limits, &mut budget, &mut stats),
        Err(Error::MemoryLimit)
    ));
    assert!(stats.growths > 0);
    assert!(stats.scratch_peak_bytes <= limits.scratch_bytes);
    assert_eq!(stats.scratch_final_bytes, 0);
    assert!(budget.remaining() < 100_000);
    assert!(re
        .is_match(b"ab", Limits::default(), &mut Budget::new(100_000))
        .unwrap());
}

#[test]
fn prepared_program_outlives_pattern_and_supports_concurrent_borrowed_searches() {
    let re = {
        let pattern = String::from(r"^(a+)\1$");
        compile(pattern.as_bytes(), "")
    };
    assert!(re.program_bytes() < 4096);
    assert!(re.compile_scratch_peak_bytes() < 64 * 1024);
    std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    for _ in 0..20 {
                        assert!(re
                            .is_match(b"aaaa", Limits::default(), &mut Budget::new(100_000))
                            .unwrap());
                        assert!(!re
                            .is_match(b"aaa", Limits::default(), &mut Budget::new(100_000))
                            .unwrap());
                    }
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    });
}

#[test]
fn compilation_errors_and_shared_work_allowance_are_preserved() {
    for (pattern, flags) in [(b"(".as_slice(), ""), (b"a", "gg")] {
        assert!(matches!(
            Regex::compile(pattern, flags, Limits::default(), &mut Budget::new(100_000)),
            Err(Error::Compile(_))
        ));
    }
    let limits = Limits {
        program_bytes: 1,
        ..Limits::default()
    };
    assert!(matches!(
        Regex::compile(b"a", "", limits, &mut Budget::new(100_000)),
        Err(Error::MemoryLimit)
    ));
    let mut budget = Budget::new(100_000);
    let re = Regex::compile(b"a", "", Limits::default(), &mut budget).unwrap();
    let after_compile = budget.remaining();
    assert!(after_compile < 100_000);
    assert!(re.is_match(b"a", Limits::default(), &mut budget).unwrap());
    assert!(budget.remaining() < after_compile);
}

#[test]
fn growing_native_adapter_matches_direct_perex_captures_and_work() {
    let patterns = [
        "a",
        "(a|b)+",
        "(a|(b))+",
        "((a)?b)+",
        "(a+)(b)?",
        "a*?",
        "(?<=a)(b)",
        "(?=(a+))a",
        "(a+)\\1",
        "^.*$",
        "([^/]+)/(.*)",
        "(.)",
    ];
    let subjects = ["", "a", "ab", "aaab", "abab", "aaa", "x/y", "😀ab", "a\nb"];
    let mut cases = 0;
    for flags in ["", "u", "iu"] {
        for pattern in patterns {
            let re = compile(pattern.as_bytes(), flags);
            for text in subjects {
                let mut expected = vec![None; re.capture_count()];
                let mut direct_budget = Budget::new(1_000_000);
                let expected_match = re
                    .program
                    .with_view(|program| {
                        let mut registers = vec![0; program.register_count()];
                        let mut frames = vec![Frame::default(); 4096];
                        let mut undo = vec![Undo::default(); 16384];
                        perex::executor::find(
                            program,
                            Input::utf8(text),
                            0,
                            Scratch {
                                registers: &mut registers,
                                frames: &mut frames,
                                undo: &mut undo,
                            },
                            &mut expected,
                            &mut direct_budget,
                        )
                        .unwrap()
                    })
                    .unwrap();
                let mut actual = vec![None; re.capture_count()];
                let mut budget = Budget::new(1_000_000);
                let mut stats = SearchStats::default();
                let matched = re
                    .find(
                        text.as_bytes(),
                        0,
                        Some(&mut actual),
                        Limits::default(),
                        &mut budget,
                        &mut stats,
                    )
                    .unwrap();
                assert_eq!(
                    (matched, actual),
                    (expected_match, expected),
                    "/{pattern}/{flags} {text:?}"
                );
                assert_eq!(
                    direct_budget.remaining() - budget.remaining(),
                    text.len() + if matched { re.capture_count() } else { 0 }
                );
                assert_eq!(stats.scratch_final_bytes, 0);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 324);
}

#[test]
fn compilation_retries_both_arenas_with_one_budget_and_exact_final_storage() {
    let class: String = (0..128).map(|n| format!("\\u{:04x}", n * 2)).collect();
    let pattern = format!("^({})[{class}]$", "a".repeat(160));
    let mut budget = Budget::new(100_000_000);
    let re = Regex::compile(pattern.as_bytes(), "u", Limits::default(), &mut budget).unwrap();
    assert!(budget.remaining() < 100_000_000 - pattern.len());
    assert!(
        re.compile_scratch_peak_bytes()
            > 64 * (std::mem::size_of::<Node>() + std::mem::size_of::<Range>())
    );
    let required = re.program.with_view(|p| p.size_bytes()).unwrap();
    assert_eq!(re.program_bytes(), required);
    let text = format!("{}þ", "a".repeat(160));
    let mut captures = [None; 2];
    let mut stats = SearchStats::default();
    assert!(re
        .find(
            text.as_bytes(),
            0,
            Some(&mut captures),
            Limits::default(),
            &mut budget,
            &mut stats
        )
        .unwrap());
    assert_eq!(
        captures.map(|s| s.map(|s| (s.start(), s.end()))),
        [Some((0, 161)), Some((0, 160))]
    );
    let limits = Limits {
        scratch_bytes: 64 * (std::mem::size_of::<Node>() + std::mem::size_of::<Range>()),
        ..Limits::default()
    };
    let mut budget = Budget::new(100_000_000);
    assert!(matches!(
        Regex::compile(pattern.as_bytes(), "u", limits, &mut budget),
        Err(Error::MemoryLimit)
    ));
    assert!(budget.remaining() < 100_000_000 - pattern.len());
}
