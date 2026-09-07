/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100caaf50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference>:
100caaf50:      stp x24, x23, [sp, #-0x40]!
100caaf54:      stp x22, x21, [sp, #0x10]
100caaf58:      stp x20, x19, [sp, #0x20]
100caaf5c:      stp x29, x30, [sp, #0x30]
100caaf60:      add x29, sp, #0x30
100caaf64:      mov x19, x4
100caaf68:      mov x20, x3
100caaf6c:      mov x21, x2
100caaf70:      mov x22, x1
100caaf74:      mov x23, x0
100caaf78:      mov x0, x3
100caaf7c:      mov x1, x4
100caaf80:      bl  0x10085d5b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>
100caaf84:      tbz w0, #0x0, 0x100caaf9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify25check_forwarded_reference+0x4c>
100caaf88:      ldp x29, x30, [sp, #0x30]
100caaf8c:      ldp x20, x19, [sp, #0x20]
100caaf90:      ldp x22, x21, [sp, #0x10]
100caaf94:      ldp x24, x23, [sp], #0x40
100caaf98:      ret
100caaf9c:      mov x0, x23
100caafa0:      mov x1, x22
100caafa4:      mov x2, x21
100caafa8:      mov x3, x20
100caafac:      mov x4, x19
100caafb0:      bl  0x100caafe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6verify31panic_stale_forwarded_reference>
        ...
