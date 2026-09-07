/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008dc0c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>:
1008dc0c0:      sub sp, sp, #0xa0
1008dc0c4:      stp x28, x27, [sp, #0x40]
1008dc0c8:      stp x26, x25, [sp, #0x50]
1008dc0cc:      stp x24, x23, [sp, #0x60]
1008dc0d0:      stp x22, x21, [sp, #0x70]
1008dc0d4:      stp x20, x19, [sp, #0x80]
1008dc0d8:      stp x29, x30, [sp, #0x90]
1008dc0dc:      add x29, sp, #0x90
1008dc0e0:      mov w8, #0x7ffd             ; =32765
1008dc0e4:      cmp x8, x1, lsr #48
1008dc0e8:      b.ne    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc0ec:      mov x20, x0
1008dc0f0:      mov x23, x2
1008dc0f4:      mov x19, x3
1008dc0f8:      and x21, x1, #0xffffffffffff
1008dc0fc:      mov x0, x21
1008dc100:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008dc104:      cbz x0, 0x1008dc210 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x150>
1008dc108:      ldrb    w8, [x0]
1008dc10c:      cmp w8, #0x2
1008dc110:      b.ne    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc114:      ldrsb   w8, [x0, #0x1]
1008dc118:      tbnz    w8, #0x1f, 0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc11c:      ldrh    w8, [x0, #0x2]
1008dc120:      mov w9, #0xa00              ; =2560
1008dc124:      and w8, w8, w9
1008dc128:      cmp w8, #0x200
1008dc12c:      b.ne    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc130:      ldr w8, [x0, #0x4]
1008dc134:      cmp w8, #0x18
1008dc138:      b.lo    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc13c:      ldr w8, [x21]
1008dc140:      cbnz    w8, 0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc144:      mov x22, x0
1008dc148:      mov x0, x21
1008dc14c:      bl  0x1007625a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
1008dc150:      cmp x0, #0x1
1008dc154:      b.eq    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc158:      ldr w8, [x21, #0x4]
1008dc15c:      mov w9, #-0x40000001        ; =-1073741825
1008dc160:      cmp w8, w9
1008dc164:      csel    w2, wzr, w8, gt
1008dc168:      b.gt    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc16c:      ldr w22, [x22, #0x4]
1008dc170:      mov w9, #0x79b9             ; =31161
1008dc174:      movk    w9, #0x9e37, lsl #16
1008dc178:      mul w8, w8, w9
1008dc17c:      lsr x3, x8, #25
1008dc180:      mov x10, x20
1008dc184:      add x9, x20, #0x18
1008dc188:      ldrb    w13, [x9, x3]
1008dc18c:      cbz w13, 0x1008dc1d0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x110>
1008dc190:      add x11, x10, #0x98
1008dc194:      mov w12, #0x8c              ; =140
1008dc198:      mov x8, x19
1008dc19c:      and w14, w13, #0xff
1008dc1a0:      sub w13, w13, #0x1
1008dc1a4:      and x1, x13, #0xff
1008dc1a8:      cmp w14, #0x41
1008dc1ac:      b.hs    0x1008dc64c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x58c>
1008dc1b0:      umull   x13, w1, w12
1008dc1b4:      ldr w13, [x11, x13]
1008dc1b8:      cmp w13, w2
1008dc1bc:      b.eq    0x1008dc1e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x128>
1008dc1c0:      add w13, w3, #0x1
1008dc1c4:      and x3, x13, #0x7f
1008dc1c8:      ldrb    w13, [x9, x3]
1008dc1cc:      cbnz    w13, 0x1008dc19c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xdc>
1008dc1d0:      mov x0, x10
1008dc1d4:      mov x1, x21
1008dc1d8:      bl  0x1008dc688 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan>
1008dc1dc:      tbz w0, #0x0, 0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc1e0:      mov x8, x19
1008dc1e4:      mov x10, x20
1008dc1e8:      cmp x1, #0x40
1008dc1ec:      b.hs    0x1008dc64c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x58c>
1008dc1f0:      mov w9, #0x8c               ; =140
1008dc1f4:      madd    x9, x1, x9, x10
1008dc1f8:      ldr w9, [x9, #0x9c]
1008dc1fc:      lsl x24, x9, #3
1008dc200:      add x11, x24, #0x18
1008dc204:      cmp x11, x22
1008dc208:      b.ls    0x1008dc230 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x170>
1008dc20c:      mov w0, #0x0                ; =0
1008dc210:      ldp x29, x30, [sp, #0x90]
1008dc214:      ldp x20, x19, [sp, #0x80]
1008dc218:      ldp x22, x21, [sp, #0x70]
1008dc21c:      ldp x24, x23, [sp, #0x60]
1008dc220:      ldp x26, x25, [sp, #0x50]
1008dc224:      ldp x28, x27, [sp, #0x40]
1008dc228:      add sp, sp, #0xa0
1008dc22c:      ret
1008dc230:      cbz w9, 0x1008dc5a4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4e4>
1008dc234:      mov x25, #0x0               ; =0
1008dc238:      mov x26, #0x1               ; =1
1008dc23c:      movk    x26, #0x7ffc, lsl #48
1008dc240:      add x27, x21, #0x10
1008dc244:      mov w9, #0x8c               ; =140
1008dc248:      madd    x9, x1, x9, x10
1008dc24c:      add x28, x9, #0xa4
1008dc250:      ldr x21, [x27, x25]
1008dc254:      cmp x21, x26
1008dc258:      b.eq    0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc25c:      cmp x25, #0x100
1008dc260:      b.eq    0x1008dc660 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5a0>
1008dc264:      ldur    w2, [x28, #-0x4]
1008dc268:      ldr w3, [x28], #0x4
1008dc26c:      ldp x9, x1, [x10, #0x8]
1008dc270:      cmp w2, w3
1008dc274:      ccmp    x1, x3, #0x0, ls
1008dc278:      b.lo    0x1008dc5fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x53c>
1008dc27c:      cmp x1, x2
1008dc280:      b.eq    0x1008dc2a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1e8>
1008dc284:      cbz w2, 0x1008dc294 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1d4>
1008dc288:      ldrsb   w10, [x9, x2]
1008dc28c:      cmn w10, #0x41
1008dc290:      b.le    0x1008dc5fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x53c>
1008dc294:      cmp x1, x3
1008dc298:      b.eq    0x1008dc2a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1e8>
1008dc29c:      ldrsb   w10, [x9, x3]
1008dc2a0:      cmn w10, #0x41
1008dc2a4:      b.le    0x1008dc5fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x53c>
1008dc2a8:      sub x22, x3, x2
1008dc2ac:      ldr x1, [x8, #0x10]
1008dc2b0:      ldr x10, [x8]
1008dc2b4:      sub x10, x10, x1
1008dc2b8:      cmp x22, x10
1008dc2bc:      b.hi    0x1008dc2e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x224>
1008dc2c0:      cmp w3, w2
1008dc2c4:      b.ne    0x1008dc318 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x258>
1008dc2c8:      mov x10, x23
1008dc2cc:      add x1, x1, x22
1008dc2d0:      str x1, [x8, #0x10]
1008dc2d4:      add x9, x26, #0xf
1008dc2d8:      cmp x21, x9
1008dc2dc:      b.ne    0x1008dc34c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x28c>
1008dc2e0:      b   0x1008dc3d4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x314>
1008dc2e4:      stp x3, x2, [sp, #0x10]
1008dc2e8:      sub x2, x3, x2
1008dc2ec:      mov x0, x8
1008dc2f0:      mov w3, #0x1                ; =1
1008dc2f4:      mov w4, #0x1                ; =1
1008dc2f8:      str x9, [sp, #0x8]
1008dc2fc:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dc300:      ldp x9, x3, [sp, #0x8]
1008dc304:      ldr x2, [sp, #0x18]
1008dc308:      mov x8, x19
1008dc30c:      ldr x1, [x19, #0x10]
1008dc310:      cmp w3, w2
1008dc314:      b.eq    0x1008dc2c8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x208>
1008dc318:      ldr x8, [x8, #0x8]
1008dc31c:      add x0, x8, x1
1008dc320:      add x1, x9, x2
1008dc324:      mov x2, x22
1008dc328:      bl  0x100ce43ec <_writev+0x100ce43ec>
1008dc32c:      mov x8, x19
1008dc330:      ldr x1, [x19, #0x10]
1008dc334:      mov x10, x23
1008dc338:      add x1, x1, x22
1008dc33c:      str x1, [x19, #0x10]
1008dc340:      add x9, x26, #0xf
1008dc344:      cmp x21, x9
1008dc348:      b.eq    0x1008dc3d4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x314>
1008dc34c:      and x9, x21, #0xffff000000000000
1008dc350:      mov x11, #0x7ffa000000000000 ; =9221683186994511872
1008dc354:      cmp x9, x11
1008dc358:      b.eq    0x1008dc3d4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x314>
1008dc35c:      mov x11, #0x7ffd000000000000 ; =9222527611924643840
1008dc360:      cmp x9, x11
1008dc364:      b.eq    0x1008dc3d4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x314>
1008dc368:      mov x11, #-0x10000000000000 ; =-4503599627370496
1008dc36c:      add x11, x21, x11
1008dc370:      tst x21, #0x7
1008dc374:      mov x12, #-0xfffffffffffff  ; =-4503599627370495
1008dc378:      ccmp    x11, x12, #0x0, eq
1008dc37c:      b.hs    0x1008dc3d4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x314>
1008dc380:      mov x10, #0x2               ; =2
1008dc384:      movk    x10, #0x7ffc, lsl #48
1008dc388:      cmp x21, x10
1008dc38c:      b.eq    0x1008dc444 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x384>
1008dc390:      mov x10, #0x3               ; =3
1008dc394:      movk    x10, #0x7ffc, lsl #48
1008dc398:      cmp x21, x10
1008dc39c:      b.eq    0x1008dc408 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x348>
1008dc3a0:      mov x10, #0x4               ; =4
1008dc3a4:      movk    x10, #0x7ffc, lsl #48
1008dc3a8:      cmp x21, x10
1008dc3ac:      b.ne    0x1008dc44c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x38c>
1008dc3b0:      ldr x8, [x8]
1008dc3b4:      sub x8, x8, x1
1008dc3b8:      cmp x8, #0x3
1008dc3bc:      b.ls    0x1008dc56c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4ac>
1008dc3c0:      mov x8, x19
1008dc3c4:      ldr x9, [x19, #0x8]
1008dc3c8:      mov w10, #0x7274            ; =29300
1008dc3cc:      movk    w10, #0x6575, lsl #16
1008dc3d0:      b   0x1008dc510 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x450>
1008dc3d4:      tbz w10, #0x0, 0x1008dc20c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008dc3d8:      mov x0, x20
1008dc3dc:      mov x1, x21
1008dc3e0:      mov w2, #0x0                ; =0
1008dc3e4:      mov x3, x19
1008dc3e8:      bl  0x1008dc0c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
1008dc3ec:      mov x8, x19
1008dc3f0:      mov x10, x20
1008dc3f4:      cbz w0, 0x1008dc210 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x150>
1008dc3f8:      add x25, x25, #0x8
1008dc3fc:      cmp x24, x25
1008dc400:      b.ne    0x1008dc250 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x190>
1008dc404:      b   0x1008dc5d0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x510>
1008dc408:      ldr x8, [x8]
1008dc40c:      sub x8, x8, x1
1008dc410:      cmp x8, #0x4
1008dc414:      b.ls    0x1008dc588 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4c8>
1008dc418:      mov x8, x19
1008dc41c:      ldr x9, [x19, #0x8]
1008dc420:      add x9, x9, x1
1008dc424:      mov w10, #0x65              ; =101
1008dc428:      strb    w10, [x9, #0x4]
1008dc42c:      mov w10, #0x6166            ; =24934
1008dc430:      movk    w10, #0x736c, lsl #16
1008dc434:      str w10, [x9]
1008dc438:      ldr x9, [x19, #0x10]
1008dc43c:      add x9, x9, #0x5
1008dc440:      b   0x1008dc51c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x45c>
1008dc444:      ldr x8, [x8]
1008dc448:      b   0x1008dc4f4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x434>
1008dc44c:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1008dc450:      cmp x9, x8
1008dc454:      b.eq    0x1008dc47c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3bc>
1008dc458:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008dc45c:      cmp x9, x8
1008dc460:      b.ne    0x1008dc528 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x468>
1008dc464:      and x8, x21, #0xffffffffffff
1008dc468:      cmp x8, #0x1, lsl #12       ; =0x1000
1008dc46c:      b.lo    0x1008dc4f0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x430>
1008dc470:      ldr w2, [x8, #0x4]
1008dc474:      add x1, x8, #0x14
1008dc478:      b   0x1008dc53c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x47c>
1008dc47c:      strb    wzr, [sp, #0x24]
1008dc480:      str wzr, [sp, #0x20]
1008dc484:      ubfx    x1, x21, #40, #8
1008dc488:      cbz x1, 0x1008dc4d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x418>
1008dc48c:      strb    w21, [sp, #0x20]
1008dc490:      cmp x1, #0x1
1008dc494:      b.eq    0x1008dc4d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x418>
1008dc498:      lsr x8, x21, #8
1008dc49c:      strb    w8, [sp, #0x21]
1008dc4a0:      cmp x1, #0x2
1008dc4a4:      b.eq    0x1008dc4d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x418>
1008dc4a8:      lsr x8, x21, #16
1008dc4ac:      strb    w8, [sp, #0x22]
1008dc4b0:      cmp x1, #0x3
1008dc4b4:      b.eq    0x1008dc4d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x418>
1008dc4b8:      lsr x8, x21, #24
1008dc4bc:      strb    w8, [sp, #0x23]
1008dc4c0:      cmp x1, #0x4
1008dc4c4:      b.eq    0x1008dc4d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x418>
1008dc4c8:      lsr x8, x21, #32
1008dc4cc:      strb    w8, [sp, #0x24]
1008dc4d0:      cmp x1, #0x5
1008dc4d4:      b.ne    0x1008dc674 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5b4>
1008dc4d8:      add x8, sp, #0x28
1008dc4dc:      add x0, sp, #0x20
1008dc4e0:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008dc4e4:      ldr w8, [sp, #0x28]
1008dc4e8:      tbz w8, #0x0, 0x1008dc538 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x478>
1008dc4ec:      ldr x1, [x19, #0x10]
1008dc4f0:      ldr x8, [x19]
1008dc4f4:      sub x8, x8, x1
1008dc4f8:      cmp x8, #0x3
1008dc4fc:      b.ls    0x1008dc550 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x490>
1008dc500:      mov x8, x19
1008dc504:      ldr x9, [x19, #0x8]
1008dc508:      mov w10, #0x756e            ; =30062
1008dc50c:      movk    w10, #0x6c6c, lsl #16
1008dc510:      str w10, [x9, x1]
1008dc514:      ldr x9, [x19, #0x10]
1008dc518:      add x9, x9, #0x4
1008dc51c:      str x9, [x19, #0x10]
1008dc520:      mov x10, x20
1008dc524:      b   0x1008dc3f8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x338>
1008dc528:      fmov    d0, x21
1008dc52c:      mov x0, x19
1008dc530:      bl  0x100914d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008dc534:      b   0x1008dc544 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x484>
1008dc538:      ldp x1, x2, [sp, #0x30]
1008dc53c:      mov x0, x19
1008dc540:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008dc544:      mov x8, x19
1008dc548:      mov x10, x20
1008dc54c:      b   0x1008dc3f8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x338>
1008dc550:      mov x0, x19
1008dc554:      mov w2, #0x4                ; =4
1008dc558:      mov w3, #0x1                ; =1
1008dc55c:      mov w4, #0x1                ; =1
1008dc560:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dc564:      ldr x1, [x19, #0x10]
1008dc568:      b   0x1008dc500 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x440>
1008dc56c:      mov x0, x19
1008dc570:      mov w2, #0x4                ; =4
1008dc574:      mov w3, #0x1                ; =1
1008dc578:      mov w4, #0x1                ; =1
1008dc57c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dc580:      ldr x1, [x19, #0x10]
1008dc584:      b   0x1008dc3c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x300>
1008dc588:      mov x0, x19
1008dc58c:      mov w2, #0x5                ; =5
1008dc590:      mov w3, #0x1                ; =1
1008dc594:      mov w4, #0x1                ; =1
1008dc598:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dc59c:      ldr x1, [x19, #0x10]
1008dc5a0:      b   0x1008dc418 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x358>
1008dc5a4:      ldr x1, [x8, #0x10]
1008dc5a8:      ldr x9, [x8]
1008dc5ac:      sub x9, x9, x1
1008dc5b0:      cmp x9, #0x1
1008dc5b4:      b.ls    0x1008dc60c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x54c>
1008dc5b8:      ldr x9, [x8, #0x8]
1008dc5bc:      mov w10, #0x7d7b            ; =32123
1008dc5c0:      strh    w10, [x9, x1]
1008dc5c4:      ldr x9, [x8, #0x10]
1008dc5c8:      add x9, x9, #0x2
1008dc5cc:      b   0x1008dc5f0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x530>
1008dc5d0:      ldr x20, [x8, #0x10]
1008dc5d4:      ldr x9, [x8]
1008dc5d8:      cmp x9, x20
1008dc5dc:      b.eq    0x1008dc62c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x56c>
1008dc5e0:      ldr x9, [x8, #0x8]
1008dc5e4:      mov w10, #0x7d              ; =125
1008dc5e8:      strb    w10, [x9, x20]
1008dc5ec:      add x9, x20, #0x1
1008dc5f0:      str x9, [x8, #0x10]
1008dc5f4:      mov w0, #0x1                ; =1
1008dc5f8:      b   0x1008dc210 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x150>
1008dc5fc:      adrp    x4, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
1008dc600:      add x4, x4, #0xcb0
1008dc604:      mov x0, x9
1008dc608:      bl  0x100c984f8 <__RNvNtCsjgY6bXVaRmE_4core3str16slice_error_fail>
1008dc60c:      mov x0, x8
1008dc610:      mov w2, #0x2                ; =2
1008dc614:      mov w3, #0x1                ; =1
1008dc618:      mov w4, #0x1                ; =1
1008dc61c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dc620:      mov x8, x19
1008dc624:      ldr x1, [x19, #0x10]
1008dc628:      b   0x1008dc5b8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4f8>
1008dc62c:      mov x0, x8
1008dc630:      mov x1, x20
1008dc634:      mov w2, #0x1                ; =1
1008dc638:      mov w3, #0x1                ; =1
1008dc63c:      mov w4, #0x1                ; =1
1008dc640:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dc644:      mov x8, x19
1008dc648:      b   0x1008dc5e0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x520>
1008dc64c:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
1008dc650:      add x2, x2, #0xcc8
1008dc654:      mov x0, x1
1008dc658:      mov w1, #0x40               ; =64
1008dc65c:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008dc660:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
1008dc664:      add x2, x2, #0xc98
1008dc668:      mov w0, #0x21               ; =33
1008dc66c:      mov w1, #0x21               ; =33
1008dc670:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008dc674:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
1008dc678:      add x2, x2, #0xd10
1008dc67c:      mov w0, #0x5                ; =5
1008dc680:      mov w1, #0x5                ; =5
1008dc684:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
