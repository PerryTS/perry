/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081dcc0 <_js_object_alloc_with_shape>:
10081dcc0:      sub sp, sp, #0xb0
10081dcc4:      stp x28, x27, [sp, #0x50]
10081dcc8:      stp x26, x25, [sp, #0x60]
10081dccc:      stp x24, x23, [sp, #0x70]
10081dcd0:      stp x22, x21, [sp, #0x80]
10081dcd4:      stp x20, x19, [sp, #0x90]
10081dcd8:      stp x29, x30, [sp, #0xa0]
10081dcdc:      add x29, sp, #0xa0
10081dce0:      mov x23, x3
10081dce4:      mov x22, x2
10081dce8:      mov x19, x1
10081dcec:      mov x20, x0
10081dcf0:      mov w8, #0x2                ; =2
10081dcf4:      cmp w1, #0x2
10081dcf8:      csel    w26, w1, w8, hi
10081dcfc:      ubfiz   x8, x26, #3, #32
10081dd00:      mov w9, #0x3ffd             ; =16381
10081dd04:      adrp    x27, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081dd08:      cmp w1, w9
10081dd0c:      b.ls    0x10081dd34 <_js_object_alloc_with_shape+0x74>
10081dd10:      add x0, x8, #0x10
10081dd14:      mov w1, #0x8                ; =8
10081dd18:      mov w2, #0x2                ; =2
10081dd1c:      bl  0x1004da858 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081dd20:      mov x24, x0
10081dd24:      ldurb   w8, [x0, #-0x7]
10081dd28:      orr w8, w8, #0x20
10081dd2c:      sturb   w8, [x0, #-0x7]
10081dd30:      b   0x10081dea0 <_js_object_alloc_with_shape+0x1e0>
10081dd34:      add x21, x8, #0x18
10081dd38:      ldr x8, [x27, #0x48]
10081dd3c:      cmn x8, #0x1
10081dd40:      b.eq    0x10081e898 <_js_object_alloc_with_shape+0xbd8>
10081dd44:      mrs x9, TPIDRRO_EL0
10081dd48:      and x9, x9, #0xfffffffffffffff8
10081dd4c:      ldr x0, [x9, x8, lsl #3]
10081dd50:      cbz x0, 0x10081e898 <_js_object_alloc_with_shape+0xbd8>
10081dd54:      ldr x8, [x0, #0x28]
10081dd58:      ldrb    w8, [x8]
10081dd5c:      tbz w8, #0x0, 0x10081ddc8 <_js_object_alloc_with_shape+0x108>
10081dd60:      ldr x8, [x27, #0x48]
10081dd64:      cmn x8, #0x1
10081dd68:      b.eq    0x10081ea28 <_js_object_alloc_with_shape+0xd68>
10081dd6c:      mrs x9, TPIDRRO_EL0
10081dd70:      and x9, x9, #0xfffffffffffffff8
10081dd74:      ldr x0, [x9, x8, lsl #3]
10081dd78:      cbz x0, 0x10081ea28 <_js_object_alloc_with_shape+0xd68>
10081dd7c:      ldr x25, [x0, #0x20]
10081dd80:      ldr x8, [x25]
10081dd84:      cbnz    x8, 0x10081ea38 <_js_object_alloc_with_shape+0xd78>
10081dd88:      mov x8, #-0x1               ; =-1
10081dd8c:      str x8, [x25]
10081dd90:      ldr x1, [x25, #0x18]
10081dd94:      cbz x1, 0x10081ddc4 <_js_object_alloc_with_shape+0x104>
10081dd98:      mov x0, #0x0                ; =0
10081dd9c:      ldr x8, [x25, #0x10]
10081dda0:      lsl x10, x1, #4
10081dda4:      mov x9, x8
10081dda8:      ldr x11, [x9, #0x8]
10081ddac:      cmp x11, x21
10081ddb0:      b.eq    0x10081dfa0 <_js_object_alloc_with_shape+0x2e0>
10081ddb4:      add x0, x0, #0x1
10081ddb8:      add x9, x9, #0x10
10081ddbc:      subs    x10, x10, #0x10
10081ddc0:      b.ne    0x10081dda8 <_js_object_alloc_with_shape+0xe8>
10081ddc4:      str xzr, [x25]
10081ddc8:      mov x0, x21
10081ddcc:      bl  0x1004da49c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081ddd0:      mov w8, #0x2                ; =2
10081ddd4:      strb    w8, [x0]
10081ddd8:      ldr x8, [x27, #0x48]
10081dddc:      cmn x8, #0x1
10081dde0:      b.eq    0x10081e8ac <_js_object_alloc_with_shape+0xbec>
10081dde4:      mrs x9, TPIDRRO_EL0
10081dde8:      and x9, x9, #0xfffffffffffffff8
10081ddec:      ldr x8, [x9, x8, lsl #3]
10081ddf0:      cbz x8, 0x10081e8ac <_js_object_alloc_with_shape+0xbec>
10081ddf4:      ldr x8, [x8, #0x30]
10081ddf8:      ldrb    w8, [x8]
10081ddfc:      orr w8, w8, #0x2
10081de00:      strb    w8, [x0, #0x1]
10081de04:      ldr x8, [x27, #0x48]
10081de08:      cmn x8, #0x1
10081de0c:      b.eq    0x10081e8c0 <_js_object_alloc_with_shape+0xc00>
10081de10:      mrs x9, TPIDRRO_EL0
10081de14:      and x9, x9, #0xfffffffffffffff8
10081de18:      ldr x8, [x9, x8, lsl #3]
10081de1c:      cbz x8, 0x10081e8c0 <_js_object_alloc_with_shape+0xc00>
10081de20:      ldr x8, [x8, #0x30]
10081de24:      ldrb    w8, [x8]
10081de28:      tbz w8, #0x0, 0x10081de94 <_js_object_alloc_with_shape+0x1d4>
10081de2c:      ldrb    w8, [x0]
10081de30:      sub w9, w8, #0x15
10081de34:      cmn w9, #0x14
10081de38:      b.lo    0x10081de94 <_js_object_alloc_with_shape+0x1d4>
10081de3c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081de40:      add x9, x9, #0xb40
10081de44:      add x8, x9, x8, lsl #5
10081de48:      ldrb    w8, [x8, #0x1b]
10081de4c:      tbnz    w8, #0x0, 0x10081de94 <_js_object_alloc_with_shape+0x1d4>
10081de50:      mov x25, x0
10081de54:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081de58:      add x0, x0, #0x658
10081de5c:      ldr x8, [x0]
10081de60:      blr x8
10081de64:      mov x24, x0
10081de68:      ldrb    w8, [x0, #0x18]
10081de6c:      cbnz    w8, 0x10081eb68 <_js_object_alloc_with_shape+0xea8>
10081de70:      ldr x28, [x24, #0x10]
10081de74:      ldr x8, [x24]
10081de78:      cmp x28, x8
10081de7c:      mov x0, x25
10081de80:      b.eq    0x10081eb98 <_js_object_alloc_with_shape+0xed8>
10081de84:      ldr x8, [x24, #0x8]
10081de88:      str x0, [x8, x28, lsl #3]
10081de8c:      add x8, x28, #0x1
10081de90:      str x8, [x24, #0x10]
10081de94:      strh    wzr, [x0, #0x2]
10081de98:      str w21, [x0, #0x4]
10081de9c:      add x24, x0, #0x8
10081dea0:      stp xzr, xzr, [x24]
10081dea4:      lsl x2, x26, #3
10081dea8:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x58a8>
10081deac:      add x1, x1, #0xcf0
10081deb0:      add x0, x24, #0x10
10081deb4:      bl  0x100af9190 <_writev+0x100af9190>
10081deb8:      lsr x8, x24, #3
10081debc:      cmp x8, #0x201
10081dec0:      b.lo    0x10081df14 <_js_object_alloc_with_shape+0x254>
10081dec4:      ldurb   w8, [x24, #-0x8]
10081dec8:      sub w9, w8, #0x15
10081decc:      cmn w9, #0x14
10081ded0:      b.lo    0x10081df14 <_js_object_alloc_with_shape+0x254>
10081ded4:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081ded8:      add x9, x9, #0xb40
10081dedc:      add x8, x9, x8, lsl #5
10081dee0:      ldrb    w8, [x8, #0x14]
10081dee4:      mov w9, #0x16               ; =22
10081dee8:      lsr w8, w9, w8
10081deec:      tbz w8, #0x0, 0x10081df14 <_js_object_alloc_with_shape+0x254>
10081def0:      ldurh   w8, [x24, #-0x6]
10081def4:      mov w9, #0x4000             ; =16384
10081def8:      bfxil   w9, w8, #0, #13
10081defc:      sturh   w9, [x24, #-0x6]
10081df00:      mov x0, x24
10081df04:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081df08:      ldurh   w8, [x24, #-0x6]
10081df0c:      and w8, w8, #0xffffefff
10081df10:      sturh   w8, [x24, #-0x6]
10081df14:      bl  0x10026a3d0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
10081df18:      str x0, [sp, #0x18]
10081df1c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10081df20:      stp x24, x8, [sp, #0x30]
10081df24:      mov w8, #0x1                ; =1
10081df28:      str x8, [sp, #0x28]
10081df2c:      add x0, sp, #0x28
10081df30:      bl  0x10026a4d8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10081df34:      mov x28, x0
10081df38:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081df3c:      ldr w21, [x24, #0x634]
10081df40:      cmp w21, #0x300
10081df44:      b.hs    0x10081e0c8 <_js_object_alloc_with_shape+0x408>
10081df48:      ldr x8, [x27, #0x48]
10081df4c:      cmn x8, #0x1
10081df50:      b.eq    0x10081e0b8 <_js_object_alloc_with_shape+0x3f8>
10081df54:      mrs x9, TPIDRRO_EL0
10081df58:      and x9, x9, #0xfffffffffffffff8
10081df5c:      ldr x0, [x9, x8, lsl #3]
10081df60:      cbz x0, 0x10081e0b8 <_js_object_alloc_with_shape+0x3f8>
10081df64:      add x8, x0, x21, lsl #3
10081df68:      ldr x0, [x8, #0x1e8]
10081df6c:      cbz x0, 0x10081e0c8 <_js_object_alloc_with_shape+0x408>
10081df70:      ldr x0, [x0]
10081df74:      cbz x0, 0x10081e0dc <_js_object_alloc_with_shape+0x41c>
10081df78:      and w8, w20, #0xff
10081df7c:      add x8, x0, w8, uxtw #4
10081df80:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081df84:      ldr w9, [x8]
10081df88:      cmp w9, w20
10081df8c:      b.ne    0x10081e0f8 <_js_object_alloc_with_shape+0x438>
10081df90:      ldr x25, [x8, #0x8]
10081df94:      ldr w26, [x8, #0x4]
10081df98:      cbnz    x25, 0x10081e65c <_js_object_alloc_with_shape+0x99c>
10081df9c:      b   0x10081e1ec <_js_object_alloc_with_shape+0x52c>
10081dfa0:      cmp x0, x1
10081dfa4:      b.hs    0x10081eb64 <_js_object_alloc_with_shape+0xea4>
10081dfa8:      ldr x24, [x9]
10081dfac:      subs    x10, x1, #0x1
10081dfb0:      ldr q0, [x8, x10, lsl #4]
10081dfb4:      str q0, [x9]
10081dfb8:      str x10, [x25, #0x18]
10081dfbc:      b.ne    0x10081dfe4 <_js_object_alloc_with_shape+0x324>
10081dfc0:      ldr x8, [x27, #0x48]
10081dfc4:      cmn x8, #0x1
10081dfc8:      b.eq    0x10081eb30 <_js_object_alloc_with_shape+0xe70>
10081dfcc:      mrs x9, TPIDRRO_EL0
10081dfd0:      and x9, x9, #0xfffffffffffffff8
10081dfd4:      ldr x0, [x9, x8, lsl #3]
10081dfd8:      cbz x0, 0x10081eb30 <_js_object_alloc_with_shape+0xe70>
10081dfdc:      ldr x8, [x0, #0x28]
10081dfe0:      strb    wzr, [x8]
10081dfe4:      ldr x8, [x25]
10081dfe8:      add x8, x8, #0x1
10081dfec:      str x8, [x25]
10081dff0:      mov w8, #0x2                ; =2
10081dff4:      mov x28, x24
10081dff8:      strb    w8, [x28, #-0x8]!
10081dffc:      ldr x8, [x27, #0x48]
10081e000:      cmn x8, #0x1
10081e004:      b.eq    0x10081eb14 <_js_object_alloc_with_shape+0xe54>
10081e008:      mrs x9, TPIDRRO_EL0
10081e00c:      and x9, x9, #0xfffffffffffffff8
10081e010:      ldr x0, [x9, x8, lsl #3]
10081e014:      cbz x0, 0x10081eb14 <_js_object_alloc_with_shape+0xe54>
10081e018:      ldr x8, [x0, #0x30]
10081e01c:      ldrb    w8, [x8]
10081e020:      orr w8, w8, #0x2
10081e024:      sturb   w8, [x24, #-0x7]
10081e028:      ldr x8, [x27, #0x48]
10081e02c:      cmn x8, #0x1
10081e030:      b.eq    0x10081eb1c <_js_object_alloc_with_shape+0xe5c>
10081e034:      mrs x9, TPIDRRO_EL0
10081e038:      and x9, x9, #0xfffffffffffffff8
10081e03c:      ldr x0, [x9, x8, lsl #3]
10081e040:      cbz x0, 0x10081eb1c <_js_object_alloc_with_shape+0xe5c>
10081e044:      ldr x8, [x0, #0x30]
10081e048:      ldrb    w8, [x8]
10081e04c:      tbz w8, #0x0, 0x10081e0ac <_js_object_alloc_with_shape+0x3ec>
10081e050:      ldrb    w8, [x28]
10081e054:      sub w9, w8, #0x15
10081e058:      cmn w9, #0x14
10081e05c:      b.lo    0x10081e0ac <_js_object_alloc_with_shape+0x3ec>
10081e060:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081e064:      add x9, x9, #0xb40
10081e068:      add x8, x9, x8, lsl #5
10081e06c:      ldrb    w8, [x8, #0x1b]
10081e070:      tbnz    w8, #0x0, 0x10081e0ac <_js_object_alloc_with_shape+0x3ec>
10081e074:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081e078:      add x0, x0, #0x658
10081e07c:      ldr x8, [x0]
10081e080:      blr x8
10081e084:      ldrb    w8, [x0, #0x18]
10081e088:      cbnz    w8, 0x10081eba8 <_js_object_alloc_with_shape+0xee8>
10081e08c:      ldr x25, [x0, #0x10]
10081e090:      ldr x8, [x0]
10081e094:      cmp x25, x8
10081e098:      b.eq    0x10081ebe4 <_js_object_alloc_with_shape+0xf24>
10081e09c:      ldr x8, [x0, #0x8]
10081e0a0:      str x28, [x8, x25, lsl #3]
10081e0a4:      add x8, x25, #0x1
10081e0a8:      str x8, [x0, #0x10]
10081e0ac:      sturh   wzr, [x24, #-0x6]
10081e0b0:      stur    w21, [x24, #-0x4]
10081e0b4:      b   0x10081dea0 <_js_object_alloc_with_shape+0x1e0>
10081e0b8:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e0bc:      add x8, x0, x21, lsl #3
10081e0c0:      ldr x0, [x8, #0x1e8]
10081e0c4:      cbnz    x0, 0x10081df70 <_js_object_alloc_with_shape+0x2b0>
10081e0c8:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081e0cc:      add x0, x0, #0x360
10081e0d0:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081e0d4:      ldr x0, [x0]
10081e0d8:      cbnz    x0, 0x10081df78 <_js_object_alloc_with_shape+0x2b8>
10081e0dc:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081e0e0:      and w8, w20, #0xff
10081e0e4:      add x8, x0, w8, uxtw #4
10081e0e8:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081e0ec:      ldr w9, [x8]
10081e0f0:      cmp w9, w20
10081e0f4:      b.eq    0x10081df90 <_js_object_alloc_with_shape+0x2d0>
10081e0f8:      ldr x8, [x0, #0x5088]
10081e0fc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081e100:      cmp x8, x9
10081e104:      b.hs    0x10081eb58 <_js_object_alloc_with_shape+0xe98>
10081e108:      add x9, x8, #0x1
10081e10c:      str x9, [x0, #0x5088]
10081e110:      ldr x9, [x0, #0x50a8]
10081e114:      cbz x9, 0x10081e1c8 <_js_object_alloc_with_shape+0x508>
10081e118:      mov x9, #0x0                ; =0
10081e11c:      mov w10, w20
10081e120:      mov x11, #0x7c15            ; =31765
10081e124:      movk    x11, #0x7f4a, lsl #16
10081e128:      movk    x11, #0x79b9, lsl #32
10081e12c:      movk    x11, #0x9e37, lsl #48
10081e130:      mul x10, x10, x11
10081e134:      eor x13, x10, x10, lsr #32
10081e138:      lsr x12, x10, #57
10081e13c:      ldr x10, [x0, #0x5098]
10081e140:      ldr x11, [x0, #0x5090]
10081e144:      dup.8b  v0, w12
10081e148:      movi.2d v1, #0xffffffffffffffff
10081e14c:      mov w12, #0x18              ; =24
10081e150:      and x13, x13, x10
10081e154:      ldr d2, [x11, x13]
10081e158:      cmeq.8b v3, v2, v0
10081e15c:      fmov    x14, d3
10081e160:      ands    x14, x14, #0x8080808080808080
10081e164:      b.eq    0x10081e198 <_js_object_alloc_with_shape+0x4d8>
10081e168:      rbit    x15, x14
10081e16c:      clz x15, x15
10081e170:      add x15, x13, x15, lsr #3
10081e174:      and x15, x15, x10
10081e178:      mneg    x15, x15, x12
10081e17c:      add x15, x11, x15
10081e180:      ldur    w16, [x15, #-0x18]
10081e184:      cmp w20, w16
10081e188:      b.eq    0x10081e1dc <_js_object_alloc_with_shape+0x51c>
10081e18c:      sub x15, x14, #0x2
10081e190:      ands    x14, x15, x14
10081e194:      b.ne    0x10081e168 <_js_object_alloc_with_shape+0x4a8>
10081e198:      cmeq.8b v2, v2, v1
10081e19c:      fmov    x14, d2
10081e1a0:      cbnz    x14, 0x10081e1c8 <_js_object_alloc_with_shape+0x508>
10081e1a4:      add x9, x9, #0x8
10081e1a8:      add x13, x13, x9
10081e1ac:      and x13, x13, x10
10081e1b0:      ldr d2, [x11, x13]
10081e1b4:      cmeq.8b v3, v2, v0
10081e1b8:      fmov    x14, d3
10081e1bc:      ands    x14, x14, #0x8080808080808080
10081e1c0:      b.ne    0x10081e168 <_js_object_alloc_with_shape+0x4a8>
10081e1c4:      b   0x10081e198 <_js_object_alloc_with_shape+0x4d8>
10081e1c8:      mov w26, #0x0               ; =0
10081e1cc:      mov x25, #0x0               ; =0
10081e1d0:      str x8, [x0, #0x5088]
10081e1d4:      cbnz    x25, 0x10081e65c <_js_object_alloc_with_shape+0x99c>
10081e1d8:      b   0x10081e1ec <_js_object_alloc_with_shape+0x52c>
10081e1dc:      ldur    x25, [x15, #-0x10]
10081e1e0:      ldur    w26, [x15, #-0x8]
10081e1e4:      str x8, [x0, #0x5088]
10081e1e8:      cbnz    x25, 0x10081e65c <_js_object_alloc_with_shape+0x99c>
10081e1ec:      and w8, w20, #0xff
10081e1f0:      str x8, [sp, #0x8]
10081e1f4:      mov w26, #0x0               ; =0
10081e1f8:      mov w8, #0x0                ; =0
10081e1fc:      mov w9, #0x8                ; =8
10081e200:      str x9, [sp, #0x10]
10081e204:      mov w25, w23
10081e208:      b   0x10081e21c <_js_object_alloc_with_shape+0x55c>
10081e20c:      mov w26, #0x1               ; =1
10081e210:      mov x22, x21
10081e214:      mov w8, #0x1                ; =1
10081e218:      cbnz    x23, 0x10081e274 <_js_object_alloc_with_shape+0x5b4>
10081e21c:      tbnz    w8, #0x0, 0x10081e268 <_js_object_alloc_with_shape+0x5a8>
10081e220:      mov x21, x22
10081e224:      mov x23, #0x0               ; =0
10081e228:      cbz x25, 0x10081e20c <_js_object_alloc_with_shape+0x54c>
10081e22c:      ldrb    w8, [x21, x23]
10081e230:      cbz w8, 0x10081e254 <_js_object_alloc_with_shape+0x594>
10081e234:      add x23, x23, #0x1
10081e238:      cmp x25, x23
10081e23c:      b.ne    0x10081e22c <_js_object_alloc_with_shape+0x56c>
10081e240:      mov w26, #0x1               ; =1
10081e244:      mov x22, x21
10081e248:      mov w8, #0x1                ; =1
10081e24c:      mov x23, x25
10081e250:      b   0x10081e218 <_js_object_alloc_with_shape+0x558>
10081e254:      mvn x9, x23
10081e258:      add x25, x9, x25
10081e25c:      add x9, x21, x23
10081e260:      add x22, x9, #0x1
10081e264:      b   0x10081e218 <_js_object_alloc_with_shape+0x558>
10081e268:      mov x23, #0x0               ; =0
10081e26c:      mov w21, #0x1               ; =1
10081e270:      b   0x10081e374 <_js_object_alloc_with_shape+0x6b4>
10081e274:      cbz x21, 0x10081e364 <_js_object_alloc_with_shape+0x6a4>
10081e278:      mov w0, #0x40               ; =64
10081e27c:      mov w1, #0x8                ; =8
10081e280:      bl  0x100af6548 <_mi_malloc_aligned>
10081e284:      cbz x0, 0x10081ebf4 <_js_object_alloc_with_shape+0xf34>
10081e288:      stp x21, x23, [x0]
10081e28c:      mov w8, #0x4                ; =4
10081e290:      stp x8, x0, [sp, #0x28]
10081e294:      mov w23, #0x1               ; =1
10081e298:      str x23, [sp, #0x38]
10081e29c:      mov x8, x25
10081e2a0:      mov x10, x22
10081e2a4:      mov x9, x26
10081e2a8:      b   0x10081e2bc <_js_object_alloc_with_shape+0x5fc>
10081e2ac:      mov w26, #0x1               ; =1
10081e2b0:      mov x10, x24
10081e2b4:      mov w9, #0x1                ; =1
10081e2b8:      cbnz    x21, 0x10081e310 <_js_object_alloc_with_shape+0x650>
10081e2bc:      tbnz    w9, #0x0, 0x10081e350 <_js_object_alloc_with_shape+0x690>
10081e2c0:      mov x24, x10
10081e2c4:      mov x21, #0x0               ; =0
10081e2c8:      cbz x8, 0x10081e2ac <_js_object_alloc_with_shape+0x5ec>
10081e2cc:      ldrb    w9, [x24, x21]
10081e2d0:      cbz w9, 0x10081e2f4 <_js_object_alloc_with_shape+0x634>
10081e2d4:      add x21, x21, #0x1
10081e2d8:      cmp x8, x21
10081e2dc:      b.ne    0x10081e2cc <_js_object_alloc_with_shape+0x60c>
10081e2e0:      mov w26, #0x1               ; =1
10081e2e4:      mov x10, x24
10081e2e8:      mov w9, #0x1                ; =1
10081e2ec:      mov x21, x8
10081e2f0:      b   0x10081e2b8 <_js_object_alloc_with_shape+0x5f8>
10081e2f4:      mvn x10, x21
10081e2f8:      add x25, x10, x8
10081e2fc:      add x8, x24, x21
10081e300:      add x22, x8, #0x1
10081e304:      mov x8, x25
10081e308:      mov x10, x22
10081e30c:      b   0x10081e2b8 <_js_object_alloc_with_shape+0x5f8>
10081e310:      cbz x24, 0x10081e350 <_js_object_alloc_with_shape+0x690>
10081e314:      ldr x8, [sp, #0x28]
10081e318:      cmp x23, x8
10081e31c:      b.eq    0x10081e330 <_js_object_alloc_with_shape+0x670>
10081e320:      add x8, x0, x23, lsl #4
10081e324:      stp x24, x21, [x8]
10081e328:      add x23, x23, #0x1
10081e32c:      b   0x10081e298 <_js_object_alloc_with_shape+0x5d8>
10081e330:      add x0, sp, #0x28
10081e334:      mov x1, x23
10081e338:      mov w2, #0x1                ; =1
10081e33c:      mov w3, #0x8                ; =8
10081e340:      mov w4, #0x10               ; =16
10081e344:      bl  0x100ad5bd0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081e348:      ldr x0, [sp, #0x30]
10081e34c:      b   0x10081e320 <_js_object_alloc_with_shape+0x660>
10081e350:      ldp x8, x9, [sp, #0x28]
10081e354:      str x9, [sp, #0x10]
10081e358:      cmp x8, #0x0
10081e35c:      cset    w21, eq
10081e360:      b   0x10081e374 <_js_object_alloc_with_shape+0x6b4>
10081e364:      mov x23, #0x0               ; =0
10081e368:      mov w21, #0x1               ; =1
10081e36c:      mov w8, #0x8                ; =8
10081e370:      str x8, [sp, #0x10]
10081e374:      ubfiz   x8, x23, #3, #32
10081e378:      add x0, x8, #0x8
10081e37c:      mov w1, #0x8                ; =8
10081e380:      mov w2, #0x1                ; =1
10081e384:      bl  0x1004db2e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081e388:      mov x22, x0
10081e38c:      stp w23, w23, [x0]
10081e390:      lsr x8, x0, #3
10081e394:      cmp x8, #0x201
10081e398:      b.lo    0x10081e428 <_js_object_alloc_with_shape+0x768>
10081e39c:      ldurb   w8, [x22, #-0x8]
10081e3a0:      cmp w8, #0x1
10081e3a4:      b.ne    0x10081e3d0 <_js_object_alloc_with_shape+0x710>
10081e3a8:      ldurh   w8, [x22, #-0x6]
10081e3ac:      mov w9, #0x1080             ; =4224
10081e3b0:      mov w10, #0xef7f            ; =61311
10081e3b4:      and w10, w8, w10
10081e3b8:      sturh   w10, [x22, #-0x6]
10081e3bc:      tst w8, w9
10081e3c0:      b.eq    0x10081e3e0 <_js_object_alloc_with_shape+0x720>
10081e3c4:      mov x0, x22
10081e3c8:      bl  0x1002b6f40 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081e3cc:      ldurb   w8, [x22, #-0x8]
10081e3d0:      sub w9, w8, #0x15
10081e3d4:      cmn w9, #0x14
10081e3d8:      b.hs    0x10081e3e4 <_js_object_alloc_with_shape+0x724>
10081e3dc:      b   0x10081e428 <_js_object_alloc_with_shape+0x768>
10081e3e0:      mov w8, #0x1                ; =1
10081e3e4:      mov w8, w8
10081e3e8:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081e3ec:      add x9, x9, #0xb40
10081e3f0:      add x8, x9, x8, lsl #5
10081e3f4:      ldrb    w8, [x8, #0x14]
10081e3f8:      mov w9, #0x16               ; =22
10081e3fc:      lsr w8, w9, w8
10081e400:      tbz w8, #0x0, 0x10081e428 <_js_object_alloc_with_shape+0x768>
10081e404:      ldurh   w8, [x22, #-0x6]
10081e408:      mov w9, #0x4000             ; =16384
10081e40c:      bfxil   w9, w8, #0, #13
10081e410:      sturh   w9, [x22, #-0x6]
10081e414:      mov x0, x22
10081e418:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081e41c:      ldurh   w8, [x22, #-0x6]
10081e420:      and w8, w8, #0xffffefff
10081e424:      sturh   w8, [x22, #-0x6]
10081e428:      stp w21, w19, [sp]
10081e42c:      mov x19, x20
10081e430:      mov x20, x28
10081e434:      bl  0x10026a3d0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
10081e438:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10081e43c:      stp x22, x8, [sp, #0x30]
10081e440:      mov w8, #0x1                ; =1
10081e444:      stp x0, x8, [sp, #0x20]
10081e448:      add x0, sp, #0x28
10081e44c:      bl  0x10026a4d8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10081e450:      mov x22, x0
10081e454:      cbz x23, 0x10081e518 <_js_object_alloc_with_shape+0x858>
10081e458:      mov x25, #0x0               ; =0
10081e45c:      mov x26, #0x7fffffffffffffff ; =9223372036854775807
10081e460:      mov w28, #0x18              ; =24
10081e464:      ldr x8, [sp, #0x10]
10081e468:      mov x24, x8
10081e46c:      add x21, x8, x23, lsl #4
10081e470:      b   0x10081e4a4 <_js_object_alloc_with_shape+0x7e4>
10081e474:      mov x0, x22
10081e478:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e47c:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081e480:      bfxil   x2, x23, #0, #48
10081e484:      add x8, x0, x25, lsl #3
10081e488:      str x2, [x8, #0x8]
10081e48c:      mov x1, x25
10081e490:      bl  0x1004f10dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081e494:      add x24, x24, #0x10
10081e498:      add x25, x25, #0x1
10081e49c:      cmp x24, x21
10081e4a0:      b.eq    0x10081e518 <_js_object_alloc_with_shape+0x858>
10081e4a4:      ldr x0, [x24]
10081e4a8:      ldr w1, [x24, #0x8]
10081e4ac:      bl  0x100885194 <_js_string_from_bytes_longlived>
10081e4b0:      mov x23, x0
10081e4b4:      ldr x8, [x27, #0x48]
10081e4b8:      cmn x8, #0x1
10081e4bc:      b.eq    0x10081e474 <_js_object_alloc_with_shape+0x7b4>
10081e4c0:      mrs x9, TPIDRRO_EL0
10081e4c4:      and x9, x9, #0xfffffffffffffff8
10081e4c8:      ldr x8, [x9, x8, lsl #3]
10081e4cc:      cbz x8, 0x10081e474 <_js_object_alloc_with_shape+0x7b4>
10081e4d0:      ldr x8, [x8, #0x19e8]
10081e4d4:      cbz x8, 0x10081e474 <_js_object_alloc_with_shape+0x7b4>
10081e4d8:      ldr x9, [x8]
10081e4dc:      cmp x9, x26
10081e4e0:      b.hs    0x10081eb38 <_js_object_alloc_with_shape+0xe78>
10081e4e4:      add x10, x9, #0x1
10081e4e8:      str x10, [x8]
10081e4ec:      ldr x10, [x8, #0x18]
10081e4f0:      cmp x22, x10
10081e4f4:      b.hs    0x10081eb44 <_js_object_alloc_with_shape+0xe84>
10081e4f8:      ldr x10, [x8, #0x10]
10081e4fc:      madd    x10, x22, x28, x10
10081e500:      ldr x11, [x10]
10081e504:      cmp x11, #0x1
10081e508:      b.ne    0x10081eb48 <_js_object_alloc_with_shape+0xe88>
10081e50c:      ldr x0, [x10, #0x8]
10081e510:      str x9, [x8]
10081e514:      b   0x10081e47c <_js_object_alloc_with_shape+0x7bc>
10081e518:      ldr x8, [x27, #0x48]
10081e51c:      cmn x8, #0x1
10081e520:      b.eq    0x10081e590 <_js_object_alloc_with_shape+0x8d0>
10081e524:      mrs x9, TPIDRRO_EL0
10081e528:      and x9, x9, #0xfffffffffffffff8
10081e52c:      ldr x8, [x9, x8, lsl #3]
10081e530:      cbz x8, 0x10081e590 <_js_object_alloc_with_shape+0x8d0>
10081e534:      ldr x8, [x8, #0x19e8]
10081e538:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081e53c:      cbz x8, 0x10081e5c8 <_js_object_alloc_with_shape+0x908>
10081e540:      ldr x9, [x8]
10081e544:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081e548:      cmp x9, x10
10081e54c:      b.hs    0x10081eb38 <_js_object_alloc_with_shape+0xe78>
10081e550:      add x10, x9, #0x1
10081e554:      str x10, [x8]
10081e558:      ldr x10, [x8, #0x18]
10081e55c:      cmp x22, x10
10081e560:      b.hs    0x10081eb44 <_js_object_alloc_with_shape+0xe84>
10081e564:      ldr x10, [x8, #0x10]
10081e568:      mov w11, #0x18              ; =24
10081e56c:      madd    x10, x22, x11, x10
10081e570:      ldr x11, [x10]
10081e574:      cmp x11, #0x1
10081e578:      b.ne    0x10081eb48 <_js_object_alloc_with_shape+0xe88>
10081e57c:      mov x28, x20
10081e580:      mov x20, x19
10081e584:      ldr x25, [x10, #0x8]
10081e588:      str x9, [x8]
10081e58c:      b   0x10081e5dc <_js_object_alloc_with_shape+0x91c>
10081e590:      mov x0, x22
10081e594:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e598:      mov x25, x0
10081e59c:      mov x28, x20
10081e5a0:      mov x20, x19
10081e5a4:      ldr w19, [sp, #0x4]
10081e5a8:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081e5ac:      mov x0, x20
10081e5b0:      mov x1, x25
10081e5b4:      bl  0x10032223c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081e5b8:      ldr w21, [x24, #0x634]
10081e5bc:      cmp w21, #0x300
10081e5c0:      b.lo    0x10081e5f8 <_js_object_alloc_with_shape+0x938>
10081e5c4:      b   0x10081e8f0 <_js_object_alloc_with_shape+0xc30>
10081e5c8:      mov x0, x22
10081e5cc:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e5d0:      mov x25, x0
10081e5d4:      mov x28, x20
10081e5d8:      mov x20, x19
10081e5dc:      ldr w19, [sp, #0x4]
10081e5e0:      mov x0, x20
10081e5e4:      mov x1, x25
10081e5e8:      bl  0x10032223c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081e5ec:      ldr w21, [x24, #0x634]
10081e5f0:      cmp w21, #0x300
10081e5f4:      b.hs    0x10081e8f0 <_js_object_alloc_with_shape+0xc30>
10081e5f8:      ldr x8, [x27, #0x48]
10081e5fc:      cmn x8, #0x1
10081e600:      b.eq    0x10081e8e0 <_js_object_alloc_with_shape+0xc20>
10081e604:      mrs x9, TPIDRRO_EL0
10081e608:      and x9, x9, #0xfffffffffffffff8
10081e60c:      ldr x0, [x9, x8, lsl #3]
10081e610:      cbz x0, 0x10081e8e0 <_js_object_alloc_with_shape+0xc20>
10081e614:      add x8, x0, x21, lsl #3
10081e618:      ldr x0, [x8, #0x1e8]
10081e61c:      cbz x0, 0x10081e8f0 <_js_object_alloc_with_shape+0xc30>
10081e620:      ldr x0, [x0]
10081e624:      cbz x0, 0x10081e904 <_js_object_alloc_with_shape+0xc44>
10081e628:      ldr x8, [sp, #0x8]
10081e62c:      add x8, x0, x8, lsl #4
10081e630:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081e634:      ldr w9, [x8]
10081e638:      cmp w9, w20
10081e63c:      b.ne    0x10081e920 <_js_object_alloc_with_shape+0xc60>
10081e640:      ldr w26, [x8, #0x4]
10081e644:      add x0, sp, #0x20
10081e648:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e64c:      ldr w8, [sp]
10081e650:      tbnz    w8, #0x0, 0x10081e65c <_js_object_alloc_with_shape+0x99c>
10081e654:      ldr x0, [sp, #0x10]
10081e658:      bl  0x100af62c0 <_mi_free>
10081e65c:      ldr x8, [x27, #0x48]
10081e660:      cmn x8, #0x1
10081e664:      b.eq    0x10081e6cc <_js_object_alloc_with_shape+0xa0c>
10081e668:      mrs x9, TPIDRRO_EL0
10081e66c:      and x9, x9, #0xfffffffffffffff8
10081e670:      ldr x8, [x9, x8, lsl #3]
10081e674:      cbz x8, 0x10081e6cc <_js_object_alloc_with_shape+0xa0c>
10081e678:      ldr x8, [x8, #0x19e8]
10081e67c:      cbz x8, 0x10081e6cc <_js_object_alloc_with_shape+0xa0c>
10081e680:      ldr x9, [x8]
10081e684:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081e688:      cmp x9, x10
10081e68c:      b.hs    0x10081eb38 <_js_object_alloc_with_shape+0xe78>
10081e690:      add x10, x9, #0x1
10081e694:      str x10, [x8]
10081e698:      ldr x10, [x8, #0x18]
10081e69c:      cmp x28, x10
10081e6a0:      b.hs    0x10081eb44 <_js_object_alloc_with_shape+0xe84>
10081e6a4:      ldr x10, [x8, #0x10]
10081e6a8:      mov w11, #0x18              ; =24
10081e6ac:      madd    x10, x28, x11, x10
10081e6b0:      ldr x11, [x10]
10081e6b4:      cmp x11, #0x1
10081e6b8:      b.ne    0x10081eb48 <_js_object_alloc_with_shape+0xe88>
10081e6bc:      ldr x20, [x10, #0x8]
10081e6c0:      str x9, [x8]
10081e6c4:      cbnz    w26, 0x10081e6dc <_js_object_alloc_with_shape+0xa1c>
10081e6c8:      b   0x10081e6f4 <_js_object_alloc_with_shape+0xa34>
10081e6cc:      mov x0, x28
10081e6d0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e6d4:      mov x20, x0
10081e6d8:      cbz w26, 0x10081e6f4 <_js_object_alloc_with_shape+0xa34>
10081e6dc:      mov x0, x20
10081e6e0:      mov x1, x26
10081e6e4:      mov x2, x25
10081e6e8:      mov x3, x19
10081e6ec:      bl  0x10059c13c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34try_birth_stamp_preinstalled_shape>
10081e6f0:      tbnz    w0, #0x0, 0x10081ea70 <_js_object_alloc_with_shape+0xdb0>
10081e6f4:      ldr w21, [x20, #0x4]
10081e6f8:      ldr w22, [x24, #0x634]
10081e6fc:      cmp w22, #0x300
10081e700:      b.hs    0x10081e7d0 <_js_object_alloc_with_shape+0xb10>
10081e704:      ldr x8, [x27, #0x48]
10081e708:      cmn x8, #0x1
10081e70c:      b.eq    0x10081e7c0 <_js_object_alloc_with_shape+0xb00>
10081e710:      mrs x9, TPIDRRO_EL0
10081e714:      and x9, x9, #0xfffffffffffffff8
10081e718:      ldr x0, [x9, x8, lsl #3]
10081e71c:      cbz x0, 0x10081e7c0 <_js_object_alloc_with_shape+0xb00>
10081e720:      add x8, x0, x22, lsl #3
10081e724:      ldr x0, [x8, #0x1e8]
10081e728:      cbz x0, 0x10081e7d0 <_js_object_alloc_with_shape+0xb10>
10081e72c:      ldr x0, [x0]
10081e730:      cbz x0, 0x10081e7e4 <_js_object_alloc_with_shape+0xb24>
10081e734:      mov w8, #-0x40000001        ; =-1073741825
10081e738:      cmp w21, w8
10081e73c:      b.gt    0x10081e7f4 <_js_object_alloc_with_shape+0xb34>
10081e740:      ldr x9, [x0, #0x5198]
10081e744:      ubfx    x8, x21, #15, #15
10081e748:      cmp x8, x9
10081e74c:      b.hs    0x10081e7f4 <_js_object_alloc_with_shape+0xb34>
10081e750:      ldr x9, [x0, #0x5190]
10081e754:      ldr x8, [x9, x8, lsl #3]
10081e758:      cbz x8, 0x10081e7f4 <_js_object_alloc_with_shape+0xb34>
10081e75c:      ubfx    x9, x21, #5, #10
10081e760:      ldr x8, [x8, x9, lsl #3]
10081e764:      cbz x8, 0x10081e7f4 <_js_object_alloc_with_shape+0xb34>
10081e768:      and x9, x21, #0x1f
10081e76c:      add x8, x8, x9, lsl #5
10081e770:      ldrb    w9, [x8, #0x1c]
10081e774:      tbz w9, #0x0, 0x10081e7f4 <_js_object_alloc_with_shape+0xb34>
10081e778:      mov w10, #0x90              ; =144
10081e77c:      tst w9, w10
10081e780:      cset    w10, ne
10081e784:      ldp x11, x12, [x8]
10081e788:      ubfx    w13, w9, #5, #1
10081e78c:      ldr w14, [x8, #0x18]
10081e790:      ubfx    w9, w9, #2, #1
10081e794:      stp x11, x8, [sp, #0x28]
10081e798:      str x12, [sp, #0x38]
10081e79c:      ldr d0, [x8, #0x10]
10081e7a0:      str d0, [sp, #0x40]
10081e7a4:      str w14, [sp, #0x48]
10081e7a8:      strb    w9, [sp, #0x4c]
10081e7ac:      strb    w10, [sp, #0x4d]
10081e7b0:      strb    w13, [sp, #0x4e]
10081e7b4:      cmp x11, x25
10081e7b8:      b.ne    0x10081e800 <_js_object_alloc_with_shape+0xb40>
10081e7bc:      b   0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e7c0:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e7c4:      add x8, x0, x22, lsl #3
10081e7c8:      ldr x0, [x8, #0x1e8]
10081e7cc:      cbnz    x0, 0x10081e72c <_js_object_alloc_with_shape+0xa6c>
10081e7d0:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081e7d4:      add x0, x0, #0x360
10081e7d8:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081e7dc:      ldr x0, [x0]
10081e7e0:      cbnz    x0, 0x10081e734 <_js_object_alloc_with_shape+0xa74>
10081e7e4:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081e7e8:      mov w8, #-0x40000001        ; =-1073741825
10081e7ec:      cmp w21, w8
10081e7f0:      b.le    0x10081e740 <_js_object_alloc_with_shape+0xa80>
10081e7f4:      mov w8, #0x2                ; =2
10081e7f8:      strb    w8, [sp, #0x4e]
10081e7fc:      cbz x25, 0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e800:      lsr x8, x20, #3
10081e804:      cmp x8, #0x201
10081e808:      b.lo    0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e80c:      ldursh  w8, [x20, #-0x6]
10081e810:      tst w8, #0x1000
10081e814:      mov w9, #-0x4001            ; =-16385
10081e818:      ccmp    w8, w9, #0x4, eq
10081e81c:      b.gt    0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e820:      lsr x9, x20, #3
10081e824:      cmp x9, #0x201
10081e828:      b.lo    0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e82c:      ldurb   w9, [x20, #-0x8]
10081e830:      sub w10, w9, #0x15
10081e834:      cmn w10, #0x14
10081e838:      b.lo    0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e83c:      and w8, w8, #0xffff
10081e840:      adrp    x10, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081e844:      add x10, x10, #0xb40
10081e848:      add x9, x10, x9, lsl #5
10081e84c:      ldrb    w9, [x9, #0x14]
10081e850:      mov w10, #0x16              ; =22
10081e854:      lsr w9, w10, w9
10081e858:      tbz w9, #0x0, 0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e85c:      and w9, w8, #0xffffefff
10081e860:      sturh   w9, [x20, #-0x6]
10081e864:      ands    w21, w8, #0xc000
10081e868:      b.eq    0x10081ea44 <_js_object_alloc_with_shape+0xd84>
10081e86c:      and w8, w8, #0xfff
10081e870:      sturh   w8, [x20, #-0x6]
10081e874:      mov x0, x20
10081e878:      bl  0x1004143e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20typed_layouts_remove>
10081e87c:      cmp w21, #0x4, lsl #12      ; =0x4000
10081e880:      b.eq    0x10081e88c <_js_object_alloc_with_shape+0xbcc>
10081e884:      mov x0, x20
10081e888:      bl  0x100413ae0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables17slot_masks_remove>
10081e88c:      mov x0, x20
10081e890:      bl  0x1002b6f40 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081e894:      b   0x10081ea4c <_js_object_alloc_with_shape+0xd8c>
10081e898:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e89c:      ldr x8, [x0, #0x28]
10081e8a0:      ldrb    w8, [x8]
10081e8a4:      tbnz    w8, #0x0, 0x10081dd60 <_js_object_alloc_with_shape+0xa0>
10081e8a8:      b   0x10081ddc8 <_js_object_alloc_with_shape+0x108>
10081e8ac:      mov x24, x0
10081e8b0:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e8b4:      mov x8, x0
10081e8b8:      mov x0, x24
10081e8bc:      b   0x10081ddf4 <_js_object_alloc_with_shape+0x134>
10081e8c0:      mov x24, x0
10081e8c4:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e8c8:      mov x8, x0
10081e8cc:      mov x0, x24
10081e8d0:      ldr x8, [x8, #0x30]
10081e8d4:      ldrb    w8, [x8]
10081e8d8:      tbnz    w8, #0x0, 0x10081de2c <_js_object_alloc_with_shape+0x16c>
10081e8dc:      b   0x10081de94 <_js_object_alloc_with_shape+0x1d4>
10081e8e0:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e8e4:      add x8, x0, x21, lsl #3
10081e8e8:      ldr x0, [x8, #0x1e8]
10081e8ec:      cbnz    x0, 0x10081e620 <_js_object_alloc_with_shape+0x960>
10081e8f0:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081e8f4:      add x0, x0, #0x360
10081e8f8:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081e8fc:      ldr x0, [x0]
10081e900:      cbnz    x0, 0x10081e628 <_js_object_alloc_with_shape+0x968>
10081e904:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081e908:      ldr x8, [sp, #0x8]
10081e90c:      add x8, x0, x8, lsl #4
10081e910:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081e914:      ldr w9, [x8]
10081e918:      cmp w9, w20
10081e91c:      b.eq    0x10081e640 <_js_object_alloc_with_shape+0x980>
10081e920:      ldr x8, [x0, #0x5088]
10081e924:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081e928:      cmp x8, x9
10081e92c:      b.hs    0x10081eb58 <_js_object_alloc_with_shape+0xe98>
10081e930:      add x9, x8, #0x1
10081e934:      str x9, [x0, #0x5088]
10081e938:      ldr x9, [x0, #0x50a8]
10081e93c:      cbz x9, 0x10081e9f0 <_js_object_alloc_with_shape+0xd30>
10081e940:      mov x9, #0x0                ; =0
10081e944:      mov w10, w20
10081e948:      mov x11, #0x7c15            ; =31765
10081e94c:      movk    x11, #0x7f4a, lsl #16
10081e950:      movk    x11, #0x79b9, lsl #32
10081e954:      movk    x11, #0x9e37, lsl #48
10081e958:      mul x10, x10, x11
10081e95c:      eor x13, x10, x10, lsr #32
10081e960:      lsr x12, x10, #57
10081e964:      ldr x10, [x0, #0x5098]
10081e968:      ldr x11, [x0, #0x5090]
10081e96c:      dup.8b  v0, w12
10081e970:      movi.2d v1, #0xffffffffffffffff
10081e974:      mov w12, #0x18              ; =24
10081e978:      and x13, x13, x10
10081e97c:      ldr d2, [x11, x13]
10081e980:      cmeq.8b v3, v2, v0
10081e984:      fmov    x14, d3
10081e988:      ands    x14, x14, #0x8080808080808080
10081e98c:      b.eq    0x10081e9c0 <_js_object_alloc_with_shape+0xd00>
10081e990:      rbit    x15, x14
10081e994:      clz x15, x15
10081e998:      add x15, x13, x15, lsr #3
10081e99c:      and x15, x15, x10
10081e9a0:      mneg    x15, x15, x12
10081e9a4:      add x15, x11, x15
10081e9a8:      ldur    w16, [x15, #-0x18]
10081e9ac:      cmp w20, w16
10081e9b0:      b.eq    0x10081ea0c <_js_object_alloc_with_shape+0xd4c>
10081e9b4:      sub x15, x14, #0x2
10081e9b8:      ands    x14, x15, x14
10081e9bc:      b.ne    0x10081e990 <_js_object_alloc_with_shape+0xcd0>
10081e9c0:      cmeq.8b v2, v2, v1
10081e9c4:      fmov    x14, d2
10081e9c8:      cbnz    x14, 0x10081e9f0 <_js_object_alloc_with_shape+0xd30>
10081e9cc:      add x9, x9, #0x8
10081e9d0:      add x13, x13, x9
10081e9d4:      and x13, x13, x10
10081e9d8:      ldr d2, [x11, x13]
10081e9dc:      cmeq.8b v3, v2, v0
10081e9e0:      fmov    x14, d3
10081e9e4:      ands    x14, x14, #0x8080808080808080
10081e9e8:      b.ne    0x10081e990 <_js_object_alloc_with_shape+0xcd0>
10081e9ec:      b   0x10081e9c0 <_js_object_alloc_with_shape+0xd00>
10081e9f0:      mov w26, #0x0               ; =0
10081e9f4:      str x8, [x0, #0x5088]
10081e9f8:      add x0, sp, #0x20
10081e9fc:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081ea00:      ldr w8, [sp]
10081ea04:      tbz w8, #0x0, 0x10081e654 <_js_object_alloc_with_shape+0x994>
10081ea08:      b   0x10081e65c <_js_object_alloc_with_shape+0x99c>
10081ea0c:      ldur    w26, [x15, #-0x8]
10081ea10:      str x8, [x0, #0x5088]
10081ea14:      add x0, sp, #0x20
10081ea18:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081ea1c:      ldr w8, [sp]
10081ea20:      tbz w8, #0x0, 0x10081e654 <_js_object_alloc_with_shape+0x994>
10081ea24:      b   0x10081e65c <_js_object_alloc_with_shape+0x99c>
10081ea28:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081ea2c:      ldr x25, [x0, #0x20]
10081ea30:      ldr x8, [x25]
10081ea34:      cbz x8, 0x10081dd88 <_js_object_alloc_with_shape+0xc8>
10081ea38:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
10081ea3c:      add x0, x0, #0x150
10081ea40:      bl  0x100ab14ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081ea44:      mov x0, x20
10081ea48:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081ea4c:      add x1, sp, #0x28
10081ea50:      mov x0, x20
10081ea54:      mov x2, x25
10081ea58:      mov x3, x19
10081ea5c:      bl  0x10059896c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25publish_object_shape_from>
10081ea60:      mov x0, x20
10081ea64:      mov x1, x26
10081ea68:      mov x2, x19
10081ea6c:      bl  0x1005981c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081ea70:      ldr x8, [x27, #0x48]
10081ea74:      cmn x8, #0x1
10081ea78:      b.eq    0x10081eadc <_js_object_alloc_with_shape+0xe1c>
10081ea7c:      mrs x9, TPIDRRO_EL0
10081ea80:      and x9, x9, #0xfffffffffffffff8
10081ea84:      ldr x8, [x9, x8, lsl #3]
10081ea88:      cbz x8, 0x10081eadc <_js_object_alloc_with_shape+0xe1c>
10081ea8c:      ldr x8, [x8, #0x19e8]
10081ea90:      cbz x8, 0x10081eadc <_js_object_alloc_with_shape+0xe1c>
10081ea94:      ldr x9, [x8]
10081ea98:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081ea9c:      cmp x9, x10
10081eaa0:      b.hs    0x10081eb38 <_js_object_alloc_with_shape+0xe78>
10081eaa4:      add x10, x9, #0x1
10081eaa8:      str x10, [x8]
10081eaac:      ldr x10, [x8, #0x18]
10081eab0:      cmp x28, x10
10081eab4:      b.hs    0x10081eb44 <_js_object_alloc_with_shape+0xe84>
10081eab8:      ldr x10, [x8, #0x10]
10081eabc:      mov w11, #0x18              ; =24
10081eac0:      madd    x10, x28, x11, x10
10081eac4:      ldr x11, [x10]
10081eac8:      cmp x11, #0x1
10081eacc:      b.ne    0x10081eb48 <_js_object_alloc_with_shape+0xe88>
10081ead0:      ldr x19, [x10, #0x8]
10081ead4:      str x9, [x8]
10081ead8:      b   0x10081eae8 <_js_object_alloc_with_shape+0xe28>
10081eadc:      mov x0, x28
10081eae0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081eae4:      mov x19, x0
10081eae8:      add x0, sp, #0x18
10081eaec:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081eaf0:      mov x0, x19
10081eaf4:      ldp x29, x30, [sp, #0xa0]
10081eaf8:      ldp x20, x19, [sp, #0x90]
10081eafc:      ldp x22, x21, [sp, #0x80]
10081eb00:      ldp x24, x23, [sp, #0x70]
10081eb04:      ldp x26, x25, [sp, #0x60]
10081eb08:      ldp x28, x27, [sp, #0x50]
10081eb0c:      add sp, sp, #0xb0
10081eb10:      ret
10081eb14:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081eb18:      b   0x10081e018 <_js_object_alloc_with_shape+0x358>
10081eb1c:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081eb20:      ldr x8, [x0, #0x30]
10081eb24:      ldrb    w8, [x8]
10081eb28:      tbnz    w8, #0x0, 0x10081e050 <_js_object_alloc_with_shape+0x390>
10081eb2c:      b   0x10081e0ac <_js_object_alloc_with_shape+0x3ec>
10081eb30:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081eb34:      b   0x10081dfdc <_js_object_alloc_with_shape+0x31c>
10081eb38:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
10081eb3c:      add x0, x0, #0x8a8
10081eb40:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081eb44:      bl  0x100ae2dc8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
10081eb48:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0x9b0>
10081eb4c:      add x0, x0, #0x2ac
10081eb50:      mov w1, #0xb                ; =11
10081eb54:      bl  0x100ae2d90 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10081eb58:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081eb5c:      add x0, x0, #0x7c8
10081eb60:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081eb64:      bl  0x100ab11bc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081eb68:      cmp w8, #0x1
10081eb6c:      b.ne    0x10081ebb0 <_js_object_alloc_with_shape+0xef0>
10081eb70:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081eb74:      add x1, x1, #0xbb8
10081eb78:      mov x0, x24
10081eb7c:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081eb80:      strb    wzr, [x24, #0x18]
10081eb84:      ldr x28, [x24, #0x10]
10081eb88:      ldr x8, [x24]
10081eb8c:      cmp x28, x8
10081eb90:      mov x0, x25
10081eb94:      b.ne    0x10081de84 <_js_object_alloc_with_shape+0x1c4>
10081eb98:      mov x0, x24
10081eb9c:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081eba0:      mov x0, x25
10081eba4:      b   0x10081de84 <_js_object_alloc_with_shape+0x1c4>
10081eba8:      cmp w8, #0x2
10081ebac:      b.ne    0x10081ebbc <_js_object_alloc_with_shape+0xefc>
10081ebb0:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081ebb4:      add x0, x0, #0x850
10081ebb8:      bl  0x100aef91c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081ebbc:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081ebc0:      add x1, x1, #0xbb8
10081ebc4:      mov x25, x0
10081ebc8:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081ebcc:      mov x0, x25
10081ebd0:      strb    wzr, [x25, #0x18]
10081ebd4:      ldr x25, [x25, #0x10]
10081ebd8:      ldr x8, [x0]
10081ebdc:      cmp x25, x8
10081ebe0:      b.ne    0x10081e09c <_js_object_alloc_with_shape+0x3dc>
10081ebe4:      str x0, [sp, #0x10]
10081ebe8:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081ebec:      ldr x0, [sp, #0x10]
10081ebf0:      b   0x10081e09c <_js_object_alloc_with_shape+0x3dc>
10081ebf4:      mov w0, #0x8                ; =8
10081ebf8:      mov w1, #0x40               ; =64
10081ebfc:      bl  0x100ab1164 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
