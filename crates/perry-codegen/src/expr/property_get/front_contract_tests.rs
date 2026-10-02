//! The miss front includes target-specific directory lookup blocks. Follow
//! their CFG and the call operand rather than assuming a single block or ELF.

use super::{tower_block, tower_blocks, tower_cond_br};
type Blocks = [(String, Vec<String>)];

fn targets(body: &[String]) -> Vec<&str> {
    body.last()
        .into_iter()
        .flat_map(|line| line.split("label %").skip(1))
        .map(|label| label.trim_end_matches([',', ' ']))
        .collect()
}

fn predecessors<'a>(blocks: &'a Blocks, label: &str) -> Vec<&'a str> {
    blocks
        .iter()
        .filter(|(_, body)| targets(body).contains(&label))
        .map(|(name, _)| name.as_str())
        .collect()
}

fn verify_front_flow(blocks: &Blocks) -> Result<usize, String> {
    let calls: Vec<_> = blocks
        .iter()
        .enumerate()
        .filter(|(_, (_, body))| {
            body.iter()
                .any(|line| line.contains("call double @js_object_get_field_ic_front("))
        })
        .collect();
    let [(index, (call_label, call_body))] = calls.as_slice() else {
        return Err(format!("expected exactly one front call: {calls:?}"));
    };
    let (entry, _) = tower_block(blocks, "pic.miss.front");
    let (token, token_body) = tower_block(blocks, "pic.token");
    if predecessors(blocks, entry) != [token] || tower_cond_br(token_body).2 != entry {
        return Err("the front entry must be dominated by the token compare".into());
    }
    // Every branch between the token miss and the front call resolves the
    // directory; both lookup failures still call the front using the empty
    // directory. No path may escape to a collecting exit or bypass the call.
    if call_label != entry {
        let (tsd, _) = tower_block(blocks, "agent_ptr.hot_tls.tsd");
        let (fast, _) = tower_block(blocks, "agent_ptr.hot_tls.fast");
        let (slow, _) = tower_block(blocks, "agent_ptr.hot_tls.slow");
        let (join, _) = tower_block(blocks, "agent_ptr.join");
        if call_label != join {
            return Err(format!(
                "the directory lookup must join at the front call: {call_label}"
            ));
        }
        for (label, expected_targets, expected_preds) in [
            (entry, vec![tsd, slow], vec![token]),
            (tsd, vec![fast, slow], vec![entry]),
            (fast, vec![join], vec![tsd]),
            (slow, vec![join], vec![entry, tsd]),
        ] {
            let body = &blocks.iter().find(|(l, _)| l == label).unwrap().1;
            if targets(body) != expected_targets || predecessors(blocks, label) != expected_preds {
                return Err(format!("directory CFG changed at {label}: {body:?}"));
            }
            if body
                .iter()
                .any(|line| line.contains("@js_object_get_field"))
            {
                return Err(format!(
                    "directory lookup must not call a property exit: {body:?}"
                ));
            }
        }
        let lookup_blocks: Vec<_> = blocks
            .iter()
            .filter(|(l, _)| l.starts_with("agent_ptr."))
            .map(|(l, _)| l.as_str())
            .collect();
        if lookup_blocks != [tsd, fast, slow, join] {
            return Err(format!(
                "directory lookup must keep exactly four blocks: {lookup_blocks:?}"
            ));
        }
        if predecessors(blocks, join) != [fast, slow] {
            return Err(
                "the front call must have only the two directory lookup predecessors".into(),
            );
        }
    }
    if !call_body
        .last()
        .is_some_and(|line| line.starts_with("br i1 %"))
    {
        return Err("front decline must use a live conditional branch".into());
    }
    let (cond, served, declined) = tower_cond_br(call_body);
    let call = call_body
        .iter()
        .find(|line| line.contains("call double @js_object_get_field_ic_front("))
        .unwrap();
    let answer = call.split_once(" = ").unwrap().0;
    let bits = call_body
        .iter()
        .find_map(|line| {
            let (reg, rhs) = line.split_once(" = ")?;
            (rhs == format!("bitcast double {answer} to i64")).then_some(reg)
        })
        .ok_or("the decline must classify the front's own answer")?;
    if !call_body.iter().any(|line| {
        line == &format!(
            "{cond} = icmp ne i64 {bits}, {}",
            crate::nanbox::TAG_HOLE_I64
        )
    }) || !served.starts_with("pget.recv_merge.")
        || !declined.starts_with("pic.miss.call.")
    {
        return Err(format!(
            "the front must branch on its answer's TAG_HOLE decline: {call_body:?}"
        ));
    }
    let (_, merge) = tower_block(blocks, "pget.recv_merge");
    if !merge.iter().any(|line| {
        line.contains(" = phi double ") && line.contains(&format!("[ {answer}, %{call_label} ]"))
    }) {
        return Err(
            "the merge must take the served answer from the actual front call block".into(),
        );
    }
    if predecessors(blocks, &declined)
        .iter()
        .any(|p| *p != call_label && !p.starts_with("pget.recv_"))
    {
        return Err(
            "the slow call is reached only from the front decline or receiver failure".into(),
        );
    }
    Ok(*index)
}

pub(super) fn front_call_block(blocks: &Blocks) -> (&str, &[String]) {
    let index = verify_front_flow(blocks).unwrap_or_else(|e| panic!("{e}: {blocks:?}"));
    (&blocks[index].0, &blocks[index].1)
}

fn def<'a>(blocks: &'a Blocks, reg: &str) -> Result<&'a str, String> {
    blocks
        .iter()
        .flat_map(|(_, body)| body)
        .find_map(|line| {
            let (lhs, rhs) = line.split_once(" = ")?;
            (lhs == reg).then_some(rhs)
        })
        .ok_or_else(|| format!("no definition of {reg} in the tower function"))
}

/// Prove the front's first operand is the directory read, including Apple's
/// guarded phi and Windows' TEB-derived block. Nearby unrelated TLS text is
/// insufficient: every step must define the operand passed to the call.
pub(super) fn verify_front_directory(blocks: &Blocks) -> Result<(), String> {
    let index = verify_front_flow(blocks)?;
    let call = blocks[index]
        .1
        .iter()
        .find(|line| line.contains("call double @js_object_get_field_ic_front("))
        .unwrap();
    let dir = call
        .split_once("(ptr ")
        .unwrap()
        .1
        .split(',')
        .next()
        .unwrap();
    let mut value = dir;
    let rhs = def(blocks, value)?;
    if let Some(phi) = rhs.strip_prefix("phi ptr [ ") {
        let (fast_value, rest) = phi.split_once(", %").ok_or("directory phi's fast value")?;
        let (fast, slow) = rest
            .split_once(" ], [ ")
            .ok_or("directory phi's two incoming edges")?;
        let (slow_label, _) = tower_block(blocks, "agent_ptr.hot_tls.slow");
        let (fast_label, _) = tower_block(blocks, "agent_ptr.hot_tls.fast");
        if fast != fast_label || slow != format!("@PERRY_EMPTY_SHAPE_DIR, %{slow_label} ]") {
            return Err(format!(
                "directory phi must select the slot or empty fallback: {rhs}"
            ));
        }
        value = fast_value;
    }
    let rhs = def(blocks, value)?;
    if rhs == "call ptr @perry_shape_dir_cell()" {
        return Ok(());
    }
    let slot = rhs
        .strip_prefix("load ptr, ptr ")
        .ok_or("directory must be a pointer load")?;
    let at = def(blocks, slot)?;
    let block = at
        .strip_prefix("getelementptr i8, ptr ")
        .and_then(|s| s.strip_suffix(", i64 0"))
        .ok_or_else(|| format!("directory must come from agent pointer slot zero: {at}"))?;
    if block == "@PERRY_AGENT_PTRS" {
        return Ok(());
    }
    let rhs = def(blocks, block)?;
    if let Some(field) = rhs.strip_prefix("load ptr, ptr ") {
        let field = def(blocks, field)?;
        if !field.starts_with("getelementptr i8, ptr %")
            || !field.ends_with(&format!(
                ", i64 {}",
                crate::runtime_abi::HOT_TLS_AGENT_PTRS_OFFSET
            ))
        {
            return Err(format!(
                "Apple directory must use HotTls.agent_ptrs: {field}"
            ));
        }
        let hot = field
            .strip_prefix("getelementptr i8, ptr ")
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        let slot = def(blocks, hot)?
            .strip_prefix("load ptr, ptr ")
            .ok_or("Apple TSD slot load")?;
        let addr = def(blocks, slot)?
            .strip_prefix("inttoptr i64 ")
            .and_then(|s| s.strip_suffix(" to ptr"))
            .ok_or("Apple TSD slot address")?;
        let (base, offset) = def(blocks, addr)?
            .strip_prefix("add i64 ")
            .and_then(|s| s.split_once(", "))
            .ok_or("Apple TSD index")?;
        let tsd = def(blocks, base)?
            .strip_prefix("and i64 ")
            .and_then(|s| s.strip_suffix(", -8"))
            .ok_or("Apple TSD base mask")?;
        if def(blocks, tsd)? != "call i64 asm sideeffect \"mrs $0, tpidrro_el0\", \"=r\"()" {
            return Err("Apple directory must derive from the current thread pointer".into());
        }
        let key = def(blocks, offset)?
            .strip_prefix("shl i64 ")
            .and_then(|s| s.strip_suffix(", 3"))
            .ok_or("Apple TSD key offset")?;
        if def(blocks, key)? != "load atomic i64, ptr @PERRY_HOT_TSD_KEY monotonic, align 8" {
            return Err("Apple directory must use the published TSD key".into());
        }
        for (prefix, predicate) in [
            ("pic.miss.front", format!("icmp ne i64 {key}, -1")),
            ("agent_ptr.hot_tls.tsd", format!("icmp ne ptr {hot}, null")),
        ] {
            let (_, body) = tower_block(blocks, prefix);
            if !body.last().is_some_and(|l| l.starts_with("br i1 %")) {
                return Err(format!(
                    "Apple lookup must consult its live {prefix} predicate"
                ));
            }
            let cond = tower_cond_br(body).0;
            if def(blocks, &cond)? != predicate {
                return Err(format!("wrong Apple lookup guard in {prefix}"));
            }
        }
        return Ok(());
    }
    // Windows: TLS array -> image's indexed TLS block -> SECREL offset.
    let (image, offset) = rhs
        .strip_prefix("getelementptr i8, ptr ")
        .and_then(|s| s.split_once(", i64 "))
        .ok_or("Windows agent block address")?;
    let offset = def(blocks, offset)?
        .strip_prefix("zext i32 ")
        .and_then(|s| s.strip_suffix(" to i64"))
        .ok_or("SECREL extension")?;
    if def(blocks, offset)? != "load i32, ptr @PERRY_AGENT_PTRS_SECREL" {
        return Err("wrong Windows SECREL".into());
    }
    let entry = def(blocks, image)?
        .strip_prefix("load ptr, ptr ")
        .ok_or("Windows image TLS block load")?;
    let (array, index) = def(blocks, entry)?
        .strip_prefix("getelementptr ptr, ptr ")
        .and_then(|s| s.split_once(", i64 "))
        .ok_or("Windows TLS image entry")?;
    let index = def(blocks, index)?
        .strip_prefix("zext i32 ")
        .and_then(|s| s.strip_suffix(" to i64"))
        .ok_or("TLS index extension")?;
    if def(blocks, index)? != "load i32, ptr @_tls_index"
        || def(blocks, array)?
            != "load ptr, ptr addrspace(256) inttoptr (i64 88 to ptr addrspace(256)), align 8"
    {
        return Err(
            "Windows directory must derive from this thread's TEB and image TLS index".into(),
        );
    }
    Ok(())
}

#[test]
fn front_contract_rejects_lookup_bypasses_wrong_slots_and_wrong_declines() {
    for target in [
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        let mut opts = super::ir_opts(false, None);
        opts.target = Some(target.into());
        let ir = String::from_utf8(
            crate::compile_module(&super::module_with_nullish_read(), opts).unwrap(),
        )
        .unwrap();
        let blocks = tower_blocks(&ir);
        let index = verify_front_flow(&blocks).unwrap();
        verify_front_directory(&blocks).unwrap();
        let mut wrong = blocks.clone();
        let cond = tower_cond_br(&wrong[index].1).0;
        let term = wrong[index].1.last_mut().unwrap();
        *term = term.replacen(&cond, "true", 1);
        assert!(
            verify_front_flow(&wrong).is_err(),
            "{target}: hardwired decline"
        );
        let mut wrong = blocks.clone();
        for (_, body) in &mut wrong {
            for line in body {
                if line.contains("getelementptr i8, ptr ") && line.ends_with(", i64 0") {
                    *line = line.strip_suffix(", i64 0").unwrap().to_string() + ", i64 8";
                }
            }
        }
        assert_ne!(wrong, blocks, "{target}: slot sabotage must alter IR");
        assert!(
            verify_front_directory(&wrong).is_err(),
            "{target}: wrong agent slot"
        );
        let mut wrong = blocks.clone();
        let call = wrong[index]
            .1
            .iter_mut()
            .find(|l| l.contains("call double @js_object_get_field_ic_front("))
            .unwrap();
        let operand = call
            .split_once("(ptr ")
            .unwrap()
            .1
            .split(',')
            .next()
            .unwrap()
            .to_string();
        *call = call.replacen(
            &format!("(ptr {operand},"),
            "(ptr @PERRY_EMPTY_SHAPE_DIR,",
            1,
        );
        assert_ne!(wrong, blocks, "{target}: operand sabotage must alter IR");
        assert!(
            verify_front_directory(&wrong).is_err(),
            "{target}: disconnected directory operand"
        );
        let mut wrong = blocks.clone();
        let (entry, _) = tower_block(&blocks, "pic.miss.front");
        let (_, _, slow) = tower_cond_br(&blocks[index].1);
        let body = &mut wrong.iter_mut().find(|(l, _)| l == entry).unwrap().1;
        *body.last_mut().unwrap() = format!("br label %{slow}");
        assert!(verify_front_flow(&wrong).is_err(), "{target}: front bypass");
    }
}
