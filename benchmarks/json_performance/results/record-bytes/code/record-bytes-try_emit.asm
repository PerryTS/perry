/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008f00b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit>:
1008f00b8:      sub sp, sp, #0x50
1008f00bc:      stp x22, x21, [sp, #0x20]
1008f00c0:      stp x20, x19, [sp, #0x30]
1008f00c4:      stp x29, x30, [sp, #0x40]
1008f00c8:      add x29, sp, #0x40
1008f00cc:      mov x21, x2
1008f00d0:      mov x19, x1
1008f00d4:      mov x20, x0
1008f00d8:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008f00dc:      cbz x0, 0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f00e0:      ldrb    w8, [x0]
1008f00e4:      cmp w8, #0x1
1008f00e8:      b.ne    0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f00ec:      ldrsb   w8, [x0, #0x1]
1008f00f0:      tbnz    w8, #0x1f, 0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f00f4:      ldr w8, [x20]
1008f00f8:      cmp w8, #0x2
1008f00fc:      b.lo    0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f0100:      ldr w9, [x20, #0x4]
1008f0104:      cmp w8, w9
1008f0108:      b.ls    0x1008f0124 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x6c>
1008f010c:      mov w0, #0x0                ; =0
1008f0110:      ldp x29, x30, [sp, #0x40]
1008f0114:      ldp x20, x19, [sp, #0x30]
1008f0118:      ldp x22, x21, [sp, #0x20]
1008f011c:      add sp, sp, #0x50
1008f0120:      ret
1008f0124:      ldr w9, [x0, #0x4]
1008f0128:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008f012c:      add x0, x0, #0x4d0
1008f0130:      ldr x10, [x0]
1008f0134:      blr x10
1008f0138:      cmp w21, #0x3e5
1008f013c:      b.hi    0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f0140:      lsl x8, x8, #3
1008f0144:      add x8, x8, #0x10
1008f0148:      cmp x8, x9
1008f014c:      b.hi    0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f0150:      ldrb    w8, [x0]
1008f0154:      tbnz    w8, #0x0, 0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f0158:      mov x21, x20
1008f015c:      ldr x1, [x21, #0x8]!
1008f0160:      add x0, sp, #0x8
1008f0164:      bl  0x1008efec4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records6record>
1008f0168:      ldr x8, [sp, #0x8]
1008f016c:      cbz x8, 0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f0170:      ldp x1, x8, [sp, #0x10]
1008f0174:      stp xzr, x8, [sp, #0x8]
1008f0178:      add x0, sp, #0x8
1008f017c:      bl  0x1008b0ef8 <__RINvYINtNtNtCsjgY6bXVaRmE_4core3ops5range5RangejENtNtNtNtBa_4iter6traits8iterator8Iterator8try_folduNCINvNvBL_3any5checkjNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emits_0E0INtNtB8_12control_flow11ControlFlowuEEB23_>
1008f0180:      tbz w0, #0x0, 0x1008f010c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1008f0184:      mov x0, x20
1008f0188:      mov x1, x21
1008f018c:      mov x2, x19
1008f0190:      ldp x29, x30, [sp, #0x40]
1008f0194:      ldp x20, x19, [sp, #0x30]
1008f0198:      ldp x22, x21, [sp, #0x20]
1008f019c:      add sp, sp, #0x50
1008f01a0:      b   0x1008efca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array>
1008f01a4:      nop
1008f01a8:      nop
1008f01ac:      nop
1008f01b0:      nop
1008f01b4:      nop
1008f01b8:      nop
1008f01bc:      nop
