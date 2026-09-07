/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/key33-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010021ad34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new>:
10021ad34:      sub sp, sp, #0x40
10021ad38:      stp x20, x19, [sp, #0x20]
10021ad3c:      stp x29, x30, [sp, #0x30]
10021ad40:      add x29, sp, #0x30
10021ad44:      strb    wzr, [sp, #0xc]
10021ad48:      str wzr, [sp, #0x8]
10021ad4c:      and x8, x0, #0xffff000000000000
10021ad50:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10021ad54:      cmp x8, x9
10021ad58:      b.eq    0x10021af34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x200>
10021ad5c:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
10021ad60:      cmp x8, x9
10021ad64:      b.ne    0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021ad68:      ubfx    x19, x0, #40, #8
10021ad6c:      cbz x19, 0x10021adbc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x88>
10021ad70:      strb    w0, [sp, #0x8]
10021ad74:      cmp x19, #0x1
10021ad78:      b.eq    0x10021adbc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x88>
10021ad7c:      lsr x9, x0, #8
10021ad80:      strb    w9, [sp, #0x9]
10021ad84:      cmp x19, #0x2
10021ad88:      b.eq    0x10021adbc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x88>
10021ad8c:      lsr x9, x0, #16
10021ad90:      strb    w9, [sp, #0xa]
10021ad94:      cmp x19, #0x3
10021ad98:      b.eq    0x10021adbc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x88>
10021ad9c:      lsr x9, x0, #24
10021ada0:      strb    w9, [sp, #0xb]
10021ada4:      cmp x19, #0x4
10021ada8:      b.eq    0x10021adbc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x88>
10021adac:      lsr x9, x0, #32
10021adb0:      strb    w9, [sp, #0xc]
10021adb4:      cmp x19, #0x5
10021adb8:      b.ne    0x10021b338 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x604>
10021adbc:      add x1, sp, #0x8
10021adc0:      mov w2, w19
10021adc4:      and w9, w19, #0xfffffffc
10021adc8:      cmp w9, #0x4
10021adcc:      b.ne    0x10021af5c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x228>
10021add0:      ldr w9, [x1]
10021add4:      tbz w2, #0x1, 0x10021ade0 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0xac>
10021add8:      ldrh    w10, [x1, #0x4]
10021addc:      orr x9, x9, x10, lsl #32
10021ade0:      tbz w2, #0x0, 0x10021adf8 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0xc4>
10021ade4:      sub x10, x2, #0x1
10021ade8:      ldrb    w11, [x1, x10]
10021adec:      lsl x10, x10, #3
10021adf0:      lsl x10, x11, x10
10021adf4:      orr x9, x10, x9
10021adf8:      lsl x10, x2, #3
10021adfc:      mov x11, #0x2020202020202020 ; =2314885530818453536
10021ae00:      lsl x10, x11, x10
10021ae04:      orr x10, x9, x10
10021ae08:      eor x11, x10, #0x2222222222222222
10021ae0c:      mov x12, #-0x101010101010102 ; =-72340172838076674
10021ae10:      movk    x12, #0xfeff
10021ae14:      mov x13, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
10021ae18:      orr x13, x13, #0x4444444444444444
10021ae1c:      eor x13, x10, x13
10021ae20:      add x13, x13, x12
10021ae24:      mov x14, #-0x2020202020202021 ; =-2314885530818453537
10021ae28:      movk    x14, #0xdfe0
10021ae2c:      add x14, x10, x14
10021ae30:      orr x13, x13, x14
10021ae34:      add x11, x11, x12
10021ae38:      orr x11, x13, x11
10021ae3c:      bic x11, x11, x9
10021ae40:      mov x13, #-0x3333333333333334 ; =-3689348814741910324
10021ae44:      orr x13, x13, #0xe1e1e1e1e1e1e1e1
10021ae48:      eor x10, x10, x13
10021ae4c:      add x10, x10, x12
10021ae50:      and x9, x10, x9
10021ae54:      orr x9, x11, x9
10021ae58:      tst x9, #0x8080808080808080
10021ae5c:      cset    w9, ne
10021ae60:      mov x10, #0x7fff000000000000 ; =9223090561878065152
10021ae64:      cmp x8, x10
10021ae68:      b.eq    0x10021b24c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x518>
10021ae6c:      cmp w19, #0x40
10021ae70:      b.hs    0x10021b01c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x2e8>
10021ae74:      ands    x8, x2, #0x38
10021ae78:      b.eq    0x10021ae9c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x168>
10021ae7c:      and x10, x2, #0x38
10021ae80:      neg x10, x10
10021ae84:      mov x11, x1
10021ae88:      ldr x12, [x11], #0x8
10021ae8c:      tst x12, #0x8080808080808080
10021ae90:      b.ne    0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021ae94:      adds    x10, x10, #0x8
10021ae98:      b.ne    0x10021ae88 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x154>
10021ae9c:      mov x3, x19
10021aea0:      and x10, x2, #0x7
10021aea4:      cbz x10, 0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021aea8:      add x8, x1, x8
10021aeac:      ldrsb   w11, [x8]
10021aeb0:      tbnz    w11, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021aeb4:      mov x3, x19
10021aeb8:      cmp x10, #0x1
10021aebc:      b.eq    0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021aec0:      ldrsb   w11, [x8, #0x1]
10021aec4:      tbnz    w11, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021aec8:      mov x3, x19
10021aecc:      cmp x10, #0x2
10021aed0:      b.eq    0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021aed4:      ldrsb   w11, [x8, #0x2]
10021aed8:      tbnz    w11, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021aedc:      mov x3, x19
10021aee0:      cmp x10, #0x3
10021aee4:      b.eq    0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021aee8:      ldrsb   w11, [x8, #0x3]
10021aeec:      tbnz    w11, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021aef0:      mov x3, x19
10021aef4:      cmp x10, #0x4
10021aef8:      b.eq    0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021aefc:      ldrsb   w11, [x8, #0x4]
10021af00:      tbnz    w11, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021af04:      mov x3, x19
10021af08:      cmp x10, #0x5
10021af0c:      b.eq    0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021af10:      ldrsb   w11, [x8, #0x5]
10021af14:      tbnz    w11, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021af18:      mov x3, x19
10021af1c:      cmp x10, #0x6
10021af20:      b.eq    0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021af24:      ldrsb   w8, [x8, #0x6]
10021af28:      mov x3, x19
10021af2c:      tbz w8, #0x1f, 0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021af30:      b   0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021af34:      ands    x9, x0, #0xffffffffffff
10021af38:      b.eq    0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021af3c:      ldr w19, [x9, #0x4]
10021af40:      cmn w19, #0x3
10021af44:      b.hi    0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021af48:      add x1, x9, #0x14
10021af4c:      mov w2, w19
10021af50:      and w9, w19, #0xfffffffc
10021af54:      cmp w9, #0x4
10021af58:      b.eq    0x10021add0 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x9c>
10021af5c:      cmp w19, #0x10
10021af60:      b.lo    0x10021afb4 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x280>
10021af64:      mov w9, #0x10               ; =16
10021af68:      movi.16b    v0, #0x22
10021af6c:      movi.16b    v1, #0x5c
10021af70:      movi.16b    v2, #0x20
10021af74:      mov x10, x1
10021af78:      movi.16b    v3, #0xed
10021af7c:      ldr q4, [x10], #0x10
10021af80:      cmeq.16b    v5, v4, v0
10021af84:      cmeq.16b    v6, v4, v1
10021af88:      orr.16b v5, v6, v5
10021af8c:      cmhi.16b    v6, v2, v4
10021af90:      cmeq.16b    v4, v4, v3
10021af94:      orr.16b v4, v4, v6
10021af98:      orr.16b v4, v4, v5
10021af9c:      addp.2d d4, v4
10021afa0:      fmov    x11, d4
10021afa4:      cbnz    x11, 0x10021b234 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x500>
10021afa8:      add x9, x9, #0x10
10021afac:      cmp x9, x2
10021afb0:      b.ls    0x10021af7c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x248>
10021afb4:      and x10, x2, #0xfffffff0
10021afb8:      add x11, x1, x10
10021afbc:      tbnz    w2, #0x3, 0x10021b0a8 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x374>
10021afc0:      tst x2, #0x7
10021afc4:      b.eq    0x10021b008 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x2d4>
10021afc8:      and x9, x2, #0x8
10021afcc:      add x11, x11, x9
10021afd0:      sub x9, x2, x9
10021afd4:      sub x10, x9, x10
10021afd8:      ldrb    w12, [x11], #0x1
10021afdc:      mov w9, #0x1                ; =1
10021afe0:      cmp w12, #0x22
10021afe4:      b.eq    0x10021b240 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x50c>
10021afe8:      cmp w12, #0x5c
10021afec:      b.eq    0x10021b240 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x50c>
10021aff0:      cmp w12, #0x20
10021aff4:      b.lo    0x10021b240 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x50c>
10021aff8:      cmp w12, #0xed
10021affc:      b.eq    0x10021b240 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x50c>
10021b000:      subs    x10, x10, #0x1
10021b004:      b.ne    0x10021afd8 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x2a4>
10021b008:      mov w9, #0x0                ; =0
10021b00c:      mov x10, #0x7fff000000000000 ; =9223090561878065152
10021b010:      cmp x8, x10
10021b014:      b.ne    0x10021ae6c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x138>
10021b018:      b   0x10021b24c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x518>
10021b01c:      and x8, x2, #0xffffffc0
10021b020:      add x8, x1, x8
10021b024:      mov x10, x1
10021b028:      ldp q0, q1, [x10]
10021b02c:      ldp q2, q3, [x10, #0x20]
10021b030:      orr.16b v0, v1, v0
10021b034:      orr.16b v1, v2, v3
10021b038:      orr.16b v0, v0, v1
10021b03c:      umaxv.16b   b0, v0
10021b040:      fmov    w11, s0
10021b044:      tbnz    w11, #0x7, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021b048:      add x10, x10, #0x40
10021b04c:      cmp x10, x8
10021b050:      b.ne    0x10021b028 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x2f4>
10021b054:      ands    x10, x2, #0x30
10021b058:      b.eq    0x10021b07c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x348>
10021b05c:      mov x11, x10
10021b060:      mov x12, x8
10021b064:      ldr q0, [x12], #0x10
10021b068:      umaxv.16b   b0, v0
10021b06c:      fmov    w13, s0
10021b070:      tbnz    w13, #0x7, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021b074:      subs    x11, x11, #0x10
10021b078:      b.ne    0x10021b064 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x330>
10021b07c:      mov x3, x19
10021b080:      and x11, x2, #0xf
10021b084:      cbz x11, 0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021b088:      add x8, x8, x10
10021b08c:      ldrsb   w10, [x8]
10021b090:      tbnz    w10, #0x1f, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021b094:      add x8, x8, #0x1
10021b098:      subs    x11, x11, #0x1
10021b09c:      b.ne    0x10021b08c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x358>
10021b0a0:      mov x3, x19
10021b0a4:      b   0x10021b254 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x520>
10021b0a8:      mov x12, #0x101010101010101 ; =72340172838076673
10021b0ac:      movk    x12, #0x100
10021b0b0:      ldr x9, [x11]
10021b0b4:      eor x13, x9, #0x2222222222222222
10021b0b8:      sub x13, x12, x13
10021b0bc:      orr x14, x13, x9
10021b0c0:      mov x13, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
10021b0c4:      bics    xzr, x13, x14
10021b0c8:      b.ne    0x10021b124 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x3f0>
10021b0cc:      mov x14, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
10021b0d0:      orr x14, x14, #0x4444444444444444
10021b0d4:      eor x14, x9, x14
10021b0d8:      sub x12, x12, x14
10021b0dc:      orr x12, x12, x9
10021b0e0:      bics    xzr, x13, x12
10021b0e4:      b.ne    0x10021b124 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x3f0>
10021b0e8:      mov x12, #0x2020202020202020 ; =2314885530818453536
10021b0ec:      movk    x12, #0x201f
10021b0f0:      sub x12, x12, x9
10021b0f4:      orr x12, x12, x9
10021b0f8:      bics    xzr, x13, x12
10021b0fc:      b.ne    0x10021b124 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x3f0>
10021b100:      mov x12, #-0x3333333333333334 ; =-3689348814741910324
10021b104:      orr x12, x12, #0xe1e1e1e1e1e1e1e1
10021b108:      eor x12, x9, x12
10021b10c:      mov x13, #-0x101010101010102 ; =-72340172838076674
10021b110:      movk    x13, #0xfeff
10021b114:      add x12, x12, x13
10021b118:      and x12, x9, x12
10021b11c:      tst x12, #0x8080808080808080
10021b120:      b.eq    0x10021afc0 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x28c>
10021b124:      and w10, w9, #0xff
10021b128:      cmp w10, #0x22
10021b12c:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b130:      cmp w10, #0x5c
10021b134:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b138:      cmp w10, #0x20
10021b13c:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b140:      cmp w10, #0xed
10021b144:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b148:      ubfx    w10, w9, #8, #8
10021b14c:      cmp w10, #0x22
10021b150:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b154:      cmp w10, #0x5c
10021b158:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b15c:      cmp w10, #0xed
10021b160:      mov w11, #0x20              ; =32
10021b164:      ccmp    w10, w11, #0x0, ne
10021b168:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b16c:      ubfx    w10, w9, #16, #8
10021b170:      cmp w10, #0x22
10021b174:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b178:      cmp w10, #0x5c
10021b17c:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b180:      cmp w10, #0xed
10021b184:      ccmp    w10, w11, #0x0, ne
10021b188:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b18c:      lsr w10, w9, #24
10021b190:      cmp w10, #0x22
10021b194:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b198:      cmp w10, #0x5c
10021b19c:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1a0:      cmp w10, #0xed
10021b1a4:      ccmp    w10, w11, #0x0, ne
10021b1a8:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1ac:      ubfx    x10, x9, #32, #8
10021b1b0:      cmp w10, #0x22
10021b1b4:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1b8:      cmp w10, #0x5c
10021b1bc:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1c0:      cmp w10, #0xed
10021b1c4:      ccmp    w10, w11, #0x0, ne
10021b1c8:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1cc:      ubfx    x10, x9, #40, #8
10021b1d0:      cmp w10, #0x22
10021b1d4:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1d8:      cmp w10, #0x5c
10021b1dc:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1e0:      cmp w10, #0xed
10021b1e4:      ccmp    w10, w11, #0x0, ne
10021b1e8:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1ec:      ubfx    x10, x9, #48, #8
10021b1f0:      cmp w10, #0x22
10021b1f4:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b1f8:      cmp w10, #0x5c
10021b1fc:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b200:      cmp w10, #0xed
10021b204:      ccmp    w10, w11, #0x0, ne
10021b208:      b.lo    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b20c:      lsr x10, x9, #56
10021b210:      cmp w10, #0x22
10021b214:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b218:      cmp w10, #0x5c
10021b21c:      b.eq    0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b220:      lsr x9, x9, #61
10021b224:      cmp x10, #0xed
10021b228:      ccmp    x9, #0x0, #0x4, ne
10021b22c:      b.ne    0x10021b008 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x2d4>
10021b230:      b   0x10021b23c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x508>
10021b234:      fmov    x9, d4
10021b238:      cbz x9, 0x10021b240 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x50c>
10021b23c:      mov w9, #0x1                ; =1
10021b240:      mov x10, #0x7fff000000000000 ; =9223090561878065152
10021b244:      cmp x8, x10
10021b248:      b.ne    0x10021ae6c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x138>
10021b24c:      and x8, x0, #0xffffffffffff
10021b250:      ldr w3, [x8]
10021b254:      tbz w9, #0x0, 0x10021b270 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x53c>
10021b258:      add x0, sp, #0x10
10021b25c:      bl  0x10021b56c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
10021b260:      ldr w8, [sp, #0x10]
10021b264:      tbz w8, #0x0, 0x10021b290 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x55c>
10021b268:      ldp w9, w8, [sp, #0x18]
10021b26c:      b   0x10021b2d0 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x59c>
10021b270:      mov w8, #0x3                ; =3
10021b274:      cmp x2, #0x3
10021b278:      csel    x8, x2, x8, lo
10021b27c:      cbz w19, 0x10021b2b0 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x57c>
10021b280:      add x9, x1, x2
10021b284:      ldurb   w9, [x9, #-0x1]
10021b288:      cmp w9, #0xbf
10021b28c:      b.ls    0x10021b2a8 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x574>
10021b290:      mov x0, #0x0                ; =0
10021b294:      mov x1, x19
10021b298:      ldp x29, x30, [sp, #0x30]
10021b29c:      ldp x20, x19, [sp, #0x20]
10021b2a0:      add sp, sp, #0x40
10021b2a4:      ret
10021b2a8:      cmp w19, #0x1
10021b2ac:      b.ne    0x10021b2dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x5a8>
10021b2b0:      mov w9, #0x0                ; =0
10021b2b4:      mov x0, #0x0                ; =0
10021b2b8:      cmn w3, #0x3
10021b2bc:      b.hi    0x10021b294 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x560>
10021b2c0:      tbnz    w9, #0x0, 0x10021b294 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x560>
10021b2c4:      add w8, w3, #0x2
10021b2c8:      add w9, w19, #0x2
10021b2cc:      mov w19, #0x0               ; =0
10021b2d0:      mov w9, w9
10021b2d4:      orr x0, x9, x8, lsl #32
10021b2d8:      b   0x10021b294 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x560>
10021b2dc:      add x9, x1, x2
10021b2e0:      ldurb   w9, [x9, #-0x2]
10021b2e4:      cmp w9, #0xdf
10021b2e8:      b.ls    0x10021b2f4 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x5c0>
10021b2ec:      mov w9, #0x2                ; =2
10021b2f0:      b   0x10021b32c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x5f8>
10021b2f4:      cmp w19, #0x2
10021b2f8:      b.eq    0x10021b2b0 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x57c>
10021b2fc:      add x11, x1, x2
10021b300:      mov x10, #-0x3              ; =-3
10021b304:      ldrb    w9, [x11, x10]
10021b308:      cmp w9, #0xef
10021b30c:      b.hi    0x10021b328 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x5f4>
10021b310:      mov w9, #0x0                ; =0
10021b314:      sub x10, x10, #0x1
10021b318:      add x12, x8, x10
10021b31c:      cmn x12, #0x1
10021b320:      b.ne    0x10021b304 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x5d0>
10021b324:      b   0x10021b2b4 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x580>
10021b328:      neg x9, x10
10021b32c:      cmp x9, x8
10021b330:      cset    w9, ls
10021b334:      b   0x10021b2b4 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new+0x580>
10021b338:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
10021b33c:      add x2, x2, #0xe58
10021b340:      mov w0, #0x5                ; =5
10021b344:      mov w1, #0x5                ; =5
10021b348:      bl  0x100ab140c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
