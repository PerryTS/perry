/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081bf00 <_js_object_alloc_class_dynamic_parent>:
10081bf00:      sub sp, sp, #0xa0
10081bf04:      stp x28, x27, [sp, #0x40]
10081bf08:      stp x26, x25, [sp, #0x50]
10081bf0c:      stp x24, x23, [sp, #0x60]
10081bf10:      stp x22, x21, [sp, #0x70]
10081bf14:      stp x20, x19, [sp, #0x80]
10081bf18:      stp x29, x30, [sp, #0x90]
10081bf1c:      add x29, sp, #0x90
10081bf20:      mov x27, x3
10081bf24:      mov x22, x2
10081bf28:      mov x21, x1
10081bf2c:      mov x19, x0
10081bf30:      bl  0x10057e060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object19class_meta_registry19get_parent_class_id>
10081bf34:      mov x20, x1
10081bf38:      mov w1, #0x0                ; =0
10081bf3c:      cmp w0, #0x1
10081bf40:      b.ne    0x10081bfe8 <_js_object_alloc_class_dynamic_parent+0xe8>
10081bf44:      cbz w20, 0x10081bfe8 <_js_object_alloc_class_dynamic_parent+0xe8>
10081bf48:      add x0, sp, #0x10
10081bf4c:      mov x1, x20
10081bf50:      bl  0x1005923d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc27registered_class_keys_array>
10081bf54:      ldr w8, [sp, #0x10]
10081bf58:      tbz w8, #0x0, 0x10081bfe4 <_js_object_alloc_class_dynamic_parent+0xe4>
10081bf5c:      ldr x26, [sp, #0x18]
10081bf60:      ldr w24, [x26]
10081bf64:      mov w21, #0x2717            ; =10007
10081bf68:      mov w23, #0x8480            ; =33920
10081bf6c:      movk    w23, #0x1e, lsl #16
10081bf70:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081bf74:      ldr w25, [x8, #0x634]
10081bf78:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081bf7c:      cmp w25, #0x300
10081bf80:      b.hs    0x10081c028 <_js_object_alloc_class_dynamic_parent+0x128>
10081bf84:      ldr x8, [x28, #0x48]
10081bf88:      cmn x8, #0x1
10081bf8c:      b.eq    0x10081c018 <_js_object_alloc_class_dynamic_parent+0x118>
10081bf90:      mrs x9, TPIDRRO_EL0
10081bf94:      and x9, x9, #0xfffffffffffffff8
10081bf98:      ldr x0, [x9, x8, lsl #3]
10081bf9c:      cbz x0, 0x10081c018 <_js_object_alloc_class_dynamic_parent+0x118>
10081bfa0:      add x8, x0, x25, lsl #3
10081bfa4:      ldr x0, [x8, #0x1e8]
10081bfa8:      cbz x0, 0x10081c028 <_js_object_alloc_class_dynamic_parent+0x128>
10081bfac:      madd    w23, w19, w21, w23
10081bfb0:      ldr x0, [x0]
10081bfb4:      cbz x0, 0x10081c040 <_js_object_alloc_class_dynamic_parent+0x140>
10081bfb8:      and w8, w23, #0xff
10081bfbc:      add x8, x0, w8, uxtw #4
10081bfc0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081bfc4:      ldr w9, [x8]
10081bfc8:      cmp w9, w23
10081bfcc:      b.ne    0x10081c05c <_js_object_alloc_class_dynamic_parent+0x15c>
10081bfd0:      ldr x21, [x8, #0x8]
10081bfd4:      ldr w25, [x8, #0x4]
10081bfd8:      cbz x21, 0x10081c138 <_js_object_alloc_class_dynamic_parent+0x238>
10081bfdc:      ldr w22, [x21]
10081bfe0:      b   0x10081c488 <_js_object_alloc_class_dynamic_parent+0x588>
10081bfe4:      mov x1, x20
10081bfe8:      mov x0, x19
10081bfec:      mov x2, x21
10081bff0:      mov x3, x22
10081bff4:      mov x4, x27
10081bff8:      ldp x29, x30, [sp, #0x90]
10081bffc:      ldp x20, x19, [sp, #0x80]
10081c000:      ldp x22, x21, [sp, #0x70]
10081c004:      ldp x24, x23, [sp, #0x60]
10081c008:      ldp x26, x25, [sp, #0x50]
10081c00c:      ldp x28, x27, [sp, #0x40]
10081c010:      add sp, sp, #0xa0
10081c014:      b   0x10081cac0 <_js_object_alloc_class_with_keys>
10081c018:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c01c:      add x8, x0, x25, lsl #3
10081c020:      ldr x0, [x8, #0x1e8]
10081c024:      cbnz    x0, 0x10081bfac <_js_object_alloc_class_dynamic_parent+0xac>
10081c028:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081c02c:      add x0, x0, #0x330
10081c030:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081c034:      madd    w23, w19, w21, w23
10081c038:      ldr x0, [x0]
10081c03c:      cbnz    x0, 0x10081bfb8 <_js_object_alloc_class_dynamic_parent+0xb8>
10081c040:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081c044:      and w8, w23, #0xff
10081c048:      add x8, x0, w8, uxtw #4
10081c04c:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c050:      ldr w9, [x8]
10081c054:      cmp w9, w23
10081c058:      b.eq    0x10081bfd0 <_js_object_alloc_class_dynamic_parent+0xd0>
10081c05c:      ldr x8, [x0, #0x5088]
10081c060:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081c064:      cmp x8, x9
10081c068:      b.hs    0x10081c9e4 <_js_object_alloc_class_dynamic_parent+0xae4>
10081c06c:      add x9, x8, #0x1
10081c070:      str x9, [x0, #0x5088]
10081c074:      ldr x9, [x0, #0x50a8]
10081c078:      cbz x9, 0x10081c128 <_js_object_alloc_class_dynamic_parent+0x228>
10081c07c:      mov x9, #0x0                ; =0
10081c080:      mov x10, #0x7c15            ; =31765
10081c084:      movk    x10, #0x7f4a, lsl #16
10081c088:      movk    x10, #0x79b9, lsl #32
10081c08c:      movk    x10, #0x9e37, lsl #48
10081c090:      mul x10, x23, x10
10081c094:      eor x13, x10, x10, lsr #32
10081c098:      lsr x12, x10, #57
10081c09c:      ldr x10, [x0, #0x5098]
10081c0a0:      ldr x11, [x0, #0x5090]
10081c0a4:      dup.8b  v0, w12
10081c0a8:      movi.2d v1, #0xffffffffffffffff
10081c0ac:      mov w12, #0x18              ; =24
10081c0b0:      and x13, x13, x10
10081c0b4:      ldr d2, [x11, x13]
10081c0b8:      cmeq.8b v3, v2, v0
10081c0bc:      fmov    x14, d3
10081c0c0:      ands    x14, x14, #0x8080808080808080
10081c0c4:      b.eq    0x10081c0f8 <_js_object_alloc_class_dynamic_parent+0x1f8>
10081c0c8:      rbit    x15, x14
10081c0cc:      clz x15, x15
10081c0d0:      add x15, x13, x15, lsr #3
10081c0d4:      and x15, x15, x10
10081c0d8:      mneg    x15, x15, x12
10081c0dc:      add x15, x11, x15
10081c0e0:      ldur    w16, [x15, #-0x18]
10081c0e4:      cmp w23, w16
10081c0e8:      b.eq    0x10081c1c8 <_js_object_alloc_class_dynamic_parent+0x2c8>
10081c0ec:      sub x15, x14, #0x2
10081c0f0:      ands    x14, x15, x14
10081c0f4:      b.ne    0x10081c0c8 <_js_object_alloc_class_dynamic_parent+0x1c8>
10081c0f8:      cmeq.8b v2, v2, v1
10081c0fc:      fmov    x14, d2
10081c100:      cbnz    x14, 0x10081c128 <_js_object_alloc_class_dynamic_parent+0x228>
10081c104:      add x9, x9, #0x8
10081c108:      add x13, x13, x9
10081c10c:      and x13, x13, x10
10081c110:      ldr d2, [x11, x13]
10081c114:      cmeq.8b v3, v2, v0
10081c118:      fmov    x14, d3
10081c11c:      ands    x14, x14, #0x8080808080808080
10081c120:      b.ne    0x10081c0c8 <_js_object_alloc_class_dynamic_parent+0x1c8>
10081c124:      b   0x10081c0f8 <_js_object_alloc_class_dynamic_parent+0x1f8>
10081c128:      mov w25, #0x0               ; =0
10081c12c:      mov x21, #0x0               ; =0
10081c130:      str x8, [x0, #0x5088]
10081c134:      cbnz    x21, 0x10081bfdc <_js_object_alloc_class_dynamic_parent+0xdc>
10081c138:      mov x25, #0x0               ; =0
10081c13c:      mov w9, #0x1                ; =1
10081c140:      mov w8, #0x8                ; =8
10081c144:      str x8, [sp, #0x8]
10081c148:      cbz x22, 0x10081c2d4 <_js_object_alloc_class_dynamic_parent+0x3d4>
10081c14c:      cbz w27, 0x10081c2d4 <_js_object_alloc_class_dynamic_parent+0x3d4>
10081c150:      str x26, [sp]
10081c154:      mov w26, #0x0               ; =0
10081c158:      mov w8, #0x0                ; =0
10081c15c:      mov w9, #0x8                ; =8
10081c160:      str x9, [sp, #0x8]
10081c164:      mov w21, w27
10081c168:      b   0x10081c17c <_js_object_alloc_class_dynamic_parent+0x27c>
10081c16c:      mov w26, #0x1               ; =1
10081c170:      mov x22, x25
10081c174:      mov w8, #0x1                ; =1
10081c178:      cbnz    x27, 0x10081c1e8 <_js_object_alloc_class_dynamic_parent+0x2e8>
10081c17c:      tbnz    w8, #0x0, 0x10081c1dc <_js_object_alloc_class_dynamic_parent+0x2dc>
10081c180:      mov x25, x22
10081c184:      mov x27, #0x0               ; =0
10081c188:      cbz x21, 0x10081c16c <_js_object_alloc_class_dynamic_parent+0x26c>
10081c18c:      ldrb    w8, [x25, x27]
10081c190:      cbz w8, 0x10081c1b4 <_js_object_alloc_class_dynamic_parent+0x2b4>
10081c194:      add x27, x27, #0x1
10081c198:      cmp x21, x27
10081c19c:      b.ne    0x10081c18c <_js_object_alloc_class_dynamic_parent+0x28c>
10081c1a0:      mov w26, #0x1               ; =1
10081c1a4:      mov x22, x25
10081c1a8:      mov w8, #0x1                ; =1
10081c1ac:      mov x27, x21
10081c1b0:      b   0x10081c178 <_js_object_alloc_class_dynamic_parent+0x278>
10081c1b4:      mvn x9, x27
10081c1b8:      add x21, x9, x21
10081c1bc:      add x9, x25, x27
10081c1c0:      add x22, x9, #0x1
10081c1c4:      b   0x10081c178 <_js_object_alloc_class_dynamic_parent+0x278>
10081c1c8:      ldur    x21, [x15, #-0x10]
10081c1cc:      ldur    w25, [x15, #-0x8]
10081c1d0:      str x8, [x0, #0x5088]
10081c1d4:      cbnz    x21, 0x10081bfdc <_js_object_alloc_class_dynamic_parent+0xdc>
10081c1d8:      b   0x10081c138 <_js_object_alloc_class_dynamic_parent+0x238>
10081c1dc:      mov x25, #0x0               ; =0
10081c1e0:      mov w9, #0x1                ; =1
10081c1e4:      b   0x10081c2d0 <_js_object_alloc_class_dynamic_parent+0x3d0>
10081c1e8:      mov w0, #0x40               ; =64
10081c1ec:      mov w1, #0x8                ; =8
10081c1f0:      bl  0x100af6348 <_mi_malloc_aligned>
10081c1f4:      cbz x0, 0x10081ca80 <_js_object_alloc_class_dynamic_parent+0xb80>
10081c1f8:      stp x25, x27, [x0]
10081c1fc:      mov w8, #0x4                ; =4
10081c200:      stp x8, x0, [sp, #0x28]
10081c204:      mov w25, #0x1               ; =1
10081c208:      str x25, [sp, #0x38]
10081c20c:      mov x8, x21
10081c210:      mov x10, x22
10081c214:      mov x9, x26
10081c218:      b   0x10081c22c <_js_object_alloc_class_dynamic_parent+0x32c>
10081c21c:      mov w26, #0x1               ; =1
10081c220:      mov x10, x27
10081c224:      mov w9, #0x1                ; =1
10081c228:      cbnz    x28, 0x10081c280 <_js_object_alloc_class_dynamic_parent+0x380>
10081c22c:      tbnz    w9, #0x0, 0x10081c2bc <_js_object_alloc_class_dynamic_parent+0x3bc>
10081c230:      mov x27, x10
10081c234:      mov x28, #0x0               ; =0
10081c238:      cbz x8, 0x10081c21c <_js_object_alloc_class_dynamic_parent+0x31c>
10081c23c:      ldrb    w9, [x27, x28]
10081c240:      cbz w9, 0x10081c264 <_js_object_alloc_class_dynamic_parent+0x364>
10081c244:      add x28, x28, #0x1
10081c248:      cmp x8, x28
10081c24c:      b.ne    0x10081c23c <_js_object_alloc_class_dynamic_parent+0x33c>
10081c250:      mov w26, #0x1               ; =1
10081c254:      mov x10, x27
10081c258:      mov w9, #0x1                ; =1
10081c25c:      mov x28, x8
10081c260:      b   0x10081c228 <_js_object_alloc_class_dynamic_parent+0x328>
10081c264:      mvn x10, x28
10081c268:      add x21, x10, x8
10081c26c:      add x8, x27, x28
10081c270:      add x22, x8, #0x1
10081c274:      mov x8, x21
10081c278:      mov x10, x22
10081c27c:      b   0x10081c228 <_js_object_alloc_class_dynamic_parent+0x328>
10081c280:      ldr x8, [sp, #0x28]
10081c284:      cmp x25, x8
10081c288:      b.eq    0x10081c29c <_js_object_alloc_class_dynamic_parent+0x39c>
10081c28c:      add x8, x0, x25, lsl #4
10081c290:      stp x27, x28, [x8]
10081c294:      add x25, x25, #0x1
10081c298:      b   0x10081c208 <_js_object_alloc_class_dynamic_parent+0x308>
10081c29c:      add x0, sp, #0x28
10081c2a0:      mov x1, x25
10081c2a4:      mov w2, #0x1                ; =1
10081c2a8:      mov w3, #0x8                ; =8
10081c2ac:      mov w4, #0x10               ; =16
10081c2b0:      bl  0x100ad59d0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081c2b4:      ldr x0, [sp, #0x30]
10081c2b8:      b   0x10081c28c <_js_object_alloc_class_dynamic_parent+0x38c>
10081c2bc:      ldp x8, x9, [sp, #0x28]
10081c2c0:      str x9, [sp, #0x8]
10081c2c4:      cmp x8, #0x0
10081c2c8:      cset    w9, eq
10081c2cc:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c2d0:      ldr x26, [sp]
10081c2d4:      str w9, [sp]
10081c2d8:      add w22, w24, w25
10081c2dc:      ubfiz   x8, x22, #3, #32
10081c2e0:      add x0, x8, #0x8
10081c2e4:      mov w1, #0x8                ; =8
10081c2e8:      mov w2, #0x1                ; =1
10081c2ec:      bl  0x1004db0e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081c2f0:      mov x21, x0
10081c2f4:      stp w22, w22, [x0]
10081c2f8:      lsr x8, x0, #3
10081c2fc:      cmp x8, #0x201
10081c300:      b.lo    0x10081c390 <_js_object_alloc_class_dynamic_parent+0x490>
10081c304:      ldurb   w8, [x21, #-0x8]
10081c308:      cmp w8, #0x1
10081c30c:      b.ne    0x10081c338 <_js_object_alloc_class_dynamic_parent+0x438>
10081c310:      ldurh   w8, [x21, #-0x6]
10081c314:      mov w9, #0x1080             ; =4224
10081c318:      mov w10, #0xef7f            ; =61311
10081c31c:      and w10, w8, w10
10081c320:      sturh   w10, [x21, #-0x6]
10081c324:      tst w8, w9
10081c328:      b.eq    0x10081c348 <_js_object_alloc_class_dynamic_parent+0x448>
10081c32c:      mov x0, x21
10081c330:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081c334:      ldurb   w8, [x21, #-0x8]
10081c338:      sub w9, w8, #0x15
10081c33c:      cmn w9, #0x14
10081c340:      b.hs    0x10081c34c <_js_object_alloc_class_dynamic_parent+0x44c>
10081c344:      b   0x10081c390 <_js_object_alloc_class_dynamic_parent+0x490>
10081c348:      mov w8, #0x1                ; =1
10081c34c:      mov w8, w8
10081c350:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c354:      add x9, x9, #0xb10
10081c358:      add x8, x9, x8, lsl #5
10081c35c:      ldrb    w8, [x8, #0x14]
10081c360:      mov w9, #0x16               ; =22
10081c364:      lsr w8, w9, w8
10081c368:      tbz w8, #0x0, 0x10081c390 <_js_object_alloc_class_dynamic_parent+0x490>
10081c36c:      ldurh   w8, [x21, #-0x6]
10081c370:      mov w9, #0x4000             ; =16384
10081c374:      bfxil   w9, w8, #0, #13
10081c378:      sturh   w9, [x21, #-0x6]
10081c37c:      mov x0, x21
10081c380:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081c384:      ldurh   w8, [x21, #-0x6]
10081c388:      and w8, w8, #0xffffefff
10081c38c:      sturh   w8, [x21, #-0x6]
10081c390:      cbz w24, 0x10081c3c8 <_js_object_alloc_class_dynamic_parent+0x4c8>
10081c394:      mov x1, #0x0                ; =0
10081c398:      add x26, x26, #0x8
10081c39c:      add x27, x1, #0x1
10081c3a0:      lsl x8, x1, #3
10081c3a4:      ldr d0, [x26, x8]
10081c3a8:      add x8, x21, x8
10081c3ac:      str d0, [x8, #0x8]
10081c3b0:      fmov    x2, d0
10081c3b4:      mov x0, x21
10081c3b8:      bl  0x1004f0edc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081c3bc:      mov x1, x27
10081c3c0:      cmp x24, x27
10081c3c4:      b.ne    0x10081c39c <_js_object_alloc_class_dynamic_parent+0x49c>
10081c3c8:      ldr x27, [sp, #0x8]
10081c3cc:      cbz x25, 0x10081c410 <_js_object_alloc_class_dynamic_parent+0x510>
10081c3d0:      add x25, x27, x25, lsl #4
10081c3d4:      mov x26, x27
10081c3d8:      ldr x0, [x26]
10081c3dc:      ldr w1, [x26, #0x8]
10081c3e0:      bl  0x100884f94 <_js_string_from_bytes_longlived>
10081c3e4:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081c3e8:      bfxil   x2, x0, #0, #48
10081c3ec:      add x8, x21, x24, lsl #3
10081c3f0:      str x2, [x8, #0x8]
10081c3f4:      mov x0, x21
10081c3f8:      mov x1, x24
10081c3fc:      bl  0x1004f0edc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081c400:      add x24, x24, #0x1
10081c404:      add x26, x26, #0x10
10081c408:      cmp x26, x25
10081c40c:      b.ne    0x10081c3d8 <_js_object_alloc_class_dynamic_parent+0x4d8>
10081c410:      mov x0, x23
10081c414:      mov x1, x21
10081c418:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081c41c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c420:      ldr w24, [x8, #0x634]
10081c424:      cmp w24, #0x300
10081c428:      and w10, w23, #0xff
10081c42c:      b.hs    0x10081c87c <_js_object_alloc_class_dynamic_parent+0x97c>
10081c430:      ldr x8, [x28, #0x48]
10081c434:      cmn x8, #0x1
10081c438:      b.eq    0x10081c868 <_js_object_alloc_class_dynamic_parent+0x968>
10081c43c:      mrs x9, TPIDRRO_EL0
10081c440:      and x9, x9, #0xfffffffffffffff8
10081c444:      ldr x0, [x9, x8, lsl #3]
10081c448:      cbz x0, 0x10081c868 <_js_object_alloc_class_dynamic_parent+0x968>
10081c44c:      add x8, x0, x24, lsl #3
10081c450:      ldr x0, [x8, #0x1e8]
10081c454:      cbz x0, 0x10081c87c <_js_object_alloc_class_dynamic_parent+0x97c>
10081c458:      ldr x0, [x0]
10081c45c:      cbz x0, 0x10081c894 <_js_object_alloc_class_dynamic_parent+0x994>
10081c460:      add x8, x0, x10, lsl #4
10081c464:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c468:      ldr w9, [x8]
10081c46c:      cmp w9, w23
10081c470:      b.ne    0x10081c8b0 <_js_object_alloc_class_dynamic_parent+0x9b0>
10081c474:      ldr w25, [x8, #0x4]
10081c478:      ldr w8, [sp]
10081c47c:      tbnz    w8, #0x0, 0x10081c488 <_js_object_alloc_class_dynamic_parent+0x588>
10081c480:      mov x0, x27
10081c484:      bl  0x100af60c0 <_mi_free>
10081c488:      mov w8, #0x2                ; =2
10081c48c:      cmp w22, #0x2
10081c490:      csel    w27, w22, w8, hi
10081c494:      ubfiz   x8, x27, #3, #32
10081c498:      mov w9, #0x3ffd             ; =16381
10081c49c:      cmp w22, w9
10081c4a0:      b.ls    0x10081c4c8 <_js_object_alloc_class_dynamic_parent+0x5c8>
10081c4a4:      add x0, x8, #0x10
10081c4a8:      mov w1, #0x8                ; =8
10081c4ac:      mov w2, #0x2                ; =2
10081c4b0:      bl  0x1004da658 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081c4b4:      mov x24, x0
10081c4b8:      ldurb   w8, [x0, #-0x7]
10081c4bc:      orr w8, w8, #0x20
10081c4c0:      sturb   w8, [x0, #-0x7]
10081c4c4:      b   0x10081c634 <_js_object_alloc_class_dynamic_parent+0x734>
10081c4c8:      add x23, x8, #0x18
10081c4cc:      ldr x8, [x28, #0x48]
10081c4d0:      cmn x8, #0x1
10081c4d4:      b.eq    0x10081c820 <_js_object_alloc_class_dynamic_parent+0x920>
10081c4d8:      mrs x9, TPIDRRO_EL0
10081c4dc:      and x9, x9, #0xfffffffffffffff8
10081c4e0:      ldr x0, [x9, x8, lsl #3]
10081c4e4:      cbz x0, 0x10081c820 <_js_object_alloc_class_dynamic_parent+0x920>
10081c4e8:      ldr x8, [x0, #0x28]
10081c4ec:      ldrb    w8, [x8]
10081c4f0:      tbz w8, #0x0, 0x10081c55c <_js_object_alloc_class_dynamic_parent+0x65c>
10081c4f4:      ldr x8, [x28, #0x48]
10081c4f8:      cmn x8, #0x1
10081c4fc:      b.eq    0x10081c9a4 <_js_object_alloc_class_dynamic_parent+0xaa4>
10081c500:      mrs x9, TPIDRRO_EL0
10081c504:      and x9, x9, #0xfffffffffffffff8
10081c508:      ldr x0, [x9, x8, lsl #3]
10081c50c:      cbz x0, 0x10081c9a4 <_js_object_alloc_class_dynamic_parent+0xaa4>
10081c510:      ldr x26, [x0, #0x20]
10081c514:      ldr x8, [x26]
10081c518:      cbnz    x8, 0x10081c9b4 <_js_object_alloc_class_dynamic_parent+0xab4>
10081c51c:      mov x8, #-0x1               ; =-1
10081c520:      str x8, [x26]
10081c524:      ldr x1, [x26, #0x18]
10081c528:      cbz x1, 0x10081c558 <_js_object_alloc_class_dynamic_parent+0x658>
10081c52c:      mov x0, #0x0                ; =0
10081c530:      ldr x8, [x26, #0x10]
10081c534:      lsl x10, x1, #4
10081c538:      mov x9, x8
10081c53c:      ldr x11, [x9, #0x8]
10081c540:      cmp x11, x23
10081c544:      b.eq    0x10081c700 <_js_object_alloc_class_dynamic_parent+0x800>
10081c548:      add x0, x0, #0x1
10081c54c:      add x9, x9, #0x10
10081c550:      subs    x10, x10, #0x10
10081c554:      b.ne    0x10081c53c <_js_object_alloc_class_dynamic_parent+0x63c>
10081c558:      str xzr, [x26]
10081c55c:      mov x0, x23
10081c560:      bl  0x1004da29c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081c564:      mov w8, #0x2                ; =2
10081c568:      strb    w8, [x0]
10081c56c:      ldr x8, [x28, #0x48]
10081c570:      cmn x8, #0x1
10081c574:      b.eq    0x10081c834 <_js_object_alloc_class_dynamic_parent+0x934>
10081c578:      mrs x9, TPIDRRO_EL0
10081c57c:      and x9, x9, #0xfffffffffffffff8
10081c580:      ldr x8, [x9, x8, lsl #3]
10081c584:      cbz x8, 0x10081c834 <_js_object_alloc_class_dynamic_parent+0x934>
10081c588:      ldr x8, [x8, #0x30]
10081c58c:      ldrb    w8, [x8]
10081c590:      orr w8, w8, #0x2
10081c594:      strb    w8, [x0, #0x1]
10081c598:      ldr x8, [x28, #0x48]
10081c59c:      cmn x8, #0x1
10081c5a0:      b.eq    0x10081c848 <_js_object_alloc_class_dynamic_parent+0x948>
10081c5a4:      mrs x9, TPIDRRO_EL0
10081c5a8:      and x9, x9, #0xfffffffffffffff8
10081c5ac:      ldr x8, [x9, x8, lsl #3]
10081c5b0:      cbz x8, 0x10081c848 <_js_object_alloc_class_dynamic_parent+0x948>
10081c5b4:      ldr x8, [x8, #0x30]
10081c5b8:      ldrb    w8, [x8]
10081c5bc:      tbz w8, #0x0, 0x10081c628 <_js_object_alloc_class_dynamic_parent+0x728>
10081c5c0:      ldrb    w8, [x0]
10081c5c4:      sub w9, w8, #0x15
10081c5c8:      cmn w9, #0x14
10081c5cc:      b.lo    0x10081c628 <_js_object_alloc_class_dynamic_parent+0x728>
10081c5d0:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c5d4:      add x9, x9, #0xb10
10081c5d8:      add x8, x9, x8, lsl #5
10081c5dc:      ldrb    w8, [x8, #0x1b]
10081c5e0:      tbnz    w8, #0x0, 0x10081c628 <_js_object_alloc_class_dynamic_parent+0x728>
10081c5e4:      mov x26, x0
10081c5e8:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c5ec:      add x0, x0, #0x658
10081c5f0:      ldr x8, [x0]
10081c5f4:      blr x8
10081c5f8:      mov x24, x0
10081c5fc:      ldrb    w8, [x0, #0x18]
10081c600:      cbnz    w8, 0x10081c9f4 <_js_object_alloc_class_dynamic_parent+0xaf4>
10081c604:      ldr x28, [x24, #0x10]
10081c608:      ldr x8, [x24]
10081c60c:      cmp x28, x8
10081c610:      mov x0, x26
10081c614:      b.eq    0x10081ca24 <_js_object_alloc_class_dynamic_parent+0xb24>
10081c618:      ldr x8, [x24, #0x8]
10081c61c:      str x0, [x8, x28, lsl #3]
10081c620:      add x8, x28, #0x1
10081c624:      str x8, [x24, #0x10]
10081c628:      strh    wzr, [x0, #0x2]
10081c62c:      str w23, [x0, #0x4]
10081c630:      add x24, x0, #0x8
10081c634:      stp w19, w20, [x24]
10081c638:      lsl x2, x27, #3
10081c63c:      str xzr, [x24, #0x8]
10081c640:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5aa8>
10081c644:      add x1, x1, #0xaf0
10081c648:      add x0, x24, #0x10
10081c64c:      bl  0x100af8f90 <_writev+0x100af8f90>
10081c650:      mov x0, x24
10081c654:      mov x1, x21
10081c658:      mov x2, x22
10081c65c:      bl  0x100324600 <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object31set_object_keys_array_with_live>
10081c660:      lsr x8, x24, #3
10081c664:      cmp x8, #0x201
10081c668:      b.lo    0x10081c6bc <_js_object_alloc_class_dynamic_parent+0x7bc>
10081c66c:      ldurb   w8, [x24, #-0x8]
10081c670:      sub w9, w8, #0x15
10081c674:      cmn w9, #0x14
10081c678:      b.lo    0x10081c6bc <_js_object_alloc_class_dynamic_parent+0x7bc>
10081c67c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c680:      add x9, x9, #0xb10
10081c684:      add x8, x9, x8, lsl #5
10081c688:      ldrb    w8, [x8, #0x14]
10081c68c:      mov w9, #0x16               ; =22
10081c690:      lsr w8, w9, w8
10081c694:      tbz w8, #0x0, 0x10081c6bc <_js_object_alloc_class_dynamic_parent+0x7bc>
10081c698:      ldurh   w8, [x24, #-0x6]
10081c69c:      mov w9, #0x4000             ; =16384
10081c6a0:      bfxil   w9, w8, #0, #13
10081c6a4:      sturh   w9, [x24, #-0x6]
10081c6a8:      mov x0, x24
10081c6ac:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081c6b0:      ldurh   w8, [x24, #-0x6]
10081c6b4:      and w8, w8, #0xffffefff
10081c6b8:      sturh   w8, [x24, #-0x6]
10081c6bc:      mov x0, x24
10081c6c0:      mov x1, x25
10081c6c4:      mov x2, x22
10081c6c8:      bl  0x100597fc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081c6cc:      mov x0, x19
10081c6d0:      mov x1, x22
10081c6d4:      mov x2, x21
10081c6d8:      bl  0x100591c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc25remember_class_keys_array>
10081c6dc:      mov x0, x24
10081c6e0:      ldp x29, x30, [sp, #0x90]
10081c6e4:      ldp x20, x19, [sp, #0x80]
10081c6e8:      ldp x22, x21, [sp, #0x70]
10081c6ec:      ldp x24, x23, [sp, #0x60]
10081c6f0:      ldp x26, x25, [sp, #0x50]
10081c6f4:      ldp x28, x27, [sp, #0x40]
10081c6f8:      add sp, sp, #0xa0
10081c6fc:      ret
10081c700:      cmp x0, x1
10081c704:      b.hs    0x10081c9f0 <_js_object_alloc_class_dynamic_parent+0xaf0>
10081c708:      ldr x24, [x9]
10081c70c:      subs    x10, x1, #0x1
10081c710:      ldr q0, [x8, x10, lsl #4]
10081c714:      str q0, [x9]
10081c718:      str x10, [x26, #0x18]
10081c71c:      b.ne    0x10081c744 <_js_object_alloc_class_dynamic_parent+0x844>
10081c720:      ldr x8, [x28, #0x48]
10081c724:      cmn x8, #0x1
10081c728:      b.eq    0x10081c9dc <_js_object_alloc_class_dynamic_parent+0xadc>
10081c72c:      mrs x9, TPIDRRO_EL0
10081c730:      and x9, x9, #0xfffffffffffffff8
10081c734:      ldr x0, [x9, x8, lsl #3]
10081c738:      cbz x0, 0x10081c9dc <_js_object_alloc_class_dynamic_parent+0xadc>
10081c73c:      ldr x8, [x0, #0x28]
10081c740:      strb    wzr, [x8]
10081c744:      ldr x8, [x26]
10081c748:      add x8, x8, #0x1
10081c74c:      str x8, [x26]
10081c750:      mov w8, #0x2                ; =2
10081c754:      mov x9, x28
10081c758:      mov x28, x24
10081c75c:      strb    w8, [x28, #-0x8]!
10081c760:      mov x26, x9
10081c764:      ldr x8, [x9, #0x48]
10081c768:      cmn x8, #0x1
10081c76c:      b.eq    0x10081c9c0 <_js_object_alloc_class_dynamic_parent+0xac0>
10081c770:      mrs x9, TPIDRRO_EL0
10081c774:      and x9, x9, #0xfffffffffffffff8
10081c778:      ldr x0, [x9, x8, lsl #3]
10081c77c:      cbz x0, 0x10081c9c0 <_js_object_alloc_class_dynamic_parent+0xac0>
10081c780:      ldr x8, [x0, #0x30]
10081c784:      ldrb    w8, [x8]
10081c788:      orr w8, w8, #0x2
10081c78c:      sturb   w8, [x24, #-0x7]
10081c790:      ldr x8, [x26, #0x48]
10081c794:      cmn x8, #0x1
10081c798:      b.eq    0x10081c9c8 <_js_object_alloc_class_dynamic_parent+0xac8>
10081c79c:      mrs x9, TPIDRRO_EL0
10081c7a0:      and x9, x9, #0xfffffffffffffff8
10081c7a4:      ldr x0, [x9, x8, lsl #3]
10081c7a8:      cbz x0, 0x10081c9c8 <_js_object_alloc_class_dynamic_parent+0xac8>
10081c7ac:      ldr x8, [x0, #0x30]
10081c7b0:      ldrb    w8, [x8]
10081c7b4:      tbz w8, #0x0, 0x10081c814 <_js_object_alloc_class_dynamic_parent+0x914>
10081c7b8:      ldrb    w8, [x28]
10081c7bc:      sub w9, w8, #0x15
10081c7c0:      cmn w9, #0x14
10081c7c4:      b.lo    0x10081c814 <_js_object_alloc_class_dynamic_parent+0x914>
10081c7c8:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c7cc:      add x9, x9, #0xb10
10081c7d0:      add x8, x9, x8, lsl #5
10081c7d4:      ldrb    w8, [x8, #0x1b]
10081c7d8:      tbnz    w8, #0x0, 0x10081c814 <_js_object_alloc_class_dynamic_parent+0x914>
10081c7dc:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c7e0:      add x0, x0, #0x658
10081c7e4:      ldr x8, [x0]
10081c7e8:      blr x8
10081c7ec:      ldrb    w8, [x0, #0x18]
10081c7f0:      cbnz    w8, 0x10081ca34 <_js_object_alloc_class_dynamic_parent+0xb34>
10081c7f4:      ldr x26, [x0, #0x10]
10081c7f8:      ldr x8, [x0]
10081c7fc:      cmp x26, x8
10081c800:      b.eq    0x10081ca70 <_js_object_alloc_class_dynamic_parent+0xb70>
10081c804:      ldr x8, [x0, #0x8]
10081c808:      str x28, [x8, x26, lsl #3]
10081c80c:      add x8, x26, #0x1
10081c810:      str x8, [x0, #0x10]
10081c814:      sturh   wzr, [x24, #-0x6]
10081c818:      stur    w23, [x24, #-0x4]
10081c81c:      b   0x10081c634 <_js_object_alloc_class_dynamic_parent+0x734>
10081c820:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c824:      ldr x8, [x0, #0x28]
10081c828:      ldrb    w8, [x8]
10081c82c:      tbnz    w8, #0x0, 0x10081c4f4 <_js_object_alloc_class_dynamic_parent+0x5f4>
10081c830:      b   0x10081c55c <_js_object_alloc_class_dynamic_parent+0x65c>
10081c834:      mov x24, x0
10081c838:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c83c:      mov x8, x0
10081c840:      mov x0, x24
10081c844:      b   0x10081c588 <_js_object_alloc_class_dynamic_parent+0x688>
10081c848:      mov x24, x0
10081c84c:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c850:      mov x8, x0
10081c854:      mov x0, x24
10081c858:      ldr x8, [x8, #0x30]
10081c85c:      ldrb    w8, [x8]
10081c860:      tbnz    w8, #0x0, 0x10081c5c0 <_js_object_alloc_class_dynamic_parent+0x6c0>
10081c864:      b   0x10081c628 <_js_object_alloc_class_dynamic_parent+0x728>
10081c868:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c86c:      and w10, w23, #0xff
10081c870:      add x8, x0, x24, lsl #3
10081c874:      ldr x0, [x8, #0x1e8]
10081c878:      cbnz    x0, 0x10081c458 <_js_object_alloc_class_dynamic_parent+0x558>
10081c87c:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081c880:      add x0, x0, #0x330
10081c884:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081c888:      and w10, w23, #0xff
10081c88c:      ldr x0, [x0]
10081c890:      cbnz    x0, 0x10081c460 <_js_object_alloc_class_dynamic_parent+0x560>
10081c894:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081c898:      and w10, w23, #0xff
10081c89c:      add x8, x0, x10, lsl #4
10081c8a0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c8a4:      ldr w9, [x8]
10081c8a8:      cmp w9, w23
10081c8ac:      b.eq    0x10081c474 <_js_object_alloc_class_dynamic_parent+0x574>
10081c8b0:      ldr x8, [x0, #0x5088]
10081c8b4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081c8b8:      cmp x8, x9
10081c8bc:      b.hs    0x10081c9e4 <_js_object_alloc_class_dynamic_parent+0xae4>
10081c8c0:      add x9, x8, #0x1
10081c8c4:      str x9, [x0, #0x5088]
10081c8c8:      ldr x9, [x0, #0x50a8]
10081c8cc:      cbz x9, 0x10081c97c <_js_object_alloc_class_dynamic_parent+0xa7c>
10081c8d0:      mov x9, #0x0                ; =0
10081c8d4:      mov x10, #0x7c15            ; =31765
10081c8d8:      movk    x10, #0x7f4a, lsl #16
10081c8dc:      movk    x10, #0x79b9, lsl #32
10081c8e0:      movk    x10, #0x9e37, lsl #48
10081c8e4:      mul x10, x23, x10
10081c8e8:      eor x13, x10, x10, lsr #32
10081c8ec:      lsr x12, x10, #57
10081c8f0:      ldr x10, [x0, #0x5098]
10081c8f4:      ldr x11, [x0, #0x5090]
10081c8f8:      dup.8b  v0, w12
10081c8fc:      movi.2d v1, #0xffffffffffffffff
10081c900:      mov w12, #0x18              ; =24
10081c904:      and x13, x13, x10
10081c908:      ldr d2, [x11, x13]
10081c90c:      cmeq.8b v3, v2, v0
10081c910:      fmov    x14, d3
10081c914:      ands    x14, x14, #0x8080808080808080
10081c918:      b.eq    0x10081c94c <_js_object_alloc_class_dynamic_parent+0xa4c>
10081c91c:      rbit    x15, x14
10081c920:      clz x15, x15
10081c924:      add x15, x13, x15, lsr #3
10081c928:      and x15, x15, x10
10081c92c:      mneg    x15, x15, x12
10081c930:      add x15, x11, x15
10081c934:      ldur    w16, [x15, #-0x18]
10081c938:      cmp w23, w16
10081c93c:      b.eq    0x10081c990 <_js_object_alloc_class_dynamic_parent+0xa90>
10081c940:      sub x15, x14, #0x2
10081c944:      ands    x14, x15, x14
10081c948:      b.ne    0x10081c91c <_js_object_alloc_class_dynamic_parent+0xa1c>
10081c94c:      cmeq.8b v2, v2, v1
10081c950:      fmov    x14, d2
10081c954:      cbnz    x14, 0x10081c97c <_js_object_alloc_class_dynamic_parent+0xa7c>
10081c958:      add x9, x9, #0x8
10081c95c:      add x13, x13, x9
10081c960:      and x13, x13, x10
10081c964:      ldr d2, [x11, x13]
10081c968:      cmeq.8b v3, v2, v0
10081c96c:      fmov    x14, d3
10081c970:      ands    x14, x14, #0x8080808080808080
10081c974:      b.ne    0x10081c91c <_js_object_alloc_class_dynamic_parent+0xa1c>
10081c978:      b   0x10081c94c <_js_object_alloc_class_dynamic_parent+0xa4c>
10081c97c:      mov w25, #0x0               ; =0
10081c980:      str x8, [x0, #0x5088]
10081c984:      ldr w8, [sp]
10081c988:      tbz w8, #0x0, 0x10081c480 <_js_object_alloc_class_dynamic_parent+0x580>
10081c98c:      b   0x10081c488 <_js_object_alloc_class_dynamic_parent+0x588>
10081c990:      ldur    w25, [x15, #-0x8]
10081c994:      str x8, [x0, #0x5088]
10081c998:      ldr w8, [sp]
10081c99c:      tbz w8, #0x0, 0x10081c480 <_js_object_alloc_class_dynamic_parent+0x580>
10081c9a0:      b   0x10081c488 <_js_object_alloc_class_dynamic_parent+0x588>
10081c9a4:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c9a8:      ldr x26, [x0, #0x20]
10081c9ac:      ldr x8, [x26]
10081c9b0:      cbz x8, 0x10081c51c <_js_object_alloc_class_dynamic_parent+0x61c>
10081c9b4:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081c9b8:      add x0, x0, #0x120
10081c9bc:      bl  0x100ab12ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081c9c0:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c9c4:      b   0x10081c780 <_js_object_alloc_class_dynamic_parent+0x880>
10081c9c8:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c9cc:      ldr x8, [x0, #0x30]
10081c9d0:      ldrb    w8, [x8]
10081c9d4:      tbnz    w8, #0x0, 0x10081c7b8 <_js_object_alloc_class_dynamic_parent+0x8b8>
10081c9d8:      b   0x10081c814 <_js_object_alloc_class_dynamic_parent+0x914>
10081c9dc:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c9e0:      b   0x10081c73c <_js_object_alloc_class_dynamic_parent+0x83c>
10081c9e4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081c9e8:      add x0, x0, #0x798
10081c9ec:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081c9f0:      bl  0x100ab0fbc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081c9f4:      cmp w8, #0x1
10081c9f8:      b.ne    0x10081ca3c <_js_object_alloc_class_dynamic_parent+0xb3c>
10081c9fc:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081ca00:      add x1, x1, #0xbb8
10081ca04:      mov x0, x24
10081ca08:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081ca0c:      strb    wzr, [x24, #0x18]
10081ca10:      ldr x28, [x24, #0x10]
10081ca14:      ldr x8, [x24]
10081ca18:      cmp x28, x8
10081ca1c:      mov x0, x26
10081ca20:      b.ne    0x10081c618 <_js_object_alloc_class_dynamic_parent+0x718>
10081ca24:      mov x0, x24
10081ca28:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081ca2c:      mov x0, x26
10081ca30:      b   0x10081c618 <_js_object_alloc_class_dynamic_parent+0x718>
10081ca34:      cmp w8, #0x2
10081ca38:      b.ne    0x10081ca48 <_js_object_alloc_class_dynamic_parent+0xb48>
10081ca3c:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081ca40:      add x0, x0, #0x850
10081ca44:      bl  0x100aef71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081ca48:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081ca4c:      add x1, x1, #0xbb8
10081ca50:      mov x26, x0
10081ca54:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081ca58:      mov x0, x26
10081ca5c:      strb    wzr, [x26, #0x18]
10081ca60:      ldr x26, [x26, #0x10]
10081ca64:      ldr x8, [x0]
10081ca68:      cmp x26, x8
10081ca6c:      b.ne    0x10081c804 <_js_object_alloc_class_dynamic_parent+0x904>
10081ca70:      str x0, [sp, #0x8]
10081ca74:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081ca78:      ldr x0, [sp, #0x8]
10081ca7c:      b   0x10081c804 <_js_object_alloc_class_dynamic_parent+0x904>
10081ca80:      mov w0, #0x8                ; =8
10081ca84:      mov w1, #0x40               ; =64
10081ca88:      bl  0x100ab0f64 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
        ...
