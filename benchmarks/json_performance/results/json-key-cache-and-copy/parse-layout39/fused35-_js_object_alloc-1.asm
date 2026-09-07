/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081cac0 <_js_object_alloc_class_with_keys>:
10081cac0:      sub sp, sp, #0x80
10081cac4:      stp x28, x27, [sp, #0x20]
10081cac8:      stp x26, x25, [sp, #0x30]
10081cacc:      stp x24, x23, [sp, #0x40]
10081cad0:      stp x22, x21, [sp, #0x50]
10081cad4:      stp x20, x19, [sp, #0x60]
10081cad8:      stp x29, x30, [sp, #0x70]
10081cadc:      add x29, sp, #0x70
10081cae0:      mov x24, x4
10081cae4:      mov x22, x3
10081cae8:      mov x19, x2
10081caec:      mov x23, x1
10081caf0:      mov x20, x0
10081caf4:      cbz w1, 0x10081cb04 <_js_object_alloc_class_with_keys+0x44>
10081caf8:      mov x0, x20
10081cafc:      mov x1, x23
10081cb00:      bl  0x1006d1a30 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry13parent_static14register_class>
10081cb04:      mov w8, #0x2                ; =2
10081cb08:      cmp w19, #0x2
10081cb0c:      csel    w8, w19, w8, hi
10081cb10:      ubfiz   x8, x8, #3, #32
10081cb14:      mov w9, #0x3ffd             ; =16381
10081cb18:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081cb1c:      cmp w19, w9
10081cb20:      b.ls    0x10081cb5c <_js_object_alloc_class_with_keys+0x9c>
10081cb24:      add x0, x8, #0x10
10081cb28:      mov w1, #0x8                ; =8
10081cb2c:      mov w2, #0x2                ; =2
10081cb30:      bl  0x1004da658 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081cb34:      mov x21, x0
10081cb38:      ldurb   w8, [x0, #-0x7]
10081cb3c:      orr w8, w8, #0x20
10081cb40:      sturb   w8, [x0, #-0x7]
10081cb44:      stp w20, w23, [x0]
10081cb48:      str xzr, [x0, #0x8]
10081cb4c:      lsr x8, x0, #3
10081cb50:      cmp x8, #0x201
10081cb54:      b.hs    0x10081ccdc <_js_object_alloc_class_with_keys+0x21c>
10081cb58:      b   0x10081ce60 <_js_object_alloc_class_with_keys+0x3a0>
10081cb5c:      add x25, x8, #0x18
10081cb60:      ldr x8, [x28, #0x48]
10081cb64:      cmn x8, #0x1
10081cb68:      b.eq    0x10081d364 <_js_object_alloc_class_with_keys+0x8a4>
10081cb6c:      mrs x9, TPIDRRO_EL0
10081cb70:      and x9, x9, #0xfffffffffffffff8
10081cb74:      ldr x0, [x9, x8, lsl #3]
10081cb78:      cbz x0, 0x10081d364 <_js_object_alloc_class_with_keys+0x8a4>
10081cb7c:      ldr x8, [x0, #0x28]
10081cb80:      ldrb    w8, [x8]
10081cb84:      tbz w8, #0x0, 0x10081cbf0 <_js_object_alloc_class_with_keys+0x130>
10081cb88:      ldr x8, [x28, #0x48]
10081cb8c:      cmn x8, #0x1
10081cb90:      b.eq    0x10081d4d8 <_js_object_alloc_class_with_keys+0xa18>
10081cb94:      mrs x9, TPIDRRO_EL0
10081cb98:      and x9, x9, #0xfffffffffffffff8
10081cb9c:      ldr x0, [x9, x8, lsl #3]
10081cba0:      cbz x0, 0x10081d4d8 <_js_object_alloc_class_with_keys+0xa18>
10081cba4:      ldr x26, [x0, #0x20]
10081cba8:      ldr x8, [x26]
10081cbac:      cbnz    x8, 0x10081d4e8 <_js_object_alloc_class_with_keys+0xa28>
10081cbb0:      mov x8, #-0x1               ; =-1
10081cbb4:      str x8, [x26]
10081cbb8:      ldr x1, [x26, #0x18]
10081cbbc:      cbz x1, 0x10081cbec <_js_object_alloc_class_with_keys+0x12c>
10081cbc0:      mov x0, #0x0                ; =0
10081cbc4:      ldr x8, [x26, #0x10]
10081cbc8:      lsl x10, x1, #4
10081cbcc:      mov x9, x8
10081cbd0:      ldr x11, [x9, #0x8]
10081cbd4:      cmp x11, x25
10081cbd8:      b.eq    0x10081cd30 <_js_object_alloc_class_with_keys+0x270>
10081cbdc:      add x0, x0, #0x1
10081cbe0:      add x9, x9, #0x10
10081cbe4:      subs    x10, x10, #0x10
10081cbe8:      b.ne    0x10081cbd0 <_js_object_alloc_class_with_keys+0x110>
10081cbec:      str xzr, [x26]
10081cbf0:      mov x0, x25
10081cbf4:      bl  0x1004da29c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081cbf8:      mov w8, #0x2                ; =2
10081cbfc:      strb    w8, [x0]
10081cc00:      ldr x8, [x28, #0x48]
10081cc04:      cmn x8, #0x1
10081cc08:      b.eq    0x10081d378 <_js_object_alloc_class_with_keys+0x8b8>
10081cc0c:      mrs x9, TPIDRRO_EL0
10081cc10:      and x9, x9, #0xfffffffffffffff8
10081cc14:      ldr x8, [x9, x8, lsl #3]
10081cc18:      cbz x8, 0x10081d378 <_js_object_alloc_class_with_keys+0x8b8>
10081cc1c:      ldr x8, [x8, #0x30]
10081cc20:      ldrb    w8, [x8]
10081cc24:      orr w8, w8, #0x2
10081cc28:      strb    w8, [x0, #0x1]
10081cc2c:      ldr x8, [x28, #0x48]
10081cc30:      cmn x8, #0x1
10081cc34:      b.eq    0x10081d38c <_js_object_alloc_class_with_keys+0x8cc>
10081cc38:      mrs x9, TPIDRRO_EL0
10081cc3c:      and x9, x9, #0xfffffffffffffff8
10081cc40:      ldr x8, [x9, x8, lsl #3]
10081cc44:      cbz x8, 0x10081d38c <_js_object_alloc_class_with_keys+0x8cc>
10081cc48:      ldr x8, [x8, #0x30]
10081cc4c:      ldrb    w8, [x8]
10081cc50:      tbz w8, #0x0, 0x10081ccbc <_js_object_alloc_class_with_keys+0x1fc>
10081cc54:      ldrb    w8, [x0]
10081cc58:      sub w9, w8, #0x15
10081cc5c:      cmn w9, #0x14
10081cc60:      b.lo    0x10081ccbc <_js_object_alloc_class_with_keys+0x1fc>
10081cc64:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081cc68:      add x9, x9, #0xb10
10081cc6c:      add x8, x9, x8, lsl #5
10081cc70:      ldrb    w8, [x8, #0x1b]
10081cc74:      tbnz    w8, #0x0, 0x10081ccbc <_js_object_alloc_class_with_keys+0x1fc>
10081cc78:      mov x26, x0
10081cc7c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081cc80:      add x0, x0, #0x658
10081cc84:      ldr x8, [x0]
10081cc88:      blr x8
10081cc8c:      mov x21, x0
10081cc90:      ldrb    w8, [x0, #0x18]
10081cc94:      cbnz    w8, 0x10081d528 <_js_object_alloc_class_with_keys+0xa68>
10081cc98:      ldr x27, [x21, #0x10]
10081cc9c:      ldr x8, [x21]
10081cca0:      cmp x27, x8
10081cca4:      mov x0, x26
10081cca8:      b.eq    0x10081d558 <_js_object_alloc_class_with_keys+0xa98>
10081ccac:      ldr x8, [x21, #0x8]
10081ccb0:      str x0, [x8, x27, lsl #3]
10081ccb4:      add x8, x27, #0x1
10081ccb8:      str x8, [x21, #0x10]
10081ccbc:      strh    wzr, [x0, #0x2]
10081ccc0:      str w25, [x0, #0x4]
10081ccc4:      add x21, x0, #0x8
10081ccc8:      stp w20, w23, [x21]
10081cccc:      str xzr, [x21, #0x8]
10081ccd0:      lsr x8, x21, #3
10081ccd4:      cmp x8, #0x201
10081ccd8:      b.lo    0x10081ce60 <_js_object_alloc_class_with_keys+0x3a0>
10081ccdc:      ldurb   w8, [x21, #-0x8]
10081cce0:      sub w9, w8, #0x15
10081cce4:      cmn w9, #0x14
10081cce8:      b.lo    0x10081ce60 <_js_object_alloc_class_with_keys+0x3a0>
10081ccec:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081ccf0:      add x9, x9, #0xb10
10081ccf4:      add x8, x9, x8, lsl #5
10081ccf8:      ldrb    w8, [x8, #0x14]
10081ccfc:      mov w9, #0x16               ; =22
10081cd00:      lsr w8, w9, w8
10081cd04:      tbz w8, #0x0, 0x10081ce60 <_js_object_alloc_class_with_keys+0x3a0>
10081cd08:      ldurh   w8, [x21, #-0x6]
10081cd0c:      mov w9, #0x4000             ; =16384
10081cd10:      bfxil   w9, w8, #0, #13
10081cd14:      sturh   w9, [x21, #-0x6]
10081cd18:      mov x0, x21
10081cd1c:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081cd20:      ldurh   w8, [x21, #-0x6]
10081cd24:      and w8, w8, #0xffffefff
10081cd28:      sturh   w8, [x21, #-0x6]
10081cd2c:      b   0x10081ce60 <_js_object_alloc_class_with_keys+0x3a0>
10081cd30:      cmp x0, x1
10081cd34:      b.hs    0x10081d524 <_js_object_alloc_class_with_keys+0xa64>
10081cd38:      ldr x21, [x9]
10081cd3c:      subs    x10, x1, #0x1
10081cd40:      ldr q0, [x8, x10, lsl #4]
10081cd44:      str q0, [x9]
10081cd48:      str x10, [x26, #0x18]
10081cd4c:      b.ne    0x10081cd74 <_js_object_alloc_class_with_keys+0x2b4>
10081cd50:      ldr x8, [x28, #0x48]
10081cd54:      cmn x8, #0x1
10081cd58:      b.eq    0x10081d510 <_js_object_alloc_class_with_keys+0xa50>
10081cd5c:      mrs x9, TPIDRRO_EL0
10081cd60:      and x9, x9, #0xfffffffffffffff8
10081cd64:      ldr x0, [x9, x8, lsl #3]
10081cd68:      cbz x0, 0x10081d510 <_js_object_alloc_class_with_keys+0xa50>
10081cd6c:      ldr x8, [x0, #0x28]
10081cd70:      strb    wzr, [x8]
10081cd74:      ldr x8, [x26]
10081cd78:      add x8, x8, #0x1
10081cd7c:      str x8, [x26]
10081cd80:      mov w8, #0x2                ; =2
10081cd84:      mov x27, x21
10081cd88:      strb    w8, [x27, #-0x8]!
10081cd8c:      ldr x8, [x28, #0x48]
10081cd90:      cmn x8, #0x1
10081cd94:      b.eq    0x10081d4f4 <_js_object_alloc_class_with_keys+0xa34>
10081cd98:      mrs x9, TPIDRRO_EL0
10081cd9c:      and x9, x9, #0xfffffffffffffff8
10081cda0:      ldr x0, [x9, x8, lsl #3]
10081cda4:      cbz x0, 0x10081d4f4 <_js_object_alloc_class_with_keys+0xa34>
10081cda8:      ldr x8, [x0, #0x30]
10081cdac:      ldrb    w8, [x8]
10081cdb0:      orr w8, w8, #0x2
10081cdb4:      sturb   w8, [x21, #-0x7]
10081cdb8:      ldr x8, [x28, #0x48]
10081cdbc:      cmn x8, #0x1
10081cdc0:      b.eq    0x10081d4fc <_js_object_alloc_class_with_keys+0xa3c>
10081cdc4:      mrs x9, TPIDRRO_EL0
10081cdc8:      and x9, x9, #0xfffffffffffffff8
10081cdcc:      ldr x0, [x9, x8, lsl #3]
10081cdd0:      cbz x0, 0x10081d4fc <_js_object_alloc_class_with_keys+0xa3c>
10081cdd4:      ldr x8, [x0, #0x30]
10081cdd8:      ldrb    w8, [x8]
10081cddc:      tbz w8, #0x0, 0x10081ce44 <_js_object_alloc_class_with_keys+0x384>
10081cde0:      ldrb    w8, [x27]
10081cde4:      sub w9, w8, #0x15
10081cde8:      cmn w9, #0x14
10081cdec:      b.lo    0x10081ce44 <_js_object_alloc_class_with_keys+0x384>
10081cdf0:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081cdf4:      add x9, x9, #0xb10
10081cdf8:      add x8, x9, x8, lsl #5
10081cdfc:      ldrb    w8, [x8, #0x1b]
10081ce00:      tbnz    w8, #0x0, 0x10081ce44 <_js_object_alloc_class_with_keys+0x384>
10081ce04:      mov x26, x28
10081ce08:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081ce0c:      add x0, x0, #0x658
10081ce10:      ldr x8, [x0]
10081ce14:      blr x8
10081ce18:      ldrb    w8, [x0, #0x18]
10081ce1c:      cbnz    w8, 0x10081d568 <_js_object_alloc_class_with_keys+0xaa8>
10081ce20:      ldr x28, [x0, #0x10]
10081ce24:      ldr x8, [x0]
10081ce28:      cmp x28, x8
10081ce2c:      b.eq    0x10081d5a4 <_js_object_alloc_class_with_keys+0xae4>
10081ce30:      ldr x8, [x0, #0x8]
10081ce34:      str x27, [x8, x28, lsl #3]
10081ce38:      add x8, x28, #0x1
10081ce3c:      str x8, [x0, #0x10]
10081ce40:      mov x28, x26
10081ce44:      sturh   wzr, [x21, #-0x6]
10081ce48:      stur    w25, [x21, #-0x4]
10081ce4c:      stp w20, w23, [x21]
10081ce50:      str xzr, [x21, #0x8]
10081ce54:      lsr x8, x21, #3
10081ce58:      cmp x8, #0x201
10081ce5c:      b.hs    0x10081ccdc <_js_object_alloc_class_with_keys+0x21c>
10081ce60:      mov w8, #0x2717             ; =10007
10081ce64:      mov w9, #0x86a3             ; =34467
10081ce68:      movk    w9, #0x1, lsl #16
10081ce6c:      mul w9, w19, w9
10081ce70:      madd    w23, w20, w8, w9
10081ce74:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081ce78:      ldr w25, [x8, #0x634]
10081ce7c:      cmp w25, #0x300
10081ce80:      b.hs    0x10081cef4 <_js_object_alloc_class_with_keys+0x434>
10081ce84:      ldr x8, [x28, #0x48]
10081ce88:      cmn x8, #0x1
10081ce8c:      b.eq    0x10081cee4 <_js_object_alloc_class_with_keys+0x424>
10081ce90:      mrs x9, TPIDRRO_EL0
10081ce94:      and x9, x9, #0xfffffffffffffff8
10081ce98:      ldr x0, [x9, x8, lsl #3]
10081ce9c:      cbz x0, 0x10081cee4 <_js_object_alloc_class_with_keys+0x424>
10081cea0:      add x8, x0, x25, lsl #3
10081cea4:      ldr x0, [x8, #0x1e8]
10081cea8:      cbz x0, 0x10081cef4 <_js_object_alloc_class_with_keys+0x434>
10081ceac:      add w8, w23, #0xf4, lsl #12 ; =0xf4000
10081ceb0:      add w23, w8, #0x240
10081ceb4:      ldr x0, [x0]
10081ceb8:      cbz x0, 0x10081cf10 <_js_object_alloc_class_with_keys+0x450>
10081cebc:      and w8, w23, #0xff
10081cec0:      add x8, x0, w8, uxtw #4
10081cec4:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081cec8:      ldr w9, [x8]
10081cecc:      cmp w9, w23
10081ced0:      b.ne    0x10081cf2c <_js_object_alloc_class_with_keys+0x46c>
10081ced4:      ldr x26, [x8, #0x8]
10081ced8:      cbz x26, 0x10081d01c <_js_object_alloc_class_with_keys+0x55c>
10081cedc:      ldr w27, [x8, #0x4]
10081cee0:      b   0x10081d310 <_js_object_alloc_class_with_keys+0x850>
10081cee4:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cee8:      add x8, x0, x25, lsl #3
10081ceec:      ldr x0, [x8, #0x1e8]
10081cef0:      cbnz    x0, 0x10081ceac <_js_object_alloc_class_with_keys+0x3ec>
10081cef4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081cef8:      add x0, x0, #0x330
10081cefc:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081cf00:      add w8, w23, #0xf4, lsl #12 ; =0xf4000
10081cf04:      add w23, w8, #0x240
10081cf08:      ldr x0, [x0]
10081cf0c:      cbnz    x0, 0x10081cebc <_js_object_alloc_class_with_keys+0x3fc>
10081cf10:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081cf14:      and w8, w23, #0xff
10081cf18:      add x8, x0, w8, uxtw #4
10081cf1c:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081cf20:      ldr w9, [x8]
10081cf24:      cmp w9, w23
10081cf28:      b.eq    0x10081ced4 <_js_object_alloc_class_with_keys+0x414>
10081cf2c:      ldr x8, [x0, #0x5088]
10081cf30:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081cf34:      cmp x8, x9
10081cf38:      b.hs    0x10081d518 <_js_object_alloc_class_with_keys+0xa58>
10081cf3c:      add x9, x8, #0x1
10081cf40:      str x9, [x0, #0x5088]
10081cf44:      ldr x9, [x0, #0x50a8]
10081cf48:      cbz x9, 0x10081cff8 <_js_object_alloc_class_with_keys+0x538>
10081cf4c:      mov x9, #0x0                ; =0
10081cf50:      mov x10, #0x7c15            ; =31765
10081cf54:      movk    x10, #0x7f4a, lsl #16
10081cf58:      movk    x10, #0x79b9, lsl #32
10081cf5c:      movk    x10, #0x9e37, lsl #48
10081cf60:      mul x10, x23, x10
10081cf64:      eor x13, x10, x10, lsr #32
10081cf68:      lsr x12, x10, #57
10081cf6c:      ldr x10, [x0, #0x5098]
10081cf70:      ldr x11, [x0, #0x5090]
10081cf74:      dup.8b  v0, w12
10081cf78:      movi.2d v1, #0xffffffffffffffff
10081cf7c:      mov w12, #0x18              ; =24
10081cf80:      and x13, x13, x10
10081cf84:      ldr d2, [x11, x13]
10081cf88:      cmeq.8b v3, v2, v0
10081cf8c:      fmov    x14, d3
10081cf90:      ands    x14, x14, #0x8080808080808080
10081cf94:      b.eq    0x10081cfc8 <_js_object_alloc_class_with_keys+0x508>
10081cf98:      rbit    x15, x14
10081cf9c:      clz x15, x15
10081cfa0:      add x15, x13, x15, lsr #3
10081cfa4:      and x15, x15, x10
10081cfa8:      mneg    x15, x15, x12
10081cfac:      add x15, x11, x15
10081cfb0:      ldur    w16, [x15, #-0x18]
10081cfb4:      cmp w23, w16
10081cfb8:      b.eq    0x10081d00c <_js_object_alloc_class_with_keys+0x54c>
10081cfbc:      sub x15, x14, #0x2
10081cfc0:      ands    x14, x15, x14
10081cfc4:      b.ne    0x10081cf98 <_js_object_alloc_class_with_keys+0x4d8>
10081cfc8:      cmeq.8b v2, v2, v1
10081cfcc:      fmov    x14, d2
10081cfd0:      cbnz    x14, 0x10081cff8 <_js_object_alloc_class_with_keys+0x538>
10081cfd4:      add x9, x9, #0x8
10081cfd8:      add x13, x13, x9
10081cfdc:      and x13, x13, x10
10081cfe0:      ldr d2, [x11, x13]
10081cfe4:      cmeq.8b v3, v2, v0
10081cfe8:      fmov    x14, d3
10081cfec:      ands    x14, x14, #0x8080808080808080
10081cff0:      b.ne    0x10081cf98 <_js_object_alloc_class_with_keys+0x4d8>
10081cff4:      b   0x10081cfc8 <_js_object_alloc_class_with_keys+0x508>
10081cff8:      mov w27, #0x0               ; =0
10081cffc:      mov x26, #0x0               ; =0
10081d000:      str x8, [x0, #0x5088]
10081d004:      cbnz    x26, 0x10081d310 <_js_object_alloc_class_with_keys+0x850>
10081d008:      b   0x10081d01c <_js_object_alloc_class_with_keys+0x55c>
10081d00c:      ldur    x26, [x15, #-0x10]
10081d010:      ldur    w27, [x15, #-0x8]
10081d014:      str x8, [x0, #0x5088]
10081d018:      cbnz    x26, 0x10081d310 <_js_object_alloc_class_with_keys+0x850>
10081d01c:      mov w26, #0x0               ; =0
10081d020:      mov w8, #0x0                ; =0
10081d024:      mov w9, #0x8                ; =8
10081d028:      str x9, [sp]
10081d02c:      mov w27, w24
10081d030:      b   0x10081d044 <_js_object_alloc_class_with_keys+0x584>
10081d034:      mov w26, #0x1               ; =1
10081d038:      mov x22, x24
10081d03c:      mov w8, #0x1                ; =1
10081d040:      cbnz    x28, 0x10081d09c <_js_object_alloc_class_with_keys+0x5dc>
10081d044:      tbnz    w8, #0x0, 0x10081d090 <_js_object_alloc_class_with_keys+0x5d0>
10081d048:      mov x24, x22
10081d04c:      mov x28, #0x0               ; =0
10081d050:      cbz x27, 0x10081d034 <_js_object_alloc_class_with_keys+0x574>
10081d054:      ldrb    w8, [x24, x28]
10081d058:      cbz w8, 0x10081d07c <_js_object_alloc_class_with_keys+0x5bc>
10081d05c:      add x28, x28, #0x1
10081d060:      cmp x27, x28
10081d064:      b.ne    0x10081d054 <_js_object_alloc_class_with_keys+0x594>
10081d068:      mov w26, #0x1               ; =1
10081d06c:      mov x22, x24
10081d070:      mov w8, #0x1                ; =1
10081d074:      mov x28, x27
10081d078:      b   0x10081d040 <_js_object_alloc_class_with_keys+0x580>
10081d07c:      mvn x9, x28
10081d080:      add x27, x9, x27
10081d084:      add x9, x24, x28
10081d088:      add x22, x9, #0x1
10081d08c:      b   0x10081d040 <_js_object_alloc_class_with_keys+0x580>
10081d090:      mov x24, #0x0               ; =0
10081d094:      mov w25, #0x1               ; =1
10081d098:      b   0x10081d198 <_js_object_alloc_class_with_keys+0x6d8>
10081d09c:      cbz x24, 0x10081d18c <_js_object_alloc_class_with_keys+0x6cc>
10081d0a0:      mov w0, #0x40               ; =64
10081d0a4:      mov w1, #0x8                ; =8
10081d0a8:      bl  0x100af6348 <_mi_malloc_aligned>
10081d0ac:      cbz x0, 0x10081d5b4 <_js_object_alloc_class_with_keys+0xaf4>
10081d0b0:      stp x24, x28, [x0]
10081d0b4:      mov w8, #0x4                ; =4
10081d0b8:      stp x8, x0, [sp, #0x8]
10081d0bc:      mov w24, #0x1               ; =1
10081d0c0:      str x24, [sp, #0x18]
10081d0c4:      mov x8, x27
10081d0c8:      mov x10, x22
10081d0cc:      mov x9, x26
10081d0d0:      b   0x10081d0e4 <_js_object_alloc_class_with_keys+0x624>
10081d0d4:      mov w26, #0x1               ; =1
10081d0d8:      mov x10, x25
10081d0dc:      mov w9, #0x1                ; =1
10081d0e0:      cbnz    x28, 0x10081d138 <_js_object_alloc_class_with_keys+0x678>
10081d0e4:      tbnz    w9, #0x0, 0x10081d178 <_js_object_alloc_class_with_keys+0x6b8>
10081d0e8:      mov x25, x10
10081d0ec:      mov x28, #0x0               ; =0
10081d0f0:      cbz x8, 0x10081d0d4 <_js_object_alloc_class_with_keys+0x614>
10081d0f4:      ldrb    w9, [x25, x28]
10081d0f8:      cbz w9, 0x10081d11c <_js_object_alloc_class_with_keys+0x65c>
10081d0fc:      add x28, x28, #0x1
10081d100:      cmp x8, x28
10081d104:      b.ne    0x10081d0f4 <_js_object_alloc_class_with_keys+0x634>
10081d108:      mov w26, #0x1               ; =1
10081d10c:      mov x10, x25
10081d110:      mov w9, #0x1                ; =1
10081d114:      mov x28, x8
10081d118:      b   0x10081d0e0 <_js_object_alloc_class_with_keys+0x620>
10081d11c:      mvn x10, x28
10081d120:      add x27, x10, x8
10081d124:      add x8, x25, x28
10081d128:      add x22, x8, #0x1
10081d12c:      mov x8, x27
10081d130:      mov x10, x22
10081d134:      b   0x10081d0e0 <_js_object_alloc_class_with_keys+0x620>
10081d138:      cbz x25, 0x10081d178 <_js_object_alloc_class_with_keys+0x6b8>
10081d13c:      ldr x8, [sp, #0x8]
10081d140:      cmp x24, x8
10081d144:      b.eq    0x10081d158 <_js_object_alloc_class_with_keys+0x698>
10081d148:      add x8, x0, x24, lsl #4
10081d14c:      stp x25, x28, [x8]
10081d150:      add x24, x24, #0x1
10081d154:      b   0x10081d0c0 <_js_object_alloc_class_with_keys+0x600>
10081d158:      add x0, sp, #0x8
10081d15c:      mov x1, x24
10081d160:      mov w2, #0x1                ; =1
10081d164:      mov w3, #0x8                ; =8
10081d168:      mov w4, #0x10               ; =16
10081d16c:      bl  0x100ad59d0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081d170:      ldr x0, [sp, #0x10]
10081d174:      b   0x10081d148 <_js_object_alloc_class_with_keys+0x688>
10081d178:      ldp x8, x9, [sp, #0x8]
10081d17c:      str x9, [sp]
10081d180:      cmp x8, #0x0
10081d184:      cset    w25, eq
10081d188:      b   0x10081d198 <_js_object_alloc_class_with_keys+0x6d8>
10081d18c:      mov w25, #0x1               ; =1
10081d190:      mov w8, #0x8                ; =8
10081d194:      str x8, [sp]
10081d198:      ubfiz   x8, x24, #3, #32
10081d19c:      add x0, x8, #0x8
10081d1a0:      mov w1, #0x8                ; =8
10081d1a4:      mov w2, #0x1                ; =1
10081d1a8:      bl  0x1004db0e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081d1ac:      mov x26, x0
10081d1b0:      stp w24, w24, [x0]
10081d1b4:      lsr x8, x0, #3
10081d1b8:      cmp x8, #0x201
10081d1bc:      b.lo    0x10081d24c <_js_object_alloc_class_with_keys+0x78c>
10081d1c0:      ldurb   w8, [x26, #-0x8]
10081d1c4:      cmp w8, #0x1
10081d1c8:      b.ne    0x10081d1f4 <_js_object_alloc_class_with_keys+0x734>
10081d1cc:      ldurh   w8, [x26, #-0x6]
10081d1d0:      mov w9, #0x1080             ; =4224
10081d1d4:      mov w10, #0xef7f            ; =61311
10081d1d8:      and w10, w8, w10
10081d1dc:      sturh   w10, [x26, #-0x6]
10081d1e0:      tst w8, w9
10081d1e4:      b.eq    0x10081d204 <_js_object_alloc_class_with_keys+0x744>
10081d1e8:      mov x0, x26
10081d1ec:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081d1f0:      ldurb   w8, [x26, #-0x8]
10081d1f4:      sub w9, w8, #0x15
10081d1f8:      cmn w9, #0x14
10081d1fc:      b.hs    0x10081d208 <_js_object_alloc_class_with_keys+0x748>
10081d200:      b   0x10081d24c <_js_object_alloc_class_with_keys+0x78c>
10081d204:      mov w8, #0x1                ; =1
10081d208:      mov w8, w8
10081d20c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d210:      add x9, x9, #0xb10
10081d214:      add x8, x9, x8, lsl #5
10081d218:      ldrb    w8, [x8, #0x14]
10081d21c:      mov w9, #0x16               ; =22
10081d220:      lsr w8, w9, w8
10081d224:      tbz w8, #0x0, 0x10081d24c <_js_object_alloc_class_with_keys+0x78c>
10081d228:      ldurh   w8, [x26, #-0x6]
10081d22c:      mov w9, #0x4000             ; =16384
10081d230:      bfxil   w9, w8, #0, #13
10081d234:      sturh   w9, [x26, #-0x6]
10081d238:      mov x0, x26
10081d23c:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081d240:      ldurh   w8, [x26, #-0x6]
10081d244:      and w8, w8, #0xffffefff
10081d248:      sturh   w8, [x26, #-0x6]
10081d24c:      cbz x24, 0x10081d298 <_js_object_alloc_class_with_keys+0x7d8>
10081d250:      mov x22, #0x0               ; =0
10081d254:      ldr x27, [sp]
10081d258:      add x24, x27, x24, lsl #4
10081d25c:      add x28, x22, #0x1
10081d260:      ldr x0, [x27]
10081d264:      ldr w1, [x27, #0x8]
10081d268:      bl  0x100884f94 <_js_string_from_bytes_longlived>
10081d26c:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081d270:      bfxil   x2, x0, #0, #48
10081d274:      add x8, x26, x22, lsl #3
10081d278:      str x2, [x8, #0x8]
10081d27c:      mov x0, x26
10081d280:      mov x1, x22
10081d284:      bl  0x1004f0edc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081d288:      add x27, x27, #0x10
10081d28c:      mov x22, x28
10081d290:      cmp x27, x24
10081d294:      b.ne    0x10081d25c <_js_object_alloc_class_with_keys+0x79c>
10081d298:      mov x0, x23
10081d29c:      mov x1, x26
10081d2a0:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081d2a4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d2a8:      ldr w22, [x8, #0x634]
10081d2ac:      cmp w22, #0x300
10081d2b0:      b.hs    0x10081d3bc <_js_object_alloc_class_with_keys+0x8fc>
10081d2b4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d2b8:      ldr x8, [x8, #0x48]
10081d2bc:      cmn x8, #0x1
10081d2c0:      b.eq    0x10081d3ac <_js_object_alloc_class_with_keys+0x8ec>
10081d2c4:      mrs x9, TPIDRRO_EL0
10081d2c8:      and x9, x9, #0xfffffffffffffff8
10081d2cc:      ldr x0, [x9, x8, lsl #3]
10081d2d0:      cbz x0, 0x10081d3ac <_js_object_alloc_class_with_keys+0x8ec>
10081d2d4:      add x8, x0, x22, lsl #3
10081d2d8:      ldr x0, [x8, #0x1e8]
10081d2dc:      cbz x0, 0x10081d3bc <_js_object_alloc_class_with_keys+0x8fc>
10081d2e0:      ldr x0, [x0]
10081d2e4:      cbz x0, 0x10081d3d0 <_js_object_alloc_class_with_keys+0x910>
10081d2e8:      and w8, w23, #0xff
10081d2ec:      add x8, x0, x8, lsl #4
10081d2f0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d2f4:      ldr w9, [x8]
10081d2f8:      cmp w9, w23
10081d2fc:      b.ne    0x10081d3ec <_js_object_alloc_class_with_keys+0x92c>
10081d300:      ldr w27, [x8, #0x4]
10081d304:      tbnz    w25, #0x0, 0x10081d310 <_js_object_alloc_class_with_keys+0x850>
10081d308:      ldr x0, [sp]
10081d30c:      bl  0x100af60c0 <_mi_free>
10081d310:      mov x0, x21
10081d314:      mov x1, x26
10081d318:      mov x2, x19
10081d31c:      bl  0x100324600 <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object31set_object_keys_array_with_live>
10081d320:      mov x0, x21
10081d324:      mov x1, x27
10081d328:      mov x2, x19
10081d32c:      bl  0x100597fc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081d330:      mov x0, x20
10081d334:      mov x1, x19
10081d338:      mov x2, x26
10081d33c:      bl  0x100591c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc25remember_class_keys_array>
10081d340:      mov x0, x21
10081d344:      ldp x29, x30, [sp, #0x70]
10081d348:      ldp x20, x19, [sp, #0x60]
10081d34c:      ldp x22, x21, [sp, #0x50]
10081d350:      ldp x24, x23, [sp, #0x40]
10081d354:      ldp x26, x25, [sp, #0x30]
10081d358:      ldp x28, x27, [sp, #0x20]
10081d35c:      add sp, sp, #0x80
10081d360:      ret
10081d364:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d368:      ldr x8, [x0, #0x28]
10081d36c:      ldrb    w8, [x8]
10081d370:      tbnz    w8, #0x0, 0x10081cb88 <_js_object_alloc_class_with_keys+0xc8>
10081d374:      b   0x10081cbf0 <_js_object_alloc_class_with_keys+0x130>
10081d378:      mov x21, x0
10081d37c:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d380:      mov x8, x0
10081d384:      mov x0, x21
10081d388:      b   0x10081cc1c <_js_object_alloc_class_with_keys+0x15c>
10081d38c:      mov x21, x0
10081d390:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d394:      mov x8, x0
10081d398:      mov x0, x21
10081d39c:      ldr x8, [x8, #0x30]
10081d3a0:      ldrb    w8, [x8]
10081d3a4:      tbnz    w8, #0x0, 0x10081cc54 <_js_object_alloc_class_with_keys+0x194>
10081d3a8:      b   0x10081ccbc <_js_object_alloc_class_with_keys+0x1fc>
10081d3ac:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d3b0:      add x8, x0, x22, lsl #3
10081d3b4:      ldr x0, [x8, #0x1e8]
10081d3b8:      cbnz    x0, 0x10081d2e0 <_js_object_alloc_class_with_keys+0x820>
10081d3bc:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081d3c0:      add x0, x0, #0x330
10081d3c4:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081d3c8:      ldr x0, [x0]
10081d3cc:      cbnz    x0, 0x10081d2e8 <_js_object_alloc_class_with_keys+0x828>
10081d3d0:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081d3d4:      and w8, w23, #0xff
10081d3d8:      add x8, x0, x8, lsl #4
10081d3dc:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d3e0:      ldr w9, [x8]
10081d3e4:      cmp w9, w23
10081d3e8:      b.eq    0x10081d300 <_js_object_alloc_class_with_keys+0x840>
10081d3ec:      ldr x8, [x0, #0x5088]
10081d3f0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081d3f4:      cmp x8, x9
10081d3f8:      b.hs    0x10081d518 <_js_object_alloc_class_with_keys+0xa58>
10081d3fc:      add x9, x8, #0x1
10081d400:      str x9, [x0, #0x5088]
10081d404:      ldr x9, [x0, #0x50a8]
10081d408:      cbz x9, 0x10081d4b8 <_js_object_alloc_class_with_keys+0x9f8>
10081d40c:      mov x9, #0x0                ; =0
10081d410:      mov x10, #0x7c15            ; =31765
10081d414:      movk    x10, #0x7f4a, lsl #16
10081d418:      movk    x10, #0x79b9, lsl #32
10081d41c:      movk    x10, #0x9e37, lsl #48
10081d420:      mul x10, x23, x10
10081d424:      eor x13, x10, x10, lsr #32
10081d428:      lsr x12, x10, #57
10081d42c:      ldr x10, [x0, #0x5098]
10081d430:      ldr x11, [x0, #0x5090]
10081d434:      dup.8b  v0, w12
10081d438:      movi.2d v1, #0xffffffffffffffff
10081d43c:      mov w12, #0x18              ; =24
10081d440:      and x13, x13, x10
10081d444:      ldr d2, [x11, x13]
10081d448:      cmeq.8b v3, v2, v0
10081d44c:      fmov    x14, d3
10081d450:      ands    x14, x14, #0x8080808080808080
10081d454:      b.eq    0x10081d488 <_js_object_alloc_class_with_keys+0x9c8>
10081d458:      rbit    x15, x14
10081d45c:      clz x15, x15
10081d460:      add x15, x13, x15, lsr #3
10081d464:      and x15, x15, x10
10081d468:      mneg    x15, x15, x12
10081d46c:      add x15, x11, x15
10081d470:      ldur    w16, [x15, #-0x18]
10081d474:      cmp w23, w16
10081d478:      b.eq    0x10081d4c8 <_js_object_alloc_class_with_keys+0xa08>
10081d47c:      sub x15, x14, #0x2
10081d480:      ands    x14, x15, x14
10081d484:      b.ne    0x10081d458 <_js_object_alloc_class_with_keys+0x998>
10081d488:      cmeq.8b v2, v2, v1
10081d48c:      fmov    x14, d2
10081d490:      cbnz    x14, 0x10081d4b8 <_js_object_alloc_class_with_keys+0x9f8>
10081d494:      add x9, x9, #0x8
10081d498:      add x13, x13, x9
10081d49c:      and x13, x13, x10
10081d4a0:      ldr d2, [x11, x13]
10081d4a4:      cmeq.8b v3, v2, v0
10081d4a8:      fmov    x14, d3
10081d4ac:      ands    x14, x14, #0x8080808080808080
10081d4b0:      b.ne    0x10081d458 <_js_object_alloc_class_with_keys+0x998>
10081d4b4:      b   0x10081d488 <_js_object_alloc_class_with_keys+0x9c8>
10081d4b8:      mov w27, #0x0               ; =0
10081d4bc:      str x8, [x0, #0x5088]
10081d4c0:      tbz w25, #0x0, 0x10081d308 <_js_object_alloc_class_with_keys+0x848>
10081d4c4:      b   0x10081d310 <_js_object_alloc_class_with_keys+0x850>
10081d4c8:      ldur    w27, [x15, #-0x8]
10081d4cc:      str x8, [x0, #0x5088]
10081d4d0:      tbz w25, #0x0, 0x10081d308 <_js_object_alloc_class_with_keys+0x848>
10081d4d4:      b   0x10081d310 <_js_object_alloc_class_with_keys+0x850>
10081d4d8:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d4dc:      ldr x26, [x0, #0x20]
10081d4e0:      ldr x8, [x26]
10081d4e4:      cbz x8, 0x10081cbb0 <_js_object_alloc_class_with_keys+0xf0>
10081d4e8:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081d4ec:      add x0, x0, #0x120
10081d4f0:      bl  0x100ab12ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081d4f4:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d4f8:      b   0x10081cda8 <_js_object_alloc_class_with_keys+0x2e8>
10081d4fc:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d500:      ldr x8, [x0, #0x30]
10081d504:      ldrb    w8, [x8]
10081d508:      tbnz    w8, #0x0, 0x10081cde0 <_js_object_alloc_class_with_keys+0x320>
10081d50c:      b   0x10081ce44 <_js_object_alloc_class_with_keys+0x384>
10081d510:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d514:      b   0x10081cd6c <_js_object_alloc_class_with_keys+0x2ac>
10081d518:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081d51c:      add x0, x0, #0x798
10081d520:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081d524:      bl  0x100ab0fbc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081d528:      cmp w8, #0x1
10081d52c:      b.ne    0x10081d570 <_js_object_alloc_class_with_keys+0xab0>
10081d530:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081d534:      add x1, x1, #0xbb8
10081d538:      mov x0, x21
10081d53c:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081d540:      strb    wzr, [x21, #0x18]
10081d544:      ldr x27, [x21, #0x10]
10081d548:      ldr x8, [x21]
10081d54c:      cmp x27, x8
10081d550:      mov x0, x26
10081d554:      b.ne    0x10081ccac <_js_object_alloc_class_with_keys+0x1ec>
10081d558:      mov x0, x21
10081d55c:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081d560:      mov x0, x26
10081d564:      b   0x10081ccac <_js_object_alloc_class_with_keys+0x1ec>
10081d568:      cmp w8, #0x2
10081d56c:      b.ne    0x10081d57c <_js_object_alloc_class_with_keys+0xabc>
10081d570:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081d574:      add x0, x0, #0x850
10081d578:      bl  0x100aef71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081d57c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081d580:      add x1, x1, #0xbb8
10081d584:      mov x28, x0
10081d588:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081d58c:      mov x0, x28
10081d590:      strb    wzr, [x28, #0x18]
10081d594:      ldr x28, [x28, #0x10]
10081d598:      ldr x8, [x0]
10081d59c:      cmp x28, x8
10081d5a0:      b.ne    0x10081ce30 <_js_object_alloc_class_with_keys+0x370>
10081d5a4:      str x0, [sp]
10081d5a8:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081d5ac:      ldr x0, [sp]
10081d5b0:      b   0x10081ce30 <_js_object_alloc_class_with_keys+0x370>
10081d5b4:      mov w0, #0x8                ; =8
10081d5b8:      mov w1, #0x40               ; =64
10081d5bc:      bl  0x100ab0f64 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
