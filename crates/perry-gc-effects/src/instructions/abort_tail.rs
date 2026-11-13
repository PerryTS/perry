//! Drop edges in an unconditionally aborting suffix do not return to the
//! agent. This proof depends on control flow, never the TLS or dropped type.
use super::*;

pub(super) fn is_drop(target: &str) -> bool {
    let target = target.strip_prefix(".text.").unwrap_or(target);
    let dem = format!("{:#}", rustc_demangle::demangle(target));
    dem.starts_with("core::ptr::drop_glue::") || dem.starts_with("core::ptr::drop_in_place<")
}

pub(super) fn doomed(insns: &[Insn], arch: Arch, fmt: Format) -> Vec<bool> {
    let addresses: HashMap<_, _> = insns
        .iter()
        .enumerate()
        .map(|(i, ins)| (ins.addr, i))
        .collect();
    let aborts: Vec<_> = insns
        .iter()
        .map(|ins| {
            matches!(flow(ins, arch).0, Flow::Call | Flow::Jump | Flow::Exit)
                && ins.relocs.iter().any(|(_, target)| {
                    let target = fmt.norm(target);
                    let target = target.strip_prefix(".text.").unwrap_or(&target);
                    format!("{:#}", rustc_demangle::demangle(target)) == "std::process::abort"
                })
        })
        .collect();
    let mut result = aborts.clone();
    loop {
        let mut changed = false;
        for i in (0..insns.len()).rev() {
            if result[i] {
                continue;
            }
            let (kind, target) = flow(&insns[i], arch);
            let fall = result.get(i + 1).copied().unwrap_or(false);
            let branch = target
                .and_then(|a| addresses.get(&a))
                .is_some_and(|&i| result[i]);
            let proven = match kind {
                Flow::None | Flow::Call => fall,
                Flow::Jump => branch,
                Flow::Cond => fall && branch,
                Flow::Exit => false,
            };
            if proven {
                result[i] = true;
                changed = true;
            }
        }
        if !changed {
            return result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn abort(addr: u64) -> Insn {
        let mut ins = Insn::new(addr, "callq *(%rip)");
        ins.relocs
            .push(("GOTPCREL".into(), "std::process::abort".into()));
        ins
    }
    #[test]
    fn unconditional_abort_proof_rejects_returning_and_looping_successors() {
        let mut code = vec![
            Insn::new(0, "callq 0x0"),
            Insn::new(1, "je 0x3"),
            abort(2),
            Insn::new(3, "retq"),
        ];
        assert_eq!(
            doomed(&code, Arch::X86, Format::Elf),
            vec![false, false, true, false]
        );
        code[3] = abort(3);
        assert_eq!(doomed(&code, Arch::X86, Format::Elf), vec![true; 4]);
        code[3] = Insn::new(3, "jmp 0x1");
        assert_eq!(
            doomed(&code, Arch::X86, Format::Elf),
            vec![false, false, true, false]
        );
    }
    #[test]
    fn drop_recognition_does_not_depend_on_tls_or_error_type() {
        assert!(is_drop("core::ptr::drop_glue::<AnyOwner>"));
        assert!(!is_drop("tls::initializer"));
        assert!(!is_drop("allocator"));
    }
}
