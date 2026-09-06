/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/plan-scan-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010055d724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
10055d724:      sub sp, sp, #0x40
10055d728:      stp x20, x19, [sp, #0x20]
10055d72c:      stp x29, x30, [sp, #0x30]
10055d730:      add x29, sp, #0x30
10055d734:      strb    wzr, [sp, #0xc]
10055d738:      str wzr, [sp, #0x8]
10055d73c:      and x9, x1, #0xffff000000000000
10055d740:      mov x8, #0x7fff000000000000 ; =9223090561878065152
10055d744:      cmp x9, x8
10055d748:      b.eq    0x10055d928 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x204>
10055d74c:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
10055d750:      cmp x9, x8
10055d754:      b.ne    0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d758:      ubfx    x8, x1, #40, #8
10055d75c:      cbz x8, 0x10055d7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
10055d760:      strb    w1, [sp, #0x8]
10055d764:      cmp x8, #0x1
10055d768:      b.eq    0x10055d7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
10055d76c:      lsr x10, x1, #8
10055d770:      strb    w10, [sp, #0x9]
10055d774:      cmp x8, #0x2
10055d778:      b.eq    0x10055d7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
10055d77c:      lsr x10, x1, #16
10055d780:      strb    w10, [sp, #0xa]
10055d784:      cmp x8, #0x3
10055d788:      b.eq    0x10055d7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
10055d78c:      lsr x10, x1, #24
10055d790:      strb    w10, [sp, #0xb]
10055d794:      cmp x8, #0x4
10055d798:      b.eq    0x10055d7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
10055d79c:      lsr x10, x1, #32
10055d7a0:      strb    w10, [sp, #0xc]
10055d7a4:      cmp x8, #0x5
10055d7a8:      b.ne    0x10055dd34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x610>
10055d7ac:      mov x10, x1
10055d7b0:      add x1, sp, #0x8
10055d7b4:      mov w2, w8
10055d7b8:      and w11, w8, #0xfffffffc
10055d7bc:      cmp w11, #0x4
10055d7c0:      b.ne    0x10055d954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x230>
10055d7c4:      ldr w11, [x1]
10055d7c8:      tbz w2, #0x1, 0x10055d7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xb0>
10055d7cc:      ldrh    w12, [x1, #0x4]
10055d7d0:      orr x11, x11, x12, lsl #32
10055d7d4:      tbz w2, #0x0, 0x10055d7ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xc8>
10055d7d8:      sub x12, x2, #0x1
10055d7dc:      ldrb    w13, [x1, x12]
10055d7e0:      lsl x12, x12, #3
10055d7e4:      lsl x12, x13, x12
10055d7e8:      orr x11, x12, x11
10055d7ec:      lsl x12, x2, #3
10055d7f0:      mov x13, #0x2020202020202020 ; =2314885530818453536
10055d7f4:      lsl x12, x13, x12
10055d7f8:      orr x12, x11, x12
10055d7fc:      eor x13, x12, #0x2222222222222222
10055d800:      mov x14, #-0x101010101010102 ; =-72340172838076674
10055d804:      movk    x14, #0xfeff
10055d808:      mov x15, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
10055d80c:      orr x15, x15, #0x4444444444444444
10055d810:      eor x15, x12, x15
10055d814:      add x15, x15, x14
10055d818:      mov x16, #-0x2020202020202021 ; =-2314885530818453537
10055d81c:      movk    x16, #0xdfe0
10055d820:      add x16, x12, x16
10055d824:      orr x15, x15, x16
10055d828:      add x13, x13, x14
10055d82c:      orr x13, x15, x13
10055d830:      bic x13, x13, x11
10055d834:      mov x15, #-0x3333333333333334 ; =-3689348814741910324
10055d838:      orr x15, x15, #0xe1e1e1e1e1e1e1e1
10055d83c:      eor x12, x12, x15
10055d840:      add x12, x12, x14
10055d844:      and x11, x12, x11
10055d848:      orr x11, x13, x11
10055d84c:      tst x11, #0x8080808080808080
10055d850:      cset    w11, ne
10055d854:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10055d858:      cmp x9, x12
10055d85c:      b.eq    0x10055dc44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x520>
10055d860:      cmp w8, #0x40
10055d864:      b.hs    0x10055da14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2f0>
10055d868:      ands    x9, x2, #0x38
10055d86c:      b.eq    0x10055d890 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x16c>
10055d870:      and x10, x2, #0x38
10055d874:      neg x10, x10
10055d878:      mov x12, x1
10055d87c:      ldr x13, [x12], #0x8
10055d880:      tst x13, #0x8080808080808080
10055d884:      b.ne    0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d888:      adds    x10, x10, #0x8
10055d88c:      b.ne    0x10055d87c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x158>
10055d890:      mov x3, x8
10055d894:      and x10, x2, #0x7
10055d898:      cbz x10, 0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d89c:      add x9, x1, x9
10055d8a0:      ldrsb   w12, [x9]
10055d8a4:      tbnz    w12, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d8a8:      mov x3, x8
10055d8ac:      cmp x10, #0x1
10055d8b0:      b.eq    0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d8b4:      ldrsb   w12, [x9, #0x1]
10055d8b8:      tbnz    w12, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d8bc:      mov x3, x8
10055d8c0:      cmp x10, #0x2
10055d8c4:      b.eq    0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d8c8:      ldrsb   w12, [x9, #0x2]
10055d8cc:      tbnz    w12, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d8d0:      mov x3, x8
10055d8d4:      cmp x10, #0x3
10055d8d8:      b.eq    0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d8dc:      ldrsb   w12, [x9, #0x3]
10055d8e0:      tbnz    w12, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d8e4:      mov x3, x8
10055d8e8:      cmp x10, #0x4
10055d8ec:      b.eq    0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d8f0:      ldrsb   w12, [x9, #0x4]
10055d8f4:      tbnz    w12, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d8f8:      mov x3, x8
10055d8fc:      cmp x10, #0x5
10055d900:      b.eq    0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d904:      ldrsb   w12, [x9, #0x5]
10055d908:      tbnz    w12, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d90c:      mov x3, x8
10055d910:      cmp x10, #0x6
10055d914:      b.eq    0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d918:      ldrsb   w9, [x9, #0x6]
10055d91c:      mov x3, x8
10055d920:      tbz w9, #0x1f, 0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055d924:      b   0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d928:      ands    x11, x1, #0xffffffffffff
10055d92c:      b.eq    0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d930:      ldr w8, [x11, #0x4]
10055d934:      cmn w8, #0x3
10055d938:      b.hi    0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055d93c:      mov x10, x1
10055d940:      add x1, x11, #0x14
10055d944:      mov w2, w8
10055d948:      and w11, w8, #0xfffffffc
10055d94c:      cmp w11, #0x4
10055d950:      b.eq    0x10055d7c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xa0>
10055d954:      cmp w8, #0x10
10055d958:      b.lo    0x10055d9ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x288>
10055d95c:      mov w11, #0x10              ; =16
10055d960:      movi.16b    v0, #0x22
10055d964:      movi.16b    v1, #0x5c
10055d968:      movi.16b    v2, #0x20
10055d96c:      mov x12, x1
10055d970:      movi.16b    v3, #0xed
10055d974:      ldr q4, [x12], #0x10
10055d978:      cmeq.16b    v5, v4, v0
10055d97c:      cmeq.16b    v6, v4, v1
10055d980:      orr.16b v5, v6, v5
10055d984:      cmhi.16b    v6, v2, v4
10055d988:      cmeq.16b    v4, v4, v3
10055d98c:      orr.16b v4, v4, v6
10055d990:      orr.16b v4, v4, v5
10055d994:      addp.2d d4, v4
10055d998:      fmov    x13, d4
10055d99c:      cbnz    x13, 0x10055dc2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x508>
10055d9a0:      add x11, x11, #0x10
10055d9a4:      cmp x11, x2
10055d9a8:      b.ls    0x10055d974 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x250>
10055d9ac:      and x12, x2, #0xfffffff0
10055d9b0:      add x13, x1, x12
10055d9b4:      tbnz    w2, #0x3, 0x10055daa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
10055d9b8:      tst x2, #0x7
10055d9bc:      b.eq    0x10055da00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2dc>
10055d9c0:      and x11, x2, #0x8
10055d9c4:      add x13, x13, x11
10055d9c8:      sub x11, x2, x11
10055d9cc:      sub x12, x11, x12
10055d9d0:      ldrb    w14, [x13], #0x1
10055d9d4:      mov w11, #0x1               ; =1
10055d9d8:      cmp w14, #0x22
10055d9dc:      b.eq    0x10055dc38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10055d9e0:      cmp w14, #0x5c
10055d9e4:      b.eq    0x10055dc38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10055d9e8:      cmp w14, #0x20
10055d9ec:      b.lo    0x10055dc38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10055d9f0:      cmp w14, #0xed
10055d9f4:      b.eq    0x10055dc38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10055d9f8:      subs    x12, x12, #0x1
10055d9fc:      b.ne    0x10055d9d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2ac>
10055da00:      mov w11, #0x0               ; =0
10055da04:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10055da08:      cmp x9, x12
10055da0c:      b.ne    0x10055d860 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
10055da10:      b   0x10055dc44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x520>
10055da14:      and x9, x2, #0xffffffc0
10055da18:      add x9, x1, x9
10055da1c:      mov x10, x1
10055da20:      ldp q0, q1, [x10]
10055da24:      ldp q2, q3, [x10, #0x20]
10055da28:      orr.16b v0, v1, v0
10055da2c:      orr.16b v1, v2, v3
10055da30:      orr.16b v0, v0, v1
10055da34:      umaxv.16b   b0, v0
10055da38:      fmov    w12, s0
10055da3c:      tbnz    w12, #0x7, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055da40:      add x10, x10, #0x40
10055da44:      cmp x10, x9
10055da48:      b.ne    0x10055da20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2fc>
10055da4c:      ands    x10, x2, #0x30
10055da50:      b.eq    0x10055da74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x350>
10055da54:      mov x12, x10
10055da58:      mov x13, x9
10055da5c:      ldr q0, [x13], #0x10
10055da60:      umaxv.16b   b0, v0
10055da64:      fmov    w14, s0
10055da68:      tbnz    w14, #0x7, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055da6c:      subs    x12, x12, #0x10
10055da70:      b.ne    0x10055da5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x338>
10055da74:      mov x3, x8
10055da78:      and x12, x2, #0xf
10055da7c:      cbz x12, 0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055da80:      add x9, x9, x10
10055da84:      ldrsb   w10, [x9]
10055da88:      tbnz    w10, #0x1f, 0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055da8c:      add x9, x9, #0x1
10055da90:      subs    x12, x12, #0x1
10055da94:      b.ne    0x10055da84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x360>
10055da98:      mov x3, x8
10055da9c:      b   0x10055dc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10055daa0:      mov x14, #0x101010101010101 ; =72340172838076673
10055daa4:      movk    x14, #0x100
10055daa8:      ldr x11, [x13]
10055daac:      eor x15, x11, #0x2222222222222222
10055dab0:      sub x15, x14, x15
10055dab4:      orr x16, x15, x11
10055dab8:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
10055dabc:      bics    xzr, x15, x16
10055dac0:      b.ne    0x10055db1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
10055dac4:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
10055dac8:      orr x16, x16, #0x4444444444444444
10055dacc:      eor x16, x11, x16
10055dad0:      sub x14, x14, x16
10055dad4:      orr x14, x14, x11
10055dad8:      bics    xzr, x15, x14
10055dadc:      b.ne    0x10055db1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
10055dae0:      mov x14, #0x2020202020202020 ; =2314885530818453536
10055dae4:      movk    x14, #0x201f
10055dae8:      sub x14, x14, x11
10055daec:      orr x14, x14, x11
10055daf0:      bics    xzr, x15, x14
10055daf4:      b.ne    0x10055db1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
10055daf8:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
10055dafc:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
10055db00:      eor x14, x11, x14
10055db04:      mov x15, #-0x101010101010102 ; =-72340172838076674
10055db08:      movk    x15, #0xfeff
10055db0c:      add x14, x14, x15
10055db10:      and x14, x11, x14
10055db14:      tst x14, #0x8080808080808080
10055db18:      b.eq    0x10055d9b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x294>
10055db1c:      and w12, w11, #0xff
10055db20:      cmp w12, #0x22
10055db24:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db28:      cmp w12, #0x5c
10055db2c:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db30:      cmp w12, #0x20
10055db34:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db38:      cmp w12, #0xed
10055db3c:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db40:      ubfx    w12, w11, #8, #8
10055db44:      cmp w12, #0x22
10055db48:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db4c:      cmp w12, #0x5c
10055db50:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db54:      cmp w12, #0xed
10055db58:      mov w13, #0x20              ; =32
10055db5c:      ccmp    w12, w13, #0x0, ne
10055db60:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db64:      ubfx    w12, w11, #16, #8
10055db68:      cmp w12, #0x22
10055db6c:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db70:      cmp w12, #0x5c
10055db74:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db78:      cmp w12, #0xed
10055db7c:      ccmp    w12, w13, #0x0, ne
10055db80:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db84:      lsr w12, w11, #24
10055db88:      cmp w12, #0x22
10055db8c:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db90:      cmp w12, #0x5c
10055db94:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055db98:      cmp w12, #0xed
10055db9c:      ccmp    w12, w13, #0x0, ne
10055dba0:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dba4:      ubfx    x12, x11, #32, #8
10055dba8:      cmp w12, #0x22
10055dbac:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbb0:      cmp w12, #0x5c
10055dbb4:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbb8:      cmp w12, #0xed
10055dbbc:      ccmp    w12, w13, #0x0, ne
10055dbc0:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbc4:      ubfx    x12, x11, #40, #8
10055dbc8:      cmp w12, #0x22
10055dbcc:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbd0:      cmp w12, #0x5c
10055dbd4:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbd8:      cmp w12, #0xed
10055dbdc:      ccmp    w12, w13, #0x0, ne
10055dbe0:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbe4:      ubfx    x12, x11, #48, #8
10055dbe8:      cmp w12, #0x22
10055dbec:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbf0:      cmp w12, #0x5c
10055dbf4:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dbf8:      cmp w12, #0xed
10055dbfc:      ccmp    w12, w13, #0x0, ne
10055dc00:      b.lo    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dc04:      lsr x12, x11, #56
10055dc08:      cmp w12, #0x22
10055dc0c:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dc10:      cmp w12, #0x5c
10055dc14:      b.eq    0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dc18:      lsr x11, x11, #61
10055dc1c:      cmp x12, #0xed
10055dc20:      ccmp    x11, #0x0, #0x4, ne
10055dc24:      b.ne    0x10055da00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2dc>
10055dc28:      b   0x10055dc34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10055dc2c:      fmov    x11, d4
10055dc30:      cbz x11, 0x10055dc38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10055dc34:      mov w11, #0x1               ; =1
10055dc38:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10055dc3c:      cmp x9, x12
10055dc40:      b.ne    0x10055d860 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
10055dc44:      and x9, x10, #0xffffffffffff
10055dc48:      ldr w3, [x9]
10055dc4c:      tbz w11, #0x0, 0x10055dc80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x55c>
10055dc50:      mov x19, x0
10055dc54:      add x0, sp, #0x10
10055dc58:      bl  0x1005177b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
10055dc5c:      ldr w8, [sp, #0x10]
10055dc60:      tbz w8, #0x0, 0x10055dcb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x594>
10055dc64:      ldur    x8, [sp, #0x14]
10055dc68:      stur    x8, [x19, #0x4]
10055dc6c:      ldr w8, [sp, #0x1c]
10055dc70:      str w8, [x19, #0xc]
10055dc74:      mov w8, #0x1                ; =1
10055dc78:      str w8, [x19]
10055dc7c:      b   0x10055dca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
10055dc80:      mov w9, #0x3                ; =3
10055dc84:      cmp x2, #0x3
10055dc88:      csel    x9, x2, x9, lo
10055dc8c:      cbz w8, 0x10055dd20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
10055dc90:      add x10, x1, x2
10055dc94:      ldurb   w10, [x10, #-0x1]
10055dc98:      cmp w10, #0xbf
10055dc9c:      b.ls    0x10055dcc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5a0>
10055dca0:      mov w8, #-0x1               ; =-1
10055dca4:      str w8, [x0]
10055dca8:      ldp x29, x30, [sp, #0x30]
10055dcac:      ldp x20, x19, [sp, #0x20]
10055dcb0:      add sp, sp, #0x40
10055dcb4:      ret
10055dcb8:      mov w8, #-0x1               ; =-1
10055dcbc:      str w8, [x19]
10055dcc0:      b   0x10055dca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
10055dcc4:      cmp w8, #0x1
10055dcc8:      b.eq    0x10055dd20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
10055dccc:      add x10, x1, x2
10055dcd0:      ldurb   w10, [x10, #-0x2]
10055dcd4:      cmp w10, #0xdf
10055dcd8:      b.ls    0x10055dce4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5c0>
10055dcdc:      mov w10, #0x2               ; =2
10055dce0:      b   0x10055dd18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
10055dce4:      cmp w8, #0x2
10055dce8:      b.eq    0x10055dd20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
10055dcec:      add x11, x1, x2
10055dcf0:      mov x10, #-0x3              ; =-3
10055dcf4:      ldrb    w12, [x11, x10]
10055dcf8:      cmp w12, #0xef
10055dcfc:      b.hi    0x10055dd14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f0>
10055dd00:      sub x10, x10, #0x1
10055dd04:      add x12, x9, x10
10055dd08:      cmn x12, #0x1
10055dd0c:      b.ne    0x10055dcf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5d0>
10055dd10:      b   0x10055dd20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
10055dd14:      neg x10, x10
10055dd18:      cmp x10, x9
10055dd1c:      b.ls    0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055dd20:      cmn w3, #0x3
10055dd24:      b.hi    0x10055dca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10055dd28:      stp wzr, w8, [x0]
10055dd2c:      str w3, [x0, #0x8]
10055dd30:      b   0x10055dca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
10055dd34:      adrp    x2, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055dd38:      add x2, x2, #0xac8
10055dd3c:      mov w0, #0x5                ; =5
10055dd40:      mov w1, #0x5                ; =5
10055dd44:      bl  0x100c7f40c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
