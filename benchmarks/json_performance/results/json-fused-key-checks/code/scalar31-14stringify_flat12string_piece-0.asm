/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bd710 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece>:
1004bd710:      sub sp, sp, #0x40
1004bd714:      stp x20, x19, [sp, #0x20]
1004bd718:      stp x29, x30, [sp, #0x30]
1004bd71c:      add x29, sp, #0x30
1004bd720:      strb    wzr, [sp, #0xc]
1004bd724:      str wzr, [sp, #0x8]
1004bd728:      and x9, x1, #0xffff000000000000
1004bd72c:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1004bd730:      cmp x9, x8
1004bd734:      b.eq    0x1004bd914 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x204>
1004bd738:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004bd73c:      cmp x9, x8
1004bd740:      b.ne    0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd744:      ubfx    x8, x1, #40, #8
1004bd748:      cbz x8, 0x1004bd798 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004bd74c:      strb    w1, [sp, #0x8]
1004bd750:      cmp x8, #0x1
1004bd754:      b.eq    0x1004bd798 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004bd758:      lsr x10, x1, #8
1004bd75c:      strb    w10, [sp, #0x9]
1004bd760:      cmp x8, #0x2
1004bd764:      b.eq    0x1004bd798 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004bd768:      lsr x10, x1, #16
1004bd76c:      strb    w10, [sp, #0xa]
1004bd770:      cmp x8, #0x3
1004bd774:      b.eq    0x1004bd798 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004bd778:      lsr x10, x1, #24
1004bd77c:      strb    w10, [sp, #0xb]
1004bd780:      cmp x8, #0x4
1004bd784:      b.eq    0x1004bd798 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004bd788:      lsr x10, x1, #32
1004bd78c:      strb    w10, [sp, #0xc]
1004bd790:      cmp x8, #0x5
1004bd794:      b.ne    0x1004bdd20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x610>
1004bd798:      mov x10, x1
1004bd79c:      add x1, sp, #0x8
1004bd7a0:      mov w2, w8
1004bd7a4:      and w11, w8, #0xfffffffc
1004bd7a8:      cmp w11, #0x4
1004bd7ac:      b.ne    0x1004bd940 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x230>
1004bd7b0:      ldr w11, [x1]
1004bd7b4:      tbz w2, #0x1, 0x1004bd7c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0xb0>
1004bd7b8:      ldrh    w12, [x1, #0x4]
1004bd7bc:      orr x11, x11, x12, lsl #32
1004bd7c0:      tbz w2, #0x0, 0x1004bd7d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0xc8>
1004bd7c4:      sub x12, x2, #0x1
1004bd7c8:      ldrb    w13, [x1, x12]
1004bd7cc:      lsl x12, x12, #3
1004bd7d0:      lsl x12, x13, x12
1004bd7d4:      orr x11, x12, x11
1004bd7d8:      lsl x12, x2, #3
1004bd7dc:      mov x13, #0x2020202020202020 ; =2314885530818453536
1004bd7e0:      lsl x12, x13, x12
1004bd7e4:      orr x12, x11, x12
1004bd7e8:      eor x13, x12, #0x2222222222222222
1004bd7ec:      mov x14, #-0x101010101010102 ; =-72340172838076674
1004bd7f0:      movk    x14, #0xfeff
1004bd7f4:      mov x15, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1004bd7f8:      orr x15, x15, #0x4444444444444444
1004bd7fc:      eor x15, x12, x15
1004bd800:      add x15, x15, x14
1004bd804:      mov x16, #-0x2020202020202021 ; =-2314885530818453537
1004bd808:      movk    x16, #0xdfe0
1004bd80c:      add x16, x12, x16
1004bd810:      orr x15, x15, x16
1004bd814:      add x13, x13, x14
1004bd818:      orr x13, x15, x13
1004bd81c:      bic x13, x13, x11
1004bd820:      mov x15, #-0x3333333333333334 ; =-3689348814741910324
1004bd824:      orr x15, x15, #0xe1e1e1e1e1e1e1e1
1004bd828:      eor x12, x12, x15
1004bd82c:      add x12, x12, x14
1004bd830:      and x11, x12, x11
1004bd834:      orr x11, x13, x11
1004bd838:      tst x11, #0x8080808080808080
1004bd83c:      cset    w11, ne
1004bd840:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1004bd844:      cmp x9, x12
1004bd848:      b.eq    0x1004bdc30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x520>
1004bd84c:      cmp w8, #0x40
1004bd850:      b.hs    0x1004bda00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x2f0>
1004bd854:      ands    x9, x2, #0x38
1004bd858:      b.eq    0x1004bd87c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x16c>
1004bd85c:      and x10, x2, #0x38
1004bd860:      neg x10, x10
1004bd864:      mov x12, x1
1004bd868:      ldr x13, [x12], #0x8
1004bd86c:      tst x13, #0x8080808080808080
1004bd870:      b.ne    0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd874:      adds    x10, x10, #0x8
1004bd878:      b.ne    0x1004bd868 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x158>
1004bd87c:      mov x3, x8
1004bd880:      and x10, x2, #0x7
1004bd884:      cbz x10, 0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd888:      add x9, x1, x9
1004bd88c:      ldrsb   w12, [x9]
1004bd890:      tbnz    w12, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd894:      mov x3, x8
1004bd898:      cmp x10, #0x1
1004bd89c:      b.eq    0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd8a0:      ldrsb   w12, [x9, #0x1]
1004bd8a4:      tbnz    w12, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd8a8:      mov x3, x8
1004bd8ac:      cmp x10, #0x2
1004bd8b0:      b.eq    0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd8b4:      ldrsb   w12, [x9, #0x2]
1004bd8b8:      tbnz    w12, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd8bc:      mov x3, x8
1004bd8c0:      cmp x10, #0x3
1004bd8c4:      b.eq    0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd8c8:      ldrsb   w12, [x9, #0x3]
1004bd8cc:      tbnz    w12, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd8d0:      mov x3, x8
1004bd8d4:      cmp x10, #0x4
1004bd8d8:      b.eq    0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd8dc:      ldrsb   w12, [x9, #0x4]
1004bd8e0:      tbnz    w12, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd8e4:      mov x3, x8
1004bd8e8:      cmp x10, #0x5
1004bd8ec:      b.eq    0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd8f0:      ldrsb   w12, [x9, #0x5]
1004bd8f4:      tbnz    w12, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd8f8:      mov x3, x8
1004bd8fc:      cmp x10, #0x6
1004bd900:      b.eq    0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd904:      ldrsb   w9, [x9, #0x6]
1004bd908:      mov x3, x8
1004bd90c:      tbz w9, #0x1f, 0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bd910:      b   0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd914:      ands    x11, x1, #0xffffffffffff
1004bd918:      b.eq    0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd91c:      ldr w8, [x11, #0x4]
1004bd920:      cmn w8, #0x3
1004bd924:      b.hi    0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bd928:      mov x10, x1
1004bd92c:      add x1, x11, #0x14
1004bd930:      mov w2, w8
1004bd934:      and w11, w8, #0xfffffffc
1004bd938:      cmp w11, #0x4
1004bd93c:      b.eq    0x1004bd7b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0xa0>
1004bd940:      cmp w8, #0x10
1004bd944:      b.lo    0x1004bd998 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x288>
1004bd948:      mov w11, #0x10              ; =16
1004bd94c:      movi.16b    v0, #0x22
1004bd950:      movi.16b    v1, #0x5c
1004bd954:      movi.16b    v2, #0x20
1004bd958:      mov x12, x1
1004bd95c:      movi.16b    v3, #0xed
1004bd960:      ldr q4, [x12], #0x10
1004bd964:      cmeq.16b    v5, v4, v0
1004bd968:      cmeq.16b    v6, v4, v1
1004bd96c:      orr.16b v5, v6, v5
1004bd970:      cmhi.16b    v6, v2, v4
1004bd974:      cmeq.16b    v4, v4, v3
1004bd978:      orr.16b v4, v4, v6
1004bd97c:      orr.16b v4, v4, v5
1004bd980:      addp.2d d4, v4
1004bd984:      fmov    x13, d4
1004bd988:      cbnz    x13, 0x1004bdc18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x508>
1004bd98c:      add x11, x11, #0x10
1004bd990:      cmp x11, x2
1004bd994:      b.ls    0x1004bd960 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x250>
1004bd998:      and x12, x2, #0xfffffff0
1004bd99c:      add x13, x1, x12
1004bd9a0:      tbnz    w2, #0x3, 0x1004bda8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1004bd9a4:      tst x2, #0x7
1004bd9a8:      b.eq    0x1004bd9ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x2dc>
1004bd9ac:      and x11, x2, #0x8
1004bd9b0:      add x13, x13, x11
1004bd9b4:      sub x11, x2, x11
1004bd9b8:      sub x12, x11, x12
1004bd9bc:      ldrb    w14, [x13], #0x1
1004bd9c0:      mov w11, #0x1               ; =1
1004bd9c4:      cmp w14, #0x22
1004bd9c8:      b.eq    0x1004bdc24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x514>
1004bd9cc:      cmp w14, #0x5c
1004bd9d0:      b.eq    0x1004bdc24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x514>
1004bd9d4:      cmp w14, #0x20
1004bd9d8:      b.lo    0x1004bdc24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x514>
1004bd9dc:      cmp w14, #0xed
1004bd9e0:      b.eq    0x1004bdc24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x514>
1004bd9e4:      subs    x12, x12, #0x1
1004bd9e8:      b.ne    0x1004bd9bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x2ac>
1004bd9ec:      mov w11, #0x0               ; =0
1004bd9f0:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1004bd9f4:      cmp x9, x12
1004bd9f8:      b.ne    0x1004bd84c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x13c>
1004bd9fc:      b   0x1004bdc30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x520>
1004bda00:      and x9, x2, #0xffffffc0
1004bda04:      add x9, x1, x9
1004bda08:      mov x10, x1
1004bda0c:      ldp q0, q1, [x10]
1004bda10:      ldp q2, q3, [x10, #0x20]
1004bda14:      orr.16b v0, v1, v0
1004bda18:      orr.16b v1, v2, v3
1004bda1c:      orr.16b v0, v0, v1
1004bda20:      umaxv.16b   b0, v0
1004bda24:      fmov    w12, s0
1004bda28:      tbnz    w12, #0x7, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bda2c:      add x10, x10, #0x40
1004bda30:      cmp x10, x9
1004bda34:      b.ne    0x1004bda0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x2fc>
1004bda38:      ands    x10, x2, #0x30
1004bda3c:      b.eq    0x1004bda60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x350>
1004bda40:      mov x12, x10
1004bda44:      mov x13, x9
1004bda48:      ldr q0, [x13], #0x10
1004bda4c:      umaxv.16b   b0, v0
1004bda50:      fmov    w14, s0
1004bda54:      tbnz    w14, #0x7, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bda58:      subs    x12, x12, #0x10
1004bda5c:      b.ne    0x1004bda48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x338>
1004bda60:      mov x3, x8
1004bda64:      and x12, x2, #0xf
1004bda68:      cbz x12, 0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bda6c:      add x9, x9, x10
1004bda70:      ldrsb   w10, [x9]
1004bda74:      tbnz    w10, #0x1f, 0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bda78:      add x9, x9, #0x1
1004bda7c:      subs    x12, x12, #0x1
1004bda80:      b.ne    0x1004bda70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x360>
1004bda84:      mov x3, x8
1004bda88:      b   0x1004bdc38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x528>
1004bda8c:      mov x14, #0x101010101010101 ; =72340172838076673
1004bda90:      movk    x14, #0x100
1004bda94:      ldr x11, [x13]
1004bda98:      eor x15, x11, #0x2222222222222222
1004bda9c:      sub x15, x14, x15
1004bdaa0:      orr x16, x15, x11
1004bdaa4:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
1004bdaa8:      bics    xzr, x15, x16
1004bdaac:      b.ne    0x1004bdb08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
1004bdab0:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1004bdab4:      orr x16, x16, #0x4444444444444444
1004bdab8:      eor x16, x11, x16
1004bdabc:      sub x14, x14, x16
1004bdac0:      orr x14, x14, x11
1004bdac4:      bics    xzr, x15, x14
1004bdac8:      b.ne    0x1004bdb08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
1004bdacc:      mov x14, #0x2020202020202020 ; =2314885530818453536
1004bdad0:      movk    x14, #0x201f
1004bdad4:      sub x14, x14, x11
1004bdad8:      orr x14, x14, x11
1004bdadc:      bics    xzr, x15, x14
1004bdae0:      b.ne    0x1004bdb08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
1004bdae4:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
1004bdae8:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
1004bdaec:      eor x14, x11, x14
1004bdaf0:      mov x15, #-0x101010101010102 ; =-72340172838076674
1004bdaf4:      movk    x15, #0xfeff
1004bdaf8:      add x14, x14, x15
1004bdafc:      and x14, x11, x14
1004bdb00:      tst x14, #0x8080808080808080
1004bdb04:      b.eq    0x1004bd9a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x294>
1004bdb08:      and w12, w11, #0xff
1004bdb0c:      cmp w12, #0x22
1004bdb10:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb14:      cmp w12, #0x5c
1004bdb18:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb1c:      cmp w12, #0x20
1004bdb20:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb24:      cmp w12, #0xed
1004bdb28:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb2c:      ubfx    w12, w11, #8, #8
1004bdb30:      cmp w12, #0x22
1004bdb34:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb38:      cmp w12, #0x5c
1004bdb3c:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb40:      cmp w12, #0xed
1004bdb44:      mov w13, #0x20              ; =32
1004bdb48:      ccmp    w12, w13, #0x0, ne
1004bdb4c:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb50:      ubfx    w12, w11, #16, #8
1004bdb54:      cmp w12, #0x22
1004bdb58:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb5c:      cmp w12, #0x5c
1004bdb60:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb64:      cmp w12, #0xed
1004bdb68:      ccmp    w12, w13, #0x0, ne
1004bdb6c:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb70:      lsr w12, w11, #24
1004bdb74:      cmp w12, #0x22
1004bdb78:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb7c:      cmp w12, #0x5c
1004bdb80:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb84:      cmp w12, #0xed
1004bdb88:      ccmp    w12, w13, #0x0, ne
1004bdb8c:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb90:      ubfx    x12, x11, #32, #8
1004bdb94:      cmp w12, #0x22
1004bdb98:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdb9c:      cmp w12, #0x5c
1004bdba0:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdba4:      cmp w12, #0xed
1004bdba8:      ccmp    w12, w13, #0x0, ne
1004bdbac:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbb0:      ubfx    x12, x11, #40, #8
1004bdbb4:      cmp w12, #0x22
1004bdbb8:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbbc:      cmp w12, #0x5c
1004bdbc0:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbc4:      cmp w12, #0xed
1004bdbc8:      ccmp    w12, w13, #0x0, ne
1004bdbcc:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbd0:      ubfx    x12, x11, #48, #8
1004bdbd4:      cmp w12, #0x22
1004bdbd8:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbdc:      cmp w12, #0x5c
1004bdbe0:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbe4:      cmp w12, #0xed
1004bdbe8:      ccmp    w12, w13, #0x0, ne
1004bdbec:      b.lo    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbf0:      lsr x12, x11, #56
1004bdbf4:      cmp w12, #0x22
1004bdbf8:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdbfc:      cmp w12, #0x5c
1004bdc00:      b.eq    0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdc04:      lsr x11, x11, #61
1004bdc08:      cmp x12, #0xed
1004bdc0c:      ccmp    x11, #0x0, #0x4, ne
1004bdc10:      b.ne    0x1004bd9ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x2dc>
1004bdc14:      b   0x1004bdc20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x510>
1004bdc18:      fmov    x11, d4
1004bdc1c:      cbz x11, 0x1004bdc24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x514>
1004bdc20:      mov w11, #0x1               ; =1
1004bdc24:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1004bdc28:      cmp x9, x12
1004bdc2c:      b.ne    0x1004bd84c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x13c>
1004bdc30:      and x9, x10, #0xffffffffffff
1004bdc34:      ldr w3, [x9]
1004bdc38:      tbz w11, #0x0, 0x1004bdc6c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x55c>
1004bdc3c:      mov x19, x0
1004bdc40:      add x0, sp, #0x10
1004bdc44:      bl  0x10021acf4 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
1004bdc48:      ldr w8, [sp, #0x10]
1004bdc4c:      tbz w8, #0x0, 0x1004bdca4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x594>
1004bdc50:      ldur    x8, [sp, #0x14]
1004bdc54:      stur    x8, [x19, #0x4]
1004bdc58:      ldr w8, [sp, #0x1c]
1004bdc5c:      str w8, [x19, #0xc]
1004bdc60:      mov w8, #0x1                ; =1
1004bdc64:      str w8, [x19]
1004bdc68:      b   0x1004bdc94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x584>
1004bdc6c:      mov w9, #0x3                ; =3
1004bdc70:      cmp x2, #0x3
1004bdc74:      csel    x9, x2, x9, lo
1004bdc78:      cbz w8, 0x1004bdd0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
1004bdc7c:      add x10, x1, x2
1004bdc80:      ldurb   w10, [x10, #-0x1]
1004bdc84:      cmp w10, #0xbf
1004bdc88:      b.ls    0x1004bdcb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5a0>
1004bdc8c:      mov w8, #-0x1               ; =-1
1004bdc90:      str w8, [x0]
1004bdc94:      ldp x29, x30, [sp, #0x30]
1004bdc98:      ldp x20, x19, [sp, #0x20]
1004bdc9c:      add sp, sp, #0x40
1004bdca0:      ret
1004bdca4:      mov w8, #-0x1               ; =-1
1004bdca8:      str w8, [x19]
1004bdcac:      b   0x1004bdc94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x584>
1004bdcb0:      cmp w8, #0x1
1004bdcb4:      b.eq    0x1004bdd0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
1004bdcb8:      add x10, x1, x2
1004bdcbc:      ldurb   w10, [x10, #-0x2]
1004bdcc0:      cmp w10, #0xdf
1004bdcc4:      b.ls    0x1004bdcd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5c0>
1004bdcc8:      mov w10, #0x2               ; =2
1004bdccc:      b   0x1004bdd04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
1004bdcd0:      cmp w8, #0x2
1004bdcd4:      b.eq    0x1004bdd0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
1004bdcd8:      add x11, x1, x2
1004bdcdc:      mov x10, #-0x3              ; =-3
1004bdce0:      ldrb    w12, [x11, x10]
1004bdce4:      cmp w12, #0xef
1004bdce8:      b.hi    0x1004bdd00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5f0>
1004bdcec:      sub x10, x10, #0x1
1004bdcf0:      add x12, x9, x10
1004bdcf4:      cmn x12, #0x1
1004bdcf8:      b.ne    0x1004bdce0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5d0>
1004bdcfc:      b   0x1004bdd0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
1004bdd00:      neg x10, x10
1004bdd04:      cmp x10, x9
1004bdd08:      b.ls    0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bdd0c:      cmn w3, #0x3
1004bdd10:      b.hi    0x1004bdc8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1004bdd14:      stp wzr, w8, [x0]
1004bdd18:      str w3, [x0, #0x8]
1004bdd1c:      b   0x1004bdc94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece+0x584>
1004bdd20:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004bdd24:      add x2, x2, #0xe58
1004bdd28:      mov w0, #0x5                ; =5
1004bdd2c:      mov w1, #0x5                ; =5
1004bdd30:      bl  0x100ab0d0c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
