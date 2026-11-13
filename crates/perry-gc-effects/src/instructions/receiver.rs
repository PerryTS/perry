use super::*;

pub(super) fn record(
    node: &mut Node,
    insns: &[Insn],
    arch: Arch,
    switches: &HashSet<usize>,
    i: usize,
    target: &str,
) {
    if !crate::receiver::method(&node.fmt.norm(target)) {
        return;
    }
    let receiver = if arch == Arch::X86 && node.fmt == Format::Elf {
        Cfg::new(insns, arch, switches).receiver(i, "rdi", 0)
    } else {
        None
    };
    node.receiver_calls.push((node.fmt.norm(target), receiver));
}

impl Cfg<'_> {
    fn receiver(&self, i: usize, reg: &str, depth: usize) -> Option<BTreeSet<String>> {
        if depth > 8 {
            return None;
        }
        let mut values = BTreeSet::new();
        let mut seen = HashSet::new();
        let mut work = vec![(self.block_of[i], i)];
        while let Some((block, upto)) = work.pop() {
            let mut found = false;
            for k in (self.starts[block]..upto).rev() {
                let ins = &self.insns[k];
                if !writes(ins, reg, Arch::X86) {
                    continue;
                }
                let value = if (ins.mn == "leaq" && ins.ops.contains("(%rip)"))
                    || (ins.mn == "movq"
                        && ins.ops.contains("(%rip)")
                        && ins.relocs.iter().any(|(r, _)| r.contains("GOTPCREL")))
                {
                    ins.relocs
                        .iter()
                        .find(|(r, _)| r.ends_with(":receiver"))
                        .map(|(_, t)| BTreeSet::from([t.clone()]))
                } else if ins.mn == "movq" {
                    rx!(r"^%(r\w+), %(r\w+)$")
                        .captures(&ins.ops)
                        .and_then(|c| self.receiver(k, &x86_family(&c[1]), depth + 1))
                } else {
                    None
                }?;
                values.extend(value);
                found = true;
                break;
            }
            if found {
                continue;
            }
            if block == 0 {
                if reg != "rdi" {
                    return None;
                }
                values.insert("$self".into());
            } else {
                if self.pred[block].is_empty() {
                    return None;
                }
                for &previous in &self.pred[block] {
                    if seen.insert(previous) {
                        work.push((previous, self.end(previous)));
                    }
                }
            }
        }
        (!values.is_empty()).then_some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn load(addr: u64, key: &str) -> Insn {
        let mut ins = Insn::new(addr, "leaq (%rip), %rdi");
        ins.relocs.push(("other:receiver".into(), key.into()));
        ins
    }
    #[test]
    fn receiver_union_and_unknown_branch_are_conservative() {
        let mut code = vec![
            Insn::new(0, "je 0x4"),
            load(1, "first"),
            Insn::new(2, "jmp 0x5"),
            Insn::new(3, "nop"),
            load(4, "second"),
            Insn::new(5, "callq 0x0"),
        ];
        let facts =
            |code: &[Insn]| Cfg::new(code, Arch::X86, &HashSet::new()).receiver(5, "rdi", 0);
        assert_eq!(
            facts(&code),
            Some(BTreeSet::from(["first".into(), "second".into()]))
        );
        code[4] = Insn::new(4, "movq (%rax), %rdi");
        assert_eq!(facts(&code), None);
    }
    #[test]
    fn field_load_and_clobbered_argument_are_not_receiver_addresses() {
        let mut code = vec![
            load(0, "key"),
            Insn::new(1, "callq 0x0"),
            Insn::new(2, "callq 0x0"),
        ];
        assert_eq!(
            Cfg::new(&code, Arch::X86, &HashSet::new()).receiver(2, "rdi", 0),
            None
        );
        code[0].mn = "movq".into();
        code[1].mn = "nop".into();
        assert_eq!(
            Cfg::new(&code, Arch::X86, &HashSet::new()).receiver(2, "rdi", 0),
            None
        );
    }
}
