/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001009408e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>:
1009408e8:      sub sp, sp, #0x1c0
1009408ec:      stp x28, x27, [sp, #0x160]
1009408f0:      stp x26, x25, [sp, #0x170]
1009408f4:      stp x24, x23, [sp, #0x180]
1009408f8:      stp x22, x21, [sp, #0x190]
1009408fc:      stp x20, x19, [sp, #0x1a0]
100940900:      stp x29, x30, [sp, #0x1b0]
100940904:      add x29, sp, #0x1b0
100940908:      mov x19, x3
10094090c:      str w2, [sp, #0x1c]
100940910:      mov x21, x0
100940914:      add x0, sp, #0xc0
100940918:      bl  0x1009744c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records6record>
10094091c:      ldr x8, [sp, #0xc0]
100940920:      cbz x8, 0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940924:      ldp x23, x22, [sp, #0xc8]
100940928:      ldp x9, x8, [sp, #0xd8]
10094092c:      stp x9, x8, [sp, #0x8]
100940930:      str x22, [sp, #0xc0]
100940934:      ldr x8, [x21, #0x30]
100940938:      cbz x8, 0x1009409d0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xe8>
10094093c:      add x0, x21, #0x38
100940940:      add x1, sp, #0xc0
100940944:      bl  0x1009f70e8 <__RINvYNtNtNtCs8BpVhDwHqJW_3std4hash6random11RandomStateNtNtCsjgY6bXVaRmE_4core4hash11BuildHasher8hash_oneRyECs5gMwpk3Cs4e_13perry_runtime>
100940948:      mov x8, #0x0                ; =0
10094094c:      lsr x11, x0, #57
100940950:      ldp x10, x9, [x21, #0x18]
100940954:      dup.8b  v0, w11
100940958:      movi.2d v1, #0xffffffffffffffff
10094095c:      and x11, x0, x9
100940960:      ldr d2, [x10, x11]
100940964:      cmeq.8b v3, v2, v0
100940968:      fmov    x12, d3
10094096c:      ands    x12, x12, #0x8080808080808080
100940970:      b.eq    0x1009409a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xb8>
100940974:      rbit    x13, x12
100940978:      clz x13, x13
10094097c:      add x13, x11, x13, lsr #3
100940980:      and x13, x13, x9
100940984:      sub x13, x10, x13, lsl #4
100940988:      ldur    x14, [x13, #-0x10]
10094098c:      cmp x14, x22
100940990:      b.eq    0x100940a08 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x120>
100940994:      sub x13, x12, #0x2
100940998:      ands    x12, x13, x12
10094099c:      b.ne    0x100940974 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8c>
1009409a0:      cmeq.8b v2, v2, v1
1009409a4:      fmov    x12, d2
1009409a8:      cbnz    x12, 0x1009409d0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xe8>
1009409ac:      add x8, x8, #0x8
1009409b0:      add x0, x11, x8
1009409b4:      and x11, x0, x9
1009409b8:      ldr d2, [x10, x11]
1009409bc:      cmeq.8b v3, v2, v0
1009409c0:      fmov    x12, d3
1009409c4:      ands    x12, x12, #0x8080808080808080
1009409c8:      b.ne    0x100940974 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8c>
1009409cc:      b   0x1009409a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xb8>
1009409d0:      ldr x8, [x21, #0x10]
1009409d4:      cmp x8, #0x40
1009409d8:      b.eq    0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
1009409dc:      ldr w8, [x23]
1009409e0:      cbz w8, 0x100940d60 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x478>
1009409e4:      mov w0, #0x0                ; =0
1009409e8:      ldp x29, x30, [sp, #0x1b0]
1009409ec:      ldp x20, x19, [sp, #0x1a0]
1009409f0:      ldp x22, x21, [sp, #0x190]
1009409f4:      ldp x24, x23, [sp, #0x180]
1009409f8:      ldp x26, x25, [sp, #0x170]
1009409fc:      ldp x28, x27, [sp, #0x160]
100940a00:      add sp, sp, #0x1c0
100940a04:      ret
100940a08:      ldur    x23, [x13, #-0x8]
100940a0c:      ldr x8, [sp, #0x10]
100940a10:      cbz x8, 0x100941050 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x768>
100940a14:      mov x28, #0x0               ; =0
100940a18:      mov x22, #0x0               ; =0
100940a1c:      mov x26, #0x1               ; =1
100940a20:      movk    x26, #0x7ffc, lsl #48
100940a24:      mov w9, #0x98               ; =152
100940a28:      mov x8, #0x18               ; =24
100940a2c:      madd    x20, x23, x9, x8
100940a30:      ldr x8, [sp, #0x8]
100940a34:      ldr x25, [x8, x28, lsl #3]
100940a38:      cmp x25, x26
100940a3c:      b.eq    0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940a40:      ldr x1, [x21, #0x10]
100940a44:      cmp x23, x1
100940a48:      b.hs    0x1009412bc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x9d4>
100940a4c:      cmp x28, #0x20
100940a50:      b.eq    0x1009412a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x9c0>
100940a54:      mov x24, x22
100940a58:      ldr x8, [x21, #0x8]
100940a5c:      mov w9, #0x98               ; =152
100940a60:      mul x9, x23, x9
100940a64:      add x9, x8, x9
100940a68:      ldr w22, [x8, x20]
100940a6c:      ldp x27, x1, [x9, #0x8]
100940a70:      subs    x26, x22, x24
100940a74:      ccmp    x1, x22, #0x0, hs
100940a78:      b.lo    0x100940e78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x590>
100940a7c:      cmp x24, x1
100940a80:      b.eq    0x100940aa8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1c0>
100940a84:      cbz x24, 0x100940a94 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1ac>
100940a88:      ldrsb   w8, [x27, x24]
100940a8c:      cmn w8, #0x41
100940a90:      b.le    0x100940e78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x590>
100940a94:      cmp x1, x22
100940a98:      b.eq    0x100940aa8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1c0>
100940a9c:      ldrsb   w8, [x27, x22]
100940aa0:      cmn w8, #0x41
100940aa4:      b.le    0x100940e78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x590>
100940aa8:      ldr x1, [x19, #0x10]
100940aac:      ldr x8, [x19]
100940ab0:      sub x8, x8, x1
100940ab4:      cmp x26, x8
100940ab8:      b.hi    0x100940cdc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3f4>
100940abc:      cmp x22, x24
100940ac0:      b.eq    0x100940adc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1f4>
100940ac4:      ldr x8, [x19, #0x8]
100940ac8:      add x0, x8, x1
100940acc:      add x1, x27, x24
100940ad0:      mov x2, x26
100940ad4:      bl  0x100ce596c <_writev+0x100ce596c>
100940ad8:      ldr x1, [x19, #0x10]
100940adc:      add x24, x1, x26
100940ae0:      str x24, [x19, #0x10]
100940ae4:      mov x26, #0x1               ; =1
100940ae8:      movk    x26, #0x7ffc, lsl #48
100940aec:      add x8, x26, #0xf
100940af0:      cmp x25, x8
100940af4:      b.eq    0x100940b7c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x294>
100940af8:      and x8, x25, #0xffff000000000000
100940afc:      mov x9, #0x7ffa000000000000 ; =9221683186994511872
100940b00:      cmp x8, x9
100940b04:      b.eq    0x100940b7c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x294>
100940b08:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
100940b0c:      cmp x8, x9
100940b10:      b.eq    0x100940b7c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x294>
100940b14:      mov x9, #-0x10000000000000  ; =-4503599627370496
100940b18:      add x9, x25, x9
100940b1c:      tst x25, #0x7
100940b20:      mov x10, #-0xfffffffffffff  ; =-4503599627370495
100940b24:      ccmp    x9, x10, #0x0, eq
100940b28:      b.hs    0x100940b7c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x294>
100940b2c:      mov x9, #0x2                ; =2
100940b30:      movk    x9, #0x7ffc, lsl #48
100940b34:      cmp x25, x9
100940b38:      b.eq    0x100940c8c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3a4>
100940b3c:      mov x9, #0x3                ; =3
100940b40:      movk    x9, #0x7ffc, lsl #48
100940b44:      cmp x25, x9
100940b48:      b.eq    0x100940bb4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2cc>
100940b4c:      mov x9, #0x4                ; =4
100940b50:      movk    x9, #0x7ffc, lsl #48
100940b54:      cmp x25, x9
100940b58:      b.ne    0x100940bec <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x304>
100940b5c:      ldr x8, [x19]
100940b60:      sub x8, x8, x24
100940b64:      cmp x8, #0x3
100940b68:      b.ls    0x100940d40 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x458>
100940b6c:      ldr x8, [x19, #0x8]
100940b70:      mov w9, #0x7274             ; =29300
100940b74:      movk    w9, #0x6575, lsl #16
100940b78:      b   0x100940ca8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3c0>
100940b7c:      ldr w8, [sp, #0x1c]
100940b80:      tbz w8, #0x0, 0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940b84:      mov x0, x21
100940b88:      mov x1, x25
100940b8c:      mov w2, #0x0                ; =0
100940b90:      mov x3, x19
100940b94:      bl  0x1009408e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
100940b98:      cbz w0, 0x1009409e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x100>
100940b9c:      add x28, x28, #0x1
100940ba0:      add x20, x20, #0x4
100940ba4:      ldr x8, [sp, #0x10]
100940ba8:      cmp x8, x28
100940bac:      b.ne    0x100940a30 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x148>
100940bb0:      b   0x100940e18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x530>
100940bb4:      ldr x8, [x19]
100940bb8:      sub x8, x8, x24
100940bbc:      cmp x8, #0x4
100940bc0:      b.ls    0x100940d20 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x438>
100940bc4:      ldr x8, [x19, #0x8]
100940bc8:      add x8, x8, x24
100940bcc:      mov w9, #0x65               ; =101
100940bd0:      strb    w9, [x8, #0x4]
100940bd4:      mov w9, #0x6166             ; =24934
100940bd8:      movk    w9, #0x736c, lsl #16
100940bdc:      str w9, [x8]
100940be0:      ldr x8, [x19, #0x10]
100940be4:      add x8, x8, #0x5
100940be8:      b   0x100940cb4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3cc>
100940bec:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100940bf0:      cmp x8, x9
100940bf4:      b.eq    0x100940c1c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x334>
100940bf8:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100940bfc:      cmp x8, x9
100940c00:      b.ne    0x100940cbc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3d4>
100940c04:      and x8, x25, #0xffffffffffff
100940c08:      cmp x8, #0x1, lsl #12       ; =0x1000
100940c0c:      b.lo    0x100940c8c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3a4>
100940c10:      ldr w2, [x8, #0x4]
100940c14:      add x1, x8, #0x14
100940c18:      b   0x100940cd0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3e8>
100940c1c:      strb    wzr, [sp, #0x24]
100940c20:      str wzr, [sp, #0x20]
100940c24:      ubfx    x1, x25, #40, #8
100940c28:      cbz x1, 0x100940c78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x390>
100940c2c:      strb    w25, [sp, #0x20]
100940c30:      cmp x1, #0x1
100940c34:      b.eq    0x100940c78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x390>
100940c38:      lsr x8, x25, #8
100940c3c:      strb    w8, [sp, #0x21]
100940c40:      cmp x1, #0x2
100940c44:      b.eq    0x100940c78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x390>
100940c48:      lsr x8, x25, #16
100940c4c:      strb    w8, [sp, #0x22]
100940c50:      cmp x1, #0x3
100940c54:      b.eq    0x100940c78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x390>
100940c58:      lsr x8, x25, #24
100940c5c:      strb    w8, [sp, #0x23]
100940c60:      cmp x1, #0x4
100940c64:      b.eq    0x100940c78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x390>
100940c68:      lsr x8, x25, #32
100940c6c:      strb    w8, [sp, #0x24]
100940c70:      cmp x1, #0x5
100940c74:      b.ne    0x1009412e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xa00>
100940c78:      add x8, sp, #0xc0
100940c7c:      add x0, sp, #0x20
100940c80:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100940c84:      ldr w8, [sp, #0xc0]
100940c88:      tbz w8, #0x0, 0x100940ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3e4>
100940c8c:      ldr x8, [x19]
100940c90:      sub x8, x8, x24
100940c94:      cmp x8, #0x3
100940c98:      b.ls    0x100940d00 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x418>
100940c9c:      ldr x8, [x19, #0x8]
100940ca0:      mov w9, #0x756e             ; =30062
100940ca4:      movk    w9, #0x6c6c, lsl #16
100940ca8:      str w9, [x8, x24]
100940cac:      ldr x8, [x19, #0x10]
100940cb0:      add x8, x8, #0x4
100940cb4:      str x8, [x19, #0x10]
100940cb8:      b   0x100940b9c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2b4>
100940cbc:      fmov    d0, x25
100940cc0:      mov x0, x19
100940cc4:      bl  0x100971010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100940cc8:      b   0x100940b9c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2b4>
100940ccc:      ldp x1, x2, [sp, #0xc8]
100940cd0:      mov x0, x19
100940cd4:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100940cd8:      b   0x100940b9c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2b4>
100940cdc:      mov x0, x19
100940ce0:      mov x2, x26
100940ce4:      mov w3, #0x1                ; =1
100940ce8:      mov w4, #0x1                ; =1
100940cec:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100940cf0:      ldr x1, [x19, #0x10]
100940cf4:      cmp x22, x24
100940cf8:      b.ne    0x100940ac4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1dc>
100940cfc:      b   0x100940adc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1f4>
100940d00:      mov x0, x19
100940d04:      mov x1, x24
100940d08:      mov w2, #0x4                ; =4
100940d0c:      mov w3, #0x1                ; =1
100940d10:      mov w4, #0x1                ; =1
100940d14:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100940d18:      ldr x24, [x19, #0x10]
100940d1c:      b   0x100940c9c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3b4>
100940d20:      mov x0, x19
100940d24:      mov x1, x24
100940d28:      mov w2, #0x5                ; =5
100940d2c:      mov w3, #0x1                ; =1
100940d30:      mov w4, #0x1                ; =1
100940d34:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100940d38:      ldr x24, [x19, #0x10]
100940d3c:      b   0x100940bc4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2dc>
100940d40:      mov x0, x19
100940d44:      mov x1, x24
100940d48:      mov w2, #0x4                ; =4
100940d4c:      mov w3, #0x1                ; =1
100940d50:      mov w4, #0x1                ; =1
100940d54:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100940d58:      ldr x24, [x19, #0x10]
100940d5c:      b   0x100940b6c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x284>
100940d60:      mov x0, #0x0                ; =0
100940d64:      bl  0x100611d98 <__RNvYNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1k_6option6OptionQIB1Z_INtNtB1k_4cell4CellhEEEEE9call_onceBa_>
100940d68:      ldrb    w8, [x0]
100940d6c:      cbnz    w8, 0x100940d80 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x498>
100940d70:      mov x0, #0x0                ; =0
100940d74:      bl  0x100611d78 <__RNvYNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json24CACHED_OBJECT_PROTO_BITS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1j_6option6OptionQIB1Y_INtNtB1j_4cell4CellyEEEEE9call_onceBa_>
100940d78:      ldr x8, [x0]
100940d7c:      cbz x8, 0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940d80:      mov x0, x23
100940d84:      bl  0x1005ba8d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
100940d88:      cbz w0, 0x1009409e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x100>
100940d8c:      mov x0, x22
100940d90:      bl  0x1006003cc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object13field_get_set11enumeration24keys_contain_array_index>
100940d94:      tbnz    w0, #0x0, 0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940d98:      movi.2d v0, #0000000000000000
100940d9c:      stur    q0, [sp, #0xa8]
100940da0:      stur    q0, [sp, #0x98]
100940da4:      stur    q0, [sp, #0x88]
100940da8:      stur    q0, [sp, #0x78]
100940dac:      stur    q0, [sp, #0x68]
100940db0:      stur    q0, [sp, #0x58]
100940db4:      stur    q0, [sp, #0x48]
100940db8:      stur    q0, [sp, #0x38]
100940dbc:      mov w8, #0x1                ; =1
100940dc0:      stp xzr, x8, [sp, #0x20]
100940dc4:      str xzr, [sp, #0x30]
100940dc8:      strb    wzr, [sp, #0xbc]
100940dcc:      str wzr, [sp, #0xb8]
100940dd0:      ldr x8, [sp, #0x10]
100940dd4:      cbz x8, 0x100940fa0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6b8>
100940dd8:      ldr x8, [x22, #0x8]
100940ddc:      and x9, x8, #0xffff000000000000
100940de0:      mov x10, #0x7fff000000000000 ; =9223090561878065152
100940de4:      cmp x9, x10
100940de8:      b.eq    0x100940e3c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x554>
100940dec:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
100940df0:      cmp x9, x10
100940df4:      b.ne    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
100940df8:      ubfx    x9, x8, #40, #8
100940dfc:      cbz x9, 0x100940e50 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x568>
100940e00:      strb    w8, [sp, #0xb8]
100940e04:      cmp x9, #0x1
100940e08:      b.ne    0x100940e5c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x574>
100940e0c:      add x0, sp, #0xb8
100940e10:      mov w1, #0x1                ; =1
100940e14:      b   0x100940ee0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5f8>
100940e18:      ldr x20, [x19, #0x10]
100940e1c:      ldr x8, [x19]
100940e20:      cmp x8, x20
100940e24:      b.eq    0x10094128c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x9a4>
100940e28:      ldr x8, [x19, #0x8]
100940e2c:      mov w9, #0x7d               ; =125
100940e30:      strb    w9, [x8, x20]
100940e34:      add x8, x20, #0x1
100940e38:      b   0x100941078 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x790>
100940e3c:      ands    x8, x8, #0xffffffffffff
100940e40:      b.eq    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
100940e44:      ldr w1, [x8, #0x4]
100940e48:      add x0, x8, #0x14
100940e4c:      b   0x100940ee0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5f8>
100940e50:      mov x1, #0x0                ; =0
100940e54:      add x0, sp, #0xb8
100940e58:      b   0x100940ee0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5f8>
100940e5c:      lsr x10, x8, #8
100940e60:      strb    w10, [sp, #0xb9]
100940e64:      cmp x9, #0x2
100940e68:      b.ne    0x100940e90 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5a8>
100940e6c:      add x0, sp, #0xb8
100940e70:      mov w1, #0x2                ; =2
100940e74:      b   0x100940ee0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5f8>
100940e78:      adrp    x4, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100940e7c:      add x4, x4, #0x400
100940e80:      mov x0, x27
100940e84:      mov x2, x24
100940e88:      mov x3, x22
100940e8c:      bl  0x100c99bf8 <__RNvNtCsjgY6bXVaRmE_4core3str16slice_error_fail>
100940e90:      lsr x10, x8, #16
100940e94:      strb    w10, [sp, #0xba]
100940e98:      cmp x9, #0x3
100940e9c:      b.ne    0x100940eac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5c4>
100940ea0:      add x0, sp, #0xb8
100940ea4:      mov w1, #0x3                ; =3
100940ea8:      b   0x100940ee0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5f8>
100940eac:      lsr x10, x8, #24
100940eb0:      strb    w10, [sp, #0xbb]
100940eb4:      cmp x9, #0x4
100940eb8:      b.ne    0x100940ec8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5e0>
100940ebc:      add x0, sp, #0xb8
100940ec0:      mov w1, #0x4                ; =4
100940ec4:      b   0x100940ee0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5f8>
100940ec8:      lsr x8, x8, #32
100940ecc:      strb    w8, [sp, #0xbc]
100940ed0:      cmp x9, #0x5
100940ed4:      b.ne    0x1009412e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xa00>
100940ed8:      add x0, sp, #0xb8
100940edc:      mov w1, #0x5                ; =5
100940ee0:      ldr x8, [x21, #0x48]
100940ee4:      cmp x8, #0x10, lsl #12      ; =0x10000
100940ee8:      b.hi    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
100940eec:      mov w9, #0x6                ; =6
100940ef0:      umull   x9, w1, w9
100940ef4:      mov w10, #0x10000           ; =65536
100940ef8:      add x9, x9, #0x4
100940efc:      sub x8, x10, x8
100940f00:      cmp x9, x8
100940f04:      b.hi    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
100940f08:      add x8, sp, #0xc0
100940f0c:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100940f10:      ldr w8, [sp, #0xc0]
100940f14:      tbz w8, #0x0, 0x100940f2c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x644>
100940f18:      ldr x8, [sp, #0x20]
100940f1c:      cbz x8, 0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940f20:      ldr x0, [sp, #0x28]
100940f24:      bl  0x100ce2ac0 <_mi_free>
100940f28:      b   0x1009409e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xfc>
100940f2c:      ldp x23, x24, [sp, #0xc8]
100940f30:      mov w20, #0x1               ; =1
100940f34:      add x0, sp, #0x20
100940f38:      mov x1, #0x0                ; =0
100940f3c:      mov w2, #0x1                ; =1
100940f40:      mov w3, #0x1                ; =1
100940f44:      mov w4, #0x1                ; =1
100940f48:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100940f4c:      ldr x8, [sp, #0x28]
100940f50:      mov w9, #0x7b               ; =123
100940f54:      strb    w9, [x8]
100940f58:      str x20, [sp, #0x30]
100940f5c:      add x0, sp, #0x20
100940f60:      mov x1, x23
100940f64:      mov x2, x24
100940f68:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100940f6c:      ldr x23, [sp, #0x30]
100940f70:      ldr x8, [sp, #0x20]
100940f74:      cmp x8, x23
100940f78:      b.eq    0x1009412cc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x9e4>
100940f7c:      ldr x8, [sp, #0x28]
100940f80:      mov w20, #0x3a              ; =58
100940f84:      strb    w20, [x8, x23]
100940f88:      add x8, x23, #0x1
100940f8c:      str x8, [sp, #0x30]
100940f90:      str w8, [sp, #0x38]
100940f94:      ldr x9, [sp, #0x10]
100940f98:      subs    x26, x9, #0x1
100940f9c:      b.ne    0x1009410a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x7b8>
100940fa0:      ldr x23, [x21, #0x10]
100940fa4:      ldr x9, [x21, #0x48]
100940fa8:      add x8, x9, x8
100940fac:      str x8, [x21, #0x48]
100940fb0:      ldr q2, [sp, #0xa0]
100940fb4:      ldr x8, [sp, #0xb0]
100940fb8:      str x8, [sp, #0x150]
100940fbc:      ldp q0, q1, [sp, #0x60]
100940fc0:      stp q0, q1, [sp, #0x100]
100940fc4:      ldp q0, q1, [sp, #0x80]
100940fc8:      str q0, [sp, #0x120]
100940fcc:      stp q1, q2, [sp, #0x130]
100940fd0:      ldp q0, q1, [sp, #0x20]
100940fd4:      stp q0, q1, [sp, #0xc0]
100940fd8:      ldp q0, q1, [sp, #0x40]
100940fdc:      stp q0, q1, [sp, #0xe0]
100940fe0:      ldr x8, [x21]
100940fe4:      cmp x23, x8
100940fe8:      b.ne    0x100940ff4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x70c>
100940fec:      mov x0, x21
100940ff0:      bl  0x100cae2dc <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records7KeyPlanE8grow_oneBS_>
100940ff4:      ldr x8, [x21, #0x8]
100940ff8:      mov w9, #0x98               ; =152
100940ffc:      ldr q2, [sp, #0x140]
100941000:      madd    x8, x23, x9, x8
100941004:      ldr x9, [sp, #0x150]
100941008:      str x9, [x8, #0x90]
10094100c:      ldp q0, q1, [sp, #0x100]
100941010:      stp q0, q1, [x8, #0x40]
100941014:      ldp q0, q1, [sp, #0x120]
100941018:      str q0, [x8, #0x60]
10094101c:      stp q1, q2, [x8, #0x70]
100941020:      ldp q0, q1, [sp, #0xc0]
100941024:      stp q0, q1, [x8]
100941028:      ldp q0, q1, [sp, #0xe0]
10094102c:      stp q0, q1, [x8, #0x20]
100941030:      add x8, x23, #0x1
100941034:      str x8, [x21, #0x10]
100941038:      add x0, x21, #0x18
10094103c:      mov x1, x22
100941040:      mov x2, x23
100941044:      bl  0x100538974 <__RNvMs1_NtCshsF3QZNC8Sh_9hashbrown3mapINtB5_7HashMapjjNtNtNtCs8BpVhDwHqJW_3std4hash6random11RandomStateE6insertCs5gMwpk3Cs4e_13perry_runtime>
100941048:      ldr x8, [sp, #0x10]
10094104c:      cbnz    x8, 0x100940a14 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x12c>
100941050:      ldr x1, [x19, #0x10]
100941054:      ldr x8, [x19]
100941058:      sub x8, x8, x1
10094105c:      cmp x8, #0x1
100941060:      b.ls    0x100941084 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x79c>
100941064:      ldr x8, [x19, #0x8]
100941068:      mov w9, #0x7d7b             ; =32123
10094106c:      strh    w9, [x8, x1]
100941070:      ldr x8, [x19, #0x10]
100941074:      add x8, x8, #0x2
100941078:      str x8, [x19, #0x10]
10094107c:      mov w0, #0x1                ; =1
100941080:      b   0x1009409e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x100>
100941084:      mov x0, x19
100941088:      mov w2, #0x2                ; =2
10094108c:      mov w3, #0x1                ; =1
100941090:      mov w4, #0x1                ; =1
100941094:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100941098:      ldr x1, [x19, #0x10]
10094109c:      b   0x100941064 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x77c>
1009410a0:      add x27, x22, #0x10
1009410a4:      mov w28, #0x1c              ; =28
1009410a8:      mov x23, x8
1009410ac:      ldr x8, [x27], #0x8
1009410b0:      and x9, x8, #0xffff000000000000
1009410b4:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1009410b8:      cmp x9, x10
1009410bc:      b.eq    0x1009410e0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x7f8>
1009410c0:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1009410c4:      cmp x9, x10
1009410c8:      and x8, x8, #0xffffffffffff
1009410cc:      ccmp    x8, #0x0, #0x4, eq
1009410d0:      b.eq    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
1009410d4:      ldr w1, [x8, #0x4]
1009410d8:      add x0, x8, #0x14
1009410dc:      b   0x100941178 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
1009410e0:      ubfx    x9, x8, #40, #8
1009410e4:      cbz x9, 0x100941100 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x818>
1009410e8:      strb    w8, [sp, #0xb8]
1009410ec:      cmp x9, #0x1
1009410f0:      b.ne    0x10094110c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x824>
1009410f4:      add x0, sp, #0xb8
1009410f8:      mov w1, #0x1                ; =1
1009410fc:      b   0x100941178 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
100941100:      mov x1, #0x0                ; =0
100941104:      add x0, sp, #0xb8
100941108:      b   0x100941178 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
10094110c:      lsr x10, x8, #8
100941110:      strb    w10, [sp, #0xb9]
100941114:      cmp x9, #0x2
100941118:      b.ne    0x100941128 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x840>
10094111c:      add x0, sp, #0xb8
100941120:      mov w1, #0x2                ; =2
100941124:      b   0x100941178 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
100941128:      lsr x10, x8, #16
10094112c:      strb    w10, [sp, #0xba]
100941130:      cmp x9, #0x3
100941134:      b.ne    0x100941144 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x85c>
100941138:      add x0, sp, #0xb8
10094113c:      mov w1, #0x3                ; =3
100941140:      b   0x100941178 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
100941144:      lsr x10, x8, #24
100941148:      strb    w10, [sp, #0xbb]
10094114c:      cmp x9, #0x4
100941150:      b.ne    0x100941160 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x878>
100941154:      add x0, sp, #0xb8
100941158:      mov w1, #0x4                ; =4
10094115c:      b   0x100941178 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
100941160:      lsr x8, x8, #32
100941164:      strb    w8, [sp, #0xbc]
100941168:      cmp x9, #0x5
10094116c:      b.ne    0x1009412e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xa00>
100941170:      add x0, sp, #0xb8
100941174:      mov w1, #0x5                ; =5
100941178:      ldr x8, [x21, #0x48]
10094117c:      add x8, x8, x23
100941180:      cmp x8, #0x10, lsl #12      ; =0x10000
100941184:      b.hi    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
100941188:      mov w9, #0x6                ; =6
10094118c:      umull   x9, w1, w9
100941190:      add x9, x9, #0x4
100941194:      mov w10, #0x10000           ; =65536
100941198:      sub x8, x10, x8
10094119c:      cmp x9, x8
1009411a0:      b.hi    0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
1009411a4:      add x8, sp, #0xc0
1009411a8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1009411ac:      ldr x8, [sp, #0xc0]
1009411b0:      cbnz    x8, 0x100940f18 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x630>
1009411b4:      ldp x24, x25, [sp, #0xc8]
1009411b8:      ldr x8, [sp, #0x20]
1009411bc:      cmp x8, x23
1009411c0:      b.eq    0x10094122c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x944>
1009411c4:      ldr x8, [sp, #0x28]
1009411c8:      mov w9, #0x2c               ; =44
1009411cc:      strb    w9, [x8, x23]
1009411d0:      add x8, x23, #0x1
1009411d4:      str x8, [sp, #0x30]
1009411d8:      add x0, sp, #0x20
1009411dc:      mov x1, x24
1009411e0:      mov x2, x25
1009411e4:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1009411e8:      ldr x23, [sp, #0x30]
1009411ec:      ldr x8, [sp, #0x20]
1009411f0:      cmp x8, x23
1009411f4:      b.eq    0x100941248 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x960>
1009411f8:      ldr x8, [sp, #0x28]
1009411fc:      strb    w20, [x8, x23]
100941200:      add x9, x23, #0x1
100941204:      str x9, [sp, #0x30]
100941208:      cmp x28, #0x98
10094120c:      b.eq    0x100941278 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x990>
100941210:      mov x8, x9
100941214:      add x9, sp, #0x20
100941218:      str w8, [x9, x28]
10094121c:      add x28, x28, #0x4
100941220:      subs    x26, x26, #0x1
100941224:      b.ne    0x1009410a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x7c0>
100941228:      b   0x100940fa0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6b8>
10094122c:      add x0, sp, #0x20
100941230:      mov x1, x23
100941234:      mov w2, #0x1                ; =1
100941238:      mov w3, #0x1                ; =1
10094123c:      mov w4, #0x1                ; =1
100941240:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100941244:      b   0x1009411c4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8dc>
100941248:      add x0, sp, #0x20
10094124c:      mov x1, x23
100941250:      mov w2, #0x1                ; =1
100941254:      mov w3, #0x1                ; =1
100941258:      mov w4, #0x1                ; =1
10094125c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100941260:      ldr x8, [sp, #0x28]
100941264:      strb    w20, [x8, x23]
100941268:      add x9, x23, #0x1
10094126c:      str x9, [sp, #0x30]
100941270:      cmp x28, #0x98
100941274:      b.ne    0x100941210 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x928>
100941278:      adrp    x2, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
10094127c:      add x2, x2, #0x418
100941280:      mov w0, #0x20               ; =32
100941284:      mov w1, #0x20               ; =32
100941288:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
10094128c:      mov x0, x19
100941290:      mov x1, x20
100941294:      mov w2, #0x1                ; =1
100941298:      mov w3, #0x1                ; =1
10094129c:      mov w4, #0x1                ; =1
1009412a0:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009412a4:      b   0x100940e28 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x540>
1009412a8:      adrp    x2, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009412ac:      add x2, x2, #0x3e8
1009412b0:      mov w0, #0x20               ; =32
1009412b4:      mov w1, #0x20               ; =32
1009412b8:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1009412bc:      adrp    x2, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009412c0:      add x2, x2, #0x3d0
1009412c4:      mov x0, x23
1009412c8:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1009412cc:      add x0, sp, #0x20
1009412d0:      mov x1, x23
1009412d4:      mov w2, #0x1                ; =1
1009412d8:      mov w3, #0x1                ; =1
1009412dc:      mov w4, #0x1                ; =1
1009412e0:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009412e4:      b   0x100940f7c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x694>
1009412e8:      adrp    x2, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009412ec:      add x2, x2, #0x448
1009412f0:      mov w0, #0x5                ; =5
1009412f4:      mov w1, #0x5                ; =5
1009412f8:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
