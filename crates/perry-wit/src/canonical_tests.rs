use super::*;

// Compare the reconstructed graph with SizeAlign on the independently parsed
// original WIT, including aliases, eight-byte payloads and resource handles.
#[test]
fn metadata_preserves_original_canonical_offsets_and_variants() {
    let source = r#"package test:layout;
    interface data {
        resource item;
        type word = u64;
        type bytes = list<u8>;
        variant choice { empty, wide(word), text(string), owned(own<item>) }
        record payload {
            small: u8,
            large: word,
            maybe: option<word>,
            outcome: result<bytes, choice>,
            nested: list<tuple<u8, word, own<item>>>,
            eventual: future<choice>,
            chunks: stream<u8>,
        }
        run: func(value: payload) -> payload;
    }
    world plugin { export data; }"#;
    let world = crate::parse(source, None).unwrap();
    let mut original = Resolve::default();
    let package = original.push_str("layout.wit", source).unwrap();
    let interface = original.packages[package].interfaces["data"];
    let payload = original.interfaces[interface].types["payload"];
    let mut sizes = SizeAlign::default();
    sizes.fill(&original);
    let ty = &world.exports[0].functions[0].params[0].ty;
    let rebuilt = layout(&world, ty).unwrap();
    assert_eq!(
        rebuilt,
        describe(&original, &sizes, Wit::Id(payload), 0).unwrap()
    );
    let LayoutKind::Fields(fields) = &rebuilt.kind else {
        panic!("record expected")
    };
    assert_eq!(
        fields[1].0, 8,
        "u64 must be eight-byte aligned on wasm32 too"
    );
    assert_eq!(fields[2].0, 16);
    let LayoutKind::Variant { payload_offset, .. } = fields[2].1.kind else {
        panic!("option expected")
    };
    assert_eq!(payload_offset, 8);
    assert_eq!(
        fields[5].1.size, 4,
        "canonical futures are handles, not payload storage"
    );
    assert_eq!(fields[6].1.size, 4);
}

#[test]
fn zero_sized_values_and_large_variant_tags_keep_canonical_sizes() {
    let cases = (0..257)
        .map(|i| format!("c{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let source=format!("package test:tags; interface data {{ record empty {{}} variant choice {{ {cases} }} run: func(x: empty) -> choice; }} world plugin {{ export data; }}");
    let world = crate::parse(&source, None).unwrap();
    let function = &world.exports[0].functions[0];
    assert_eq!(layout(&world, &function.params[0].ty).unwrap().size, 0);
    let result = layout(&world, function.result.as_ref().unwrap()).unwrap();
    assert_eq!((result.size, result.align), (2, 2));
    let LayoutKind::Variant {
        tag_bytes, cases, ..
    } = &result.kind
    else {
        panic!("variant expected")
    };
    assert_eq!(*tag_bytes, 2);
    assert_eq!(cases.len(), 257);
}

#[test]
fn malformed_contract_graphs_fail_before_layout_arithmetic() {
    let mut world = crate::parse("package test:invalid; world plugin {}", None).unwrap();
    world.types.insert(
        "cycle".into(),
        crate::Definition {
            name: "cycle".into(),
            ty: Type::Ref("cycle".into()),
        },
    );
    assert!(layout(&world, &Type::Ref("cycle".into()))
        .unwrap_err()
        .to_string()
        .contains("cyclic"));
    assert!(layout(&world, &Type::Ref("missing".into())).is_err());
    world.types.insert(
        "resource".into(),
        crate::Definition {
            name: "resource".into(),
            ty: Type::Resource,
        },
    );
    assert!(layout(&world, &Type::List(Box::new(Type::Ref("resource".into())))).is_err());
    assert_eq!(
        layout(&world, &Type::Own("resource".into())).unwrap().size,
        4
    );
    world.types.insert(
        "v0".into(),
        crate::Definition {
            name: "v0".into(),
            ty: Type::U64,
        },
    );
    for i in 1..64 {
        world.types.insert(
            format!("v{i}"),
            crate::Definition {
                name: format!("v{i}"),
                ty: Type::Tuple(vec![Type::Ref(format!("v{}", i - 1)); 2]),
            },
        );
    }
    assert!(
        layout(&world, &Type::Ref("v63".into())).is_err(),
        "an exponential aggregate must fail instead of overflowing SizeAlign"
    );
}

#[test]
fn canonical_guest_generator_keeps_async_values_explicitly_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("world.wit");
    std::fs::write(
        &path,
        "package test:guest-future; world plugin { export get: func() -> future<u64>; }",
    )
    .unwrap();
    let error = crate::canonical_guest(&path, None).unwrap_err();
    assert!(error.to_string().contains("P3 rooted-frame adapter"));
}

#[test]
fn shared_aliases_keep_metadata_linear_in_the_contract_graph() {
    let mut world = crate::parse("package test:sharing; world plugin {}", None).unwrap();
    for i in 0..41 {
        world.types.insert(
            format!("t{i}"),
            crate::Definition {
                name: format!("t{i}"),
                ty: if i == 0 {
                    Type::U8
                } else {
                    Type::Tuple(vec![Type::Ref(format!("t{}", i - 1)); 2])
                },
            },
        );
    }
    // Two gigabytes of inline values still needs only 32 metadata nodes.
    let large = layout(&world, &Type::Ref("t31".into())).unwrap();
    assert_eq!(large.size, 1 << 31);
    let mut node = large;
    let mut ids = BTreeSet::new();
    for _ in 0..31 {
        assert!(ids.insert(node.node_id()));
        let LayoutKind::Fields(fields) = &node.kind else {
            panic!("tuple expected")
        };
        assert!(Arc::ptr_eq(&fields[0].1, &fields[1].1));
        node = Arc::clone(&fields[0].1);
    }
    assert_eq!(node.size, 1);
    assert!(layout(&world, &Type::Ref("t40".into()))
        .unwrap_err()
        .to_string()
        .contains("wasm32 size limits"));
}
