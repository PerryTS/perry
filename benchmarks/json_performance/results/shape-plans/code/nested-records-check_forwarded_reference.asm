/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100ca9da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference>:
100ca9da4:      stp x24, x23, [sp, #-0x40]!
100ca9da8:      stp x22, x21, [sp, #0x10]
100ca9dac:      stp x20, x19, [sp, #0x20]
100ca9db0:      stp x29, x30, [sp, #0x30]
100ca9db4:      add x29, sp, #0x30
100ca9db8:      mov x19, x4
100ca9dbc:      mov x20, x3
100ca9dc0:      mov x21, x2
100ca9dc4:      mov x22, x1
100ca9dc8:      mov x23, x0
100ca9dcc:      mov x0, x3
100ca9dd0:      mov x1, x4
100ca9dd4:      bl  0x100548f34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>
100ca9dd8:      tbz w0, #0x0, 0x100ca9df0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference+0x4c>
100ca9ddc:      ldp x29, x30, [sp, #0x30]
100ca9de0:      ldp x20, x19, [sp, #0x20]
100ca9de4:      ldp x22, x21, [sp, #0x10]
100ca9de8:      ldp x24, x23, [sp], #0x40
100ca9dec:      ret
100ca9df0:      mov x0, x23
100ca9df4:      mov x1, x22
100ca9df8:      mov x2, x21
100ca9dfc:      mov x3, x20
100ca9e00:      mov x4, x19
100ca9e04:      bl  0x100ca9e34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify31panic_stale_forwarded_reference>
        ...
