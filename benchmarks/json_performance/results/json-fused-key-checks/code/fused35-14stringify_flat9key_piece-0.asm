/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bdd30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece>:
1004bdd30:      sub sp, sp, #0x70
1004bdd34:      stp x26, x25, [sp, #0x20]
1004bdd38:      stp x24, x23, [sp, #0x30]
1004bdd3c:      stp x22, x21, [sp, #0x40]
1004bdd40:      stp x20, x19, [sp, #0x50]
1004bdd44:      stp x29, x30, [sp, #0x60]
1004bdd48:      add x29, sp, #0x60
1004bdd4c:      strb    wzr, [sp, #0x4]
1004bdd50:      str wzr, [sp]
1004bdd54:      and x22, x1, #0xffff000000000000
1004bdd58:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1004bdd5c:      cmp x22, x8
1004bdd60:      b.eq    0x1004bddd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0xa4>
1004bdd64:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004bdd68:      cmp x22, x8
1004bdd6c:      b.ne    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdd70:      ubfx    x21, x1, #40, #8
1004bdd74:      cbz x21, 0x1004bddc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x94>
1004bdd78:      strb    w1, [sp]
1004bdd7c:      cmp x21, #0x1
1004bdd80:      b.eq    0x1004bddc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x94>
1004bdd84:      lsr x8, x1, #8
1004bdd88:      strb    w8, [sp, #0x1]
1004bdd8c:      cmp x21, #0x2
1004bdd90:      b.eq    0x1004bddc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x94>
1004bdd94:      lsr x8, x1, #16
1004bdd98:      strb    w8, [sp, #0x2]
1004bdd9c:      cmp x21, #0x3
1004bdda0:      b.eq    0x1004bddc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x94>
1004bdda4:      lsr x8, x1, #24
1004bdda8:      strb    w8, [sp, #0x3]
1004bddac:      cmp x21, #0x4
1004bddb0:      b.eq    0x1004bddc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x94>
1004bddb4:      lsr x8, x1, #32
1004bddb8:      strb    w8, [sp, #0x4]
1004bddbc:      cmp x21, #0x5
1004bddc0:      b.ne    0x1004be3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x6c0>
1004bddc4:      mov x19, sp
1004bddc8:      mov w20, w21
1004bddcc:      cbnz    w21, 0x1004bddf4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0xc4>
1004bddd0:      b   0x1004bdfd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x2a0>
1004bddd4:      ands    x8, x1, #0xffffffffffff
1004bddd8:      b.eq    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdddc:      ldr w21, [x8, #0x4]
1004bdde0:      cmn w21, #0x3
1004bdde4:      b.hi    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdde8:      add x19, x8, #0x14
1004bddec:      mov w20, w21
1004bddf0:      cbz w21, 0x1004bdfd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x2a0>
1004bddf4:      ldrb    w23, [x19]
1004bddf8:      sub w8, w23, #0x30
1004bddfc:      cmp w8, #0x9
1004bde00:      b.hi    0x1004bde48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x118>
1004bde04:      mov x24, x1
1004bde08:      mov x25, x0
1004bde0c:      add x8, sp, #0x8
1004bde10:      mov x0, x19
1004bde14:      mov x1, x20
1004bde18:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004bde1c:      ldr w8, [sp, #0x8]
1004bde20:      tbz w8, #0x0, 0x1004bde30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x100>
1004bde24:      mov x0, x25
1004bde28:      mov x1, x24
1004bde2c:      b   0x1004bde48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x118>
1004bde30:      ldp x0, x1, [sp, #0x10]
1004bde34:      bl  0x1006a1f68 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object13field_get_set11enumeration21canonical_array_index>
1004bde38:      cmp w0, #0x1
1004bde3c:      mov x0, x25
1004bde40:      mov x1, x24
1004bde44:      b.eq    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bde48:      cmp w21, #0x5
1004bde4c:      b.ls    0x1004bde60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x130>
1004bde50:      cmp w23, #0x74
1004bde54:      b.eq    0x1004be3c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x698>
1004bde58:      cmp w23, #0x5f
1004bde5c:      b.eq    0x1004be3c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x698>
1004bde60:      and w8, w21, #0xfffffffc
1004bde64:      cmp w8, #0x4
1004bde68:      b.ne    0x1004bdfd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x2a0>
1004bde6c:      ldr w8, [x19]
1004bde70:      tbz w20, #0x1, 0x1004bde7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x14c>
1004bde74:      ldrh    w9, [x19, #0x4]
1004bde78:      orr x8, x8, x9, lsl #32
1004bde7c:      tbz w20, #0x0, 0x1004bde94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x164>
1004bde80:      sub x9, x20, #0x1
1004bde84:      ldrb    w10, [x19, x9]
1004bde88:      lsl x9, x9, #3
1004bde8c:      lsl x9, x10, x9
1004bde90:      orr x8, x9, x8
1004bde94:      lsl x9, x20, #3
1004bde98:      mov x10, #0x2020202020202020 ; =2314885530818453536
1004bde9c:      lsl x9, x10, x9
1004bdea0:      orr x9, x8, x9
1004bdea4:      eor x10, x9, #0x2222222222222222
1004bdea8:      mov x11, #-0x101010101010102 ; =-72340172838076674
1004bdeac:      movk    x11, #0xfeff
1004bdeb0:      mov x12, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1004bdeb4:      orr x12, x12, #0x4444444444444444
1004bdeb8:      eor x12, x9, x12
1004bdebc:      add x12, x12, x11
1004bdec0:      mov x13, #-0x2020202020202021 ; =-2314885530818453537
1004bdec4:      movk    x13, #0xdfe0
1004bdec8:      add x13, x9, x13
1004bdecc:      orr x12, x12, x13
1004bded0:      add x10, x10, x11
1004bded4:      orr x10, x12, x10
1004bded8:      bic x10, x10, x8
1004bdedc:      mov x12, #-0x3333333333333334 ; =-3689348814741910324
1004bdee0:      orr x12, x12, #0xe1e1e1e1e1e1e1e1
1004bdee4:      eor x9, x9, x12
1004bdee8:      add x9, x9, x11
1004bdeec:      and x8, x9, x8
1004bdef0:      orr x8, x10, x8
1004bdef4:      tst x8, #0x8080808080808080
1004bdef8:      cset    w8, ne
1004bdefc:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1004bdf00:      cmp x22, x9
1004bdf04:      b.eq    0x1004be2c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x598>
1004bdf08:      cmp w21, #0x40
1004bdf0c:      b.hs    0x1004be224 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x4f4>
1004bdf10:      ands    x9, x20, #0x38
1004bdf14:      b.eq    0x1004bdf38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x208>
1004bdf18:      and x10, x20, #0x38
1004bdf1c:      neg x10, x10
1004bdf20:      mov x11, x19
1004bdf24:      ldr x12, [x11], #0x8
1004bdf28:      tst x12, #0x8080808080808080
1004bdf2c:      b.ne    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdf30:      adds    x10, x10, #0x8
1004bdf34:      b.ne    0x1004bdf24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x1f4>
1004bdf38:      mov x3, x21
1004bdf3c:      and x10, x20, #0x7
1004bdf40:      cbz x10, 0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdf44:      add x9, x19, x9
1004bdf48:      ldrsb   w11, [x9]
1004bdf4c:      tbnz    w11, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdf50:      mov x3, x21
1004bdf54:      cmp x10, #0x1
1004bdf58:      b.eq    0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdf5c:      ldrsb   w11, [x9, #0x1]
1004bdf60:      tbnz    w11, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdf64:      mov x3, x21
1004bdf68:      cmp x10, #0x2
1004bdf6c:      b.eq    0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdf70:      ldrsb   w11, [x9, #0x2]
1004bdf74:      tbnz    w11, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdf78:      mov x3, x21
1004bdf7c:      cmp x10, #0x3
1004bdf80:      b.eq    0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdf84:      ldrsb   w11, [x9, #0x3]
1004bdf88:      tbnz    w11, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdf8c:      mov x3, x21
1004bdf90:      cmp x10, #0x4
1004bdf94:      b.eq    0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdf98:      ldrsb   w11, [x9, #0x4]
1004bdf9c:      tbnz    w11, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdfa0:      mov x3, x21
1004bdfa4:      cmp x10, #0x5
1004bdfa8:      b.eq    0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdfac:      ldrsb   w11, [x9, #0x5]
1004bdfb0:      tbnz    w11, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdfb4:      mov x3, x21
1004bdfb8:      cmp x10, #0x6
1004bdfbc:      b.eq    0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdfc0:      ldrsb   w9, [x9, #0x6]
1004bdfc4:      mov x3, x21
1004bdfc8:      tbz w9, #0x1f, 0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004bdfcc:      b   0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004bdfd0:      cmp w21, #0x10
1004bdfd4:      b.lo    0x1004be028 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x2f8>
1004bdfd8:      mov w8, #0x10               ; =16
1004bdfdc:      movi.16b    v0, #0x22
1004bdfe0:      movi.16b    v1, #0x5c
1004bdfe4:      movi.16b    v2, #0x20
1004bdfe8:      mov x9, x19
1004bdfec:      movi.16b    v3, #0xed
1004bdff0:      ldr q4, [x9], #0x10
1004bdff4:      cmeq.16b    v5, v4, v0
1004bdff8:      cmeq.16b    v6, v4, v1
1004bdffc:      orr.16b v5, v6, v5
1004be000:      cmhi.16b    v6, v2, v4
1004be004:      cmeq.16b    v4, v4, v3
1004be008:      orr.16b v4, v4, v6
1004be00c:      orr.16b v4, v4, v5
1004be010:      addp.2d d4, v4
1004be014:      fmov    x10, d4
1004be018:      cbnz    x10, 0x1004be2b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x580>
1004be01c:      add x8, x8, #0x10
1004be020:      cmp x8, x20
1004be024:      b.ls    0x1004bdff0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x2c0>
1004be028:      and x9, x20, #0xfffffff0
1004be02c:      add x10, x19, x9
1004be030:      tbnz    w20, #0x3, 0x1004be098 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x368>
1004be034:      tst x20, #0x7
1004be038:      b.eq    0x1004be084 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x354>
1004be03c:      mov x11, #0x0               ; =0
1004be040:      and x8, x20, #0x8
1004be044:      add x10, x10, x8
1004be048:      sub x8, x20, x8
1004be04c:      sub x9, x8, x9
1004be050:      ldrb    w12, [x10, x11]
1004be054:      mov w8, #0x1                ; =1
1004be058:      cmp w12, #0x22
1004be05c:      b.eq    0x1004be2bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x58c>
1004be060:      cmp w12, #0x5c
1004be064:      b.eq    0x1004be2bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x58c>
1004be068:      cmp w12, #0x20
1004be06c:      b.lo    0x1004be2bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x58c>
1004be070:      cmp w12, #0xed
1004be074:      b.eq    0x1004be2bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x58c>
1004be078:      add x11, x11, #0x1
1004be07c:      cmp x9, x11
1004be080:      b.ne    0x1004be050 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x320>
1004be084:      mov w8, #0x0                ; =0
1004be088:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1004be08c:      cmp x22, x9
1004be090:      b.ne    0x1004bdf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x1d8>
1004be094:      b   0x1004be2c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x598>
1004be098:      mov x11, #0x101010101010101 ; =72340172838076673
1004be09c:      movk    x11, #0x100
1004be0a0:      ldr x8, [x10]
1004be0a4:      eor x12, x8, #0x2222222222222222
1004be0a8:      sub x12, x11, x12
1004be0ac:      orr x13, x12, x8
1004be0b0:      mov x12, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
1004be0b4:      bics    xzr, x12, x13
1004be0b8:      b.ne    0x1004be114 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x3e4>
1004be0bc:      mov x13, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1004be0c0:      orr x13, x13, #0x4444444444444444
1004be0c4:      eor x13, x8, x13
1004be0c8:      sub x11, x11, x13
1004be0cc:      orr x11, x11, x8
1004be0d0:      bics    xzr, x12, x11
1004be0d4:      b.ne    0x1004be114 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x3e4>
1004be0d8:      mov x11, #0x2020202020202020 ; =2314885530818453536
1004be0dc:      movk    x11, #0x201f
1004be0e0:      sub x11, x11, x8
1004be0e4:      orr x11, x11, x8
1004be0e8:      bics    xzr, x12, x11
1004be0ec:      b.ne    0x1004be114 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x3e4>
1004be0f0:      mov x11, #-0x3333333333333334 ; =-3689348814741910324
1004be0f4:      orr x11, x11, #0xe1e1e1e1e1e1e1e1
1004be0f8:      eor x11, x8, x11
1004be0fc:      mov x12, #-0x101010101010102 ; =-72340172838076674
1004be100:      movk    x12, #0xfeff
1004be104:      add x11, x11, x12
1004be108:      and x11, x8, x11
1004be10c:      tst x11, #0x8080808080808080
1004be110:      b.eq    0x1004be034 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x304>
1004be114:      and w9, w8, #0xff
1004be118:      cmp w9, #0x22
1004be11c:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be120:      cmp w9, #0x5c
1004be124:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be128:      cmp w9, #0x20
1004be12c:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be130:      cmp w9, #0xed
1004be134:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be138:      ubfx    w9, w8, #8, #8
1004be13c:      cmp w9, #0x22
1004be140:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be144:      cmp w9, #0x5c
1004be148:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be14c:      cmp w9, #0xed
1004be150:      mov w10, #0x20              ; =32
1004be154:      ccmp    w9, w10, #0x0, ne
1004be158:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be15c:      ubfx    w9, w8, #16, #8
1004be160:      cmp w9, #0x22
1004be164:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be168:      cmp w9, #0x5c
1004be16c:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be170:      cmp w9, #0xed
1004be174:      ccmp    w9, w10, #0x0, ne
1004be178:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be17c:      lsr w9, w8, #24
1004be180:      cmp w9, #0x22
1004be184:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be188:      cmp w9, #0x5c
1004be18c:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be190:      cmp w9, #0xed
1004be194:      ccmp    w9, w10, #0x0, ne
1004be198:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be19c:      ubfx    x9, x8, #32, #8
1004be1a0:      cmp w9, #0x22
1004be1a4:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1a8:      cmp w9, #0x5c
1004be1ac:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1b0:      cmp w9, #0xed
1004be1b4:      ccmp    w9, w10, #0x0, ne
1004be1b8:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1bc:      ubfx    x9, x8, #40, #8
1004be1c0:      cmp w9, #0x22
1004be1c4:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1c8:      cmp w9, #0x5c
1004be1cc:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1d0:      cmp w9, #0xed
1004be1d4:      ccmp    w9, w10, #0x0, ne
1004be1d8:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1dc:      ubfx    x9, x8, #48, #8
1004be1e0:      cmp w9, #0x22
1004be1e4:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1e8:      cmp w9, #0x5c
1004be1ec:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1f0:      cmp w9, #0xed
1004be1f4:      ccmp    w9, w10, #0x0, ne
1004be1f8:      b.lo    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be1fc:      lsr x9, x8, #56
1004be200:      cmp w9, #0x22
1004be204:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be208:      cmp w9, #0x5c
1004be20c:      b.eq    0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be210:      lsr x8, x8, #61
1004be214:      cmp x9, #0xed
1004be218:      ccmp    x8, #0x0, #0x4, ne
1004be21c:      b.ne    0x1004be084 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x354>
1004be220:      b   0x1004be2b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x588>
1004be224:      and x9, x20, #0xffffffc0
1004be228:      add x9, x19, x9
1004be22c:      mov x10, x19
1004be230:      ldp q0, q1, [x10]
1004be234:      ldp q2, q3, [x10, #0x20]
1004be238:      orr.16b v0, v1, v0
1004be23c:      orr.16b v1, v2, v3
1004be240:      orr.16b v0, v0, v1
1004be244:      umaxv.16b   b0, v0
1004be248:      fmov    w11, s0
1004be24c:      tbnz    w11, #0x7, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004be250:      add x10, x10, #0x40
1004be254:      cmp x10, x9
1004be258:      b.ne    0x1004be230 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x500>
1004be25c:      ands    x10, x20, #0x30
1004be260:      b.eq    0x1004be284 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x554>
1004be264:      mov x11, x10
1004be268:      mov x12, x9
1004be26c:      ldr q0, [x12], #0x10
1004be270:      umaxv.16b   b0, v0
1004be274:      fmov    w13, s0
1004be278:      tbnz    w13, #0x7, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004be27c:      subs    x11, x11, #0x10
1004be280:      b.ne    0x1004be26c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x53c>
1004be284:      mov x3, x21
1004be288:      and x11, x20, #0xf
1004be28c:      cbz x11, 0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004be290:      add x9, x9, x10
1004be294:      ldrsb   w10, [x9]
1004be298:      tbnz    w10, #0x1f, 0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004be29c:      add x9, x9, #0x1
1004be2a0:      subs    x11, x11, #0x1
1004be2a4:      b.ne    0x1004be294 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x564>
1004be2a8:      mov x3, x21
1004be2ac:      b   0x1004be2d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5a0>
1004be2b0:      fmov    x8, d4
1004be2b4:      cbz x8, 0x1004be2bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x58c>
1004be2b8:      mov w8, #0x1                ; =1
1004be2bc:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1004be2c0:      cmp x22, x9
1004be2c4:      b.ne    0x1004bdf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x1d8>
1004be2c8:      and x9, x1, #0xffffffffffff
1004be2cc:      ldr w3, [x9]
1004be2d0:      tbz w8, #0x0, 0x1004be308 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5d8>
1004be2d4:      mov x21, x0
1004be2d8:      add x0, sp, #0x8
1004be2dc:      mov x1, x19
1004be2e0:      mov x2, x20
1004be2e4:      bl  0x10021acf4 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
1004be2e8:      ldr w8, [sp, #0x8]
1004be2ec:      tbz w8, #0x0, 0x1004be34c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x61c>
1004be2f0:      ldur    x8, [sp, #0xc]
1004be2f4:      stur    x8, [x21, #0x4]
1004be2f8:      ldr w8, [sp, #0x14]
1004be2fc:      str w8, [x21, #0xc]
1004be300:      mov w8, #0x1                ; =1
1004be304:      b   0x1004be350 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x620>
1004be308:      mov w8, #0x3                ; =3
1004be30c:      cmp x20, #0x3
1004be310:      csel    x8, x20, x8, lo
1004be314:      cbz w21, 0x1004be3b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x684>
1004be318:      add x9, x19, x20
1004be31c:      ldurb   w9, [x9, #-0x1]
1004be320:      cmp w9, #0xbf
1004be324:      b.ls    0x1004be358 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x628>
1004be328:      mov w8, #-0x1               ; =-1
1004be32c:      str w8, [x0]
1004be330:      ldp x29, x30, [sp, #0x60]
1004be334:      ldp x20, x19, [sp, #0x50]
1004be338:      ldp x22, x21, [sp, #0x40]
1004be33c:      ldp x24, x23, [sp, #0x30]
1004be340:      ldp x26, x25, [sp, #0x20]
1004be344:      add sp, sp, #0x70
1004be348:      ret
1004be34c:      mov w8, #-0x1               ; =-1
1004be350:      str w8, [x21]
1004be354:      b   0x1004be330 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x600>
1004be358:      cmp w21, #0x1
1004be35c:      b.eq    0x1004be3b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x684>
1004be360:      add x9, x19, x20
1004be364:      ldurb   w9, [x9, #-0x2]
1004be368:      cmp w9, #0xdf
1004be36c:      b.ls    0x1004be378 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x648>
1004be370:      mov w9, #0x2                ; =2
1004be374:      b   0x1004be3ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x67c>
1004be378:      cmp w21, #0x2
1004be37c:      b.eq    0x1004be3b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x684>
1004be380:      add x10, x19, x20
1004be384:      mov x9, #-0x3               ; =-3
1004be388:      ldrb    w11, [x10, x9]
1004be38c:      cmp w11, #0xef
1004be390:      b.hi    0x1004be3a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x678>
1004be394:      sub x9, x9, #0x1
1004be398:      add x11, x8, x9
1004be39c:      cmn x11, #0x1
1004be3a0:      b.ne    0x1004be388 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x658>
1004be3a4:      b   0x1004be3b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x684>
1004be3a8:      neg x9, x9
1004be3ac:      cmp x9, x8
1004be3b0:      b.ls    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004be3b4:      cmn w3, #0x3
1004be3b8:      b.hi    0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004be3bc:      stp wzr, w21, [x0]
1004be3c0:      str w3, [x0, #0x8]
1004be3c4:      b   0x1004be330 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x600>
1004be3c8:      mov x23, x0
1004be3cc:      mov x0, x19
1004be3d0:      mov x24, x1
1004be3d4:      mov x1, x20
1004be3d8:      bl  0x100ade8b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json>
1004be3dc:      mov x1, x24
1004be3e0:      mov x8, x0
1004be3e4:      mov x0, x23
1004be3e8:      cbz w8, 0x1004bde60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x130>
1004be3ec:      b   0x1004be328 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece+0x5f8>
1004be3f0:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004be3f4:      add x2, x2, #0xe58
1004be3f8:      mov w0, #0x5                ; =5
1004be3fc:      mov w1, #0x5                ; =5
1004be400:      bl  0x100ab144c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
