use super::*;

fn identity() -> ModuleIdentity {
    ModuleIdentity {
        module: "main.ts".into(),
        source_hash: "source".into(),
        hir_hash: "hir".into(),
        lowering_hash: "opts".into(),
        target: "x86_64-unknown-linux-gnu".into(),
    }
}
fn site() -> Site {
    Site {
        site_id: 42,
        function: "read".into(),
        kind: "array_element".into(),
        operation: "array[index]".into(),
        observation_kind: NUMERIC_ARRAY_ELEMENT.into(),
    }
}
fn profile() -> Profile {
    Profile {
        schema_version: SCHEMA_VERSION,
        compiler: "compiler".into(),
        modules: vec![ModuleProfile {
            identity: identity(),
            sites: vec![site()],
        }],
    }
}
fn enter(profile: &Profile) -> Scope {
    Scope(ACTIVE.with(|active| {
        active.replace(Some(ModuleState::new(
            "compiler",
            Some(profile),
            identity(),
        )))
    }))
}
fn register() {
    register_site(42, "read", "array_element", "array[index]");
}

#[test]
fn exact_freshness_and_duplicate_rejections() {
    let cases: &[(&str, fn(&mut Profile))] = &[
        ("schema_mismatch", |p| p.schema_version += 1),
        ("compiler_mismatch", |p| p.compiler.push('x')),
        ("source_hash_mismatch", |p| {
            p.modules[0].identity.source_hash.push('x')
        }),
        ("target_mismatch", |p| {
            p.modules[0].identity.target.push('x')
        }),
        ("hir_hash_mismatch", |p| {
            p.modules[0].identity.hir_hash.push('x')
        }),
        ("lowering_inputs_mismatch", |p| {
            p.modules[0].identity.lowering_hash.push('x')
        }),
        ("unsupported_observation_kind", |p| {
            p.modules[0].sites[0].observation_kind = "shape_address".into()
        }),
        ("duplicate_module", |p| p.modules.push(p.modules[0].clone())),
        ("duplicate_site", |p| p.modules[0].sites.push(site())),
    ];
    for (reason, mutate) in cases {
        let mut profile = profile();
        mutate(&mut profile);
        let _scope = enter(&profile);
        register();
        assert!(select_numeric_array(42, false).is_none(), "{reason}");
        let mut records = Vec::new();
        finish_module(&mut records);
        assert!(!records.is_empty(), "{reason}");
        assert!(records.iter().all(|r| r
            .notes
            .contains(&format!("typed_feedback_replay_rejected={reason}"))));
    }
}

#[test]
fn site_matching_requires_identity_and_supported_lowering() {
    for reason in [
        "site_identity_mismatch",
        "unknown_site",
        "unsupported_site",
        "already_specialized",
    ] {
        let mut profile = profile();
        if reason == "site_identity_mismatch" {
            profile.modules[0].sites[0].function = "other".into();
        }
        let _scope = enter(&profile);
        if reason != "unknown_site" {
            register();
        }
        if matches!(reason, "site_identity_mismatch" | "already_specialized") {
            assert!(select_numeric_array(42, reason == "already_specialized").is_none());
        }
        let mut records = Vec::new();
        finish_module(&mut records);
        assert_eq!(records[0].rejected_facts[0].detail, reason);
    }
}

#[test]
fn unknown_module_and_empty_stale_profile_are_explained() {
    let session = Session::new("compiler".into(), Some(profile()));
    assert_eq!(session.finish(None).unwrap()[0].reason, "unknown_module");
    let mut profile = profile();
    profile.modules.clear();
    profile.schema_version += 1;
    let session = Session::new("compiler".into(), Some(profile));
    assert_eq!(session.finish(None).unwrap()[0].reason, "schema_mismatch");
}

#[test]
fn fresh_fact_is_consumed_once_and_scope_restores_on_unwind() {
    let _scope = enter(&profile());
    register();
    let fact = select_numeric_array(42, false).unwrap();
    assert_eq!(fact.state, "consumed");
    assert!(fact.detail.contains("advisory=true"));
    assert!(select_numeric_array(42, false).is_none());
    let result = std::panic::catch_unwind(|| {
        let _inner = enter(&profile());
        panic!("scope sabotage");
    });
    assert!(result.is_err());
    assert!(
        select_numeric_array(42, false).is_none(),
        "outer consumed state must be restored"
    );
}

fn valid_records() -> Vec<NativeRepRecord> {
    use crate::native_value::{BoundsState, BufferAccessMode, MaterializationReason};
    let _scope = enter(&profile());
    register();
    let fact = select_numeric_array(42, false).unwrap();
    let mut fast = rejection_record(&rejected("main.ts", &site(), "unused"));
    fast.expr_kind = "NumericArrayIndexGet".into();
    fast.consumer = "js_array_numeric_get_f64_unboxed".into();
    fast.native_rep = crate::native_value::NativeRep::F64;
    fast.notes = vec!["typed_feedback_replay_selected=fresh_numeric_array_observation".into()];
    fast.bounds_state = Some(BoundsState::Guarded {
        guard_id: NUMERIC_GUARD.into(),
    });
    fast.access_mode = Some(BufferAccessMode::CheckedNative);
    fast.consumed_facts = vec![
        fact.clone(),
        NativeFactUse {
            fact_id: "layout".into(),
            kind: "raw_f64_layout".into(),
            local_id: None,
            state: "consumed".into(),
            detail: NUMERIC_GUARD.into(),
            reason: None,
        },
    ];
    let mut fallback = fast.clone();
    fallback.block_label = "fallback".into();
    fallback.consumed_facts.clear();
    fallback.notes = vec![format!("typed_feedback_replay_fallback={}", fact.fact_id)];
    fallback.consumer = ARRAY_FALLBACK.into();
    fallback.access_mode = Some(BufferAccessMode::DynamicFallback);
    fallback.materialization_reason = Some(MaterializationReason::RuntimeApi);
    vec![fast, fallback]
}

#[test]
fn verifier_rejects_replay_claims_without_each_required_proof() {
    let mut errors = Vec::new();
    verify_records(&valid_records(), &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let sabotages: &[fn(&mut Vec<NativeRepRecord>)] = &[
        |r| r[0].consumed_facts.remove(0).state.clear(),
        |r| r[0].consumed_facts[0].state = "rejected".into(),
        |r| r[0].consumed_facts[0].detail = "stale".into(),
        |r| r[0].notes.clear(),
        |r| r[0].bounds_state = None,
        |r| r[0].consumed_facts[1].detail = "wrong_guard".into(),
        |r| {
            r.pop();
        },
        |r| r[1].notes.clear(),
        |r| r[1].function = "different_function".into(),
        |r| r[1].materialization_reason = None,
        |r| r[1].consumer = "wrong_fallback".into(),
    ];
    for sabotage in sabotages {
        let mut records = valid_records();
        sabotage(&mut records);
        let mut errors = Vec::new();
        verify_records(&records, &mut errors);
        assert!(
            !errors.is_empty(),
            "verifier accepted sabotaged replay record"
        );
    }
}
