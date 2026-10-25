use super::*;

#[test]
fn borrowed_order_preserves_partitions_ties_and_exact_bytes() {
    let mut orders = StringMemberOrderTable::new();
    for (class, is_static, name, order) in [
        (19, false, b"z".as_slice(), 7),
        (19, true, b"z".as_slice(), 11),
        (20, false, b"z".as_slice(), 13),
        (19, false, b"".as_slice(), 17),
        (19, false, b"\xed\xa0\x80".as_slice(), 23),
    ] {
        orders.insert((class, is_static, name.to_vec()), order);
    }
    for ((class, is_static, name), order) in &orders {
        assert_eq!(
            string_member_order(&orders, *class, *is_static, name),
            Some(*order)
        );
    }
    assert_eq!(string_member_order(&orders, 19, false, b"missing"), None);
    assert_eq!(string_member_order(&orders, 20, true, b"z"), None);
}

#[test]
fn reflection_order_keeps_definition_order_deletion_and_alias_rules() {
    const CLASS: u32 = 0x7d01_3253;
    {
        let mut guard = CLASS_STRING_MEMBER_ORDERS.write().unwrap();
        let orders = guard.get_or_insert_with(Default::default);
        for (name, instance, static_order) in [
            (b"z".as_slice(), 1, 4),
            (b"a".as_slice(), 2, 1),
            (b"tie".as_slice(), 2, 3),
            (b"@@iterator".as_slice(), 3, 2),
            (b"deleted".as_slice(), u32::MAX, u32::MAX),
        ] {
            orders.insert((CLASS, false, name.to_vec()), instance);
            orders.insert((CLASS, true, name.to_vec()), static_order);
        }
    }
    let input = [
        "unknown",
        "deleted",
        "tie",
        "z",
        "@@asyncIterator",
        "a",
        "z",
        "@@iterator",
    ];
    let mut instance: Vec<Vec<u8>> = input.iter().map(|name| name.as_bytes().to_vec()).collect();
    let mut statics = instance.clone();
    order_class_string_member_names(CLASS, false, &mut instance);
    order_class_string_member_names(CLASS, true, &mut statics);
    assert_eq!(
        instance,
        ["z", "a", "tie", "@@iterator", "unknown"].map(|s| s.as_bytes().to_vec())
    );
    assert_eq!(
        statics,
        ["a", "@@iterator", "tie", "z", "unknown"].map(|s| s.as_bytes().to_vec())
    );
    CLASS_STRING_MEMBER_ORDERS
        .write()
        .unwrap()
        .as_mut()
        .unwrap()
        .retain(|(class, _, _), _| *class != CLASS);
}
