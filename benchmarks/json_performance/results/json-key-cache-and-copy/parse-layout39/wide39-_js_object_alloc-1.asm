/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081ccc0 <_js_object_alloc_class_with_keys>:
10081ccc0:      sub sp, sp, #0x80
10081ccc4:      stp x28, x27, [sp, #0x20]
10081ccc8:      stp x26, x25, [sp, #0x30]
10081cccc:      stp x24, x23, [sp, #0x40]
10081ccd0:      stp x22, x21, [sp, #0x50]
10081ccd4:      stp x20, x19, [sp, #0x60]
10081ccd8:      stp x29, x30, [sp, #0x70]
10081ccdc:      add x29, sp, #0x70
10081cce0:      mov x24, x4
10081cce4:      mov x22, x3
10081cce8:      mov x19, x2
10081ccec:      mov x23, x1
10081ccf0:      mov x20, x0
10081ccf4:      cbz w1, 0x10081cd04 <_js_object_alloc_class_with_keys+0x44>
10081ccf8:      mov x0, x20
10081ccfc:      mov x1, x23
10081cd00:      bl  0x1006d1c30 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry13parent_static14register_class>
10081cd04:      mov w8, #0x2                ; =2
10081cd08:      cmp w19, #0x2
10081cd0c:      csel    w8, w19, w8, hi
10081cd10:      ubfiz   x8, x8, #3, #32
10081cd14:      mov w9, #0x3ffd             ; =16381
10081cd18:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081cd1c:      cmp w19, w9
10081cd20:      b.ls    0x10081cd5c <_js_object_alloc_class_with_keys+0x9c>
10081cd24:      add x0, x8, #0x10
10081cd28:      mov w1, #0x8                ; =8
10081cd2c:      mov w2, #0x2                ; =2
10081cd30:      bl  0x1004da858 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081cd34:      mov x21, x0
10081cd38:      ldurb   w8, [x0, #-0x7]
10081cd3c:      orr w8, w8, #0x20
10081cd40:      sturb   w8, [x0, #-0x7]
10081cd44:      stp w20, w23, [x0]
10081cd48:      str xzr, [x0, #0x8]
10081cd4c:      lsr x8, x0, #3
10081cd50:      cmp x8, #0x201
10081cd54:      b.hs    0x10081cedc <_js_object_alloc_class_with_keys+0x21c>
10081cd58:      b   0x10081d060 <_js_object_alloc_class_with_keys+0x3a0>
10081cd5c:      add x25, x8, #0x18
10081cd60:      ldr x8, [x28, #0x48]
10081cd64:      cmn x8, #0x1
10081cd68:      b.eq    0x10081d564 <_js_object_alloc_class_with_keys+0x8a4>
10081cd6c:      mrs x9, TPIDRRO_EL0
10081cd70:      and x9, x9, #0xfffffffffffffff8
10081cd74:      ldr x0, [x9, x8, lsl #3]
10081cd78:      cbz x0, 0x10081d564 <_js_object_alloc_class_with_keys+0x8a4>
10081cd7c:      ldr x8, [x0, #0x28]
10081cd80:      ldrb    w8, [x8]
10081cd84:      tbz w8, #0x0, 0x10081cdf0 <_js_object_alloc_class_with_keys+0x130>
10081cd88:      ldr x8, [x28, #0x48]
10081cd8c:      cmn x8, #0x1
10081cd90:      b.eq    0x10081d6d8 <_js_object_alloc_class_with_keys+0xa18>
10081cd94:      mrs x9, TPIDRRO_EL0
10081cd98:      and x9, x9, #0xfffffffffffffff8
10081cd9c:      ldr x0, [x9, x8, lsl #3]
10081cda0:      cbz x0, 0x10081d6d8 <_js_object_alloc_class_with_keys+0xa18>
10081cda4:      ldr x26, [x0, #0x20]
10081cda8:      ldr x8, [x26]
10081cdac:      cbnz    x8, 0x10081d6e8 <_js_object_alloc_class_with_keys+0xa28>
10081cdb0:      mov x8, #-0x1               ; =-1
10081cdb4:      str x8, [x26]
10081cdb8:      ldr x1, [x26, #0x18]
10081cdbc:      cbz x1, 0x10081cdec <_js_object_alloc_class_with_keys+0x12c>
10081cdc0:      mov x0, #0x0                ; =0
10081cdc4:      ldr x8, [x26, #0x10]
10081cdc8:      lsl x10, x1, #4
10081cdcc:      mov x9, x8
10081cdd0:      ldr x11, [x9, #0x8]
10081cdd4:      cmp x11, x25
10081cdd8:      b.eq    0x10081cf30 <_js_object_alloc_class_with_keys+0x270>
10081cddc:      add x0, x0, #0x1
10081cde0:      add x9, x9, #0x10
10081cde4:      subs    x10, x10, #0x10
10081cde8:      b.ne    0x10081cdd0 <_js_object_alloc_class_with_keys+0x110>
10081cdec:      str xzr, [x26]
10081cdf0:      mov x0, x25
10081cdf4:      bl  0x1004da49c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081cdf8:      mov w8, #0x2                ; =2
10081cdfc:      strb    w8, [x0]
10081ce00:      ldr x8, [x28, #0x48]
10081ce04:      cmn x8, #0x1
10081ce08:      b.eq    0x10081d578 <_js_object_alloc_class_with_keys+0x8b8>
10081ce0c:      mrs x9, TPIDRRO_EL0
10081ce10:      and x9, x9, #0xfffffffffffffff8
10081ce14:      ldr x8, [x9, x8, lsl #3]
10081ce18:      cbz x8, 0x10081d578 <_js_object_alloc_class_with_keys+0x8b8>
10081ce1c:      ldr x8, [x8, #0x30]
10081ce20:      ldrb    w8, [x8]
10081ce24:      orr w8, w8, #0x2
10081ce28:      strb    w8, [x0, #0x1]
10081ce2c:      ldr x8, [x28, #0x48]
10081ce30:      cmn x8, #0x1
10081ce34:      b.eq    0x10081d58c <_js_object_alloc_class_with_keys+0x8cc>
10081ce38:      mrs x9, TPIDRRO_EL0
10081ce3c:      and x9, x9, #0xfffffffffffffff8
10081ce40:      ldr x8, [x9, x8, lsl #3]
10081ce44:      cbz x8, 0x10081d58c <_js_object_alloc_class_with_keys+0x8cc>
10081ce48:      ldr x8, [x8, #0x30]
10081ce4c:      ldrb    w8, [x8]
10081ce50:      tbz w8, #0x0, 0x10081cebc <_js_object_alloc_class_with_keys+0x1fc>
10081ce54:      ldrb    w8, [x0]
10081ce58:      sub w9, w8, #0x15
10081ce5c:      cmn w9, #0x14
10081ce60:      b.lo    0x10081cebc <_js_object_alloc_class_with_keys+0x1fc>
10081ce64:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081ce68:      add x9, x9, #0xb40
10081ce6c:      add x8, x9, x8, lsl #5
10081ce70:      ldrb    w8, [x8, #0x1b]
10081ce74:      tbnz    w8, #0x0, 0x10081cebc <_js_object_alloc_class_with_keys+0x1fc>
10081ce78:      mov x26, x0
10081ce7c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081ce80:      add x0, x0, #0x658
10081ce84:      ldr x8, [x0]
10081ce88:      blr x8
10081ce8c:      mov x21, x0
10081ce90:      ldrb    w8, [x0, #0x18]
10081ce94:      cbnz    w8, 0x10081d728 <_js_object_alloc_class_with_keys+0xa68>
10081ce98:      ldr x27, [x21, #0x10]
10081ce9c:      ldr x8, [x21]
10081cea0:      cmp x27, x8
10081cea4:      mov x0, x26
10081cea8:      b.eq    0x10081d758 <_js_object_alloc_class_with_keys+0xa98>
10081ceac:      ldr x8, [x21, #0x8]
10081ceb0:      str x0, [x8, x27, lsl #3]
10081ceb4:      add x8, x27, #0x1
10081ceb8:      str x8, [x21, #0x10]
10081cebc:      strh    wzr, [x0, #0x2]
10081cec0:      str w25, [x0, #0x4]
10081cec4:      add x21, x0, #0x8
10081cec8:      stp w20, w23, [x21]
10081cecc:      str xzr, [x21, #0x8]
10081ced0:      lsr x8, x21, #3
10081ced4:      cmp x8, #0x201
10081ced8:      b.lo    0x10081d060 <_js_object_alloc_class_with_keys+0x3a0>
10081cedc:      ldurb   w8, [x21, #-0x8]
10081cee0:      sub w9, w8, #0x15
10081cee4:      cmn w9, #0x14
10081cee8:      b.lo    0x10081d060 <_js_object_alloc_class_with_keys+0x3a0>
10081ceec:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081cef0:      add x9, x9, #0xb40
10081cef4:      add x8, x9, x8, lsl #5
10081cef8:      ldrb    w8, [x8, #0x14]
10081cefc:      mov w9, #0x16               ; =22
10081cf00:      lsr w8, w9, w8
10081cf04:      tbz w8, #0x0, 0x10081d060 <_js_object_alloc_class_with_keys+0x3a0>
10081cf08:      ldurh   w8, [x21, #-0x6]
10081cf0c:      mov w9, #0x4000             ; =16384
10081cf10:      bfxil   w9, w8, #0, #13
10081cf14:      sturh   w9, [x21, #-0x6]
10081cf18:      mov x0, x21
10081cf1c:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081cf20:      ldurh   w8, [x21, #-0x6]
10081cf24:      and w8, w8, #0xffffefff
10081cf28:      sturh   w8, [x21, #-0x6]
10081cf2c:      b   0x10081d060 <_js_object_alloc_class_with_keys+0x3a0>
10081cf30:      cmp x0, x1
10081cf34:      b.hs    0x10081d724 <_js_object_alloc_class_with_keys+0xa64>
10081cf38:      ldr x21, [x9]
10081cf3c:      subs    x10, x1, #0x1
10081cf40:      ldr q0, [x8, x10, lsl #4]
10081cf44:      str q0, [x9]
10081cf48:      str x10, [x26, #0x18]
10081cf4c:      b.ne    0x10081cf74 <_js_object_alloc_class_with_keys+0x2b4>
10081cf50:      ldr x8, [x28, #0x48]
10081cf54:      cmn x8, #0x1
10081cf58:      b.eq    0x10081d710 <_js_object_alloc_class_with_keys+0xa50>
10081cf5c:      mrs x9, TPIDRRO_EL0
10081cf60:      and x9, x9, #0xfffffffffffffff8
10081cf64:      ldr x0, [x9, x8, lsl #3]
10081cf68:      cbz x0, 0x10081d710 <_js_object_alloc_class_with_keys+0xa50>
10081cf6c:      ldr x8, [x0, #0x28]
10081cf70:      strb    wzr, [x8]
10081cf74:      ldr x8, [x26]
10081cf78:      add x8, x8, #0x1
10081cf7c:      str x8, [x26]
10081cf80:      mov w8, #0x2                ; =2
10081cf84:      mov x27, x21
10081cf88:      strb    w8, [x27, #-0x8]!
10081cf8c:      ldr x8, [x28, #0x48]
10081cf90:      cmn x8, #0x1
10081cf94:      b.eq    0x10081d6f4 <_js_object_alloc_class_with_keys+0xa34>
10081cf98:      mrs x9, TPIDRRO_EL0
10081cf9c:      and x9, x9, #0xfffffffffffffff8
10081cfa0:      ldr x0, [x9, x8, lsl #3]
10081cfa4:      cbz x0, 0x10081d6f4 <_js_object_alloc_class_with_keys+0xa34>
10081cfa8:      ldr x8, [x0, #0x30]
10081cfac:      ldrb    w8, [x8]
10081cfb0:      orr w8, w8, #0x2
10081cfb4:      sturb   w8, [x21, #-0x7]
10081cfb8:      ldr x8, [x28, #0x48]
10081cfbc:      cmn x8, #0x1
10081cfc0:      b.eq    0x10081d6fc <_js_object_alloc_class_with_keys+0xa3c>
10081cfc4:      mrs x9, TPIDRRO_EL0
10081cfc8:      and x9, x9, #0xfffffffffffffff8
10081cfcc:      ldr x0, [x9, x8, lsl #3]
10081cfd0:      cbz x0, 0x10081d6fc <_js_object_alloc_class_with_keys+0xa3c>
10081cfd4:      ldr x8, [x0, #0x30]
10081cfd8:      ldrb    w8, [x8]
10081cfdc:      tbz w8, #0x0, 0x10081d044 <_js_object_alloc_class_with_keys+0x384>
10081cfe0:      ldrb    w8, [x27]
10081cfe4:      sub w9, w8, #0x15
10081cfe8:      cmn w9, #0x14
10081cfec:      b.lo    0x10081d044 <_js_object_alloc_class_with_keys+0x384>
10081cff0:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081cff4:      add x9, x9, #0xb40
10081cff8:      add x8, x9, x8, lsl #5
10081cffc:      ldrb    w8, [x8, #0x1b]
10081d000:      tbnz    w8, #0x0, 0x10081d044 <_js_object_alloc_class_with_keys+0x384>
10081d004:      mov x26, x28
10081d008:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d00c:      add x0, x0, #0x658
10081d010:      ldr x8, [x0]
10081d014:      blr x8
10081d018:      ldrb    w8, [x0, #0x18]
10081d01c:      cbnz    w8, 0x10081d768 <_js_object_alloc_class_with_keys+0xaa8>
10081d020:      ldr x28, [x0, #0x10]
10081d024:      ldr x8, [x0]
10081d028:      cmp x28, x8
10081d02c:      b.eq    0x10081d7a4 <_js_object_alloc_class_with_keys+0xae4>
10081d030:      ldr x8, [x0, #0x8]
10081d034:      str x27, [x8, x28, lsl #3]
10081d038:      add x8, x28, #0x1
10081d03c:      str x8, [x0, #0x10]
10081d040:      mov x28, x26
10081d044:      sturh   wzr, [x21, #-0x6]
10081d048:      stur    w25, [x21, #-0x4]
10081d04c:      stp w20, w23, [x21]
10081d050:      str xzr, [x21, #0x8]
10081d054:      lsr x8, x21, #3
10081d058:      cmp x8, #0x201
10081d05c:      b.hs    0x10081cedc <_js_object_alloc_class_with_keys+0x21c>
10081d060:      mov w8, #0x2717             ; =10007
10081d064:      mov w9, #0x86a3             ; =34467
10081d068:      movk    w9, #0x1, lsl #16
10081d06c:      mul w9, w19, w9
10081d070:      madd    w23, w20, w8, w9
10081d074:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d078:      ldr w25, [x8, #0x634]
10081d07c:      cmp w25, #0x300
10081d080:      b.hs    0x10081d0f4 <_js_object_alloc_class_with_keys+0x434>
10081d084:      ldr x8, [x28, #0x48]
10081d088:      cmn x8, #0x1
10081d08c:      b.eq    0x10081d0e4 <_js_object_alloc_class_with_keys+0x424>
10081d090:      mrs x9, TPIDRRO_EL0
10081d094:      and x9, x9, #0xfffffffffffffff8
10081d098:      ldr x0, [x9, x8, lsl #3]
10081d09c:      cbz x0, 0x10081d0e4 <_js_object_alloc_class_with_keys+0x424>
10081d0a0:      add x8, x0, x25, lsl #3
10081d0a4:      ldr x0, [x8, #0x1e8]
10081d0a8:      cbz x0, 0x10081d0f4 <_js_object_alloc_class_with_keys+0x434>
10081d0ac:      add w8, w23, #0xf4, lsl #12 ; =0xf4000
10081d0b0:      add w23, w8, #0x240
10081d0b4:      ldr x0, [x0]
10081d0b8:      cbz x0, 0x10081d110 <_js_object_alloc_class_with_keys+0x450>
10081d0bc:      and w8, w23, #0xff
10081d0c0:      add x8, x0, w8, uxtw #4
10081d0c4:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d0c8:      ldr w9, [x8]
10081d0cc:      cmp w9, w23
10081d0d0:      b.ne    0x10081d12c <_js_object_alloc_class_with_keys+0x46c>
10081d0d4:      ldr x26, [x8, #0x8]
10081d0d8:      cbz x26, 0x10081d21c <_js_object_alloc_class_with_keys+0x55c>
10081d0dc:      ldr w27, [x8, #0x4]
10081d0e0:      b   0x10081d510 <_js_object_alloc_class_with_keys+0x850>
10081d0e4:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d0e8:      add x8, x0, x25, lsl #3
10081d0ec:      ldr x0, [x8, #0x1e8]
10081d0f0:      cbnz    x0, 0x10081d0ac <_js_object_alloc_class_with_keys+0x3ec>
10081d0f4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081d0f8:      add x0, x0, #0x360
10081d0fc:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081d100:      add w8, w23, #0xf4, lsl #12 ; =0xf4000
10081d104:      add w23, w8, #0x240
10081d108:      ldr x0, [x0]
10081d10c:      cbnz    x0, 0x10081d0bc <_js_object_alloc_class_with_keys+0x3fc>
10081d110:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081d114:      and w8, w23, #0xff
10081d118:      add x8, x0, w8, uxtw #4
10081d11c:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d120:      ldr w9, [x8]
10081d124:      cmp w9, w23
10081d128:      b.eq    0x10081d0d4 <_js_object_alloc_class_with_keys+0x414>
10081d12c:      ldr x8, [x0, #0x5088]
10081d130:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081d134:      cmp x8, x9
10081d138:      b.hs    0x10081d718 <_js_object_alloc_class_with_keys+0xa58>
10081d13c:      add x9, x8, #0x1
10081d140:      str x9, [x0, #0x5088]
10081d144:      ldr x9, [x0, #0x50a8]
10081d148:      cbz x9, 0x10081d1f8 <_js_object_alloc_class_with_keys+0x538>
10081d14c:      mov x9, #0x0                ; =0
10081d150:      mov x10, #0x7c15            ; =31765
10081d154:      movk    x10, #0x7f4a, lsl #16
10081d158:      movk    x10, #0x79b9, lsl #32
10081d15c:      movk    x10, #0x9e37, lsl #48
10081d160:      mul x10, x23, x10
10081d164:      eor x13, x10, x10, lsr #32
10081d168:      lsr x12, x10, #57
10081d16c:      ldr x10, [x0, #0x5098]
10081d170:      ldr x11, [x0, #0x5090]
10081d174:      dup.8b  v0, w12
10081d178:      movi.2d v1, #0xffffffffffffffff
10081d17c:      mov w12, #0x18              ; =24
10081d180:      and x13, x13, x10
10081d184:      ldr d2, [x11, x13]
10081d188:      cmeq.8b v3, v2, v0
10081d18c:      fmov    x14, d3
10081d190:      ands    x14, x14, #0x8080808080808080
10081d194:      b.eq    0x10081d1c8 <_js_object_alloc_class_with_keys+0x508>
10081d198:      rbit    x15, x14
10081d19c:      clz x15, x15
10081d1a0:      add x15, x13, x15, lsr #3
10081d1a4:      and x15, x15, x10
10081d1a8:      mneg    x15, x15, x12
10081d1ac:      add x15, x11, x15
10081d1b0:      ldur    w16, [x15, #-0x18]
10081d1b4:      cmp w23, w16
10081d1b8:      b.eq    0x10081d20c <_js_object_alloc_class_with_keys+0x54c>
10081d1bc:      sub x15, x14, #0x2
10081d1c0:      ands    x14, x15, x14
10081d1c4:      b.ne    0x10081d198 <_js_object_alloc_class_with_keys+0x4d8>
10081d1c8:      cmeq.8b v2, v2, v1
10081d1cc:      fmov    x14, d2
10081d1d0:      cbnz    x14, 0x10081d1f8 <_js_object_alloc_class_with_keys+0x538>
10081d1d4:      add x9, x9, #0x8
10081d1d8:      add x13, x13, x9
10081d1dc:      and x13, x13, x10
10081d1e0:      ldr d2, [x11, x13]
10081d1e4:      cmeq.8b v3, v2, v0
10081d1e8:      fmov    x14, d3
10081d1ec:      ands    x14, x14, #0x8080808080808080
10081d1f0:      b.ne    0x10081d198 <_js_object_alloc_class_with_keys+0x4d8>
10081d1f4:      b   0x10081d1c8 <_js_object_alloc_class_with_keys+0x508>
10081d1f8:      mov w27, #0x0               ; =0
10081d1fc:      mov x26, #0x0               ; =0
10081d200:      str x8, [x0, #0x5088]
10081d204:      cbnz    x26, 0x10081d510 <_js_object_alloc_class_with_keys+0x850>
10081d208:      b   0x10081d21c <_js_object_alloc_class_with_keys+0x55c>
10081d20c:      ldur    x26, [x15, #-0x10]
10081d210:      ldur    w27, [x15, #-0x8]
10081d214:      str x8, [x0, #0x5088]
10081d218:      cbnz    x26, 0x10081d510 <_js_object_alloc_class_with_keys+0x850>
10081d21c:      mov w26, #0x0               ; =0
10081d220:      mov w8, #0x0                ; =0
10081d224:      mov w9, #0x8                ; =8
10081d228:      str x9, [sp]
10081d22c:      mov w27, w24
10081d230:      b   0x10081d244 <_js_object_alloc_class_with_keys+0x584>
10081d234:      mov w26, #0x1               ; =1
10081d238:      mov x22, x24
10081d23c:      mov w8, #0x1                ; =1
10081d240:      cbnz    x28, 0x10081d29c <_js_object_alloc_class_with_keys+0x5dc>
10081d244:      tbnz    w8, #0x0, 0x10081d290 <_js_object_alloc_class_with_keys+0x5d0>
10081d248:      mov x24, x22
10081d24c:      mov x28, #0x0               ; =0
10081d250:      cbz x27, 0x10081d234 <_js_object_alloc_class_with_keys+0x574>
10081d254:      ldrb    w8, [x24, x28]
10081d258:      cbz w8, 0x10081d27c <_js_object_alloc_class_with_keys+0x5bc>
10081d25c:      add x28, x28, #0x1
10081d260:      cmp x27, x28
10081d264:      b.ne    0x10081d254 <_js_object_alloc_class_with_keys+0x594>
10081d268:      mov w26, #0x1               ; =1
10081d26c:      mov x22, x24
10081d270:      mov w8, #0x1                ; =1
10081d274:      mov x28, x27
10081d278:      b   0x10081d240 <_js_object_alloc_class_with_keys+0x580>
10081d27c:      mvn x9, x28
10081d280:      add x27, x9, x27
10081d284:      add x9, x24, x28
10081d288:      add x22, x9, #0x1
10081d28c:      b   0x10081d240 <_js_object_alloc_class_with_keys+0x580>
10081d290:      mov x24, #0x0               ; =0
10081d294:      mov w25, #0x1               ; =1
10081d298:      b   0x10081d398 <_js_object_alloc_class_with_keys+0x6d8>
10081d29c:      cbz x24, 0x10081d38c <_js_object_alloc_class_with_keys+0x6cc>
10081d2a0:      mov w0, #0x40               ; =64
10081d2a4:      mov w1, #0x8                ; =8
10081d2a8:      bl  0x100af6548 <_mi_malloc_aligned>
10081d2ac:      cbz x0, 0x10081d7b4 <_js_object_alloc_class_with_keys+0xaf4>
10081d2b0:      stp x24, x28, [x0]
10081d2b4:      mov w8, #0x4                ; =4
10081d2b8:      stp x8, x0, [sp, #0x8]
10081d2bc:      mov w24, #0x1               ; =1
10081d2c0:      str x24, [sp, #0x18]
10081d2c4:      mov x8, x27
10081d2c8:      mov x10, x22
10081d2cc:      mov x9, x26
10081d2d0:      b   0x10081d2e4 <_js_object_alloc_class_with_keys+0x624>
10081d2d4:      mov w26, #0x1               ; =1
10081d2d8:      mov x10, x25
10081d2dc:      mov w9, #0x1                ; =1
10081d2e0:      cbnz    x28, 0x10081d338 <_js_object_alloc_class_with_keys+0x678>
10081d2e4:      tbnz    w9, #0x0, 0x10081d378 <_js_object_alloc_class_with_keys+0x6b8>
10081d2e8:      mov x25, x10
10081d2ec:      mov x28, #0x0               ; =0
10081d2f0:      cbz x8, 0x10081d2d4 <_js_object_alloc_class_with_keys+0x614>
10081d2f4:      ldrb    w9, [x25, x28]
10081d2f8:      cbz w9, 0x10081d31c <_js_object_alloc_class_with_keys+0x65c>
10081d2fc:      add x28, x28, #0x1
10081d300:      cmp x8, x28
10081d304:      b.ne    0x10081d2f4 <_js_object_alloc_class_with_keys+0x634>
10081d308:      mov w26, #0x1               ; =1
10081d30c:      mov x10, x25
10081d310:      mov w9, #0x1                ; =1
10081d314:      mov x28, x8
10081d318:      b   0x10081d2e0 <_js_object_alloc_class_with_keys+0x620>
10081d31c:      mvn x10, x28
10081d320:      add x27, x10, x8
10081d324:      add x8, x25, x28
10081d328:      add x22, x8, #0x1
10081d32c:      mov x8, x27
10081d330:      mov x10, x22
10081d334:      b   0x10081d2e0 <_js_object_alloc_class_with_keys+0x620>
10081d338:      cbz x25, 0x10081d378 <_js_object_alloc_class_with_keys+0x6b8>
10081d33c:      ldr x8, [sp, #0x8]
10081d340:      cmp x24, x8
10081d344:      b.eq    0x10081d358 <_js_object_alloc_class_with_keys+0x698>
10081d348:      add x8, x0, x24, lsl #4
10081d34c:      stp x25, x28, [x8]
10081d350:      add x24, x24, #0x1
10081d354:      b   0x10081d2c0 <_js_object_alloc_class_with_keys+0x600>
10081d358:      add x0, sp, #0x8
10081d35c:      mov x1, x24
10081d360:      mov w2, #0x1                ; =1
10081d364:      mov w3, #0x8                ; =8
10081d368:      mov w4, #0x10               ; =16
10081d36c:      bl  0x100ad5bd0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081d370:      ldr x0, [sp, #0x10]
10081d374:      b   0x10081d348 <_js_object_alloc_class_with_keys+0x688>
10081d378:      ldp x8, x9, [sp, #0x8]
10081d37c:      str x9, [sp]
10081d380:      cmp x8, #0x0
10081d384:      cset    w25, eq
10081d388:      b   0x10081d398 <_js_object_alloc_class_with_keys+0x6d8>
10081d38c:      mov w25, #0x1               ; =1
10081d390:      mov w8, #0x8                ; =8
10081d394:      str x8, [sp]
10081d398:      ubfiz   x8, x24, #3, #32
10081d39c:      add x0, x8, #0x8
10081d3a0:      mov w1, #0x8                ; =8
10081d3a4:      mov w2, #0x1                ; =1
10081d3a8:      bl  0x1004db2e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081d3ac:      mov x26, x0
10081d3b0:      stp w24, w24, [x0]
10081d3b4:      lsr x8, x0, #3
10081d3b8:      cmp x8, #0x201
10081d3bc:      b.lo    0x10081d44c <_js_object_alloc_class_with_keys+0x78c>
10081d3c0:      ldurb   w8, [x26, #-0x8]
10081d3c4:      cmp w8, #0x1
10081d3c8:      b.ne    0x10081d3f4 <_js_object_alloc_class_with_keys+0x734>
10081d3cc:      ldurh   w8, [x26, #-0x6]
10081d3d0:      mov w9, #0x1080             ; =4224
10081d3d4:      mov w10, #0xef7f            ; =61311
10081d3d8:      and w10, w8, w10
10081d3dc:      sturh   w10, [x26, #-0x6]
10081d3e0:      tst w8, w9
10081d3e4:      b.eq    0x10081d404 <_js_object_alloc_class_with_keys+0x744>
10081d3e8:      mov x0, x26
10081d3ec:      bl  0x1002b6f40 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081d3f0:      ldurb   w8, [x26, #-0x8]
10081d3f4:      sub w9, w8, #0x15
10081d3f8:      cmn w9, #0x14
10081d3fc:      b.hs    0x10081d408 <_js_object_alloc_class_with_keys+0x748>
10081d400:      b   0x10081d44c <_js_object_alloc_class_with_keys+0x78c>
10081d404:      mov w8, #0x1                ; =1
10081d408:      mov w8, w8
10081d40c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081d410:      add x9, x9, #0xb40
10081d414:      add x8, x9, x8, lsl #5
10081d418:      ldrb    w8, [x8, #0x14]
10081d41c:      mov w9, #0x16               ; =22
10081d420:      lsr w8, w9, w8
10081d424:      tbz w8, #0x0, 0x10081d44c <_js_object_alloc_class_with_keys+0x78c>
10081d428:      ldurh   w8, [x26, #-0x6]
10081d42c:      mov w9, #0x4000             ; =16384
10081d430:      bfxil   w9, w8, #0, #13
10081d434:      sturh   w9, [x26, #-0x6]
10081d438:      mov x0, x26
10081d43c:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081d440:      ldurh   w8, [x26, #-0x6]
10081d444:      and w8, w8, #0xffffefff
10081d448:      sturh   w8, [x26, #-0x6]
10081d44c:      cbz x24, 0x10081d498 <_js_object_alloc_class_with_keys+0x7d8>
10081d450:      mov x22, #0x0               ; =0
10081d454:      ldr x27, [sp]
10081d458:      add x24, x27, x24, lsl #4
10081d45c:      add x28, x22, #0x1
10081d460:      ldr x0, [x27]
10081d464:      ldr w1, [x27, #0x8]
10081d468:      bl  0x100885194 <_js_string_from_bytes_longlived>
10081d46c:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081d470:      bfxil   x2, x0, #0, #48
10081d474:      add x8, x26, x22, lsl #3
10081d478:      str x2, [x8, #0x8]
10081d47c:      mov x0, x26
10081d480:      mov x1, x22
10081d484:      bl  0x1004f10dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081d488:      add x27, x27, #0x10
10081d48c:      mov x22, x28
10081d490:      cmp x27, x24
10081d494:      b.ne    0x10081d45c <_js_object_alloc_class_with_keys+0x79c>
10081d498:      mov x0, x23
10081d49c:      mov x1, x26
10081d4a0:      bl  0x10032223c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081d4a4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d4a8:      ldr w22, [x8, #0x634]
10081d4ac:      cmp w22, #0x300
10081d4b0:      b.hs    0x10081d5bc <_js_object_alloc_class_with_keys+0x8fc>
10081d4b4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d4b8:      ldr x8, [x8, #0x48]
10081d4bc:      cmn x8, #0x1
10081d4c0:      b.eq    0x10081d5ac <_js_object_alloc_class_with_keys+0x8ec>
10081d4c4:      mrs x9, TPIDRRO_EL0
10081d4c8:      and x9, x9, #0xfffffffffffffff8
10081d4cc:      ldr x0, [x9, x8, lsl #3]
10081d4d0:      cbz x0, 0x10081d5ac <_js_object_alloc_class_with_keys+0x8ec>
10081d4d4:      add x8, x0, x22, lsl #3
10081d4d8:      ldr x0, [x8, #0x1e8]
10081d4dc:      cbz x0, 0x10081d5bc <_js_object_alloc_class_with_keys+0x8fc>
10081d4e0:      ldr x0, [x0]
10081d4e4:      cbz x0, 0x10081d5d0 <_js_object_alloc_class_with_keys+0x910>
10081d4e8:      and w8, w23, #0xff
10081d4ec:      add x8, x0, x8, lsl #4
10081d4f0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d4f4:      ldr w9, [x8]
10081d4f8:      cmp w9, w23
10081d4fc:      b.ne    0x10081d5ec <_js_object_alloc_class_with_keys+0x92c>
10081d500:      ldr w27, [x8, #0x4]
10081d504:      tbnz    w25, #0x0, 0x10081d510 <_js_object_alloc_class_with_keys+0x850>
10081d508:      ldr x0, [sp]
10081d50c:      bl  0x100af62c0 <_mi_free>
10081d510:      mov x0, x21
10081d514:      mov x1, x26
10081d518:      mov x2, x19
10081d51c:      bl  0x100324800 <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object31set_object_keys_array_with_live>
10081d520:      mov x0, x21
10081d524:      mov x1, x27
10081d528:      mov x2, x19
10081d52c:      bl  0x1005981c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081d530:      mov x0, x20
10081d534:      mov x1, x19
10081d538:      mov x2, x26
10081d53c:      bl  0x100591e40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc25remember_class_keys_array>
10081d540:      mov x0, x21
10081d544:      ldp x29, x30, [sp, #0x70]
10081d548:      ldp x20, x19, [sp, #0x60]
10081d54c:      ldp x22, x21, [sp, #0x50]
10081d550:      ldp x24, x23, [sp, #0x40]
10081d554:      ldp x26, x25, [sp, #0x30]
10081d558:      ldp x28, x27, [sp, #0x20]
10081d55c:      add sp, sp, #0x80
10081d560:      ret
10081d564:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d568:      ldr x8, [x0, #0x28]
10081d56c:      ldrb    w8, [x8]
10081d570:      tbnz    w8, #0x0, 0x10081cd88 <_js_object_alloc_class_with_keys+0xc8>
10081d574:      b   0x10081cdf0 <_js_object_alloc_class_with_keys+0x130>
10081d578:      mov x21, x0
10081d57c:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d580:      mov x8, x0
10081d584:      mov x0, x21
10081d588:      b   0x10081ce1c <_js_object_alloc_class_with_keys+0x15c>
10081d58c:      mov x21, x0
10081d590:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d594:      mov x8, x0
10081d598:      mov x0, x21
10081d59c:      ldr x8, [x8, #0x30]
10081d5a0:      ldrb    w8, [x8]
10081d5a4:      tbnz    w8, #0x0, 0x10081ce54 <_js_object_alloc_class_with_keys+0x194>
10081d5a8:      b   0x10081cebc <_js_object_alloc_class_with_keys+0x1fc>
10081d5ac:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d5b0:      add x8, x0, x22, lsl #3
10081d5b4:      ldr x0, [x8, #0x1e8]
10081d5b8:      cbnz    x0, 0x10081d4e0 <_js_object_alloc_class_with_keys+0x820>
10081d5bc:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081d5c0:      add x0, x0, #0x360
10081d5c4:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081d5c8:      ldr x0, [x0]
10081d5cc:      cbnz    x0, 0x10081d4e8 <_js_object_alloc_class_with_keys+0x828>
10081d5d0:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081d5d4:      and w8, w23, #0xff
10081d5d8:      add x8, x0, x8, lsl #4
10081d5dc:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d5e0:      ldr w9, [x8]
10081d5e4:      cmp w9, w23
10081d5e8:      b.eq    0x10081d500 <_js_object_alloc_class_with_keys+0x840>
10081d5ec:      ldr x8, [x0, #0x5088]
10081d5f0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081d5f4:      cmp x8, x9
10081d5f8:      b.hs    0x10081d718 <_js_object_alloc_class_with_keys+0xa58>
10081d5fc:      add x9, x8, #0x1
10081d600:      str x9, [x0, #0x5088]
10081d604:      ldr x9, [x0, #0x50a8]
10081d608:      cbz x9, 0x10081d6b8 <_js_object_alloc_class_with_keys+0x9f8>
10081d60c:      mov x9, #0x0                ; =0
10081d610:      mov x10, #0x7c15            ; =31765
10081d614:      movk    x10, #0x7f4a, lsl #16
10081d618:      movk    x10, #0x79b9, lsl #32
10081d61c:      movk    x10, #0x9e37, lsl #48
10081d620:      mul x10, x23, x10
10081d624:      eor x13, x10, x10, lsr #32
10081d628:      lsr x12, x10, #57
10081d62c:      ldr x10, [x0, #0x5098]
10081d630:      ldr x11, [x0, #0x5090]
10081d634:      dup.8b  v0, w12
10081d638:      movi.2d v1, #0xffffffffffffffff
10081d63c:      mov w12, #0x18              ; =24
10081d640:      and x13, x13, x10
10081d644:      ldr d2, [x11, x13]
10081d648:      cmeq.8b v3, v2, v0
10081d64c:      fmov    x14, d3
10081d650:      ands    x14, x14, #0x8080808080808080
10081d654:      b.eq    0x10081d688 <_js_object_alloc_class_with_keys+0x9c8>
10081d658:      rbit    x15, x14
10081d65c:      clz x15, x15
10081d660:      add x15, x13, x15, lsr #3
10081d664:      and x15, x15, x10
10081d668:      mneg    x15, x15, x12
10081d66c:      add x15, x11, x15
10081d670:      ldur    w16, [x15, #-0x18]
10081d674:      cmp w23, w16
10081d678:      b.eq    0x10081d6c8 <_js_object_alloc_class_with_keys+0xa08>
10081d67c:      sub x15, x14, #0x2
10081d680:      ands    x14, x15, x14
10081d684:      b.ne    0x10081d658 <_js_object_alloc_class_with_keys+0x998>
10081d688:      cmeq.8b v2, v2, v1
10081d68c:      fmov    x14, d2
10081d690:      cbnz    x14, 0x10081d6b8 <_js_object_alloc_class_with_keys+0x9f8>
10081d694:      add x9, x9, #0x8
10081d698:      add x13, x13, x9
10081d69c:      and x13, x13, x10
10081d6a0:      ldr d2, [x11, x13]
10081d6a4:      cmeq.8b v3, v2, v0
10081d6a8:      fmov    x14, d3
10081d6ac:      ands    x14, x14, #0x8080808080808080
10081d6b0:      b.ne    0x10081d658 <_js_object_alloc_class_with_keys+0x998>
10081d6b4:      b   0x10081d688 <_js_object_alloc_class_with_keys+0x9c8>
10081d6b8:      mov w27, #0x0               ; =0
10081d6bc:      str x8, [x0, #0x5088]
10081d6c0:      tbz w25, #0x0, 0x10081d508 <_js_object_alloc_class_with_keys+0x848>
10081d6c4:      b   0x10081d510 <_js_object_alloc_class_with_keys+0x850>
10081d6c8:      ldur    w27, [x15, #-0x8]
10081d6cc:      str x8, [x0, #0x5088]
10081d6d0:      tbz w25, #0x0, 0x10081d508 <_js_object_alloc_class_with_keys+0x848>
10081d6d4:      b   0x10081d510 <_js_object_alloc_class_with_keys+0x850>
10081d6d8:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d6dc:      ldr x26, [x0, #0x20]
10081d6e0:      ldr x8, [x26]
10081d6e4:      cbz x8, 0x10081cdb0 <_js_object_alloc_class_with_keys+0xf0>
10081d6e8:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
10081d6ec:      add x0, x0, #0x150
10081d6f0:      bl  0x100ab14ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081d6f4:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d6f8:      b   0x10081cfa8 <_js_object_alloc_class_with_keys+0x2e8>
10081d6fc:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d700:      ldr x8, [x0, #0x30]
10081d704:      ldrb    w8, [x8]
10081d708:      tbnz    w8, #0x0, 0x10081cfe0 <_js_object_alloc_class_with_keys+0x320>
10081d70c:      b   0x10081d044 <_js_object_alloc_class_with_keys+0x384>
10081d710:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d714:      b   0x10081cf6c <_js_object_alloc_class_with_keys+0x2ac>
10081d718:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081d71c:      add x0, x0, #0x7c8
10081d720:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081d724:      bl  0x100ab11bc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081d728:      cmp w8, #0x1
10081d72c:      b.ne    0x10081d770 <_js_object_alloc_class_with_keys+0xab0>
10081d730:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081d734:      add x1, x1, #0xbb8
10081d738:      mov x0, x21
10081d73c:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081d740:      strb    wzr, [x21, #0x18]
10081d744:      ldr x27, [x21, #0x10]
10081d748:      ldr x8, [x21]
10081d74c:      cmp x27, x8
10081d750:      mov x0, x26
10081d754:      b.ne    0x10081ceac <_js_object_alloc_class_with_keys+0x1ec>
10081d758:      mov x0, x21
10081d75c:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081d760:      mov x0, x26
10081d764:      b   0x10081ceac <_js_object_alloc_class_with_keys+0x1ec>
10081d768:      cmp w8, #0x2
10081d76c:      b.ne    0x10081d77c <_js_object_alloc_class_with_keys+0xabc>
10081d770:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081d774:      add x0, x0, #0x850
10081d778:      bl  0x100aef91c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081d77c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081d780:      add x1, x1, #0xbb8
10081d784:      mov x28, x0
10081d788:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081d78c:      mov x0, x28
10081d790:      strb    wzr, [x28, #0x18]
10081d794:      ldr x28, [x28, #0x10]
10081d798:      ldr x8, [x0]
10081d79c:      cmp x28, x8
10081d7a0:      b.ne    0x10081d030 <_js_object_alloc_class_with_keys+0x370>
10081d7a4:      str x0, [sp]
10081d7a8:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081d7ac:      ldr x0, [sp]
10081d7b0:      b   0x10081d030 <_js_object_alloc_class_with_keys+0x370>
10081d7b4:      mov w0, #0x8                ; =8
10081d7b8:      mov w1, #0x40               ; =64
10081d7bc:      bl  0x100ab1164 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
