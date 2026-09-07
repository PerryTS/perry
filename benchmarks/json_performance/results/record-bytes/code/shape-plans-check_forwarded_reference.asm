/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100ccbb50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference>:
100ccbb50:      stp x24, x23, [sp, #-0x40]!
100ccbb54:      stp x22, x21, [sp, #0x10]
100ccbb58:      stp x20, x19, [sp, #0x20]
100ccbb5c:      stp x29, x30, [sp, #0x30]
100ccbb60:      add x29, sp, #0x30
100ccbb64:      mov x19, x4
100ccbb68:      mov x20, x3
100ccbb6c:      mov x21, x2
100ccbb70:      mov x22, x1
100ccbb74:      mov x23, x0
100ccbb78:      mov x0, x3
100ccbb7c:      mov x1, x4
100ccbb80:      bl  0x1007d7e34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>
100ccbb84:      tbz w0, #0x0, 0x100ccbb9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference+0x4c>
100ccbb88:      ldp x29, x30, [sp, #0x30]
100ccbb8c:      ldp x20, x19, [sp, #0x20]
100ccbb90:      ldp x22, x21, [sp, #0x10]
100ccbb94:      ldp x24, x23, [sp], #0x40
100ccbb98:      ret
100ccbb9c:      mov x0, x23
100ccbba0:      mov x1, x22
100ccbba4:      mov x2, x21
100ccbba8:      mov x3, x20
100ccbbac:      mov x4, x19
100ccbbb0:      bl  0x100ccbbe8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify31panic_stale_forwarded_reference>
        ...
