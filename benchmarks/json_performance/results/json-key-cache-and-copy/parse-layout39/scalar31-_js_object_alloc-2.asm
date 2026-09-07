/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081ce80 <_js_object_alloc_with_parent>:
10081ce80:      sub sp, sp, #0x90
10081ce84:      stp x28, x27, [sp, #0x30]
10081ce88:      stp x26, x25, [sp, #0x40]
10081ce8c:      stp x24, x23, [sp, #0x50]
10081ce90:      stp x22, x21, [sp, #0x60]
10081ce94:      stp x20, x19, [sp, #0x70]
10081ce98:      stp x29, x30, [sp, #0x80]
10081ce9c:      add x29, sp, #0x80
10081cea0:      mov x19, x2
10081cea4:      mov x20, x1
10081cea8:      mov x21, x0
10081ceac:      cbz w1, 0x10081cebc <_js_object_alloc_with_parent+0x3c>
10081ceb0:      mov x0, x21
10081ceb4:      mov x1, x20
10081ceb8:      bl  0x1006d12f0 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry13parent_static14register_class>
10081cebc:      mov w8, #0x2                ; =2
10081cec0:      cmp w19, #0x2
10081cec4:      csel    w25, w19, w8, hi
10081cec8:      ubfiz   x8, x25, #3, #32
10081cecc:      mov w9, #0x3ffd             ; =16381
10081ced0:      cmp w19, w9
10081ced4:      b.ls    0x10081cefc <_js_object_alloc_with_parent+0x7c>
10081ced8:      add x0, x8, #0x10
10081cedc:      mov w1, #0x8                ; =8
10081cee0:      mov w2, #0x2                ; =2
10081cee4:      bl  0x1004d9f18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081cee8:      mov x23, x0
10081ceec:      ldurb   w8, [x0, #-0x7]
10081cef0:      orr w8, w8, #0x20
10081cef4:      sturb   w8, [x0, #-0x7]
10081cef8:      b   0x10081d06c <_js_object_alloc_with_parent+0x1ec>
10081cefc:      add x22, x8, #0x18
10081cf00:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081cf04:      ldr x8, [x24, #0x48]
10081cf08:      cmn x8, #0x1
10081cf0c:      b.eq    0x10081d23c <_js_object_alloc_with_parent+0x3bc>
10081cf10:      mrs x9, TPIDRRO_EL0
10081cf14:      and x9, x9, #0xfffffffffffffff8
10081cf18:      ldr x0, [x9, x8, lsl #3]
10081cf1c:      cbz x0, 0x10081d23c <_js_object_alloc_with_parent+0x3bc>
10081cf20:      ldr x8, [x0, #0x28]
10081cf24:      ldrb    w8, [x8]
10081cf28:      tbz w8, #0x0, 0x10081cf94 <_js_object_alloc_with_parent+0x114>
10081cf2c:      ldr x8, [x24, #0x48]
10081cf30:      cmn x8, #0x1
10081cf34:      b.eq    0x10081d284 <_js_object_alloc_with_parent+0x404>
10081cf38:      mrs x9, TPIDRRO_EL0
10081cf3c:      and x9, x9, #0xfffffffffffffff8
10081cf40:      ldr x0, [x9, x8, lsl #3]
10081cf44:      cbz x0, 0x10081d284 <_js_object_alloc_with_parent+0x404>
10081cf48:      ldr x26, [x0, #0x20]
10081cf4c:      ldr x8, [x26]
10081cf50:      cbnz    x8, 0x10081d294 <_js_object_alloc_with_parent+0x414>
10081cf54:      mov x8, #-0x1               ; =-1
10081cf58:      str x8, [x26]
10081cf5c:      ldr x1, [x26, #0x18]
10081cf60:      cbz x1, 0x10081cf90 <_js_object_alloc_with_parent+0x110>
10081cf64:      mov x0, #0x0                ; =0
10081cf68:      ldr x8, [x26, #0x10]
10081cf6c:      lsl x10, x1, #4
10081cf70:      mov x9, x8
10081cf74:      ldr x11, [x9, #0x8]
10081cf78:      cmp x11, x22
10081cf7c:      b.eq    0x10081d124 <_js_object_alloc_with_parent+0x2a4>
10081cf80:      add x9, x9, #0x10
10081cf84:      add x0, x0, #0x1
10081cf88:      subs    x10, x10, #0x10
10081cf8c:      b.ne    0x10081cf74 <_js_object_alloc_with_parent+0xf4>
10081cf90:      str xzr, [x26]
10081cf94:      mov x0, x22
10081cf98:      bl  0x1004d9b5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081cf9c:      mov w8, #0x2                ; =2
10081cfa0:      strb    w8, [x0]
10081cfa4:      ldr x8, [x24, #0x48]
10081cfa8:      cmn x8, #0x1
10081cfac:      b.eq    0x10081d250 <_js_object_alloc_with_parent+0x3d0>
10081cfb0:      mrs x9, TPIDRRO_EL0
10081cfb4:      and x9, x9, #0xfffffffffffffff8
10081cfb8:      ldr x8, [x9, x8, lsl #3]
10081cfbc:      cbz x8, 0x10081d250 <_js_object_alloc_with_parent+0x3d0>
10081cfc0:      ldr x8, [x8, #0x30]
10081cfc4:      ldrb    w8, [x8]
10081cfc8:      orr w8, w8, #0x2
10081cfcc:      strb    w8, [x0, #0x1]
10081cfd0:      ldr x8, [x24, #0x48]
10081cfd4:      cmn x8, #0x1
10081cfd8:      b.eq    0x10081d264 <_js_object_alloc_with_parent+0x3e4>
10081cfdc:      mrs x9, TPIDRRO_EL0
10081cfe0:      and x9, x9, #0xfffffffffffffff8
10081cfe4:      ldr x8, [x9, x8, lsl #3]
10081cfe8:      cbz x8, 0x10081d264 <_js_object_alloc_with_parent+0x3e4>
10081cfec:      ldr x8, [x8, #0x30]
10081cff0:      ldrb    w8, [x8]
10081cff4:      tbz w8, #0x0, 0x10081d060 <_js_object_alloc_with_parent+0x1e0>
10081cff8:      ldrb    w8, [x0]
10081cffc:      sub w9, w8, #0x15
10081d000:      cmn w9, #0x14
10081d004:      b.lo    0x10081d060 <_js_object_alloc_with_parent+0x1e0>
10081d008:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d00c:      add x9, x9, #0xb10
10081d010:      add x8, x9, x8, lsl #5
10081d014:      ldrb    w8, [x8, #0x1b]
10081d018:      tbnz    w8, #0x0, 0x10081d060 <_js_object_alloc_with_parent+0x1e0>
10081d01c:      mov x24, x0
10081d020:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d024:      add x0, x0, #0x658
10081d028:      ldr x8, [x0]
10081d02c:      blr x8
10081d030:      mov x23, x0
10081d034:      ldrb    w8, [x0, #0x18]
10081d038:      cbnz    w8, 0x10081d2c8 <_js_object_alloc_with_parent+0x448>
10081d03c:      ldr x26, [x23, #0x10]
10081d040:      ldr x8, [x23]
10081d044:      cmp x26, x8
10081d048:      mov x0, x24
10081d04c:      b.eq    0x10081d2f8 <_js_object_alloc_with_parent+0x478>
10081d050:      ldr x8, [x23, #0x8]
10081d054:      str x0, [x8, x26, lsl #3]
10081d058:      add x8, x26, #0x1
10081d05c:      str x8, [x23, #0x10]
10081d060:      strh    wzr, [x0, #0x2]
10081d064:      str w22, [x0, #0x4]
10081d068:      add x23, x0, #0x8
10081d06c:      stp w21, w20, [x23]
10081d070:      lsl x2, x25, #3
10081d074:      str xzr, [x23, #0x8]
10081d078:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x61e8>
10081d07c:      add x1, x1, #0x3b0
10081d080:      add x0, x23, #0x10
10081d084:      bl  0x100af8850 <_writev+0x100af8850>
10081d088:      lsr x8, x23, #3
10081d08c:      cmp x8, #0x201
10081d090:      b.lo    0x10081d0e4 <_js_object_alloc_with_parent+0x264>
10081d094:      ldurb   w8, [x23, #-0x8]
10081d098:      sub w9, w8, #0x15
10081d09c:      cmn w9, #0x14
10081d0a0:      b.lo    0x10081d0e4 <_js_object_alloc_with_parent+0x264>
10081d0a4:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d0a8:      add x9, x9, #0xb10
10081d0ac:      add x8, x9, x8, lsl #5
10081d0b0:      ldrb    w8, [x8, #0x14]
10081d0b4:      mov w9, #0x16               ; =22
10081d0b8:      lsr w8, w9, w8
10081d0bc:      tbz w8, #0x0, 0x10081d0e4 <_js_object_alloc_with_parent+0x264>
10081d0c0:      ldurh   w8, [x23, #-0x6]
10081d0c4:      mov w9, #0x4000             ; =16384
10081d0c8:      bfxil   w9, w8, #0, #13
10081d0cc:      sturh   w9, [x23, #-0x6]
10081d0d0:      mov x0, x23
10081d0d4:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081d0d8:      ldurh   w8, [x23, #-0x6]
10081d0dc:      and w8, w8, #0xffffefff
10081d0e0:      sturh   w8, [x23, #-0x6]
10081d0e4:      mov w8, #0x2                ; =2
10081d0e8:      strb    w8, [sp, #0x2e]
10081d0ec:      add x1, sp, #0x8
10081d0f0:      mov x0, x23
10081d0f4:      mov x2, #0x0                ; =0
10081d0f8:      mov x3, x19
10081d0fc:      bl  0x10059802c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25publish_object_shape_from>
10081d100:      mov x0, x23
10081d104:      ldp x29, x30, [sp, #0x80]
10081d108:      ldp x20, x19, [sp, #0x70]
10081d10c:      ldp x22, x21, [sp, #0x60]
10081d110:      ldp x24, x23, [sp, #0x50]
10081d114:      ldp x26, x25, [sp, #0x40]
10081d118:      ldp x28, x27, [sp, #0x30]
10081d11c:      add sp, sp, #0x90
10081d120:      ret
10081d124:      cmp x0, x1
10081d128:      b.hs    0x10081d2c4 <_js_object_alloc_with_parent+0x444>
10081d12c:      ldr x23, [x9]
10081d130:      subs    x10, x1, #0x1
10081d134:      ldr q0, [x8, x10, lsl #4]
10081d138:      str q0, [x9]
10081d13c:      str x10, [x26, #0x18]
10081d140:      b.ne    0x10081d168 <_js_object_alloc_with_parent+0x2e8>
10081d144:      ldr x8, [x24, #0x48]
10081d148:      cmn x8, #0x1
10081d14c:      b.eq    0x10081d2bc <_js_object_alloc_with_parent+0x43c>
10081d150:      mrs x9, TPIDRRO_EL0
10081d154:      and x9, x9, #0xfffffffffffffff8
10081d158:      ldr x0, [x9, x8, lsl #3]
10081d15c:      cbz x0, 0x10081d2bc <_js_object_alloc_with_parent+0x43c>
10081d160:      ldr x8, [x0, #0x28]
10081d164:      strb    wzr, [x8]
10081d168:      ldr x8, [x26]
10081d16c:      add x8, x8, #0x1
10081d170:      str x8, [x26]
10081d174:      mov w8, #0x2                ; =2
10081d178:      mov x26, x23
10081d17c:      strb    w8, [x26, #-0x8]!
10081d180:      ldr x8, [x24, #0x48]
10081d184:      cmn x8, #0x1
10081d188:      b.eq    0x10081d2a0 <_js_object_alloc_with_parent+0x420>
10081d18c:      mrs x9, TPIDRRO_EL0
10081d190:      and x9, x9, #0xfffffffffffffff8
10081d194:      ldr x0, [x9, x8, lsl #3]
10081d198:      cbz x0, 0x10081d2a0 <_js_object_alloc_with_parent+0x420>
10081d19c:      ldr x8, [x0, #0x30]
10081d1a0:      ldrb    w8, [x8]
10081d1a4:      orr w8, w8, #0x2
10081d1a8:      sturb   w8, [x23, #-0x7]
10081d1ac:      ldr x8, [x24, #0x48]
10081d1b0:      cmn x8, #0x1
10081d1b4:      b.eq    0x10081d2a8 <_js_object_alloc_with_parent+0x428>
10081d1b8:      mrs x9, TPIDRRO_EL0
10081d1bc:      and x9, x9, #0xfffffffffffffff8
10081d1c0:      ldr x0, [x9, x8, lsl #3]
10081d1c4:      cbz x0, 0x10081d2a8 <_js_object_alloc_with_parent+0x428>
10081d1c8:      ldr x8, [x0, #0x30]
10081d1cc:      ldrb    w8, [x8]
10081d1d0:      tbz w8, #0x0, 0x10081d230 <_js_object_alloc_with_parent+0x3b0>
10081d1d4:      ldrb    w8, [x26]
10081d1d8:      sub w9, w8, #0x15
10081d1dc:      cmn w9, #0x14
10081d1e0:      b.lo    0x10081d230 <_js_object_alloc_with_parent+0x3b0>
10081d1e4:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d1e8:      add x9, x9, #0xb10
10081d1ec:      add x8, x9, x8, lsl #5
10081d1f0:      ldrb    w8, [x8, #0x1b]
10081d1f4:      tbnz    w8, #0x0, 0x10081d230 <_js_object_alloc_with_parent+0x3b0>
10081d1f8:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d1fc:      add x0, x0, #0x658
10081d200:      ldr x8, [x0]
10081d204:      blr x8
10081d208:      ldrb    w8, [x0, #0x18]
10081d20c:      cbnz    w8, 0x10081d308 <_js_object_alloc_with_parent+0x488>
10081d210:      ldr x27, [x0, #0x10]
10081d214:      ldr x8, [x0]
10081d218:      cmp x27, x8
10081d21c:      b.eq    0x10081d344 <_js_object_alloc_with_parent+0x4c4>
10081d220:      ldr x8, [x0, #0x8]
10081d224:      str x26, [x8, x27, lsl #3]
10081d228:      add x8, x27, #0x1
10081d22c:      str x8, [x0, #0x10]
10081d230:      sturh   wzr, [x23, #-0x6]
10081d234:      stur    w22, [x23, #-0x4]
10081d238:      b   0x10081d06c <_js_object_alloc_with_parent+0x1ec>
10081d23c:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d240:      ldr x8, [x0, #0x28]
10081d244:      ldrb    w8, [x8]
10081d248:      tbnz    w8, #0x0, 0x10081cf2c <_js_object_alloc_with_parent+0xac>
10081d24c:      b   0x10081cf94 <_js_object_alloc_with_parent+0x114>
10081d250:      mov x23, x0
10081d254:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d258:      mov x8, x0
10081d25c:      mov x0, x23
10081d260:      b   0x10081cfc0 <_js_object_alloc_with_parent+0x140>
10081d264:      mov x23, x0
10081d268:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d26c:      mov x8, x0
10081d270:      mov x0, x23
10081d274:      ldr x8, [x8, #0x30]
10081d278:      ldrb    w8, [x8]
10081d27c:      tbnz    w8, #0x0, 0x10081cff8 <_js_object_alloc_with_parent+0x178>
10081d280:      b   0x10081d060 <_js_object_alloc_with_parent+0x1e0>
10081d284:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d288:      ldr x26, [x0, #0x20]
10081d28c:      ldr x8, [x26]
10081d290:      cbz x8, 0x10081cf54 <_js_object_alloc_with_parent+0xd4>
10081d294:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081d298:      add x0, x0, #0x120
10081d29c:      bl  0x100ab0bac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081d2a0:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d2a4:      b   0x10081d19c <_js_object_alloc_with_parent+0x31c>
10081d2a8:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d2ac:      ldr x8, [x0, #0x30]
10081d2b0:      ldrb    w8, [x8]
10081d2b4:      tbnz    w8, #0x0, 0x10081d1d4 <_js_object_alloc_with_parent+0x354>
10081d2b8:      b   0x10081d230 <_js_object_alloc_with_parent+0x3b0>
10081d2bc:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d2c0:      b   0x10081d160 <_js_object_alloc_with_parent+0x2e0>
10081d2c4:      bl  0x100ab087c <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081d2c8:      cmp w8, #0x1
10081d2cc:      b.ne    0x10081d310 <_js_object_alloc_with_parent+0x490>
10081d2d0:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081d2d4:      add x1, x1, #0xbb8
10081d2d8:      mov x0, x23
10081d2dc:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081d2e0:      strb    wzr, [x23, #0x18]
10081d2e4:      ldr x26, [x23, #0x10]
10081d2e8:      ldr x8, [x23]
10081d2ec:      cmp x26, x8
10081d2f0:      mov x0, x24
10081d2f4:      b.ne    0x10081d050 <_js_object_alloc_with_parent+0x1d0>
10081d2f8:      mov x0, x23
10081d2fc:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081d300:      mov x0, x24
10081d304:      b   0x10081d050 <_js_object_alloc_with_parent+0x1d0>
10081d308:      cmp w8, #0x2
10081d30c:      b.ne    0x10081d31c <_js_object_alloc_with_parent+0x49c>
10081d310:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081d314:      add x0, x0, #0x850
10081d318:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081d31c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081d320:      add x1, x1, #0xbb8
10081d324:      mov x24, x0
10081d328:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081d32c:      mov x0, x24
10081d330:      strb    wzr, [x24, #0x18]
10081d334:      ldr x27, [x24, #0x10]
10081d338:      ldr x8, [x24]
10081d33c:      cmp x27, x8
10081d340:      b.ne    0x10081d220 <_js_object_alloc_with_parent+0x3a0>
10081d344:      mov x24, x0
10081d348:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081d34c:      mov x0, x24
10081d350:      b   0x10081d220 <_js_object_alloc_with_parent+0x3a0>
10081d354:      nop
10081d358:      nop
10081d35c:      nop
10081d360:      nop
10081d364:      nop
10081d368:      nop
10081d36c:      nop
10081d370:      nop
10081d374:      nop
10081d378:      nop
10081d37c:      nop
