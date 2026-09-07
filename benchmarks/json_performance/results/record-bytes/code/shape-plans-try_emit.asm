/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100918640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit>:
100918640:      sub sp, sp, #0x50
100918644:      stp x22, x21, [sp, #0x20]
100918648:      stp x20, x19, [sp, #0x30]
10091864c:      stp x29, x30, [sp, #0x40]
100918650:      add x29, sp, #0x40
100918654:      mov x21, x2
100918658:      mov x19, x1
10091865c:      mov x20, x0
100918660:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100918664:      cbz x0, 0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
100918668:      ldrb    w8, [x0]
10091866c:      cmp w8, #0x1
100918670:      b.ne    0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
100918674:      ldrsb   w8, [x0, #0x1]
100918678:      tbnz    w8, #0x1f, 0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
10091867c:      ldr w8, [x20]
100918680:      cmp w8, #0x2
100918684:      b.lo    0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
100918688:      ldr w9, [x20, #0x4]
10091868c:      cmp w8, w9
100918690:      b.ls    0x1009186ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x6c>
100918694:      mov w0, #0x0                ; =0
100918698:      ldp x29, x30, [sp, #0x40]
10091869c:      ldp x20, x19, [sp, #0x30]
1009186a0:      ldp x22, x21, [sp, #0x20]
1009186a4:      add sp, sp, #0x50
1009186a8:      ret
1009186ac:      ldr w9, [x0, #0x4]
1009186b0:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
1009186b4:      add x0, x0, #0x680
1009186b8:      ldr x10, [x0]
1009186bc:      blr x10
1009186c0:      cmp w21, #0x3e5
1009186c4:      b.hi    0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1009186c8:      lsl x8, x8, #3
1009186cc:      add x8, x8, #0x10
1009186d0:      cmp x8, x9
1009186d4:      b.hi    0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1009186d8:      ldrb    w8, [x0]
1009186dc:      tbnz    w8, #0x0, 0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1009186e0:      mov x21, x20
1009186e4:      ldr x1, [x21, #0x8]!
1009186e8:      add x0, sp, #0x8
1009186ec:      bl  0x10091844c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records6record>
1009186f0:      ldr x8, [sp, #0x8]
1009186f4:      cbz x8, 0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
1009186f8:      ldp x1, x8, [sp, #0x10]
1009186fc:      stp xzr, x8, [sp, #0x8]
100918700:      add x0, sp, #0x8
100918704:      bl  0x1008d7e3c <__RINvYINtNtNtCsjgY6bXVaRmE_4core3ops5range5RangejENtNtNtNtBa_4iter6traits8iterator8Iterator8try_folduNCINvNvBL_3any5checkjNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emits_0E0INtNtB8_12control_flow11ControlFlowuEEB23_>
100918708:      tbz w0, #0x0, 0x100918694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x54>
10091870c:      mov x0, x20
100918710:      mov x1, x21
100918714:      mov x2, x19
100918718:      ldp x29, x30, [sp, #0x40]
10091871c:      ldp x20, x19, [sp, #0x30]
100918720:      ldp x22, x21, [sp, #0x20]
100918724:      add sp, sp, #0x50
100918728:      b   0x100918228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array>
10091872c:      nop
100918730:      nop
100918734:      nop
100918738:      nop
10091873c:      nop
