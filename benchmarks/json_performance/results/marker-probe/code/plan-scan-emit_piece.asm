/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/plan-scan-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
10055c638:      sub sp, sp, #0x50
10055c63c:      stp x22, x21, [sp, #0x20]
10055c640:      stp x20, x19, [sp, #0x30]
10055c644:      stp x29, x30, [sp, #0x40]
10055c648:      add x29, sp, #0x40
10055c64c:      ldr w8, [x0]
10055c650:      cbz w8, 0x10055c6f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xb8>
10055c654:      cmp w8, #0x1
10055c658:      b.ne    0x10055c774 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x13c>
10055c65c:      ldr w8, [x0, #0x4]
10055c660:      strb    wzr, [sp, #0x4]
10055c664:      str wzr, [sp]
10055c668:      and x9, x1, #0xffff000000000000
10055c66c:      mov x10, #0x7fff000000000000 ; =9223090561878065152
10055c670:      cmp x9, x10
10055c674:      b.eq    0x10055c79c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x164>
10055c678:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
10055c67c:      cmp x9, x10
10055c680:      b.ne    0x10055c8f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
10055c684:      ubfx    x9, x1, #40, #8
10055c688:      cbz x9, 0x10055c6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10055c68c:      strb    w1, [sp]
10055c690:      cmp x9, #0x1
10055c694:      b.eq    0x10055c6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10055c698:      lsr x10, x1, #8
10055c69c:      strb    w10, [sp, #0x1]
10055c6a0:      cmp x9, #0x2
10055c6a4:      b.eq    0x10055c6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10055c6a8:      lsr x10, x1, #16
10055c6ac:      strb    w10, [sp, #0x2]
10055c6b0:      cmp x9, #0x3
10055c6b4:      b.eq    0x10055c6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10055c6b8:      lsr x10, x1, #24
10055c6bc:      strb    w10, [sp, #0x3]
10055c6c0:      cmp x9, #0x4
10055c6c4:      b.eq    0x10055c6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10055c6c8:      lsr x10, x1, #32
10055c6cc:      strb    w10, [sp, #0x4]
10055c6d0:      cmp x9, #0x5
10055c6d4:      b.ne    0x10055c928 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
10055c6d8:      mov x9, sp
10055c6dc:      mov w10, #0x22              ; =34
10055c6e0:      strb    w10, [x2]
10055c6e4:      mov w11, #0x1               ; =1
10055c6e8:      cbnz    w8, 0x10055c7b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x180>
10055c6ec:      b   0x10055c8a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10055c6f0:      ldr w19, [x0, #0x4]
10055c6f4:      strb    wzr, [sp, #0x4]
10055c6f8:      str wzr, [sp]
10055c6fc:      and x8, x1, #0xffff000000000000
10055c700:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10055c704:      cmp x8, x9
10055c708:      b.eq    0x10055c8b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x27c>
10055c70c:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
10055c710:      cmp x8, x9
10055c714:      b.ne    0x10055c910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2d8>
10055c718:      ubfx    x8, x1, #40, #8
10055c71c:      cbz x8, 0x10055c76c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10055c720:      strb    w1, [sp]
10055c724:      cmp x8, #0x1
10055c728:      b.eq    0x10055c76c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10055c72c:      lsr x9, x1, #8
10055c730:      strb    w9, [sp, #0x1]
10055c734:      cmp x8, #0x2
10055c738:      b.eq    0x10055c76c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10055c73c:      lsr x9, x1, #16
10055c740:      strb    w9, [sp, #0x2]
10055c744:      cmp x8, #0x3
10055c748:      b.eq    0x10055c76c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10055c74c:      lsr x9, x1, #24
10055c750:      strb    w9, [sp, #0x3]
10055c754:      cmp x8, #0x4
10055c758:      b.eq    0x10055c76c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10055c75c:      lsr x9, x1, #32
10055c760:      strb    w9, [sp, #0x4]
10055c764:      cmp x8, #0x5
10055c768:      b.ne    0x10055c928 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
10055c76c:      mov x1, sp
10055c770:      b   0x10055c8c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x288>
10055c774:      ldur    q0, [x0, #0x8]
10055c778:      ldur    q1, [x0, #0x18]
10055c77c:      stp q0, q1, [sp]
10055c780:      ldr w19, [x0, #0x4]
10055c784:      mov x1, sp
10055c788:      mov x0, x2
10055c78c:      mov x2, x19
10055c790:      bl  0x100ccec2c <_writev+0x100ccec2c>
10055c794:      mov x0, x19
10055c798:      b   0x10055c8e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
10055c79c:      ands    x9, x1, #0xffffffffffff
10055c7a0:      add x9, x9, #0x14
10055c7a4:      csel    x9, xzr, x9, eq
10055c7a8:      mov w10, #0x22              ; =34
10055c7ac:      strb    w10, [x2]
10055c7b0:      mov w11, #0x1               ; =1
10055c7b4:      cbz w8, 0x10055c8a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10055c7b8:      mov w12, #0x5c              ; =92
10055c7bc:      mov w13, #0x3075            ; =12405
10055c7c0:      mov w14, #0x30              ; =48
10055c7c4:      adrp    x15, 0x100dce000 <_anon.9bd75d14e3ca4089e03b47eaf962fb16.70+0x23>
10055c7c8:      add x15, x15, #0x73
10055c7cc:      b   0x10055c7e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1b0>
10055c7d0:      mov w16, #0x62              ; =98
10055c7d4:      strb    w16, [x17, #0x1]
10055c7d8:      mov w16, #0x2               ; =2
10055c7dc:      add x11, x16, x11
10055c7e0:      subs    x8, x8, #0x1
10055c7e4:      b.eq    0x10055c8a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10055c7e8:      ldrb    w16, [x9], #0x1
10055c7ec:      cmp w16, #0x20
10055c7f0:      b.lo    0x10055c804 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
10055c7f4:      cmp w16, #0x5c
10055c7f8:      b.eq    0x10055c804 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
10055c7fc:      cmp w16, #0x22
10055c800:      b.ne    0x10055c88c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x254>
10055c804:      add x17, x2, x11
10055c808:      strb    w12, [x17]
10055c80c:      cmp w16, #0xb
10055c810:      b.le    0x10055c834 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
10055c814:      cmp w16, #0x21
10055c818:      b.gt    0x10055c854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x21c>
10055c81c:      cmp w16, #0xc
10055c820:      b.eq    0x10055c898 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x260>
10055c824:      cmp w16, #0xd
10055c828:      b.ne    0x10055c864 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
10055c82c:      mov w16, #0x72              ; =114
10055c830:      b   0x10055c7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10055c834:      cmp w16, #0x8
10055c838:      b.eq    0x10055c7d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x198>
10055c83c:      cmp w16, #0x9
10055c840:      b.eq    0x10055c8a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x268>
10055c844:      cmp w16, #0xa
10055c848:      b.ne    0x10055c864 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
10055c84c:      mov w16, #0x6e              ; =110
10055c850:      b   0x10055c7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10055c854:      cmp w16, #0x22
10055c858:      b.eq    0x10055c7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10055c85c:      cmp w16, #0x5c
10055c860:      b.eq    0x10055c7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10055c864:      sturh   w13, [x17, #0x1]
10055c868:      strb    w14, [x17, #0x3]
10055c86c:      lsr x0, x16, #4
10055c870:      ldrb    w0, [x15, x0]
10055c874:      strb    w0, [x17, #0x4]
10055c878:      and x16, x16, #0xf
10055c87c:      ldrb    w16, [x15, x16]
10055c880:      strb    w16, [x17, #0x5]
10055c884:      mov w16, #0x6               ; =6
10055c888:      b   0x10055c7dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
10055c88c:      strb    w16, [x2, x11]
10055c890:      mov w16, #0x1               ; =1
10055c894:      b   0x10055c7dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
10055c898:      mov w16, #0x66              ; =102
10055c89c:      b   0x10055c7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10055c8a0:      mov w16, #0x74              ; =116
10055c8a4:      b   0x10055c7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10055c8a8:      strb    w10, [x2, x11]
10055c8ac:      add x0, x11, #0x1
10055c8b0:      b   0x10055c8e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
10055c8b4:      ands    x8, x1, #0xffffffffffff
10055c8b8:      add x8, x8, #0x14
10055c8bc:      csel    x1, xzr, x8, eq
10055c8c0:      mov w20, #0x22              ; =34
10055c8c4:      mov x0, x2
10055c8c8:      strb    w20, [x0], #0x1
10055c8cc:      mov x21, x2
10055c8d0:      mov x2, x19
10055c8d4:      bl  0x100ccec2c <_writev+0x100ccec2c>
10055c8d8:      add x8, x21, x19
10055c8dc:      strb    w20, [x8, #0x1]
10055c8e0:      add x0, x19, #0x2
10055c8e4:      ldp x29, x30, [sp, #0x40]
10055c8e8:      ldp x20, x19, [sp, #0x30]
10055c8ec:      ldp x22, x21, [sp, #0x20]
10055c8f0:      add sp, sp, #0x50
10055c8f4:      ret
10055c8f8:      adrp    x0, 0x100dd4000 <_anon.9bd75d14e3ca4089e03b47eaf962fb16.686+0x1d7>
10055c8fc:      add x0, x0, #0x2ae
10055c900:      adrp    x2, 0x1010a0000 <_anon.9bd75d14e3ca4089e03b47eaf962fb16.802+0x18>
10055c904:      add x2, x2, #0x38
10055c908:      mov w1, #0x20               ; =32
10055c90c:      bl  0x100c7f340 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10055c910:      adrp    x0, 0x100dd4000 <_anon.9bd75d14e3ca4089e03b47eaf962fb16.686+0x1d7>
10055c914:      add x0, x0, #0x296
10055c918:      adrp    x2, 0x1010a0000 <_anon.9bd75d14e3ca4089e03b47eaf962fb16.802+0x18>
10055c91c:      add x2, x2, #0x20
10055c920:      mov w1, #0x18               ; =24
10055c924:      bl  0x100c7f340 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10055c928:      adrp    x2, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055c92c:      add x2, x2, #0xac8
10055c930:      mov w0, #0x5                ; =5
10055c934:      mov w1, #0x5                ; =5
10055c938:      bl  0x100c7f40c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
