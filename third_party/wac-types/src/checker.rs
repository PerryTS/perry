use crate::{
    CoreExtern, CoreFuncType, DefinedType, DefinedTypeId, Enum, Flags, FuncTypeId, InterfaceId,
    ItemKind, ModuleTypeId, PrimitiveType, Record, ResourceId, Type, Types, ValueType, Variant,
    WorldId,
};
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Represents the kind of subtyping check to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubtypeCheck {
    /// The type is a covariant check.
    Covariant,
    /// The type is a contravariant check.
    Contravariant,
}

/// Implements a subtype checker.
///
/// Subtype checking is used to type check instantiation arguments.
pub struct SubtypeChecker<'a> {
    kinds: Vec<SubtypeCheck>,
    cache: &'a mut HashSet<(ItemKind, ItemKind)>,
}

impl<'a> SubtypeChecker<'a> {
    /// Creates a new subtype checker with the given cache.
    pub fn new(cache: &'a mut HashSet<(ItemKind, ItemKind)>) -> Self {
        Self {
            kinds: Default::default(),
            cache,
        }
    }

    fn kind(&self) -> SubtypeCheck {
        self.kinds
            .last()
            .copied()
            .unwrap_or(SubtypeCheck::Covariant)
    }

    /// Checks if `a` is a subtype of `b`.
    pub fn is_subtype(&mut self, a: ItemKind, at: &Types, b: ItemKind, bt: &Types) -> Result<()> {
        if self.cache.contains(&(a, b)) {
            return Ok(());
        }

        let result = self.is_subtype_(a, at, b, bt);
        if result.is_ok() {
            self.cache.insert((a, b));
        }

        result
    }

    /// Inverts the current subtype check being performed.
    ///
    /// Returns the previous subtype check.
    pub fn invert(&mut self) -> SubtypeCheck {
        let prev = self.kind();
        self.kinds.push(match prev {
            SubtypeCheck::Covariant => SubtypeCheck::Contravariant,
            SubtypeCheck::Contravariant => SubtypeCheck::Covariant,
        });
        prev
    }

    /// Reverts to the previous check kind.
    pub fn revert(&mut self) {
        self.kinds.pop().expect("mismatched stack");
    }

    fn is_subtype_(&mut self, a: ItemKind, at: &Types, b: ItemKind, bt: &Types) -> Result<()> {
        match (a, b) {
            (ItemKind::Type(a), ItemKind::Type(b)) => self.ty(a, at, b, bt),
            (ItemKind::Func(a), ItemKind::Func(b)) => self.func(a, at, b, bt),
            (ItemKind::Instance(a), ItemKind::Instance(b)) => self.interface(a, at, b, bt),
            (ItemKind::Component(a), ItemKind::Component(b)) => self.world(a, at, b, bt),
            (ItemKind::Module(a), ItemKind::Module(b)) => self.module(a, at, b, bt),
            (ItemKind::Value(a), ItemKind::Value(b)) => self.value_type(a, at, b, bt),

            (ItemKind::Type(_), _)
            | (ItemKind::Func(_), _)
            | (ItemKind::Instance(_), _)
            | (ItemKind::Component(_), _)
            | (ItemKind::Module(_), _)
            | (ItemKind::Value(_), _) => {
                let (expected, expected_types, found, found_types) =
                    self.expected_found(&a, at, &b, bt);
                bail!(
                    "expected {expected}, found {found}",
                    expected = expected.desc(expected_types),
                    found = found.desc(found_types)
                )
            }
        }
    }

    fn expected_found<'b, T>(
        &self,
        a: &'b T,
        at: &'b Types,
        b: &'b T,
        bt: &'b Types,
    ) -> (&'b T, &'b Types, &'b T, &'b Types) {
        match self.kind() {
            // For covariant checks, the supertype is the expected type
            SubtypeCheck::Covariant => (b, bt, a, at),
            // For contravariant checks, the subtype is the expected type
            SubtypeCheck::Contravariant => (a, at, b, bt),
        }
    }

    fn resource(&self, a: ResourceId, at: &Types, b: ResourceId, bt: &Types) -> Result<()> {
        if a == b {
            return Ok(());
        }

        let a = &at[at.resolve_resource(a)];
        let b = &bt[bt.resolve_resource(b)];
        if a.name != b.name {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);

            bail!(
                "expected resource `{expected}`, found resource `{found}`",
                expected = expected.name,
                found = found.name
            );
        }

        Ok(())
    }

    fn ty(&mut self, a: Type, at: &Types, b: Type, bt: &Types) -> Result<()> {
        match (a, b) {
            (Type::Resource(a), Type::Resource(b)) => self.resource(a, at, b, bt),
            (Type::Func(a), Type::Func(b)) => self.func(a, at, b, bt),
            (Type::Value(a), Type::Value(b)) => self.value_type(a, at, b, bt),
            (Type::Interface(a), Type::Interface(b)) => self.interface(a, at, b, bt),
            (Type::World(a), Type::World(b)) => self.world(a, at, b, bt),
            (Type::Module(a), Type::Module(b)) => self.module(a, at, b, bt),

            (Type::Func(_), _)
            | (Type::Resource(_), _)
            | (Type::Value(_), _)
            | (Type::Interface(_), _)
            | (Type::World(_), _)
            | (Type::Module(_), _) => {
                let (expected, expected_types, found, found_types) =
                    self.expected_found(&a, at, &b, bt);

                bail!(
                    "expected {expected}, found {found}",
                    expected = expected.desc(expected_types),
                    found = found.desc(found_types)
                )
            }
        }
    }

    fn func(&self, a: FuncTypeId, at: &Types, b: FuncTypeId, bt: &Types) -> Result<()> {
        if a == b {
            return Ok(());
        }

        let a = &at[a];
        let b = &bt[b];

        // Note: currently subtyping for functions is done in terms of equality
        // rather than actual subtyping; the reason for this is that implementing
        // runtimes don't yet support more complex subtyping rules.

        if a.is_async != b.is_async {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected {} function, found {} function",
                if expected.is_async { "async" } else { "sync" },
                if found.is_async { "async" } else { "sync" },
            );
        }

        if a.params.len() != b.params.len() {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected function with parameter count {expected}, found parameter count {found}",
                expected = expected.params.len(),
                found = found.params.len(),
            );
        }

        for (i, ((an, a), (bn, b))) in a.params.iter().zip(b.params.iter()).enumerate() {
            if an != bn {
                let (expected, _, found, _) = self.expected_found(an, at, bn, bt);
                bail!("expected function parameter {i} to be named `{expected}`, found name `{found}`");
            }

            self.value_type(*a, at, *b, bt)
                .with_context(|| format!("mismatched type for function parameter `{bn}`"))?;
        }

        match (&a.result, &b.result) {
            (None, None) => return Ok(()),
            (Some(a), Some(b)) => {
                return self
                    .value_type(*a, at, *b, bt)
                    .context("mismatched type for function result");
            }
            (None, _) | (Some(_), _) => {
                // Handle the mismatch below
            }
        }

        let (expected, _, found, _) = self.expected_found(a, at, b, bt);
        match (&expected.result, &found.result) {
            (Some(_), None) => {
                bail!("expected function with a result, found function without a result")
            }
            (None, Some(_)) => {
                bail!("expected function without a result, found function with a result")
            }
            (Some(_), Some(_)) | (None, None) => panic!("should already be handled"),
        }
    }

    fn instance_exports(
        &mut self,
        a: &IndexMap<String, ItemKind>,
        at: &Types,
        b: &IndexMap<String, ItemKind>,
        bt: &Types,
    ) -> Result<()> {
        // For instance type subtyping, all exports in the other
        // instance type must be present in this instance type's
        // exports (i.e. it can export *more* than what this instance
        // type needs).
        for (k, b) in b.iter() {
            match a.get(k) {
                Some(a) => {
                    self.is_subtype(*a, at, *b, bt)
                        .with_context(|| format!("mismatched type for export `{k}`"))?;
                }
                None => match self.kind() {
                    SubtypeCheck::Covariant => {
                        bail!(
                            "instance is missing expected {kind} export `{k}`",
                            kind = b.desc(bt)
                        )
                    }
                    SubtypeCheck::Contravariant => {
                        bail!(
                            "instance has unexpected {kind} export `{k}`",
                            kind = b.desc(bt)
                        )
                    }
                },
            }
        }

        Ok(())
    }

    fn interface(&mut self, a: InterfaceId, at: &Types, b: InterfaceId, bt: &Types) -> Result<()> {
        if a == b {
            return Ok(());
        }

        let a = &at[a];
        let b = &bt[b];
        self.instance_exports(&a.exports, at, &b.exports, bt)
    }

    fn world(&mut self, a: WorldId, at: &Types, b: WorldId, bt: &Types) -> Result<()> {
        let a = &at[a];
        let b = &bt[b];

        // For component type subtyping, all exports in the other component
        // type must be present in this component type's exports (i.e. it
        // can export *more* than what this component type needs).
        // However, for imports, the check is reversed (i.e. it is okay
        // to import *less* than what this component type needs).
        let prev = self.invert();
        for (k, a) in a.imports.iter() {
            match b.imports.get(k) {
                Some(b) => {
                    self.is_subtype(*b, bt, *a, at)
                        .with_context(|| format!("mismatched type for import `{k}`"))?;
                }
                None => match prev {
                    SubtypeCheck::Covariant => {
                        bail!(
                            "component is missing expected {kind} import `{k}`",
                            kind = a.desc(at)
                        )
                    }
                    SubtypeCheck::Contravariant => {
                        bail!(
                            "component has unexpected import {kind} `{k}`",
                            kind = a.desc(at)
                        )
                    }
                },
            }
        }

        self.revert();

        for (k, b) in b.exports.iter() {
            match a.exports.get(k) {
                Some(a) => {
                    self.is_subtype(*a, at, *b, bt)
                        .with_context(|| format!("mismatched type for export `{k}`"))?;
                }
                None => match self.kind() {
                    SubtypeCheck::Covariant => {
                        bail!(
                            "component is missing expected {kind} export `{k}`",
                            kind = b.desc(bt)
                        )
                    }
                    SubtypeCheck::Contravariant => {
                        bail!(
                            "component has unexpected {kind} export `{k}`",
                            kind = b.desc(bt)
                        )
                    }
                },
            }
        }

        Ok(())
    }

    fn module(&mut self, a: ModuleTypeId, at: &Types, b: ModuleTypeId, bt: &Types) -> Result<()> {
        if a == b {
            return Ok(());
        }

        let a = &at[a];
        let b = &bt[b];

        // For module type subtyping, all exports in the other module
        // type must be present in expected module type's exports (i.e. it
        // can export *more* than what is expected module type needs).
        // However, for imports, the check is reversed (i.e. it is okay
        // to import *less* than what this module type needs).
        let prev = self.invert();
        for (k, a) in a.imports.iter() {
            match b.imports.get(k) {
                Some(b) => {
                    self.core_extern(b, bt, a, at).with_context(|| {
                        format!("mismatched type for import `{m}::{n}`", m = k.0, n = k.1)
                    })?;
                }
                None => match prev {
                    SubtypeCheck::Covariant => bail!(
                        "module is missing expected {a} import `{m}::{n}`",
                        m = k.0,
                        n = k.1
                    ),
                    SubtypeCheck::Contravariant => {
                        bail!(
                            "module has unexpected {a} import `{m}::{n}`",
                            m = k.0,
                            n = k.1
                        )
                    }
                },
            }
        }

        self.revert();

        for (k, b) in b.exports.iter() {
            match a.exports.get(k) {
                Some(a) => {
                    self.kinds.push(SubtypeCheck::Covariant);
                    let r = self
                        .core_extern(a, at, b, bt)
                        .with_context(|| format!("mismatched type for export `{k}`"));
                    self.kinds.pop();
                    r?;
                }
                None => match self.kind() {
                    SubtypeCheck::Covariant => {
                        bail!("module is missing expected {b} export `{k}`")
                    }
                    SubtypeCheck::Contravariant => {
                        bail!("module has unexpected {b} export `{k}`")
                    }
                },
            }
        }

        Ok(())
    }

    pub(crate) fn core_extern(
        &self,
        a: &CoreExtern,
        at: &Types,
        b: &CoreExtern,
        bt: &Types,
    ) -> Result<()> {
        macro_rules! limits_match {
            ($ai:expr, $am:expr, $bi:expr, $bm:expr) => {{
                $ai >= $bi
                    && match ($am, $bm) {
                        (Some(am), Some(bm)) => am <= bm,
                        (None, Some(_)) => false,
                        _ => true,
                    }
            }};
        }

        match (a, b) {
            (CoreExtern::Func(a), CoreExtern::Func(b)) => self.core_func(a, at, b, bt),
            (
                CoreExtern::Table {
                    element_type: ae,
                    initial: ai,
                    maximum: am,
                    table64: a64,
                    shared: ashared,
                },
                CoreExtern::Table {
                    element_type: be,
                    initial: bi,
                    maximum: bm,
                    table64: b64,
                    shared: bshared,
                },
            ) => {
                if ae != be {
                    let (expected, _, found, _) = self.expected_found(ae, at, be, bt);
                    bail!("expected table element type {expected}, found {found}");
                }

                if !limits_match!(ai, am, bi, bm) {
                    bail!("mismatched table limits");
                }

                if a64 != b64 {
                    bail!("mismatched table64 flag for tables");
                }

                if ashared != bshared {
                    bail!("mismatched shared flag for tables");
                }

                Ok(())
            }
            (
                CoreExtern::Memory {
                    memory64: a64,
                    shared: ashared,
                    initial: ai,
                    maximum: am,
                    page_size_log2: apsl,
                },
                CoreExtern::Memory {
                    memory64: b64,
                    shared: bshared,
                    initial: bi,
                    maximum: bm,
                    page_size_log2: bpsl,
                },
            ) => {
                if ashared != bshared {
                    bail!("mismatched shared flag for memories");
                }

                if a64 != b64 {
                    bail!("mismatched memory64 flag for memories");
                }

                if !limits_match!(ai, am, bi, bm) {
                    bail!("mismatched memory limits");
                }

                if apsl != bpsl {
                    bail!("mismatched page_size_log2 for memories");
                }

                Ok(())
            }
            (
                CoreExtern::Global {
                    val_type: avt,
                    mutable: am,
                    shared: ashared,
                },
                CoreExtern::Global {
                    val_type: bvt,
                    mutable: bm,
                    shared: bshared,
                },
            ) => {
                if am != bm {
                    bail!("mismatched mutable flag for globals");
                }

                if avt != bvt {
                    let (expected, _, found, _) = self.expected_found(avt, at, bvt, bt);
                    bail!("expected global type {expected}, found {found}");
                }

                if ashared != bshared {
                    bail!("mismatched shared flag for globals");
                }

                Ok(())
            }
            (CoreExtern::Tag(a), CoreExtern::Tag(b)) => self.core_func(a, at, b, bt),

            (CoreExtern::Func(_), _)
            | (CoreExtern::Table { .. }, _)
            | (CoreExtern::Memory { .. }, _)
            | (CoreExtern::Global { .. }, _)
            | (CoreExtern::Tag(_), _) => {
                let (expected, _, found, _) = self.expected_found(a, at, b, bt);
                bail!("expected {expected}, found {found}");
            }
        }
    }

    fn core_func(&self, a: &CoreFuncType, at: &Types, b: &CoreFuncType, bt: &Types) -> Result<()> {
        if a != b {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!("expected {expected}, found {found}");
        }

        Ok(())
    }

    fn value_type(&self, a: ValueType, at: &Types, b: ValueType, bt: &Types) -> Result<()> {
        let a = at.resolve_value_type(a);
        let b = bt.resolve_value_type(b);

        match (a, b) {
            (ValueType::Primitive(a), ValueType::Primitive(b)) => self.primitive(a, at, b, bt),
            (ValueType::Defined(a), ValueType::Defined(b)) => self.defined_type(a, at, b, bt),
            (ValueType::Borrow(a), ValueType::Borrow(b))
            | (ValueType::Own(a), ValueType::Own(b)) => self.resource(a, at, b, bt),

            (ValueType::Primitive(_), _)
            | (ValueType::Defined(_), _)
            | (ValueType::Borrow(_), _)
            | (ValueType::Own(_), _) => {
                let (expected, expected_types, found, found_types) =
                    self.expected_found(&a, at, &b, bt);
                bail!(
                    "expected {expected}, found {found}",
                    expected = expected.desc(expected_types),
                    found = found.desc(found_types)
                )
            }
        }
    }

    fn defined_type(
        &self,
        a: DefinedTypeId,
        at: &Types,
        b: DefinedTypeId,
        bt: &Types,
    ) -> std::result::Result<(), anyhow::Error> {
        if a == b {
            return Ok(());
        }

        let a = &at[a];
        let b = &bt[b];
        match (a, b) {
            (DefinedType::Tuple(a), DefinedType::Tuple(b)) => self.tuple(a, at, b, bt),
            (DefinedType::List(a), DefinedType::List(b)) => self
                .value_type(*a, at, *b, bt)
                .context("mismatched type for list element"),
            (DefinedType::FixedSizeList(a, asize), DefinedType::FixedSizeList(b, bsize)) => {
                if asize != bsize {
                    bail!("mismatched size for fixed size list element");
                }
                self.value_type(*a, at, *b, bt)
                    .context("mismatched type for fixed size list element")
            }
            (DefinedType::Map(akey, avalue), DefinedType::Map(bkey, bvalue)) => {
                self.value_type(*akey, at, *bkey, bt)
                    .context("mismatched type for map key")?;
                self.value_type(*avalue, at, *bvalue, bt)
                    .context("mismatched type for map value")
            }
            (DefinedType::Future(a), DefinedType::Future(b)) => self
                .payload(*a, at, *b, bt)
                .context("mismatched type for future payload"),
            (DefinedType::Stream(a), DefinedType::Stream(b)) => self
                .payload(*a, at, *b, bt)
                .context("mismatched type for stream payload"),
            (DefinedType::Option(a), DefinedType::Option(b)) => self
                .value_type(*a, at, *b, bt)
                .context("mismatched type for option"),
            (
                DefinedType::Result {
                    ok: a_ok,
                    err: a_err,
                },
                DefinedType::Result {
                    ok: b_ok,
                    err: b_err,
                },
            ) => {
                self.result("ok", a_ok, at, b_ok, bt)?;
                self.result("err", a_err, at, b_err, bt)
            }
            (DefinedType::Variant(a), DefinedType::Variant(b)) => self.variant(a, at, b, bt),
            (DefinedType::Record(a), DefinedType::Record(b)) => self.record(a, at, b, bt),
            (DefinedType::Flags(a), DefinedType::Flags(b)) => self.flags(a, at, b, bt),
            (DefinedType::Enum(a), DefinedType::Enum(b)) => self.enum_type(a, at, b, bt),
            (DefinedType::Alias(_), _) | (_, DefinedType::Alias(_)) => {
                panic!("aliases should have been resolved")
            }

            (DefinedType::Tuple(_), _)
            | (DefinedType::List(_), _)
            | (DefinedType::FixedSizeList(_, _), _)
            | (DefinedType::Map(_, _), _)
            | (DefinedType::Option(_), _)
            | (DefinedType::Result { .. }, _)
            | (DefinedType::Variant(_), _)
            | (DefinedType::Record(_), _)
            | (DefinedType::Flags(_), _)
            | (DefinedType::Enum(_), _)
            | (DefinedType::Stream(_), _)
            | (DefinedType::Future(_), _) => {
                let (expected, expected_types, found, found_types) =
                    self.expected_found(a, at, b, bt);
                bail!(
                    "expected {expected}, found {found}",
                    expected = expected.desc(expected_types),
                    found = found.desc(found_types)
                )
            }
        }
    }

    fn result(
        &self,
        desc: &str,
        a: &Option<ValueType>,
        at: &Types,
        b: &Option<ValueType>,
        bt: &Types,
    ) -> Result<()> {
        match (a, b) {
            (None, None) => return Ok(()),
            (Some(a), Some(b)) => {
                return self
                    .value_type(*a, at, *b, bt)
                    .with_context(|| format!("mismatched type for result `{desc}`"))
            }
            (Some(_), None) | (None, Some(_)) => {
                // Handle mismatch below
            }
        }

        let (expected, _, found, _) = self.expected_found(a, at, b, bt);
        match (expected, found) {
            (Some(_), None) => bail!("expected an `{desc}` for result type"),
            (None, Some(_)) => bail!("expected no `{desc}` for result type"),
            (None, None) | (Some(_), Some(_)) => panic!("expected to be handled"),
        }
    }

    fn enum_type(&self, a: &Enum, at: &Types, b: &Enum, bt: &Types) -> Result<()> {
        if a.0.len() != b.0.len() {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected an enum type case count of {expected}, found a count of {found}",
                expected = expected.0.len(),
                found = found.0.len()
            );
        }

        if let Some((index, (a, b))) =
            a.0.iter()
                .zip(b.0.iter())
                .enumerate()
                .find(|(_, (a, b))| a != b)
        {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!("expected enum case {index} to be named `{expected}`, found an enum case named `{found}`");
        }

        Ok(())
    }

    fn flags(&self, a: &Flags, at: &Types, b: &Flags, bt: &Types) -> Result<()> {
        if a.0.len() != b.0.len() {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected a flags type flag count of {expected}, found a count of {found}",
                expected = expected.0.len(),
                found = found.0.len()
            );
        }

        if let Some((index, (a, b))) =
            a.0.iter()
                .zip(b.0.iter())
                .enumerate()
                .find(|(_, (a, b))| a != b)
        {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!("expected flag {index} to be named `{expected}`, found a flag named `{found}`");
        }

        Ok(())
    }

    fn record(&self, a: &Record, at: &Types, b: &Record, bt: &Types) -> Result<()> {
        if a.fields.len() != b.fields.len() {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected a record field count of {expected}, found a count of {found}",
                expected = expected.fields.len(),
                found = found.fields.len()
            );
        }

        for (i, ((an, a), (bn, b))) in a.fields.iter().zip(b.fields.iter()).enumerate() {
            if an != bn {
                let (expected, _, found, _) = self.expected_found(an, at, bn, bt);
                bail!("expected record field {i} to be named `{expected}`, found a field named `{found}`");
            }

            self.value_type(*a, at, *b, bt)
                .with_context(|| format!("mismatched type for record field `{bn}`"))?;
        }

        Ok(())
    }

    fn variant(&self, a: &Variant, at: &Types, b: &Variant, bt: &Types) -> Result<()> {
        if a.cases.len() != b.cases.len() {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected a variant case count of {expected}, found a count of {found}",
                expected = expected.cases.len(),
                found = found.cases.len()
            );
        }

        for (i, ((an, a), (bn, b))) in a.cases.iter().zip(b.cases.iter()).enumerate() {
            if an != bn {
                let (expected, _, found, _) = self.expected_found(an, at, bn, bt);
                bail!("expected variant case {i} to be named `{expected}`, found a case named `{found}`");
            }

            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) => self
                    .value_type(*a, at, *b, bt)
                    .with_context(|| format!("mismatched type for variant case `{bn}`"))?,
                _ => {
                    let (expected, _, found, _) = self.expected_found(a, at, b, bt);
                    match (expected, found) {
                        (None, Some(_)) => {
                            bail!("expected variant case `{bn}` to be untyped, found a typed case")
                        }
                        (Some(_), None) => {
                            bail!("expected variant case `{bn}` to be typed, found an untyped case")
                        }
                        (None, None) | (Some(_), Some(_)) => panic!("expected to be handled"),
                    }
                }
            }
        }

        Ok(())
    }

    fn tuple(&self, a: &Vec<ValueType>, at: &Types, b: &Vec<ValueType>, bt: &Types) -> Result<()> {
        if a.len() != b.len() {
            let (expected, _, found, _) = self.expected_found(a, at, b, bt);
            bail!(
                "expected a tuple of size {expected}, found a tuple of size {found}",
                expected = expected.len(),
                found = found.len()
            );
        }

        for (i, (a, b)) in a.iter().zip(b.iter()).enumerate() {
            self.value_type(*a, at, *b, bt)
                .with_context(|| format!("mismatched type for tuple item {i}"))?;
        }

        Ok(())
    }

    fn payload(
        &self,
        a: Option<ValueType>,
        at: &Types,
        b: Option<ValueType>,
        bt: &Types,
    ) -> Result<()> {
        match (a, b) {
            (Some(a), Some(b)) => self.value_type(a, at, b, bt),
            (None, None) => Ok(()),
            (Some(_), None) => bail!("expected a type payload, found none"),
            (None, Some(_)) => bail!("expected no type payload, found one"),
        }
    }

    fn primitive(&self, a: PrimitiveType, at: &Types, b: PrimitiveType, bt: &Types) -> Result<()> {
        // Note: currently subtyping for primitive types is done in terms of equality
        // rather than actual subtyping; the reason for this is that implementing
        // runtimes don't yet support more complex subtyping rules.
        if a != b {
            let (expected, _, found, _) = self.expected_found(&a, at, &b, bt);
            bail!(
                "expected {expected}, found {found}",
                expected = expected.desc(),
                found = found.desc()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoreRefType, CoreType, HeapType};

    fn check_core_extern(a: &CoreExtern, b: &CoreExtern) -> Result<()> {
        let types = Types::default();
        let mut cache = HashSet::new();
        let checker = SubtypeChecker::new(&mut cache);
        checker.core_extern(a, &types, b, &types)
    }

    fn base_table() -> CoreExtern {
        CoreExtern::Table {
            element_type: CoreRefType {
                nullable: true,
                heap_type: HeapType::Func,
            },
            initial: 1,
            maximum: None,
            table64: false,
            shared: false,
        }
    }

    #[test]
    fn mismatched_table64_is_rejected() {
        let a = base_table();
        let mut b = base_table();
        if let CoreExtern::Table { table64, .. } = &mut b {
            *table64 = true;
        }
        assert!(
            check_core_extern(&a, &b).is_err(),
            "mismatched table64 should be rejected"
        );
    }

    #[test]
    fn mismatched_table_shared_is_rejected() {
        let a = base_table();
        let mut b = base_table();
        if let CoreExtern::Table { shared, .. } = &mut b {
            *shared = true;
        }
        assert!(
            check_core_extern(&a, &b).is_err(),
            "mismatched table shared should be rejected"
        );
    }

    #[test]
    fn mismatched_memory_page_size_log2_is_rejected() {
        let a = CoreExtern::Memory {
            memory64: false,
            shared: false,
            initial: 1,
            maximum: None,
            page_size_log2: Some(16),
        };
        let b = CoreExtern::Memory {
            memory64: false,
            shared: false,
            initial: 1,
            maximum: None,
            page_size_log2: Some(14),
        };
        assert!(
            check_core_extern(&a, &b).is_err(),
            "mismatched page_size_log2 should be rejected"
        );
    }

    #[test]
    fn mismatched_global_shared_is_rejected() {
        let a = CoreExtern::Global {
            val_type: CoreType::I32,
            mutable: false,
            shared: false,
        };
        let b = CoreExtern::Global {
            val_type: CoreType::I32,
            mutable: false,
            shared: true,
        };
        assert!(
            check_core_extern(&a, &b).is_err(),
            "mismatched global shared should be rejected"
        );
    }
}
