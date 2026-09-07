/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100974620 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit>:
100974620:      sub sp, sp, #0x90
100974624:      stp x24, x23, [sp, #0x50]
100974628:      stp x22, x21, [sp, #0x60]
10097462c:      stp x20, x19, [sp, #0x70]
100974630:      stp x29, x30, [sp, #0x80]
100974634:      add x29, sp, #0x80
100974638:      mov x21, x2
10097463c:      mov x19, x1
100974640:      mov x20, x0
100974644:      bl  0x1009879d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100974648:      cbz x0, 0x10097467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x5c>
10097464c:      ldrb    w8, [x0]
100974650:      cmp w8, #0x1
100974654:      b.ne    0x100974678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x58>
100974658:      ldrsb   w8, [x0, #0x1]
10097465c:      tbnz    w8, #0x1f, 0x100974678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x58>
100974660:      ldr w9, [x20]
100974664:      cmp w9, #0x2
100974668:      b.lo    0x100974678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x58>
10097466c:      ldr w8, [x20, #0x4]
100974670:      cmp w9, w8
100974674:      b.ls    0x100974694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x74>
100974678:      mov w0, #0x0                ; =0
10097467c:      ldp x29, x30, [sp, #0x80]
100974680:      ldp x20, x19, [sp, #0x70]
100974684:      ldp x22, x21, [sp, #0x60]
100974688:      ldp x24, x23, [sp, #0x50]
10097468c:      add sp, sp, #0x90
100974690:      ret
100974694:      ldr w10, [x0, #0x4]
100974698:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
10097469c:      add x0, x0, #0x3a8
1009746a0:      ldr x8, [x0]
1009746a4:      blr x8
1009746a8:      mov x8, x0
1009746ac:      mov w0, #0x0                ; =0
1009746b0:      cmp w21, #0x3e5
1009746b4:      b.hi    0x10097467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x5c>
1009746b8:      lsl x9, x9, #3
1009746bc:      add x9, x9, #0x10
1009746c0:      cmp x9, x10
1009746c4:      b.hi    0x10097467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x5c>
1009746c8:      ldrb    w8, [x8]
1009746cc:      tbnz    w8, #0x0, 0x10097467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x5c>
1009746d0:      ldr x1, [x20, #0x8]
1009746d4:      mov x0, sp
1009746d8:      bl  0x1009744c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records6record>
1009746dc:      ldr x8, [sp]
1009746e0:      cbz x8, 0x100974678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x58>
1009746e4:      ldp x1, x8, [sp, #0x18]
1009746e8:      stp xzr, x8, [sp]
1009746ec:      mov x0, sp
1009746f0:      bl  0x10093c9b0 <__RINvYINtNtNtCsjgY6bXVaRmE_4core3ops5range5RangejENtNtNtNtBa_4iter6traits8iterator8Iterator8try_folduNCINvNvBL_3any5checkjNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emits_0E0INtNtB8_12control_flow11ControlFlowuEEB23_>
1009746f4:      tbz w0, #0x0, 0x100974678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x58>
1009746f8:      ldr x21, [x19, #0x10]
1009746fc:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100974700:      add x0, x0, #0x390
100974704:      bl  0x10012d4cc <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellTyyEEE4withNCNvMNtNtBa_4hash6randomNtB1I_11RandomState3new0B21_ECs5gMwpk3Cs4e_13perry_runtime>
100974708:      adrp    x8, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
10097470c:      add x8, x8, #0x398
100974710:      ldp q0, q1, [x8]
100974714:      stur    q0, [sp, #0x18]
100974718:      stur    q1, [sp, #0x28]
10097471c:      stp x0, x1, [sp, #0x38]
100974720:      mov w8, #0x8                ; =8
100974724:      stp xzr, x8, [sp]
100974728:      str xzr, [sp, #0x10]
10097472c:      str xzr, [sp, #0x48]
100974730:      ldr x8, [x19]
100974734:      cmp x8, x21
100974738:      b.eq    0x100974838 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x218>
10097473c:      ldr x8, [x19, #0x8]
100974740:      mov w9, #0x5b               ; =91
100974744:      strb    w9, [x8, x21]
100974748:      add x8, x21, #0x1
10097474c:      str x8, [x19, #0x10]
100974750:      ldr w23, [x20]
100974754:      cbz w23, 0x10097477c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x15c>
100974758:      ldr x1, [x20, #0x8]
10097475c:      mov x0, sp
100974760:      mov w2, #0x1                ; =1
100974764:      mov x3, x19
100974768:      bl  0x1009408e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
10097476c:      cbz w0, 0x100974818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x1f8>
100974770:      cmp w23, #0x1
100974774:      b.ne    0x1009747ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x18c>
100974778:      ldr x8, [x19, #0x10]
10097477c:      ldr x9, [x19]
100974780:      cmp x9, x8
100974784:      b.eq    0x100974854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x234>
100974788:      ldr x9, [x19, #0x8]
10097478c:      mov w10, #0x5d              ; =93
100974790:      strb    w10, [x9, x8]
100974794:      add x8, x8, #0x1
100974798:      str x8, [x19, #0x10]
10097479c:      mov x0, sp
1009747a0:      bl  0x10092e900 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records7EmitterEBH_>
1009747a4:      mov w0, #0x1                ; =1
1009747a8:      b   0x10097467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x5c>
1009747ac:      add x22, x20, #0x10
1009747b0:      sub x23, x23, #0x1
1009747b4:      mov w24, #0x2c              ; =44
1009747b8:      ldr x20, [x19, #0x10]
1009747bc:      ldr x8, [x19]
1009747c0:      cmp x8, x20
1009747c4:      b.eq    0x1009747fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x1dc>
1009747c8:      ldr x8, [x19, #0x8]
1009747cc:      strb    w24, [x8, x20]
1009747d0:      add x8, x20, #0x1
1009747d4:      str x8, [x19, #0x10]
1009747d8:      ldr x1, [x22], #0x8
1009747dc:      mov x0, sp
1009747e0:      mov w2, #0x1                ; =1
1009747e4:      mov x3, x19
1009747e8:      bl  0x1009408e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
1009747ec:      tbz w0, #0x0, 0x100974818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x1f8>
1009747f0:      subs    x23, x23, #0x1
1009747f4:      b.ne    0x1009747b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x198>
1009747f8:      b   0x100974778 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x158>
1009747fc:      mov x0, x19
100974800:      mov x1, x20
100974804:      mov w2, #0x1                ; =1
100974808:      mov w3, #0x1                ; =1
10097480c:      mov w4, #0x1                ; =1
100974810:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100974814:      b   0x1009747c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x1a8>
100974818:      adrp    x2, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
10097481c:      add x2, x2, #0x698
100974820:      mov x0, x19
100974824:      mov x1, x21
100974828:      bl  0x10094066c <__RNvMNtCsctvjasLqLe9_5alloc6stringNtB2_6String8truncate>
10097482c:      mov x0, sp
100974830:      bl  0x10092e900 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records7EmitterEBH_>
100974834:      b   0x100974678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x58>
100974838:      mov x0, x19
10097483c:      mov x1, x21
100974840:      mov w2, #0x1                ; =1
100974844:      mov w3, #0x1                ; =1
100974848:      mov w4, #0x1                ; =1
10097484c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100974850:      b   0x10097473c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x11c>
100974854:      mov x0, x19
100974858:      mov x20, x8
10097485c:      mov x1, x8
100974860:      mov w2, #0x1                ; =1
100974864:      mov w3, #0x1                ; =1
100974868:      mov w4, #0x1                ; =1
10097486c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100974870:      mov x8, x20
100974874:      b   0x100974788 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit+0x168>
100974878:      nop
10097487c:      nop
