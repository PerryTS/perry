
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/empty-parse-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100260854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
100260854:      sub sp, sp, #0x20
100260858:      stp x29, x30, [sp, #0x10]
10026085c:      add x29, sp, #0x10
100260860:      strb    wzr, [sp, #0xc]
100260864:      str wzr, [sp, #0x8]
100260868:      and x11, x1, #0xffff000000000000
10026086c:      mov x8, #0x7fff000000000000 ; =9223090561878065152
100260870:      cmp x11, x8
100260874:      b.eq    0x1002608ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x98>
100260878:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
10026087c:      cmp x11, x8
100260880:      b.ne    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260884:      ubfx    x8, x1, #40, #8
100260888:      cbz x8, 0x1002608d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x84>
10026088c:      strb    w1, [sp, #0x8]
100260890:      cmp x8, #0x1
100260894:      b.eq    0x1002608d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x84>
100260898:      lsr x9, x1, #8
10026089c:      strb    w9, [sp, #0x9]
1002608a0:      cmp x8, #0x2
1002608a4:      b.eq    0x1002608d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x84>
1002608a8:      lsr x9, x1, #16
1002608ac:      strb    w9, [sp, #0xa]
1002608b0:      cmp x8, #0x3
1002608b4:      b.eq    0x1002608d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x84>
1002608b8:      lsr x9, x1, #24
1002608bc:      strb    w9, [sp, #0xb]
1002608c0:      cmp x8, #0x4
1002608c4:      b.eq    0x1002608d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x84>
1002608c8:      lsr x9, x1, #32
1002608cc:      strb    w9, [sp, #0xc]
1002608d0:      cmp x8, #0x5
1002608d4:      b.ne    0x100260d64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1002608d8:      add x10, sp, #0x8
1002608dc:      mov w9, w8
1002608e0:      cmp w8, #0x10
1002608e4:      b.hs    0x100260910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xbc>
1002608e8:      b   0x100260960 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x10c>
1002608ec:      ands    x9, x1, #0xffffffffffff
1002608f0:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
1002608f4:      ldr w8, [x9, #0x4]
1002608f8:      cmn w8, #0x2
1002608fc:      b.hs    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260900:      add x10, x9, #0x14
100260904:      mov w9, w8
100260908:      cmp w8, #0x10
10026090c:      b.lo    0x100260960 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x10c>
100260910:      mov w12, #0x10              ; =16
100260914:      movi.16b    v0, #0x22
100260918:      movi.16b    v1, #0x5c
10026091c:      movi.16b    v2, #0x20
100260920:      mov x13, x10
100260924:      movi.16b    v3, #0xed
100260928:      ldr q4, [x13], #0x10
10026092c:      cmeq.16b    v5, v4, v0
100260930:      cmeq.16b    v6, v4, v1
100260934:      orr.16b v5, v6, v5
100260938:      cmhi.16b    v6, v2, v4
10026093c:      cmeq.16b    v4, v4, v3
100260940:      orr.16b v4, v4, v6
100260944:      orr.16b v4, v4, v5
100260948:      addp.2d d4, v4
10026094c:      fmov    x14, d4
100260950:      cbnz    x14, 0x100260b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2f0>
100260954:      add x12, x12, #0x10
100260958:      cmp x12, x9
10026095c:      b.ls    0x100260928 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xd4>
100260960:      and x13, x9, #0xfffffff0
100260964:      add x14, x10, x13
100260968:      tbnz    w9, #0x3, 0x1002609b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x160>
10026096c:      tst x9, #0x7
100260970:      b.eq    0x100260b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2f8>
100260974:      and x15, x9, #0x8
100260978:      add x12, x14, x15
10026097c:      sub x14, x9, x15
100260980:      sub x13, x14, x13
100260984:      ldrb    w14, [x12], #0x1
100260988:      cmp w14, #0x22
10026098c:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260990:      cmp w14, #0x5c
100260994:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260998:      cmp w14, #0x20
10026099c:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
1002609a0:      cmp w14, #0xed
1002609a4:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
1002609a8:      subs    x13, x13, #0x1
1002609ac:      b.ne    0x100260984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x130>
1002609b0:      b   0x100260b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2f8>
1002609b4:      mov x15, #0x101010101010101 ; =72340172838076673
1002609b8:      movk    x15, #0x100
1002609bc:      ldr x12, [x14]
1002609c0:      eor x16, x12, #0x2222222222222222
1002609c4:      sub x16, x15, x16
1002609c8:      orr x17, x16, x12
1002609cc:      mov x16, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
1002609d0:      bics    xzr, x16, x17
1002609d4:      b.ne    0x100260a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1e0>
1002609d8:      mov x17, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1002609dc:      orr x17, x17, #0x4444444444444444
1002609e0:      eor x17, x12, x17
1002609e4:      sub x15, x15, x17
1002609e8:      orr x15, x15, x12
1002609ec:      bics    xzr, x16, x15
1002609f0:      b.ne    0x100260a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1e0>
1002609f4:      mov x15, #0x2020202020202020 ; =2314885530818453536
1002609f8:      movk    x15, #0x201f
1002609fc:      sub x15, x15, x12
100260a00:      orr x15, x15, x12
100260a04:      bic x15, x16, x15
100260a08:      cmp x15, #0x0
100260a0c:      mov x15, #-0x3333333333333334 ; =-3689348814741910324
100260a10:      orr x15, x15, #0xe1e1e1e1e1e1e1e1
100260a14:      eor x15, x12, x15
100260a18:      mov x16, #-0x101010101010102 ; =-72340172838076674
100260a1c:      movk    x16, #0xfeff
100260a20:      add x15, x15, x16
100260a24:      and x15, x12, x15
100260a28:      and x15, x15, #0x8080808080808080
100260a2c:      ccmp    x15, #0x0, #0x0, eq
100260a30:      b.eq    0x10026096c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
100260a34:      and w13, w12, #0xff
100260a38:      cmp w13, #0x22
100260a3c:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a40:      cmp w13, #0x5c
100260a44:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a48:      cmp w13, #0x20
100260a4c:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a50:      cmp w13, #0xed
100260a54:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a58:      ubfx    w13, w12, #8, #8
100260a5c:      cmp w13, #0x22
100260a60:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a64:      cmp w13, #0x5c
100260a68:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a6c:      cmp w13, #0xed
100260a70:      mov w14, #0x20              ; =32
100260a74:      ccmp    w13, w14, #0x0, ne
100260a78:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a7c:      ubfx    w13, w12, #16, #8
100260a80:      cmp w13, #0x22
100260a84:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a88:      cmp w13, #0x5c
100260a8c:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a90:      cmp w13, #0xed
100260a94:      ccmp    w13, w14, #0x0, ne
100260a98:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260a9c:      lsr w13, w12, #24
100260aa0:      cmp w13, #0x22
100260aa4:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260aa8:      cmp w13, #0x5c
100260aac:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260ab0:      cmp w13, #0xed
100260ab4:      ccmp    w13, w14, #0x0, ne
100260ab8:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260abc:      ubfx    x13, x12, #32, #8
100260ac0:      cmp w13, #0x22
100260ac4:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260ac8:      cmp w13, #0x5c
100260acc:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260ad0:      cmp w13, #0xed
100260ad4:      ccmp    w13, w14, #0x0, ne
100260ad8:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260adc:      ubfx    x13, x12, #40, #8
100260ae0:      cmp w13, #0x22
100260ae4:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260ae8:      cmp w13, #0x5c
100260aec:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260af0:      cmp w13, #0xed
100260af4:      ccmp    w13, w14, #0x0, ne
100260af8:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260afc:      ubfx    x13, x12, #48, #8
100260b00:      cmp w13, #0x22
100260b04:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b08:      cmp w13, #0x5c
100260b0c:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b10:      cmp w13, #0xed
100260b14:      ccmp    w13, w14, #0x0, ne
100260b18:      b.lo    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b1c:      lsr x13, x12, #56
100260b20:      cmp w13, #0x22
100260b24:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b28:      cmp w13, #0x5c
100260b2c:      b.eq    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b30:      lsr x12, x12, #61
100260b34:      cmp x13, #0xed
100260b38:      ccmp    x12, #0x0, #0x4, ne
100260b3c:      b.ne    0x100260b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2f8>
100260b40:      b   0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b44:      fmov    x12, d4
100260b48:      cbnz    x12, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260b4c:      mov w12, #0x3               ; =3
100260b50:      cmp x9, #0x3
100260b54:      csel    x12, x9, x12, lo
100260b58:      cbz w8, 0x100260b80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100260b5c:      add x13, x10, x9
100260b60:      ldurb   w13, [x13, #-0x1]
100260b64:      cmp w13, #0xbf
100260b68:      b.ls    0x100260b90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x33c>
100260b6c:      mov w8, #0x2                ; =2
100260b70:      str w8, [x0]
100260b74:      ldp x29, x30, [sp, #0x10]
100260b78:      add sp, sp, #0x20
100260b7c:      ret
100260b80:      mov x12, #0x7fff000000000000 ; =9223090561878065152
100260b84:      cmp x11, x12
100260b88:      b.eq    0x100260bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3a4>
100260b8c:      b   0x100260c1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3c8>
100260b90:      cmp w8, #0x1
100260b94:      b.eq    0x100260bec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x398>
100260b98:      add x13, x10, x9
100260b9c:      ldurb   w13, [x13, #-0x2]
100260ba0:      cmp w13, #0xdf
100260ba4:      b.ls    0x100260bb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x35c>
100260ba8:      mov w13, #0x2               ; =2
100260bac:      b   0x100260be4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x390>
100260bb0:      cmp w8, #0x2
100260bb4:      b.eq    0x100260bec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x398>
100260bb8:      add x14, x10, x9
100260bbc:      mov x13, #-0x3              ; =-3
100260bc0:      ldrb    w15, [x14, x13]
100260bc4:      cmp w15, #0xef
100260bc8:      b.hi    0x100260be0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x38c>
100260bcc:      sub x13, x13, #0x1
100260bd0:      add x15, x12, x13
100260bd4:      cmn x15, #0x1
100260bd8:      b.ne    0x100260bc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x36c>
100260bdc:      b   0x100260bec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x398>
100260be0:      neg x13, x13
100260be4:      cmp x13, x12
100260be8:      b.ls    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260bec:      mov x12, #0x7fff000000000000 ; =9223090561878065152
100260bf0:      cmp x11, x12
100260bf4:      b.ne    0x100260c14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3c0>
100260bf8:      and x9, x1, #0xffffffffffff
100260bfc:      ldr w12, [x9]
100260c00:      cmn w12, #0x3
100260c04:      b.hi    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260c08:      stp wzr, w8, [x0]
100260c0c:      str w12, [x0, #0x8]
100260c10:      b   0x100260b74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x320>
100260c14:      cmp w8, #0x40
100260c18:      b.hs    0x100260cdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x488>
100260c1c:      ands    x13, x9, #0x38
100260c20:      b.eq    0x100260c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f0>
100260c24:      and x11, x9, #0x38
100260c28:      neg x11, x11
100260c2c:      mov x12, x10
100260c30:      ldr x14, [x12], #0x8
100260c34:      tst x14, #0x8080808080808080
100260c38:      b.ne    0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260c3c:      adds    x11, x11, #0x8
100260c40:      b.ne    0x100260c30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3dc>
100260c44:      mov x12, x8
100260c48:      and x11, x9, #0x7
100260c4c:      cbz x11, 0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260c50:      add x9, x10, x13
100260c54:      ldrsb   w10, [x9]
100260c58:      tbnz    w10, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260c5c:      mov x12, x8
100260c60:      cmp x11, #0x1
100260c64:      b.eq    0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260c68:      ldrsb   w10, [x9, #0x1]
100260c6c:      tbnz    w10, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260c70:      mov x12, x8
100260c74:      cmp x11, #0x2
100260c78:      b.eq    0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260c7c:      ldrsb   w10, [x9, #0x2]
100260c80:      tbnz    w10, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260c84:      mov x12, x8
100260c88:      cmp x11, #0x3
100260c8c:      b.eq    0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260c90:      ldrsb   w10, [x9, #0x3]
100260c94:      tbnz    w10, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260c98:      mov x12, x8
100260c9c:      cmp x11, #0x4
100260ca0:      b.eq    0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260ca4:      ldrsb   w10, [x9, #0x4]
100260ca8:      tbnz    w10, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260cac:      mov x12, x8
100260cb0:      cmp x11, #0x5
100260cb4:      b.eq    0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260cb8:      ldrsb   w10, [x9, #0x5]
100260cbc:      tbnz    w10, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260cc0:      mov x12, x8
100260cc4:      cmp x11, #0x6
100260cc8:      b.eq    0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260ccc:      ldrsb   w9, [x9, #0x6]
100260cd0:      mov x12, x8
100260cd4:      tbz w9, #0x1f, 0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260cd8:      b   0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260cdc:      and x11, x9, #0xffffffc0
100260ce0:      add x11, x10, x11
100260ce4:      ldp q0, q1, [x10]
100260ce8:      ldp q2, q3, [x10, #0x20]
100260cec:      orr.16b v0, v1, v0
100260cf0:      orr.16b v1, v2, v3
100260cf4:      orr.16b v0, v0, v1
100260cf8:      umaxv.16b   b0, v0
100260cfc:      fmov    w12, s0
100260d00:      tbnz    w12, #0x7, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260d04:      add x10, x10, #0x40
100260d08:      cmp x10, x11
100260d0c:      b.ne    0x100260ce4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
100260d10:      ands    x10, x9, #0x30
100260d14:      b.eq    0x100260d38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4e4>
100260d18:      mov x12, x10
100260d1c:      mov x13, x11
100260d20:      ldr q0, [x13], #0x10
100260d24:      umaxv.16b   b0, v0
100260d28:      fmov    w14, s0
100260d2c:      tbnz    w14, #0x7, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260d30:      subs    x12, x12, #0x10
100260d34:      b.ne    0x100260d20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4cc>
100260d38:      mov x12, x8
100260d3c:      and x9, x9, #0xf
100260d40:      cbz x9, 0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260d44:      add x10, x11, x10
100260d48:      ldrsb   w11, [x10]
100260d4c:      tbnz    w11, #0x1f, 0x100260b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x318>
100260d50:      add x10, x10, #0x1
100260d54:      subs    x9, x9, #0x1
100260d58:      b.ne    0x100260d48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4f4>
100260d5c:      mov x12, x8
100260d60:      b   0x100260c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3ac>
100260d64:      adrp    x2, 0x10109b000 <_anon.7e5d8b44b4d44cb11aa03af0ef44b42e.937+0x48>
100260d68:      add x2, x2, #0xd10
100260d6c:      mov w0, #0x5                ; =5
100260d70:      mov w1, #0x5                ; =5
100260d74:      bl  0x100c8c30c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
