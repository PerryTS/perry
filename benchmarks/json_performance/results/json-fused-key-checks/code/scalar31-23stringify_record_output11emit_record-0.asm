/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c2f00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record>:
1004c2f00:      stp x28, x27, [sp, #-0x60]!
1004c2f04:      stp x26, x25, [sp, #0x10]
1004c2f08:      stp x24, x23, [sp, #0x20]
1004c2f0c:      stp x22, x21, [sp, #0x30]
1004c2f10:      stp x20, x19, [sp, #0x40]
1004c2f14:      stp x29, x30, [sp, #0x50]
1004c2f18:      add x29, sp, #0x50
1004c2f1c:      sub sp, sp, #0x5d0
1004c2f20:      ldr xzr, [sp]
1004c2f24:      str x1, [sp, #0x38]
1004c2f28:      mov x22, x0
1004c2f2c:      movi.2d v0, #0000000000000000
1004c2f30:      str d0, [sp, #0x40]
1004c2f34:      str wzr, [sp, #0x48]
1004c2f38:      str d0, [sp, #0x68]
1004c2f3c:      str wzr, [sp, #0x70]
1004c2f40:      str d0, [sp, #0x90]
1004c2f44:      str wzr, [sp, #0x98]
1004c2f48:      str d0, [sp, #0xb8]
1004c2f4c:      str wzr, [sp, #0xc0]
1004c2f50:      str d0, [sp, #0xe0]
1004c2f54:      str wzr, [sp, #0xe8]
1004c2f58:      str d0, [sp, #0x108]
1004c2f5c:      str wzr, [sp, #0x110]
1004c2f60:      str d0, [sp, #0x130]
1004c2f64:      str wzr, [sp, #0x138]
1004c2f68:      str d0, [sp, #0x158]
1004c2f6c:      str wzr, [sp, #0x160]
1004c2f70:      str d0, [sp, #0x180]
1004c2f74:      str wzr, [sp, #0x188]
1004c2f78:      str d0, [sp, #0x1a8]
1004c2f7c:      str wzr, [sp, #0x1b0]
1004c2f80:      str d0, [sp, #0x1d0]
1004c2f84:      str wzr, [sp, #0x1d8]
1004c2f88:      str d0, [sp, #0x1f8]
1004c2f8c:      str wzr, [sp, #0x200]
1004c2f90:      str d0, [sp, #0x220]
1004c2f94:      str wzr, [sp, #0x228]
1004c2f98:      str d0, [sp, #0x248]
1004c2f9c:      str wzr, [sp, #0x250]
1004c2fa0:      str d0, [sp, #0x270]
1004c2fa4:      str wzr, [sp, #0x278]
1004c2fa8:      str d0, [sp, #0x298]
1004c2fac:      str wzr, [sp, #0x2a0]
1004c2fb0:      str d0, [sp, #0x2c0]
1004c2fb4:      str wzr, [sp, #0x2c8]
1004c2fb8:      str d0, [sp, #0x2e8]
1004c2fbc:      str wzr, [sp, #0x2f0]
1004c2fc0:      str d0, [sp, #0x310]
1004c2fc4:      str wzr, [sp, #0x318]
1004c2fc8:      str d0, [sp, #0x338]
1004c2fcc:      str wzr, [sp, #0x340]
1004c2fd0:      str d0, [sp, #0x360]
1004c2fd4:      str wzr, [sp, #0x368]
1004c2fd8:      str d0, [sp, #0x388]
1004c2fdc:      str wzr, [sp, #0x390]
1004c2fe0:      str d0, [sp, #0x3b0]
1004c2fe4:      str wzr, [sp, #0x3b8]
1004c2fe8:      str d0, [sp, #0x3d8]
1004c2fec:      str wzr, [sp, #0x3e0]
1004c2ff0:      str d0, [sp, #0x400]
1004c2ff4:      str wzr, [sp, #0x408]
1004c2ff8:      str d0, [sp, #0x428]
1004c2ffc:      str wzr, [sp, #0x430]
1004c3000:      str d0, [sp, #0x450]
1004c3004:      str wzr, [sp, #0x458]
1004c3008:      str d0, [sp, #0x478]
1004c300c:      str wzr, [sp, #0x480]
1004c3010:      str d0, [sp, #0x4a0]
1004c3014:      str wzr, [sp, #0x4a8]
1004c3018:      str d0, [sp, #0x4c8]
1004c301c:      str wzr, [sp, #0x4d0]
1004c3020:      str d0, [sp, #0x4f0]
1004c3024:      str wzr, [sp, #0x4f8]
1004c3028:      str d0, [sp, #0x518]
1004c302c:      mov w8, #-0x40000000        ; =-1073741824
1004c3030:      ldr w20, [x0, #0x4]
1004c3034:      cmp w20, w8
1004c3038:      csel    w19, w20, w8, lt
1004c303c:      str wzr, [sp, #0x520]
1004c3040:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3044:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3048:      ldr w21, [x9, #0x634]
1004c304c:      cmp w21, #0x300
1004c3050:      b.hs    0x1004c30e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c3054:      ldr x8, [x8, #0x48]
1004c3058:      cmn x8, #0x1
1004c305c:      b.eq    0x1004c30d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c3060:      mrs x9, TPIDRRO_EL0
1004c3064:      and x9, x9, #0xfffffffffffffff8
1004c3068:      ldr x0, [x9, x8, lsl #3]
1004c306c:      cbz x0, 0x1004c30d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c3070:      add x8, x0, x21, lsl #3
1004c3074:      ldr x0, [x8, #0x1e8]
1004c3078:      cbz x0, 0x1004c30e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c307c:      ldr x0, [x0]
1004c3080:      cbz x0, 0x1004c30f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1f8>
1004c3084:      mov w8, #-0x40000001        ; =-1073741825
1004c3088:      cmp w20, w8
1004c308c:      b.gt    0x1004c3108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3090:      ldr x10, [x0, #0x5198]
1004c3094:      and w8, w19, #0x3fffffff
1004c3098:      lsr x9, x8, #15
1004c309c:      cmp x9, x10
1004c30a0:      b.hs    0x1004c3108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c30a4:      ldr x10, [x0, #0x5190]
1004c30a8:      ldr x9, [x10, x9, lsl #3]
1004c30ac:      cbz x9, 0x1004c3108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c30b0:      ubfx    x10, x8, #5, #10
1004c30b4:      ldr x9, [x9, x10, lsl #3]
1004c30b8:      cbz x9, 0x1004c3108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c30bc:      and x8, x8, #0x1f
1004c30c0:      add x8, x9, x8, lsl #5
1004c30c4:      ldrb    w9, [x8, #0x1c]
1004c30c8:      tbz w9, #0x0, 0x1004c3108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c30cc:      ldr x8, [x8]
1004c30d0:      b   0x1004c310c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x20c>
1004c30d4:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c30d8:      add x8, x0, x21, lsl #3
1004c30dc:      ldr x0, [x8, #0x1e8]
1004c30e0:      cbnz    x0, 0x1004c307c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x17c>
1004c30e4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
1004c30e8:      add x0, x0, #0x330
1004c30ec:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c30f0:      ldr x0, [x0]
1004c30f4:      cbnz    x0, 0x1004c3084 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x184>
1004c30f8:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c30fc:      mov w8, #-0x40000001        ; =-1073741825
1004c3100:      cmp w20, w8
1004c3104:      b.le    0x1004c3090 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x190>
1004c3108:      mov x8, #0x0                ; =0
1004c310c:      mov x23, #0x0               ; =0
1004c3110:      mov x19, #0x0               ; =0
1004c3114:      add x9, x8, #0x8
1004c3118:      sub x28, x29, #0x80
1004c311c:      add x8, x22, #0x10
1004c3120:      stp x8, x9, [sp, #0x28]
1004c3124:      add x8, sp, #0x2c0
1004c3128:      add x8, x8, #0x8
1004c312c:      stp x22, x8, [sp, #0x8]
1004c3130:      mov w22, #0x2               ; =2
1004c3134:      mov w25, #0x28              ; =40
1004c3138:      mov w26, #0x2               ; =2
1004c313c:      ldr x8, [sp, #0x30]
1004c3140:      ldr x1, [x8, x23, lsl #3]
1004c3144:      sub x0, x29, #0x80
1004c3148:      bl  0x1004bd710 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece>
1004c314c:      ldur    w8, [x29, #-0x80]
1004c3150:      cmn w8, #0x1
1004c3154:      b.eq    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3158:      ldp w10, w9, [x29, #-0x7c]
1004c315c:      ldur    w11, [x29, #-0x74]
1004c3160:      ldur    q0, [x28, #0x10]
1004c3164:      stur    q0, [x29, #-0xe0]
1004c3168:      ldur    x12, [x28, #0x20]
1004c316c:      stur    x12, [x29, #-0xd0]
1004c3170:      add x13, sp, #0x40
1004c3174:      madd    x13, x23, x25, x13
1004c3178:      stp w8, w10, [x13]
1004c317c:      stp w9, w11, [x13, #0x8]
1004c3180:      str q0, [x13, #0x10]
1004c3184:      str x12, [x13, #0x20]
1004c3188:      cbz w8, 0x1004c319c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x29c>
1004c318c:      cmp w8, #0x1
1004c3190:      csel    w24, w11, w10, eq
1004c3194:      csel    w27, w9, w10, eq
1004c3198:      b   0x1004c31a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x2a4>
1004c319c:      add w27, w10, #0x2
1004c31a0:      add w24, w9, #0x2
1004c31a4:      ldr x8, [sp, #0x28]
1004c31a8:      ldr x21, [x8, x23, lsl #3]
1004c31ac:      sub x0, x29, #0xc8
1004c31b0:      mov x1, x21
1004c31b4:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c31b8:      ldur    w8, [x29, #-0xc8]
1004c31bc:      cmn w8, #0x1
1004c31c0:      b.eq    0x1004c321c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x31c>
1004c31c4:      ldp w20, w21, [x29, #-0xc4]
1004c31c8:      ldur    w9, [x29, #-0xbc]
1004c31cc:      add x10, sp, #0x180
1004c31d0:      madd    x10, x23, x25, x10
1004c31d4:      stp w8, w20, [x10]
1004c31d8:      stp w21, w9, [x10, #0x8]
1004c31dc:      sub x11, x29, #0xc8
1004c31e0:      ldur    q0, [x11, #0x10]
1004c31e4:      str q0, [x10, #0x10]
1004c31e8:      ldur    x11, [x11, #0x20]
1004c31ec:      str x11, [x10, #0x20]
1004c31f0:      cmp w8, #0x2
1004c31f4:      b.eq    0x1004c33b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4b0>
1004c31f8:      cmp w8, #0x1
1004c31fc:      b.ne    0x1004c33cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4cc>
1004c3200:      mov x20, x9
1004c3204:      cmp x23, #0x0
1004c3208:      mov w8, #0x1                ; =1
1004c320c:      cinc    w8, w8, ne
1004c3210:      adds    w9, w27, w22
1004c3214:      b.lo    0x1004c3414 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3218:      b   0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c321c:      mov w8, #0x7ffd             ; =32765
1004c3220:      cmp x8, x21, lsr #48
1004c3224:      b.ne    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3228:      and x21, x21, #0xffffffffffff
1004c322c:      mov x0, x21
1004c3230:      bl  0x1005354c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004c3234:      cbz x0, 0x1004c353c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3238:      ldrb    w8, [x0]
1004c323c:      cmp w8, #0x1
1004c3240:      b.ne    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3244:      ldrsb   w8, [x0, #0x1]
1004c3248:      tbnz    w8, #0x1f, 0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c324c:      ldrh    w8, [x0, #0x2]
1004c3250:      tbnz    w8, #0xa, 0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3254:      ldr w8, [x0, #0x4]
1004c3258:      cmp w8, #0x10
1004c325c:      b.lo    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3260:      mov x20, x19
1004c3264:      ldr w19, [x21]
1004c3268:      cmp w19, #0x10
1004c326c:      b.hi    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3270:      ldr w9, [x21, #0x4]
1004c3274:      cmp w19, w9
1004c3278:      b.hi    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c327c:      str w27, [sp, #0x20]
1004c3280:      lsl x27, x19, #3
1004c3284:      add x9, x27, #0x10
1004c3288:      cmp x9, x8
1004c328c:      b.hi    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3290:      mov x0, x21
1004c3294:      bl  0x1004fad6c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array6header35array_has_named_properties_resolved>
1004c3298:      tbnz    w0, #0x0, 0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c329c:      mov x0, x21
1004c32a0:      bl  0x100566bc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object15prototype_chain23object_static_prototype>
1004c32a4:      cmp x0, #0x1
1004c32a8:      b.eq    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c32ac:      str w24, [sp, #0x1c]
1004c32b0:      add x24, x20, x19
1004c32b4:      cmp x24, #0x10
1004c32b8:      b.hi    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c32bc:      add x8, sp, #0x180
1004c32c0:      madd    x8, x23, x25, x8
1004c32c4:      mov w9, #-0x1               ; =-1
1004c32c8:      str w9, [x8]
1004c32cc:      stp x20, x19, [x8, #0x8]
1004c32d0:      cbz w19, 0x1004c33f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4f0>
1004c32d4:      str w22, [sp, #0x4]
1004c32d8:      mov w9, #0x28               ; =40
1004c32dc:      mov x25, #0x0               ; =0
1004c32e0:      add x19, x21, #0x8
1004c32e4:      ldr x8, [sp, #0x10]
1004c32e8:      madd    x22, x20, x9, x8
1004c32ec:      mov w20, #0x2               ; =2
1004c32f0:      mov w21, #0x2               ; =2
1004c32f4:      ldr x1, [x19, x25]
1004c32f8:      sub x0, x29, #0x80
1004c32fc:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c3300:      ldur    w8, [x29, #-0x80]
1004c3304:      cmn w8, #0x1
1004c3308:      b.eq    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c330c:      ldp w10, w9, [x29, #-0x7c]
1004c3310:      ldur    w11, [x29, #-0x74]
1004c3314:      ldur    q0, [x28, #0x10]
1004c3318:      stur    q0, [x29, #-0xa0]
1004c331c:      ldur    x12, [x28, #0x20]
1004c3320:      stur    x12, [x29, #-0x90]
1004c3324:      stp w8, w10, [x22, #-0x8]
1004c3328:      stp w9, w11, [x22]
1004c332c:      stur    q0, [x22, #0x8]
1004c3330:      str x12, [x22, #0x18]
1004c3334:      cbz w8, 0x1004c3358 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x458>
1004c3338:      cmp w8, #0x1
1004c333c:      csel    w8, w11, w10, eq
1004c3340:      csel    w10, w9, w10, eq
1004c3344:      cmp x25, #0x0
1004c3348:      cset    w9, ne
1004c334c:      adds    w10, w10, w21
1004c3350:      b.lo    0x1004c3370 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x470>
1004c3354:      b   0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3358:      add w10, w10, #0x2
1004c335c:      add w8, w9, #0x2
1004c3360:      cmp x25, #0x0
1004c3364:      cset    w9, ne
1004c3368:      adds    w10, w10, w21
1004c336c:      b.hs    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3370:      adds    w21, w10, w9
1004c3374:      b.hs    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3378:      mov x0, #0x0                ; =0
1004c337c:      adds    w8, w8, w20
1004c3380:      cset    w10, hs
1004c3384:      adds    w20, w8, w9
1004c3388:      cset    w8, hs
1004c338c:      tbnz    w10, #0x0, 0x1004c353c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3390:      tbnz    w8, #0x0, 0x1004c353c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3394:      add x25, x25, #0x8
1004c3398:      add x22, x22, #0x28
1004c339c:      cmp x27, x25
1004c33a0:      b.ne    0x1004c32f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x3f4>
1004c33a4:      mov x19, x24
1004c33a8:      ldr w22, [sp, #0x4]
1004c33ac:      b   0x1004c33fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4fc>
1004c33b0:      mov x21, x20
1004c33b4:      cmp x23, #0x0
1004c33b8:      mov w8, #0x1                ; =1
1004c33bc:      cinc    w8, w8, ne
1004c33c0:      adds    w9, w27, w22
1004c33c4:      b.lo    0x1004c3414 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c33c8:      b   0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c33cc:      add w8, w20, #0x2
1004c33d0:      add w20, w21, #0x2
1004c33d4:      mov x21, x8
1004c33d8:      cmp x23, #0x0
1004c33dc:      mov w8, #0x1                ; =1
1004c33e0:      cinc    w8, w8, ne
1004c33e4:      adds    w9, w27, w22
1004c33e8:      b.lo    0x1004c3414 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c33ec:      b   0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c33f0:      mov w20, #0x2               ; =2
1004c33f4:      mov w21, #0x2               ; =2
1004c33f8:      mov x19, x24
1004c33fc:      ldp w24, w27, [sp, #0x1c]
1004c3400:      cmp x23, #0x0
1004c3404:      mov w8, #0x1                ; =1
1004c3408:      cinc    w8, w8, ne
1004c340c:      adds    w9, w27, w22
1004c3410:      b.hs    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3414:      adds    w9, w21, w9
1004c3418:      b.hs    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c341c:      adds    w11, w9, w8
1004c3420:      b.hs    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3424:      adds    w9, w24, w26
1004c3428:      b.hs    0x1004c3538 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c342c:      mov x0, #0x0                ; =0
1004c3430:      adds    w9, w20, w9
1004c3434:      cset    w10, hs
1004c3438:      adds    w26, w9, w8
1004c343c:      cset    w8, hs
1004c3440:      tbnz    w10, #0x0, 0x1004c353c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3444:      tbnz    w8, #0x0, 0x1004c353c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3448:      add x23, x23, #0x1
1004c344c:      ldr x8, [sp, #0x38]
1004c3450:      cmp x23, x8
1004c3454:      mov x22, x11
1004c3458:      mov w25, #0x28              ; =40
1004c345c:      b.ne    0x1004c313c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x23c>
1004c3460:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c3464:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
1004c3468:      mov w8, #0x1                ; =1
1004c346c:      ldr x19, [sp, #0x8]
1004c3470:      stp x19, x9, [x29, #-0x78]
1004c3474:      stp x0, x8, [x29, #-0x88]
1004c3478:      sub x0, x29, #0x80
1004c347c:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c3480:      mov x23, x0
1004c3484:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c3488:      add x0, x0, #0xbf0
1004c348c:      ldr x8, [x0]
1004c3490:      blr x8
1004c3494:      strb    wzr, [x0]
1004c3498:      mov x0, x19
1004c349c:      bl  0x1004c2b40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
1004c34a0:      tbz w0, #0x0, 0x1004c3530 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x630>
1004c34a4:      mov x0, x22
1004c34a8:      bl  0x10032570c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004c34ac:      mov x21, x1
1004c34b0:      stp w26, w22, [x0]
1004c34b4:      stp wzr, wzr, [x0, #0xc]
1004c34b8:      str w22, [x0, #0x8]
1004c34bc:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c34c0:      ldr x8, [x24, #0x48]
1004c34c4:      cmn x8, #0x1
1004c34c8:      str x0, [sp, #0x20]
1004c34cc:      b.eq    0x1004c355c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c34d0:      mrs x9, TPIDRRO_EL0
1004c34d4:      and x9, x9, #0xfffffffffffffff8
1004c34d8:      ldr x8, [x9, x8, lsl #3]
1004c34dc:      cbz x8, 0x1004c355c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c34e0:      ldr x8, [x8, #0x19e8]
1004c34e4:      cbz x8, 0x1004c355c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c34e8:      ldr x9, [x8]
1004c34ec:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c34f0:      cmp x9, x10
1004c34f4:      b.hs    0x1004c3810 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x910>
1004c34f8:      add x10, x9, #0x1
1004c34fc:      str x10, [x8]
1004c3500:      ldr x10, [x8, #0x18]
1004c3504:      cmp x23, x10
1004c3508:      b.hs    0x1004c381c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1004c350c:      ldr x10, [x8, #0x10]
1004c3510:      mov w11, #0x18              ; =24
1004c3514:      madd    x10, x23, x11, x10
1004c3518:      ldr x11, [x10]
1004c351c:      cmp x11, #0x1
1004c3520:      b.ne    0x1004c3820 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x920>
1004c3524:      ldr x0, [x10, #0x8]
1004c3528:      str x9, [x8]
1004c352c:      b   0x1004c3564 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x664>
1004c3530:      sub x0, x29, #0x88
1004c3534:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3538:      mov x0, #0x0                ; =0
1004c353c:      add sp, sp, #0x5d0
1004c3540:      ldp x29, x30, [sp, #0x50]
1004c3544:      ldp x20, x19, [sp, #0x40]
1004c3548:      ldp x22, x21, [sp, #0x30]
1004c354c:      ldp x24, x23, [sp, #0x20]
1004c3550:      ldp x26, x25, [sp, #0x10]
1004c3554:      ldp x28, x27, [sp], #0x60
1004c3558:      ret
1004c355c:      mov x0, x23
1004c3560:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c3564:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3568:      ldr w20, [x0, #0x4]
1004c356c:      mov w8, #-0x40000000        ; =-1073741824
1004c3570:      cmp w20, w8
1004c3574:      csel    w19, w20, w8, lt
1004c3578:      ldr w22, [x9, #0x634]
1004c357c:      cmp w22, #0x300
1004c3580:      b.hs    0x1004c3620 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c3584:      ldr x8, [x24, #0x48]
1004c3588:      cmn x8, #0x1
1004c358c:      b.eq    0x1004c3604 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3590:      mrs x9, TPIDRRO_EL0
1004c3594:      and x9, x9, #0xfffffffffffffff8
1004c3598:      ldr x8, [x9, x8, lsl #3]
1004c359c:      cbz x8, 0x1004c3604 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c35a0:      add x8, x8, x22, lsl #3
1004c35a4:      ldr x8, [x8, #0x1e8]
1004c35a8:      cbz x8, 0x1004c3620 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c35ac:      ldr x8, [x8]
1004c35b0:      cbz x8, 0x1004c3644 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x744>
1004c35b4:      mov w9, #-0x40000001        ; =-1073741825
1004c35b8:      cmp w20, w9
1004c35bc:      b.gt    0x1004c3660 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c35c0:      ldr x11, [x8, #0x5198]
1004c35c4:      and w9, w19, #0x3fffffff
1004c35c8:      lsr x10, x9, #15
1004c35cc:      cmp x10, x11
1004c35d0:      b.hs    0x1004c3660 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c35d4:      ldr x8, [x8, #0x5190]
1004c35d8:      ldr x8, [x8, x10, lsl #3]
1004c35dc:      cbz x8, 0x1004c3664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c35e0:      ubfx    x10, x9, #5, #10
1004c35e4:      ldr x8, [x8, x10, lsl #3]
1004c35e8:      cbz x8, 0x1004c3664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c35ec:      and x9, x9, #0x1f
1004c35f0:      add x8, x8, x9, lsl #5
1004c35f4:      ldrb    w9, [x8, #0x1c]
1004c35f8:      tbz w9, #0x0, 0x1004c3660 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c35fc:      ldr x8, [x8]
1004c3600:      b   0x1004c3664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3604:      mov x23, x0
1004c3608:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c360c:      mov x8, x0
1004c3610:      mov x0, x23
1004c3614:      add x8, x8, x22, lsl #3
1004c3618:      ldr x8, [x8, #0x1e8]
1004c361c:      cbnz    x8, 0x1004c35ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6ac>
1004c3620:      adrp    x8, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
1004c3624:      add x8, x8, #0x330
1004c3628:      mov x22, x0
1004c362c:      mov x0, x8
1004c3630:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3634:      mov x8, x0
1004c3638:      mov x0, x22
1004c363c:      ldr x8, [x8]
1004c3640:      cbnz    x8, 0x1004c35b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6b4>
1004c3644:      mov x22, x0
1004c3648:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c364c:      mov x8, x0
1004c3650:      mov x0, x22
1004c3654:      mov w9, #-0x40000001        ; =-1073741825
1004c3658:      cmp w20, w9
1004c365c:      b.le    0x1004c35c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6c0>
1004c3660:      mov x8, #0x0                ; =0
1004c3664:      mov x19, #0x0               ; =0
1004c3668:      mov w9, #0x7b               ; =123
1004c366c:      strb    w9, [x21]
1004c3670:      add x25, x8, #0x8
1004c3674:      add x24, x0, #0x10
1004c3678:      add x8, sp, #0x2c0
1004c367c:      add x8, x8, #0x28
1004c3680:      stp x8, x25, [sp, #0x28]
1004c3684:      mov w22, #0x1               ; =1
1004c3688:      add x27, sp, #0x40
1004c368c:      mov w28, #0x3a              ; =58
1004c3690:      mov w20, #0x2c              ; =44
1004c3694:      b   0x1004c36bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c3698:      mov w8, #0x5d               ; =93
1004c369c:      strb    w8, [x21, x25]
1004c36a0:      add x22, x25, #0x1
1004c36a4:      ldp x25, x8, [sp, #0x30]
1004c36a8:      add x27, sp, #0x40
1004c36ac:      mov w28, #0x3a              ; =58
1004c36b0:      add x19, x19, #0x1
1004c36b4:      cmp x19, x8
1004c36b8:      b.eq    0x1004c37e8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c36bc:      cbz x19, 0x1004c36c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7c8>
1004c36c0:      strb    w20, [x21, x22]
1004c36c4:      add x22, x22, #0x1
1004c36c8:      add x8, x19, x19, lsl #2
1004c36cc:      lsl x23, x8, #3
1004c36d0:      ldr x1, [x25, x19, lsl #3]
1004c36d4:      add x0, x27, x23
1004c36d8:      add x2, x21, x22
1004c36dc:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c36e0:      add x8, x0, x22
1004c36e4:      strb    w28, [x21, x8]
1004c36e8:      add x26, x8, #0x1
1004c36ec:      ldr x1, [x24, x19, lsl #3]
1004c36f0:      add x9, sp, #0x180
1004c36f4:      add x0, x9, x23
1004c36f8:      ldr w9, [x0]
1004c36fc:      cmn w9, #0x1
1004c3700:      b.eq    0x1004c3724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x824>
1004c3704:      add x2, x21, x26
1004c3708:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c370c:      add x22, x0, x26
1004c3710:      add x19, x19, #0x1
1004c3714:      ldr x8, [sp, #0x38]
1004c3718:      cmp x19, x8
1004c371c:      b.ne    0x1004c36bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c3720:      b   0x1004c37e8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c3724:      ldp x23, x22, [x0, #0x8]
1004c3728:      add x25, x8, #0x2
1004c372c:      mov w8, #0x5b               ; =91
1004c3730:      strb    w8, [x21, x26]
1004c3734:      cbz x22, 0x1004c3698 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c3738:      cmp x23, #0xf
1004c373c:      b.hi    0x1004c3830 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x930>
1004c3740:      and x26, x1, #0xffffffffffff
1004c3744:      add x8, sp, #0x2c0
1004c3748:      mov w9, #0x28               ; =40
1004c374c:      madd    x8, x23, x9, x8
1004c3750:      ldp q0, q1, [x8]
1004c3754:      stp q0, q1, [x29, #-0x80]
1004c3758:      ldr x8, [x8, #0x20]
1004c375c:      stur    x8, [x29, #-0x60]
1004c3760:      ldr x1, [x26, #0x8]
1004c3764:      sub x0, x29, #0x80
1004c3768:      add x2, x21, x25
1004c376c:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3770:      add x25, x0, x25
1004c3774:      subs    x27, x22, #0x1
1004c3778:      b.eq    0x1004c3698 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c377c:      mov w9, #0x28               ; =40
1004c3780:      add x28, x26, #0x10
1004c3784:      cmp x23, #0x10
1004c3788:      mov w8, #0x10               ; =16
1004c378c:      csel    x8, x23, x8, lo
1004c3790:      sub x26, x8, #0xf
1004c3794:      add x22, x23, #0x1
1004c3798:      ldr x8, [sp, #0x28]
1004c379c:      madd    x23, x23, x9, x8
1004c37a0:      strb    w20, [x21, x25]
1004c37a4:      cbz x26, 0x1004c3834 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x934>
1004c37a8:      add x25, x25, #0x1
1004c37ac:      ldp q0, q1, [x23]
1004c37b0:      stp q0, q1, [x29, #-0x80]
1004c37b4:      ldr x8, [x23, #0x20]
1004c37b8:      stur    x8, [x29, #-0x60]
1004c37bc:      ldr x1, [x28], #0x8
1004c37c0:      sub x0, x29, #0x80
1004c37c4:      add x2, x21, x25
1004c37c8:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c37cc:      add x25, x0, x25
1004c37d0:      add x26, x26, #0x1
1004c37d4:      add x22, x22, #0x1
1004c37d8:      add x23, x23, #0x28
1004c37dc:      subs    x27, x27, #0x1
1004c37e0:      b.ne    0x1004c37a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8a0>
1004c37e4:      b   0x1004c3698 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c37e8:      mov w8, #0x7d               ; =125
1004c37ec:      strb    w8, [x21, x22]
1004c37f0:      mov x19, #0x7fff000000000000 ; =9223090561878065152
1004c37f4:      ldr x8, [sp, #0x20]
1004c37f8:      bfxil   x19, x8, #0, #48
1004c37fc:      sub x0, x29, #0x88
1004c3800:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3804:      mov x1, x19
1004c3808:      mov w0, #0x1                ; =1
1004c380c:      b   0x1004c353c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3810:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c3814:      add x0, x0, #0x8a8
1004c3818:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c381c:      bl  0x100ae2488 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c3820:      adrp    x0, 0x100bd8000 <_PERRY_EMPTY_STRING+0x2f0>
1004c3824:      add x0, x0, #0x96c
1004c3828:      mov w1, #0xb                ; =11
1004c382c:      bl  0x100ae2450 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c3830:      mov x22, x23
1004c3834:      adrp    x2, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
1004c3838:      add x2, x2, #0x0
1004c383c:      mov x0, x22
1004c3840:      mov w1, #0x10               ; =16
1004c3844:      bl  0x100ab0d0c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
