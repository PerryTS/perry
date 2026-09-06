/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/inline-object-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100834f48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
100834f48:      sub sp, sp, #0x1a0
100834f4c:      stp x28, x27, [sp, #0x140]
100834f50:      stp x26, x25, [sp, #0x150]
100834f54:      stp x24, x23, [sp, #0x160]
100834f58:      stp x22, x21, [sp, #0x170]
100834f5c:      stp x20, x19, [sp, #0x180]
100834f60:      stp x29, x30, [sp, #0x190]
100834f64:      add x29, sp, #0x190
100834f68:      mov x20, x2
100834f6c:      mov x22, x1
100834f70:      mov x19, x0
100834f74:      add x24, sp, #0x90
100834f78:      add x23, x1, #0x14
100834f7c:      cmp x2, #0x2
100834f80:      b.ne    0x100834f98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50>
100834f84:      ldrh    w8, [x23]
100834f88:      mov w9, #0x7d7b             ; =32123
100834f8c:      cmp w8, w9
100834f90:      b.eq    0x100834fcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
100834f94:      b   0x10083500c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
100834f98:      cmp x20, #0x3
100834f9c:      b.lo    0x10083500c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
100834fa0:      ldrb    w8, [x23]
100834fa4:      cmp w8, #0x20
100834fa8:      b.hi    0x100834fd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
100834fac:      mov x9, #0x2600             ; =9728
100834fb0:      movk    x9, #0x1, lsl #32
100834fb4:      lsr x9, x9, x8
100834fb8:      tbz w9, #0x0, 0x100834fd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
100834fbc:      add x0, x22, #0x14
100834fc0:      mov x1, x20
100834fc4:      bl  0x10081a228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
100834fc8:      tbz w0, #0x0, 0x100835004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
100834fcc:      bl  0x10081a2f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
100834fd0:      stp xzr, x0, [x19]
100834fd4:      b   0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
100834fd8:      cmp w8, #0x7b
100834fdc:      b.ne    0x100835004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
100834fe0:      ldrb    w8, [x22, #0x15]
100834fe4:      cmp w8, #0x20
100834fe8:      b.hi    0x100834ffc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
100834fec:      mov x9, #0x2600             ; =9728
100834ff0:      movk    x9, #0x1, lsl #32
100834ff4:      lsr x9, x9, x8
100834ff8:      tbnz    w9, #0x0, 0x100834fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
100834ffc:      cmp w8, #0x7d
100835000:      b.eq    0x100834fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
100835004:      cmp x20, #0x41
100835008:      b.hs    0x100835070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x128>
10083500c:      add x0, sp, #0x90
100835010:      add x1, x22, #0x14
100835014:      mov x2, x20
100835018:      bl  0x100821084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
10083501c:      ldr x8, [sp, #0x90]
100835020:      cbz x8, 0x10083511c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
100835024:      ldr x8, [sp, #0x118]
100835028:      str x8, [sp, #0x80]
10083502c:      ldur    q0, [x24, #0x48]
100835030:      ldur    q1, [x24, #0x58]
100835034:      stp q0, q1, [sp, #0x40]
100835038:      ldur    q0, [x24, #0x68]
10083503c:      ldur    q1, [x24, #0x78]
100835040:      stp q0, q1, [sp, #0x60]
100835044:      ldur    q0, [x24, #0x8]
100835048:      ldur    q1, [x24, #0x18]
10083504c:      stp q0, q1, [sp]
100835050:      ldur    q0, [x24, #0x28]
100835054:      ldur    q1, [x24, #0x38]
100835058:      stp q0, q1, [sp, #0x20]
10083505c:      mov x0, sp
100835060:      bl  0x100821ae4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
100835064:      tbz w0, #0x0, 0x10083511c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
100835068:      stp xzr, x1, [x19]
10083506c:      b   0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
100835070:      cmp x20, #0x3e9
100835074:      b.lo    0x10083511c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
100835078:      add x0, x22, #0x14
10083507c:      mov x1, x20
100835080:      mov w2, #0x3e8              ; =1000
100835084:      bl  0x100826214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100835088:      tbz w0, #0x0, 0x10083511c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
10083508c:      add x0, x22, #0x14
100835090:      mov x1, x20
100835094:      mov w2, #0xa120             ; =41248
100835098:      movk    w2, #0x7, lsl #16
10083509c:      bl  0x100826214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008350a0:      tbz w0, #0x0, 0x10083547c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x534>
1008350a4:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1008350a8:      add x8, x8, #0xf80
1008350ac:      adrp    x9, 0x100e0d000 <_anon.c2a4b59a01bfaad05414bd5c213a645e.1531+0x517>
1008350b0:      add x9, x9, #0x150
1008350b4:      stp x9, x8, [sp]
1008350b8:      adrp    x0, 0x100ef2000 <_anon.c2a4b59a01bfaad05414bd5c213a645e.728+0x33c>
1008350bc:      add x0, x0, #0x448
1008350c0:      add x8, sp, #0x90
1008350c4:      mov x1, sp
1008350c8:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
1008350cc:      ldr x20, [sp, #0x98]
1008350d0:      ldr w1, [sp, #0xa0]
1008350d4:      mov x0, x20
1008350d8:      mov x2, x1
1008350dc:      bl  0x100883d00 <_js_string_from_bytes_with_capacity>
1008350e0:      mov x3, x0
1008350e4:      adrp    x1, 0x100e0a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_MAC_CYRILLIC+0x20>
1008350e8:      add x1, x1, #0x82d
1008350ec:      mov w21, #0x1               ; =1
1008350f0:      mov w0, #0x2                ; =2
1008350f4:      mov w2, #0xa                ; =10
1008350f8:      mov w4, #0x1                ; =1
1008350fc:      bl  0x1007eb42c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
100835100:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100835104:      bfxil   x8, x0, #0, #48
100835108:      stp x21, x8, [x19]
10083510c:      ldr x8, [sp, #0x90]
100835110:      cbz x8, 0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
100835114:      mov x0, x20
100835118:      b   0x1008353c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x478>
10083511c:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100835120:      add x0, x0, #0x1b0
100835124:      ldr x8, [x0]
100835128:      blr x8
10083512c:      mov x21, x0
100835130:      ldrb    w8, [x0, #0x20]
100835134:      cbnz    w8, 0x100835548 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x600>
100835138:      ldr x8, [x21]
10083513c:      cbnz    x8, 0x100835594 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x64c>
100835140:      mov x23, #0x7fff000000000000 ; =9223090561878065152
100835144:      bfxil   x23, x22, #0, #48
100835148:      mov x8, #-0x1               ; =-1
10083514c:      str x8, [x21]
100835150:      mov x22, x21
100835154:      ldr x8, [x22, #0x8]!
100835158:      ldr x25, [x21, #0x18]
10083515c:      cmp x25, x8
100835160:      b.ne    0x10083516c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x224>
100835164:      mov x0, x22
100835168:      bl  0x100cbaee0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10083516c:      ldr x8, [x21, #0x10]
100835170:      str x23, [x8, x25, lsl #3]
100835174:      add x8, x25, #0x1
100835178:      str x8, [x21, #0x18]
10083517c:      ldr x8, [x21]
100835180:      add x8, x8, #0x1
100835184:      str x8, [x21]
100835188:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
10083518c:      add x0, x0, #0xa8
100835190:      ldr x8, [x0]
100835194:      blr x8
100835198:      ldrb    w9, [x0]
10083519c:      strb    wzr, [x0]
1008351a0:      adrp    x23, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
1008351a4:      add x23, x23, #0x270
1008351a8:      cbz w9, 0x1008351e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x29c>
1008351ac:      mov x8, x0
1008351b0:      ldr x9, [x23]
1008351b4:      mov x0, x23
1008351b8:      blr x9
1008351bc:      ldrb    w9, [x0]
1008351c0:      tst w9, #0x3
1008351c4:      b.ne    0x1008351dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x294>
1008351c8:      adrp    x9, 0x101201000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xf6e8>
1008351cc:      add x9, x9, #0x918
1008351d0:      ldapr   w9, [x9]
1008351d4:      cmp w9, #0x0
1008351d8:      b.le    0x1008353e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x49c>
1008351dc:      mov w9, #0x1                ; =1
1008351e0:      strb    w9, [x8]
1008351e4:      bl  0x100813240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008351e8:      bl  0x100812c38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008351ec:      ldrb    w8, [x21, #0x20]
1008351f0:      cbnz    w8, 0x100835444 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4fc>
1008351f4:      ldr x8, [x21]
1008351f8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008351fc:      cmp x8, x9
100835200:      b.hs    0x100835470 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x528>
100835204:      add x9, x8, #0x1
100835208:      str x9, [x21]
10083520c:      ldr x10, [x21, #0x18]
100835210:      mov w9, #0x1                ; =1
100835214:      cmp x25, x10
100835218:      b.hs    0x10083522c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2e4>
10083521c:      ldr x10, [x21, #0x10]
100835220:      ldr x10, [x10, x25, lsl #3]
100835224:      and x10, x10, #0xffffffffffff
100835228:      b   0x100835230 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2e8>
10083522c:      mov w10, #0x1               ; =1
100835230:      str x8, [x21]
100835234:      add x8, x10, #0x14
100835238:      movi.2d v0, #0000000000000000
10083523c:      stur    q0, [x24, #0x78]
100835240:      stur    q0, [x24, #0x68]
100835244:      stur    q0, [x24, #0x58]
100835248:      stur    q0, [x24, #0x48]
10083524c:      strb    w9, [sp, #0x120]
100835250:      mov x9, #-0x1               ; =-1
100835254:      stp x8, x20, [sp, #0xb8]
100835258:      str x9, [sp, #0x90]
10083525c:      stp xzr, xzr, [sp, #0xc8]
100835260:      str xzr, [sp, #0x118]
100835264:      add x0, sp, #0x90
100835268:      bl  0x1007db080 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10083526c:      mov x20, x0
100835270:      ldp x8, x9, [sp, #0xc0]
100835274:      cmp x9, x8
100835278:      b.hs    0x1008352ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x364>
10083527c:      ldr x10, [sp, #0xb8]
100835280:      mov x11, #0x2600            ; =9728
100835284:      movk    x11, #0x1, lsl #32
100835288:      ldrb    w12, [x10, x9]
10083528c:      cmp w12, #0x20
100835290:      b.hi    0x1008352ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x364>
100835294:      lsr x12, x11, x12
100835298:      tbz w12, #0x0, 0x1008352ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x364>
10083529c:      add x9, x9, #0x1
1008352a0:      cmp x8, x9
1008352a4:      b.ne    0x100835288 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x340>
1008352a8:      mov x9, x8
1008352ac:      ldrb    w24, [sp, #0x120]
1008352b0:      cmp x9, x8
1008352b4:      cset    w26, eq
1008352b8:      ldrb    w8, [x21, #0x20]
1008352bc:      cbnz    w8, 0x100835570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x628>
1008352c0:      ldr x8, [x21]
1008352c4:      cbnz    x8, 0x100835594 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x64c>
1008352c8:      mov x8, #-0x1               ; =-1
1008352cc:      str x8, [x21]
1008352d0:      ldr x27, [x21, #0x18]
1008352d4:      ldr x8, [x21, #0x8]
1008352d8:      cmp x27, x8
1008352dc:      b.ne    0x1008352e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3a0>
1008352e0:      mov x0, x22
1008352e4:      bl  0x100cbaee0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008352e8:      ldr x8, [x21, #0x10]
1008352ec:      str x20, [x8, x27, lsl #3]
1008352f0:      add x8, x27, #0x1
1008352f4:      str x8, [x21, #0x18]
1008352f8:      ldr x8, [x21]
1008352fc:      add x8, x8, #0x1
100835300:      str x8, [x21]
100835304:      ldr x8, [x23]
100835308:      mov x0, x23
10083530c:      blr x8
100835310:      ldrb    w8, [x0]
100835314:      and w8, w8, #0xfffffffd
100835318:      strb    w8, [x0]
10083531c:      bl  0x100813cd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
100835320:      bl  0x100818820 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
100835324:      ldrb    w8, [x21, #0x20]
100835328:      cbnz    w8, 0x1008355a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x658>
10083532c:      ldr x8, [x21]
100835330:      cbnz    x8, 0x1008355d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x688>
100835334:      and w22, w26, w24
100835338:      ldr x8, [x21, #0x18]
10083533c:      cmp x25, x8
100835340:      b.hi    0x100835348 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x400>
100835344:      str x25, [x21, #0x18]
100835348:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
10083534c:      add x0, x0, #0xc10
100835350:      bl  0x10013a29c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
100835354:      tbz w22, #0x0, 0x10083536c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x424>
100835358:      stp xzr, x20, [x19]
10083535c:      ldr x8, [sp, #0x90]
100835360:      cmn x8, #0x1
100835364:      b.ne    0x1008353b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x470>
100835368:      b   0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
10083536c:      adrp    x0, 0x100e0b000 <_anon.c2a4b59a01bfaad05414bd5c213a645e.880+0x102>
100835370:      add x0, x0, #0x557
100835374:      mov w1, #0x21               ; =33
100835378:      mov w2, #0x21               ; =33
10083537c:      bl  0x100883d00 <_js_string_from_bytes_with_capacity>
100835380:      mov x3, x0
100835384:      adrp    x1, 0x100e0a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_MAC_CYRILLIC+0x20>
100835388:      add x1, x1, #0x845
10083538c:      mov w20, #0x1               ; =1
100835390:      mov w0, #0x4                ; =4
100835394:      mov w2, #0xb                ; =11
100835398:      mov w4, #0x1                ; =1
10083539c:      bl  0x1007eb42c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008353a0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008353a4:      bfxil   x8, x0, #0, #48
1008353a8:      stp x20, x8, [x19]
1008353ac:      ldr x8, [sp, #0x90]
1008353b0:      cmn x8, #0x1
1008353b4:      b.eq    0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
1008353b8:      cbz x8, 0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
1008353bc:      ldr x0, [sp, #0x98]
1008353c0:      bl  0x100cd8880 <_mi_free>
1008353c4:      ldp x29, x30, [sp, #0x190]
1008353c8:      ldp x20, x19, [sp, #0x180]
1008353cc:      ldp x22, x21, [sp, #0x170]
1008353d0:      ldp x24, x23, [sp, #0x160]
1008353d4:      ldp x26, x25, [sp, #0x150]
1008353d8:      ldp x28, x27, [sp, #0x140]
1008353dc:      add sp, sp, #0x1a0
1008353e0:      ret
1008353e4:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008353e8:      add x0, x0, #0x608
1008353ec:      ldr x8, [x0]
1008353f0:      blr x8
1008353f4:      ldr x8, [x0]
1008353f8:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json12RAW_JSON_KEY0s_023___RUST_STD_INTERNAL_VAL+0x8>
1008353fc:      add x0, x0, #0xf58
100835400:      ldr x9, [x0]
100835404:      blr x9
100835408:      ldr x9, [x0]
10083540c:      cmp x9, x8
100835410:      b.ls    0x100835430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4e8>
100835414:      str x8, [x0]
100835418:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
10083541c:      add x0, x0, #0x1e0
100835420:      ldr x8, [x0]
100835424:      blr x8
100835428:      mov w8, #0x1                ; =1
10083542c:      strb    w8, [x0]
100835430:      bl  0x100813240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100835434:      bl  0x100813240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100835438:      bl  0x100812c38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
10083543c:      ldrb    w8, [x21, #0x20]
100835440:      cbz w8, 0x1008351f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2ac>
100835444:      cmp w8, #0x2
100835448:      b.eq    0x1008355a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x660>
10083544c:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100835450:      add x1, x1, #0x36c
100835454:      mov x0, x21
100835458:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10083545c:      strb    wzr, [x21, #0x20]
100835460:      ldr x8, [x21]
100835464:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100835468:      cmp x8, x9
10083546c:      b.lo    0x100835204 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2bc>
100835470:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100835474:      add x0, x0, #0xdc8
100835478:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10083547c:      stur    x20, [x29, #-0x68]
100835480:      mov x8, #0x7fff000000000000 ; =9223090561878065152
100835484:      bfxil   x8, x22, #0, #48
100835488:      str x8, [sp, #0x90]
10083548c:      adrp    x21, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100835490:      add x21, x21, #0xc00
100835494:      add x1, sp, #0x90
100835498:      mov x0, x21
10083549c:      bl  0x100137b50 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1008354a0:      mov x22, x0
1008354a4:      stp x0, x23, [x29, #-0x60]
1008354a8:      str x20, [sp]
1008354ac:      sub x8, x29, #0x58
1008354b0:      mov x9, sp
1008354b4:      stp x8, x9, [sp, #0x90]
1008354b8:      sub x8, x29, #0x60
1008354bc:      sub x9, x29, #0x68
1008354c0:      stp x8, x9, [sp, #0xa0]
1008354c4:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
1008354c8:      add x0, x0, #0x3d0
1008354cc:      add x1, sp, #0x90
1008354d0:      bl  0x10012c970 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1008354d4:      mov x23, x0
1008354d8:      mov x20, x1
1008354dc:      str x22, [sp, #0x90]
1008354e0:      add x1, sp, #0x90
1008354e4:      mov x0, x21
1008354e8:      bl  0x100137be8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1008354ec:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
1008354f0:      add x0, x0, #0xc10
1008354f4:      bl  0x10013a404 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1008354f8:      tbnz    w23, #0x0, 0x10083553c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5f4>
1008354fc:      adrp    x0, 0x100e03000 <_anon.d13bca72c7c43155356dec6763133824.2879+0x6dd>
100835500:      add x0, x0, #0xd2e
100835504:      mov w1, #0x29               ; =41
100835508:      mov w2, #0x29               ; =41
10083550c:      bl  0x100883d00 <_js_string_from_bytes_with_capacity>
100835510:      mov x3, x0
100835514:      adrp    x1, 0x100e0a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_MAC_CYRILLIC+0x20>
100835518:      add x1, x1, #0x845
10083551c:      mov w21, #0x1               ; =1
100835520:      mov w0, #0x4                ; =4
100835524:      mov w2, #0xb                ; =11
100835528:      mov w4, #0x1                ; =1
10083552c:      bl  0x1007eb42c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
100835530:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
100835534:      bfxil   x20, x0, #0, #48
100835538:      b   0x100835540 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5f8>
10083553c:      mov x21, #0x0               ; =0
100835540:      stp x21, x20, [x19]
100835544:      b   0x1008353c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x47c>
100835548:      cmp w8, #0x1
10083554c:      b.ne    0x1008355a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x660>
100835550:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100835554:      add x1, x1, #0x36c
100835558:      mov x0, x21
10083555c:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100835560:      strb    wzr, [x21, #0x20]
100835564:      ldr x8, [x21]
100835568:      cbz x8, 0x100835140 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1f8>
10083556c:      b   0x100835594 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x64c>
100835570:      cmp w8, #0x2
100835574:      b.eq    0x1008355a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x660>
100835578:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
10083557c:      add x1, x1, #0x36c
100835580:      mov x0, x21
100835584:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100835588:      strb    wzr, [x21, #0x20]
10083558c:      ldr x8, [x21]
100835590:      cbz x8, 0x1008352c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x380>
100835594:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100835598:      add x0, x0, #0xdf8
10083559c:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008355a0:      cmp w8, #0x2
1008355a4:      b.ne    0x1008355b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x66c>
1008355a8:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008355ac:      add x0, x0, #0xed8
1008355b0:      bl  0x100cd1edc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008355b4:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1008355b8:      add x1, x1, #0x36c
1008355bc:      mov x0, x21
1008355c0:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008355c4:      strb    wzr, [x21, #0x20]
1008355c8:      ldr x8, [x21]
1008355cc:      cbz x8, 0x100835334 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3ec>
1008355d0:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008355d4:      add x0, x0, #0xe58
1008355d8:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
