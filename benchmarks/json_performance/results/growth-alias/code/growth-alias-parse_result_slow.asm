/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003e8e0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
1003e8e0c:      sub sp, sp, #0x1a0
1003e8e10:      stp x28, x27, [sp, #0x140]
1003e8e14:      stp x26, x25, [sp, #0x150]
1003e8e18:      stp x24, x23, [sp, #0x160]
1003e8e1c:      stp x22, x21, [sp, #0x170]
1003e8e20:      stp x20, x19, [sp, #0x180]
1003e8e24:      stp x29, x30, [sp, #0x190]
1003e8e28:      add x29, sp, #0x190
1003e8e2c:      mov x21, x2
1003e8e30:      mov x22, x1
1003e8e34:      mov x19, x0
1003e8e38:      add x25, sp, #0x90
1003e8e3c:      add x20, x1, #0x14
1003e8e40:      cmp x2, #0x2
1003e8e44:      b.ne    0x1003e8e5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50>
1003e8e48:      ldrh    w8, [x20]
1003e8e4c:      mov w9, #0x7d7b             ; =32123
1003e8e50:      cmp w8, w9
1003e8e54:      b.eq    0x1003e8e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
1003e8e58:      b   0x1003e8ed0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1003e8e5c:      cmp x21, #0x3
1003e8e60:      b.lo    0x1003e8ed0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1003e8e64:      ldrb    w8, [x20]
1003e8e68:      cmp w8, #0x20
1003e8e6c:      b.hi    0x1003e8e9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1003e8e70:      mov x9, #0x2600             ; =9728
1003e8e74:      movk    x9, #0x1, lsl #32
1003e8e78:      lsr x9, x9, x8
1003e8e7c:      tbz w9, #0x0, 0x1003e8e9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1003e8e80:      add x0, x22, #0x14
1003e8e84:      mov x1, x21
1003e8e88:      bl  0x10020dc88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1003e8e8c:      tbz w0, #0x0, 0x1003e8ec8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1003e8e90:      bl  0x10020dd58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1003e8e94:      stp xzr, x0, [x19]
1003e8e98:      b   0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e8e9c:      cmp w8, #0x7b
1003e8ea0:      b.ne    0x1003e8ec8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1003e8ea4:      ldrb    w8, [x22, #0x15]
1003e8ea8:      cmp w8, #0x20
1003e8eac:      b.hi    0x1003e8ec0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
1003e8eb0:      mov x9, #0x2600             ; =9728
1003e8eb4:      movk    x9, #0x1, lsl #32
1003e8eb8:      lsr x9, x9, x8
1003e8ebc:      tbnz    w9, #0x0, 0x1003e8e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1003e8ec0:      cmp w8, #0x7d
1003e8ec4:      b.eq    0x1003e8e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1003e8ec8:      cmp x21, #0x41
1003e8ecc:      b.hs    0x1003e8f34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x128>
1003e8ed0:      add x0, sp, #0x90
1003e8ed4:      add x1, x22, #0x14
1003e8ed8:      mov x2, x21
1003e8edc:      bl  0x1003e030c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1003e8ee0:      ldr x8, [sp, #0x90]
1003e8ee4:      cbz x8, 0x1003e8fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1003e8ee8:      ldr x8, [sp, #0x118]
1003e8eec:      str x8, [sp, #0x80]
1003e8ef0:      ldur    q0, [x25, #0x48]
1003e8ef4:      ldur    q1, [x25, #0x58]
1003e8ef8:      stp q0, q1, [sp, #0x40]
1003e8efc:      ldur    q0, [x25, #0x68]
1003e8f00:      ldur    q1, [x25, #0x78]
1003e8f04:      stp q0, q1, [sp, #0x60]
1003e8f08:      ldur    q0, [x25, #0x8]
1003e8f0c:      ldur    q1, [x25, #0x18]
1003e8f10:      stp q0, q1, [sp]
1003e8f14:      ldur    q0, [x25, #0x28]
1003e8f18:      ldur    q1, [x25, #0x38]
1003e8f1c:      stp q0, q1, [sp, #0x20]
1003e8f20:      mov x0, sp
1003e8f24:      bl  0x1003e0d6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1003e8f28:      tbz w0, #0x0, 0x1003e8fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1003e8f2c:      stp xzr, x1, [x19]
1003e8f30:      b   0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e8f34:      cmp x21, #0x3e9
1003e8f38:      b.lo    0x1003e8fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1003e8f3c:      add x0, x22, #0x14
1003e8f40:      mov x1, x21
1003e8f44:      mov w2, #0x3e8              ; =1000
1003e8f48:      bl  0x1003e1910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1003e8f4c:      tbz w0, #0x0, 0x1003e8fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1003e8f50:      add x0, x22, #0x14
1003e8f54:      mov x1, x21
1003e8f58:      mov w2, #0xa120             ; =41248
1003e8f5c:      movk    w2, #0x7, lsl #16
1003e8f60:      bl  0x1003e1910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1003e8f64:      tbz w0, #0x0, 0x1003e9358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x54c>
1003e8f68:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1003e8f6c:      add x8, x8, #0xf80
1003e8f70:      adrp    x9, 0x100dd1000 <_anon.32ca3690520b3140c3df72b88a347d65.557+0x2f2>
1003e8f74:      add x9, x9, #0x1d0
1003e8f78:      stp x9, x8, [sp]
1003e8f7c:      adrp    x0, 0x100ee7000 <_anon.32ca3690520b3140c3df72b88a347d65.12+0x24>
1003e8f80:      add x0, x0, #0x8a3
1003e8f84:      add x8, sp, #0x90
1003e8f88:      mov x1, sp
1003e8f8c:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
1003e8f90:      ldr x20, [sp, #0x98]
1003e8f94:      ldr w1, [sp, #0xa0]
1003e8f98:      mov x0, x20
1003e8f9c:      mov x2, x1
1003e8fa0:      bl  0x1004727c0 <_js_string_from_bytes_with_capacity>
1003e8fa4:      mov x3, x0
1003e8fa8:      adrp    x1, 0x100dba000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x1ba>
1003e8fac:      add x1, x1, #0x593
1003e8fb0:      mov w21, #0x1               ; =1
1003e8fb4:      mov w0, #0x2                ; =2
1003e8fb8:      mov w2, #0xa                ; =10
1003e8fbc:      mov w4, #0x1                ; =1
1003e8fc0:      bl  0x1001ed5a4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1003e8fc4:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1003e8fc8:      bfxil   x8, x0, #0, #48
1003e8fcc:      stp x21, x8, [x19]
1003e8fd0:      ldr x8, [sp, #0x90]
1003e8fd4:      cbz x8, 0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e8fd8:      mov x0, x20
1003e8fdc:      b   0x1003e92ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a0>
1003e8fe0:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003e8fe4:      add x0, x0, #0x8
1003e8fe8:      ldr x8, [x0]
1003e8fec:      blr x8
1003e8ff0:      mov x20, x0
1003e8ff4:      ldrb    w8, [x0, #0x20]
1003e8ff8:      cbnz    w8, 0x1003e943c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x630>
1003e8ffc:      ldr x8, [x20]
1003e9000:      cbnz    x8, 0x1003e9488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x67c>
1003e9004:      mov x26, #0x7fff000000000000 ; =9223090561878065152
1003e9008:      bfxil   x26, x22, #0, #48
1003e900c:      mov x8, #-0x1               ; =-1
1003e9010:      str x8, [x20]
1003e9014:      mov x23, x20
1003e9018:      ldr x8, [x23, #0x8]!
1003e901c:      ldr x24, [x20, #0x18]
1003e9020:      cmp x24, x8
1003e9024:      b.ne    0x1003e9030 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x224>
1003e9028:      mov x0, x23
1003e902c:      bl  0x100cb708c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1003e9030:      ldr x8, [x20, #0x10]
1003e9034:      str x26, [x8, x24, lsl #3]
1003e9038:      add x8, x24, #0x1
1003e903c:      str x8, [x20, #0x18]
1003e9040:      ldr x8, [x20]
1003e9044:      add x8, x8, #0x1
1003e9048:      str x8, [x20]
1003e904c:      mov x0, #0x0                ; =0
1003e9050:      bl  0x10030335c <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1003e9054:      mov x22, x0
1003e9058:      ldrb    w8, [x0]
1003e905c:      strb    wzr, [x0]
1003e9060:      cbz w8, 0x1003e9094 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x288>
1003e9064:      mov x0, #0x0                ; =0
1003e9068:      bl  0x10030337c <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1003e906c:      ldrb    w8, [x0]
1003e9070:      tst w8, #0x3
1003e9074:      b.ne    0x1003e908c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x280>
1003e9078:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
1003e907c:      add x8, x8, #0x790
1003e9080:      ldapr   w8, [x8]
1003e9084:      cmp w8, #0x0
1003e9088:      b.le    0x1003e92d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4c4>
1003e908c:      mov w8, #0x1                ; =1
1003e9090:      strb    w8, [x22]
1003e9094:      bl  0x1002af600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1003e9098:      bl  0x1002af4ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1003e909c:      ldrb    w8, [x20, #0x20]
1003e90a0:      cbnz    w8, 0x1003e9320 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x514>
1003e90a4:      ldr x8, [x20]
1003e90a8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e90ac:      cmp x8, x9
1003e90b0:      b.hs    0x1003e934c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x540>
1003e90b4:      add x9, x8, #0x1
1003e90b8:      str x9, [x20]
1003e90bc:      ldr x10, [x20, #0x18]
1003e90c0:      mov w9, #0x1                ; =1
1003e90c4:      cmp x24, x10
1003e90c8:      b.hs    0x1003e90dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d0>
1003e90cc:      ldr x10, [x20, #0x10]
1003e90d0:      ldr x10, [x10, x24, lsl #3]
1003e90d4:      and x10, x10, #0xffffffffffff
1003e90d8:      b   0x1003e90e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d4>
1003e90dc:      mov w10, #0x1               ; =1
1003e90e0:      str x8, [x20]
1003e90e4:      add x8, x10, #0x14
1003e90e8:      movi.2d v0, #0000000000000000
1003e90ec:      stur    q0, [x25, #0x78]
1003e90f0:      stur    q0, [x25, #0x68]
1003e90f4:      stur    q0, [x25, #0x58]
1003e90f8:      stur    q0, [x25, #0x48]
1003e90fc:      strb    w9, [sp, #0x120]
1003e9100:      mov x9, #-0x1               ; =-1
1003e9104:      stp x8, x21, [sp, #0xb8]
1003e9108:      str x9, [sp, #0x90]
1003e910c:      stp xzr, xzr, [sp, #0xc8]
1003e9110:      str xzr, [sp, #0x118]
1003e9114:      add x0, sp, #0x90
1003e9118:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003e911c:      mov x21, x0
1003e9120:      ldp x8, x9, [sp, #0xc0]
1003e9124:      cmp x9, x8
1003e9128:      b.hs    0x1003e915c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1003e912c:      ldr x10, [sp, #0xb8]
1003e9130:      mov x11, #0x2600            ; =9728
1003e9134:      movk    x11, #0x1, lsl #32
1003e9138:      ldrb    w12, [x10, x9]
1003e913c:      cmp w12, #0x20
1003e9140:      b.hi    0x1003e915c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1003e9144:      lsr x12, x11, x12
1003e9148:      tbz w12, #0x0, 0x1003e915c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1003e914c:      add x9, x9, #0x1
1003e9150:      cmp x8, x9
1003e9154:      b.ne    0x1003e9138 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x32c>
1003e9158:      mov x9, x8
1003e915c:      ldrb    w25, [sp, #0x120]
1003e9160:      cmp x9, x8
1003e9164:      cset    w26, eq
1003e9168:      ldrb    w8, [x20, #0x20]
1003e916c:      cbnz    w8, 0x1003e9464 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x658>
1003e9170:      ldr x8, [x20]
1003e9174:      cbnz    x8, 0x1003e9488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x67c>
1003e9178:      mov x8, #-0x1               ; =-1
1003e917c:      str x8, [x20]
1003e9180:      ldr x27, [x20, #0x18]
1003e9184:      ldr x8, [x20, #0x8]
1003e9188:      cmp x27, x8
1003e918c:      b.ne    0x1003e9198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x38c>
1003e9190:      mov x0, x23
1003e9194:      bl  0x100cb708c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1003e9198:      ldr x8, [x20, #0x10]
1003e919c:      str x21, [x8, x27, lsl #3]
1003e91a0:      add x8, x27, #0x1
1003e91a4:      str x8, [x20, #0x18]
1003e91a8:      ldr x8, [x20]
1003e91ac:      add x8, x8, #0x1
1003e91b0:      str x8, [x20]
1003e91b4:      mov x0, #0x0                ; =0
1003e91b8:      bl  0x10030337c <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1003e91bc:      ldrb    w8, [x0]
1003e91c0:      and w8, w8, #0xfffffffd
1003e91c4:      strb    w8, [x0]
1003e91c8:      bl  0x1002b0098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1003e91cc:      adrp    x23, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e91d0:      add x23, x23, #0x4b8
1003e91d4:      ldapr   x8, [x23]
1003e91d8:      cbnz    x8, 0x1003e9424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x618>
1003e91dc:      ldrb    w8, [x23, #0x8]
1003e91e0:      cbz w8, 0x1003e9210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1003e91e4:      bl  0x1001971c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena4walk18arena_in_use_bytes>
1003e91e8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e91ec:      add x8, x8, #0x550
1003e91f0:      ldapr   x8, [x8]
1003e91f4:      cbnz    x8, 0x1003e94a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x69c>
1003e91f8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e91fc:      ldr x8, [x8, #0x558]
1003e9200:      cmp x0, x8
1003e9204:      b.lo    0x1003e9210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1003e9208:      mov w8, #0x1                ; =1
1003e920c:      strb    w8, [x22]
1003e9210:      ldrb    w8, [x20, #0x20]
1003e9214:      cbnz    w8, 0x1003e9494 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x688>
1003e9218:      ldr x8, [x20]
1003e921c:      cbnz    x8, 0x1003e94ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x6e0>
1003e9220:      and w22, w26, w25
1003e9224:      ldr x8, [x20, #0x18]
1003e9228:      cmp x24, x8
1003e922c:      b.hi    0x1003e9234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x428>
1003e9230:      str x24, [x20, #0x18]
1003e9234:      adrp    x0, 0x10109e000 <_anon.32ca3690520b3140c3df72b88a347d65.100+0x178>
1003e9238:      add x0, x0, #0xf98
1003e923c:      bl  0x100139bdc <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
1003e9240:      tbz w22, #0x0, 0x1003e9258 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x44c>
1003e9244:      stp xzr, x21, [x19]
1003e9248:      ldr x8, [sp, #0x90]
1003e924c:      cmn x8, #0x1
1003e9250:      b.ne    0x1003e92a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x498>
1003e9254:      b   0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e9258:      adrp    x0, 0x100dd0000 <_PERRY_EMPTY_STRING+0x44>
1003e925c:      add x0, x0, #0xa25
1003e9260:      mov w1, #0x21               ; =33
1003e9264:      mov w2, #0x21               ; =33
1003e9268:      bl  0x1004727c0 <_js_string_from_bytes_with_capacity>
1003e926c:      mov x3, x0
1003e9270:      adrp    x1, 0x100dba000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x1ba>
1003e9274:      add x1, x1, #0x5ab
1003e9278:      mov w20, #0x1               ; =1
1003e927c:      mov w0, #0x4                ; =4
1003e9280:      mov w2, #0xb                ; =11
1003e9284:      mov w4, #0x1                ; =1
1003e9288:      bl  0x1001ed5a4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1003e928c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1003e9290:      bfxil   x8, x0, #0, #48
1003e9294:      stp x20, x8, [x19]
1003e9298:      ldr x8, [sp, #0x90]
1003e929c:      cmn x8, #0x1
1003e92a0:      b.eq    0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e92a4:      cbz x8, 0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e92a8:      ldr x0, [sp, #0x98]
1003e92ac:      bl  0x100cd5f00 <_mi_free>
1003e92b0:      ldp x29, x30, [sp, #0x190]
1003e92b4:      ldp x20, x19, [sp, #0x180]
1003e92b8:      ldp x22, x21, [sp, #0x170]
1003e92bc:      ldp x24, x23, [sp, #0x160]
1003e92c0:      ldp x26, x25, [sp, #0x150]
1003e92c4:      ldp x28, x27, [sp, #0x140]
1003e92c8:      add sp, sp, #0x1a0
1003e92cc:      ret
1003e92d0:      mov x0, #0x0                ; =0
1003e92d4:      bl  0x100303434 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1003e92d8:      ldr x26, [x0]
1003e92dc:      mov x0, #0x0                ; =0
1003e92e0:      bl  0x1003031dc <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1003e92e4:      ldr x8, [x0]
1003e92e8:      cmp x8, x26
1003e92ec:      b.ls    0x1003e930c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x500>
1003e92f0:      str x26, [x0]
1003e92f4:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003e92f8:      add x0, x0, #0x860
1003e92fc:      ldr x8, [x0]
1003e9300:      blr x8
1003e9304:      mov w8, #0x1                ; =1
1003e9308:      strb    w8, [x0]
1003e930c:      bl  0x1002af600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1003e9310:      bl  0x1002af600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1003e9314:      bl  0x1002af4ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1003e9318:      ldrb    w8, [x20, #0x20]
1003e931c:      cbz w8, 0x1003e90a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x298>
1003e9320:      cmp w8, #0x2
1003e9324:      b.eq    0x1003e949c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x690>
1003e9328:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e932c:      add x1, x1, #0x824
1003e9330:      mov x0, x20
1003e9334:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e9338:      strb    wzr, [x20, #0x20]
1003e933c:      ldr x8, [x20]
1003e9340:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e9344:      cmp x8, x9
1003e9348:      b.lo    0x1003e90b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2a8>
1003e934c:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003e9350:      add x0, x0, #0xdc8
1003e9354:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1003e9358:      stur    x21, [x29, #-0x68]
1003e935c:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1003e9360:      bfxil   x8, x22, #0, #48
1003e9364:      str x8, [sp, #0x90]
1003e9368:      adrp    x22, 0x10109e000 <_anon.32ca3690520b3140c3df72b88a347d65.100+0x178>
1003e936c:      add x22, x22, #0xf90
1003e9370:      add x1, sp, #0x90
1003e9374:      mov x0, x22
1003e9378:      bl  0x100137490 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1003e937c:      mov x23, x0
1003e9380:      stp x0, x20, [x29, #-0x60]
1003e9384:      str x21, [sp]
1003e9388:      sub x8, x29, #0x58
1003e938c:      mov x9, sp
1003e9390:      stp x8, x9, [sp, #0x90]
1003e9394:      sub x8, x29, #0x60
1003e9398:      sub x9, x29, #0x68
1003e939c:      stp x8, x9, [sp, #0xa0]
1003e93a0:      adrp    x0, 0x10109d000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi4read17READ_OBJECT_CACHE+0x78>
1003e93a4:      add x0, x0, #0x938
1003e93a8:      add x1, sp, #0x90
1003e93ac:      bl  0x10012c4d8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1003e93b0:      mov x21, x0
1003e93b4:      mov x20, x1
1003e93b8:      str x23, [sp, #0x90]
1003e93bc:      add x1, sp, #0x90
1003e93c0:      mov x0, x22
1003e93c4:      bl  0x100137528 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1003e93c8:      adrp    x0, 0x10109e000 <_anon.32ca3690520b3140c3df72b88a347d65.100+0x178>
1003e93cc:      add x0, x0, #0xf98
1003e93d0:      bl  0x100139d44 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1003e93d4:      tbnz    w21, #0x0, 0x1003e9418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x60c>
1003e93d8:      adrp    x0, 0x100dd0000 <_PERRY_EMPTY_STRING+0x44>
1003e93dc:      add x0, x0, #0x265
1003e93e0:      mov w1, #0x29               ; =41
1003e93e4:      mov w2, #0x29               ; =41
1003e93e8:      bl  0x1004727c0 <_js_string_from_bytes_with_capacity>
1003e93ec:      mov x3, x0
1003e93f0:      adrp    x1, 0x100dba000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x1ba>
1003e93f4:      add x1, x1, #0x5ab
1003e93f8:      mov w21, #0x1               ; =1
1003e93fc:      mov w0, #0x4                ; =4
1003e9400:      mov w2, #0xb                ; =11
1003e9404:      mov w4, #0x1                ; =1
1003e9408:      bl  0x1001ed5a4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1003e940c:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
1003e9410:      bfxil   x20, x0, #0, #48
1003e9414:      b   0x1003e941c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x610>
1003e9418:      mov x21, #0x0               ; =0
1003e941c:      stp x21, x20, [x19]
1003e9420:      b   0x1003e92b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1003e9424:      adrp    x0, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e9428:      add x0, x0, #0x4b8
1003e942c:      bl  0x100cb5fcc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs5gMwpk3Cs4e_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1003e9430:      ldrb    w8, [x23, #0x8]
1003e9434:      cbnz    w8, 0x1003e91e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3d8>
1003e9438:      b   0x1003e9210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1003e943c:      cmp w8, #0x1
1003e9440:      b.ne    0x1003e949c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x690>
1003e9444:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e9448:      add x1, x1, #0x824
1003e944c:      mov x0, x20
1003e9450:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e9454:      strb    wzr, [x20, #0x20]
1003e9458:      ldr x8, [x20]
1003e945c:      cbz x8, 0x1003e9004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1f8>
1003e9460:      b   0x1003e9488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x67c>
1003e9464:      cmp w8, #0x2
1003e9468:      b.eq    0x1003e949c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x690>
1003e946c:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e9470:      add x1, x1, #0x824
1003e9474:      mov x0, x20
1003e9478:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e947c:      strb    wzr, [x20, #0x20]
1003e9480:      ldr x8, [x20]
1003e9484:      cbz x8, 0x1003e9178 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x36c>
1003e9488:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003e948c:      add x0, x0, #0xdf8
1003e9490:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003e9494:      cmp w8, #0x2
1003e9498:      b.ne    0x1003e94d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x6c4>
1003e949c:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1003e94a0:      add x0, x0, #0xed8
1003e94a4:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1003e94a8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e94ac:      add x8, x8, #0x550
1003e94b0:      mov x23, x0
1003e94b4:      mov x0, x8
1003e94b8:      bl  0x100cb68fc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1003e94bc:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e94c0:      ldr x8, [x8, #0x558]
1003e94c4:      cmp x23, x8
1003e94c8:      b.hs    0x1003e9208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3fc>
1003e94cc:      b   0x1003e9210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1003e94d0:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e94d4:      add x1, x1, #0x824
1003e94d8:      mov x0, x20
1003e94dc:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e94e0:      strb    wzr, [x20, #0x20]
1003e94e4:      ldr x8, [x20]
1003e94e8:      cbz x8, 0x1003e9220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x414>
1003e94ec:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003e94f0:      add x0, x0, #0xe58
1003e94f4:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
