/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100cad07c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference>:
100cad07c:      stp x24, x23, [sp, #-0x40]!
100cad080:      stp x22, x21, [sp, #0x10]
100cad084:      stp x20, x19, [sp, #0x20]
100cad088:      stp x29, x30, [sp, #0x30]
100cad08c:      add x29, sp, #0x30
100cad090:      mov x19, x4
100cad094:      mov x20, x3
100cad098:      mov x21, x2
100cad09c:      mov x22, x1
100cad0a0:      mov x23, x0
100cad0a4:      mov x0, x3
100cad0a8:      mov x1, x4
100cad0ac:      bl  0x1005912b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>
100cad0b0:      tbz w0, #0x0, 0x100cad0c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference+0x4c>
100cad0b4:      ldp x29, x30, [sp, #0x30]
100cad0b8:      ldp x20, x19, [sp, #0x20]
100cad0bc:      ldp x22, x21, [sp, #0x10]
100cad0c0:      ldp x24, x23, [sp], #0x40
100cad0c4:      ret
100cad0c8:      mov x0, x23
100cad0cc:      mov x1, x22
100cad0d0:      mov x2, x21
100cad0d4:      mov x3, x20
100cad0d8:      mov x4, x19
100cad0dc:      bl  0x100cad104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify31panic_stale_forwarded_reference>
        ...
