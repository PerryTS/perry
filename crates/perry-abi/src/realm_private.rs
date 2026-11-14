//! The identity role carried by an intrinsic private-value key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrinsicPart {
    Prototype,
    Constructor,
}

/// Qualified private keys encode a role, independently of public member
/// names. Other private state (matcher data, worker ids, and so on) has no
/// intrinsic cell target.
pub fn intrinsic_target(spelling: &str) -> Option<(&str, IntrinsicPart)> {
    let spelling = spelling.strip_prefix("#<perry:private-value:")?;
    let role = spelling.rsplit_once("[[")?.1.strip_suffix("]]>")?;
    let (name, part) = if let Some(name) = role.strip_suffix("Prototype") {
        (name, IntrinsicPart::Prototype)
    } else {
        (
            role.strip_suffix("Constructor")?,
            IntrinsicPart::Constructor,
        )
    };
    (!name.is_empty()).then_some((name, part))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_intrinsic_role_comes_from_qualified_key_not_a_family_list() {
        assert_eq!(
            intrinsic_target("#<perry:private-value:0:@0:[[ExamplePrototype]]>"),
            Some(("Example", IntrinsicPart::Prototype))
        );
        assert_eq!(
            intrinsic_target("#<perry:private-value:0:@0:[[OtherConstructor]]>"),
            Some(("Other", IntrinsicPart::Constructor))
        );
        for key in [
            "ExamplePrototype",
            "#<perry:private-value:0:@0:[[RegExpMatcher]]>",
            "#<perry:private-value:0:@0:[[Prototype]]>",
        ] {
            assert_eq!(intrinsic_target(key), None);
        }
    }
}
