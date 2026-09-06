/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010074f2e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object>:
10074f2e8:      stp x28, x27, [sp, #-0x60]!
10074f2ec:      stp x26, x25, [sp, #0x10]
10074f2f0:      stp x24, x23, [sp, #0x20]
10074f2f4:      stp x22, x21, [sp, #0x30]
10074f2f8:      stp x20, x19, [sp, #0x40]
10074f2fc:      stp x29, x30, [sp, #0x50]
10074f300:      add x29, sp, #0x50
10074f304:      sub sp, sp, #0x1c0
10074f308:      mov x19, x1
10074f30c:      mov x20, x0
10074f310:      movi.2d v0, #0000000000000000
10074f314:      str d0, [sp, #0x10]
10074f318:      str wzr, [sp, #0x18]
10074f31c:      str d0, [sp, #0x38]
10074f320:      str wzr, [sp, #0x40]
10074f324:      str d0, [sp, #0x60]
10074f328:      str wzr, [sp, #0x68]
10074f32c:      str d0, [sp, #0x88]
10074f330:      str wzr, [sp, #0x90]
10074f334:      str d0, [sp, #0xb0]
10074f338:      str wzr, [sp, #0xb8]
10074f33c:      str d0, [sp, #0xd8]
10074f340:      str wzr, [sp, #0xe0]
10074f344:      str d0, [sp, #0x100]
10074f348:      str wzr, [sp, #0x108]
10074f34c:      str d0, [sp, #0x128]
10074f350:      str wzr, [sp, #0x130]
10074f354:      ldr w21, [x0, #0x4]
10074f358:      adrp    x26, 0x10112a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
10074f35c:      add x26, x26, #0x94
10074f360:      ldr w22, [x26]
10074f364:      adrp    x25, 0x101129000 <__MergedGlobals+0x38>
10074f368:      add x25, x25, #0x768
10074f36c:      cmp w22, #0x300
10074f370:      b.hs    0x10074f978 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x690>
10074f374:      ldr x8, [x25]
10074f378:      cmn x8, #0x1
10074f37c:      b.eq    0x10074f968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x680>
10074f380:      mrs x9, TPIDRRO_EL0
10074f384:      and x9, x9, #0xfffffffffffffff8
10074f388:      ldr x0, [x9, x8, lsl #3]
10074f38c:      cbz x0, 0x10074f968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x680>
10074f390:      add x8, x0, x22, lsl #3
10074f394:      ldr x0, [x8, #0x1e8]
10074f398:      cbz x0, 0x10074f978 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x690>
10074f39c:      ldr x0, [x0]
10074f3a0:      cbz x0, 0x10074f98c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6a4>
10074f3a4:      ldr x8, [x0, #0x5190]
10074f3a8:      ubfx    x9, x21, #15, #15
10074f3ac:      ubfx    x10, x21, #5, #10
10074f3b0:      and x11, x21, #0x1f
10074f3b4:      ldr x8, [x8, x9, lsl #3]
10074f3b8:      ldr x8, [x8, x10, lsl #3]
10074f3bc:      lsl x9, x11, #5
10074f3c0:      ldr x22, [x8, x9]
10074f3c4:      ldr x1, [x22, #0x8]
10074f3c8:      sub x0, x29, #0x90
10074f3cc:      bl  0x1007500d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10074f3d0:      ldur    w8, [x29, #-0x90]
10074f3d4:      cmn w8, #0x1
10074f3d8:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f3dc:      ldur    x8, [x29, #-0x70]
10074f3e0:      ldp q1, q0, [x29, #-0x90]
10074f3e4:      stp q1, q0, [sp, #0x10]
10074f3e8:      str x8, [sp, #0x30]
10074f3ec:      ldr x1, [x20, #0x10]
10074f3f0:      sub x0, x29, #0x90
10074f3f4:      bl  0x10074fe0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10074f3f8:      ldur    w8, [x29, #-0x90]
10074f3fc:      cmn w8, #0x1
10074f400:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f404:      ldur    x8, [x29, #-0x70]
10074f408:      ldp q1, q0, [x29, #-0x90]
10074f40c:      stp q1, q0, [sp, #0xb0]
10074f410:      str x8, [sp, #0xd0]
10074f414:      ldp w10, w8, [sp, #0x10]
10074f418:      ldr w9, [sp, #0x18]
10074f41c:      cbz w10, 0x10074f448 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x160>
10074f420:      cmp w10, #0x1
10074f424:      b.ne    0x10074f470 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x188>
10074f428:      ldr w8, [sp, #0x1c]
10074f42c:      ldp w12, w10, [sp, #0xb0]
10074f430:      ldr w11, [sp, #0xb8]
10074f434:      cbz w12, 0x10074f460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x178>
10074f438:      cmp w12, #0x1
10074f43c:      b.ne    0x10074f484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x19c>
10074f440:      ldr w10, [sp, #0xbc]
10074f444:      b   0x10074f488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a0>
10074f448:      add w10, w8, #0x2
10074f44c:      add w8, w9, #0x2
10074f450:      mov x9, x10
10074f454:      ldp w12, w10, [sp, #0xb0]
10074f458:      ldr w11, [sp, #0xb8]
10074f45c:      cbnz    w12, 0x10074f438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x150>
10074f460:      add w12, w10, #0x2
10074f464:      add w10, w11, #0x2
10074f468:      mov x11, x12
10074f46c:      b   0x10074f488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a0>
10074f470:      mov x9, x8
10074f474:      ldp w12, w10, [sp, #0xb0]
10074f478:      ldr w11, [sp, #0xb8]
10074f47c:      cbnz    w12, 0x10074f438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x150>
10074f480:      b   0x10074f460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x178>
10074f484:      mov x11, x10
10074f488:      cmn w9, #0x3
10074f48c:      b.hi    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f490:      add w9, w9, #0x2
10074f494:      adds    w9, w11, w9
10074f498:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f49c:      mov x0, #0x0                ; =0
10074f4a0:      adds    w21, w9, #0x1
10074f4a4:      b.hs    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074f4a8:      cmn w8, #0x3
10074f4ac:      b.hi    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074f4b0:      mov x0, #0x0                ; =0
10074f4b4:      add w8, w8, #0x2
10074f4b8:      adds    w24, w10, w8
10074f4bc:      b.hs    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074f4c0:      cmn w24, #0x1
10074f4c4:      b.eq    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074f4c8:      add w27, w24, #0x1
10074f4cc:      cmp x19, #0x1
10074f4d0:      b.ne    0x10074f510 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x228>
10074f4d4:      ldr x8, [x25]
10074f4d8:      cmn x8, #0x1
10074f4dc:      b.eq    0x10074f588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2a0>
10074f4e0:      mrs x9, TPIDRRO_EL0
10074f4e4:      and x9, x9, #0xfffffffffffffff8
10074f4e8:      ldr x8, [x9, x8, lsl #3]
10074f4ec:      cbz x8, 0x10074f588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2a0>
10074f4f0:      ldr x8, [x8, #0x19e8]
10074f4f4:      cbz x8, 0x10074f994 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6ac>
10074f4f8:      ldr x9, [x8]
10074f4fc:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10074f500:      cmp x9, x10
10074f504:      b.hs    0x10074fd24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa3c>
10074f508:      ldr x22, [x8, #0x18]
10074f50c:      b   0x10074f5b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
10074f510:      ldr x1, [x22, #0x10]
10074f514:      sub x0, x29, #0x90
10074f518:      bl  0x1007500d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10074f51c:      ldur    w8, [x29, #-0x90]
10074f520:      cmn w8, #0x1
10074f524:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f528:      ldur    x8, [x29, #-0x70]
10074f52c:      ldp q1, q0, [x29, #-0x90]
10074f530:      stur    q1, [sp, #0x38]
10074f534:      stur    q0, [sp, #0x48]
10074f538:      str x8, [sp, #0x58]
10074f53c:      ldr x1, [x20, #0x18]
10074f540:      sub x0, x29, #0x90
10074f544:      bl  0x10074fe0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10074f548:      ldur    w8, [x29, #-0x90]
10074f54c:      cmn w8, #0x1
10074f550:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f554:      add x23, sp, #0xb0
10074f558:      ldur    x8, [x29, #-0x70]
10074f55c:      ldp q1, q0, [x29, #-0x90]
10074f560:      stur    q1, [x23, #0x28]
10074f564:      stur    q0, [x23, #0x38]
10074f568:      str x8, [sp, #0xf8]
10074f56c:      ldp w10, w8, [sp, #0x38]
10074f570:      ldr w9, [sp, #0x40]
10074f574:      cbz w10, 0x10074f9a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6c0>
10074f578:      cmp w10, #0x1
10074f57c:      b.ne    0x10074f9b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d0>
10074f580:      ldr w8, [sp, #0x44]
10074f584:      b   0x10074f9bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d4>
10074f588:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
10074f58c:      add x0, x0, #0x5f8
10074f590:      ldr x8, [x0]
10074f594:      blr x8
10074f598:      ldrb    w8, [x0, #0x20]
10074f59c:      cbnz    w8, 0x10074fcd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9f0>
10074f5a0:      ldr x8, [x0]
10074f5a4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10074f5a8:      cmp x8, x9
10074f5ac:      b.hs    0x10074fd08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa20>
10074f5b0:      ldr x22, [x0, #0x18]
10074f5b4:      stur    x22, [x29, #-0x68]
10074f5b8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10074f5bc:      stp x20, x8, [x29, #-0x88]
10074f5c0:      mov w8, #0x1                ; =1
10074f5c4:      stur    x8, [x29, #-0x90]
10074f5c8:      sub x0, x29, #0x90
10074f5cc:      bl  0x10071175c <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10074f5d0:      mov x24, x0
10074f5d4:      stur    x0, [x29, #-0xc0]
10074f5d8:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
10074f5dc:      add x0, x0, #0x258
10074f5e0:      ldr x8, [x0]
10074f5e4:      blr x8
10074f5e8:      strb    wzr, [x0]
10074f5ec:      mov x0, x20
10074f5f0:      bl  0x1003e4824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
10074f5f4:      tbz w0, #0x0, 0x10074f684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x39c>
10074f5f8:      mov x0, x21
10074f5fc:      bl  0x10071ea7c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
10074f600:      mov x20, x0
10074f604:      mov x23, x1
10074f608:      ldr x8, [x25]
10074f60c:      cmn x8, #0x1
10074f610:      b.eq    0x10074f6c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3e0>
10074f614:      mrs x9, TPIDRRO_EL0
10074f618:      and x9, x9, #0xfffffffffffffff8
10074f61c:      ldr x8, [x9, x8, lsl #3]
10074f620:      cbz x8, 0x10074f6c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3e0>
10074f624:      ldr x8, [x8, #0x19e8]
10074f628:      cbz x8, 0x10074faa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7b8>
10074f62c:      ldr x9, [x8]
10074f630:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10074f634:      cmp x9, x10
10074f638:      b.hs    0x10074fdc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xad8>
10074f63c:      add x10, x9, #0x1
10074f640:      str x10, [x8]
10074f644:      ldr x10, [x8, #0x18]
10074f648:      cmp x24, x10
10074f64c:      b.hs    0x10074fcd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ec>
10074f650:      ldr x10, [x8, #0x10]
10074f654:      mov w11, #0x18              ; =24
10074f658:      madd    x10, x24, x11, x10
10074f65c:      ldr x11, [x10]
10074f660:      cmp x11, #0x1
10074f664:      b.ne    0x10074fdcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xae4>
10074f668:      ldr x24, [x10, #0x8]
10074f66c:      str x9, [x8]
10074f670:      ldr w28, [x24, #0x4]
10074f674:      ldr w26, [x26]
10074f678:      cmp w26, #0x300
10074f67c:      b.lo    0x10074f734 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x44c>
10074f680:      b   0x10074fb2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10074f684:      ldr x8, [x25]
10074f688:      cmn x8, #0x1
10074f68c:      b.eq    0x10074f900 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x618>
10074f690:      mrs x9, TPIDRRO_EL0
10074f694:      and x9, x9, #0xfffffffffffffff8
10074f698:      ldr x8, [x9, x8, lsl #3]
10074f69c:      cbz x8, 0x10074f900 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x618>
10074f6a0:      ldr x8, [x8, #0x19e8]
10074f6a4:      cbz x8, 0x10074fac8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7e0>
10074f6a8:      ldr x9, [x8]
10074f6ac:      cbnz    x9, 0x10074fd30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa48>
10074f6b0:      ldr x9, [x8, #0x18]
10074f6b4:      cmp x22, x9
10074f6b8:      b.hi    0x10074f6c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3d8>
10074f6bc:      str x22, [x8, #0x18]
10074f6c0:      str xzr, [x8]
10074f6c4:      b   0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f6c8:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
10074f6cc:      add x0, x0, #0x5f8
10074f6d0:      ldr x8, [x0]
10074f6d4:      blr x8
10074f6d8:      ldrb    w8, [x0, #0x20]
10074f6dc:      cbnz    w8, 0x10074fd3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa54>
10074f6e0:      ldr x8, [x0]
10074f6e4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10074f6e8:      cmp x8, x9
10074f6ec:      b.hs    0x10074fd74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa8c>
10074f6f0:      add x9, x8, #0x1
10074f6f4:      str x9, [x0]
10074f6f8:      ldr x9, [x0, #0x18]
10074f6fc:      cmp x24, x9
10074f700:      b.hs    0x10074fcd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ec>
10074f704:      ldr x9, [x0, #0x10]
10074f708:      mov w10, #0x18              ; =24
10074f70c:      madd    x9, x24, x10, x9
10074f710:      ldr x10, [x9]
10074f714:      cmp x10, #0x1
10074f718:      b.ne    0x10074fd14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa2c>
10074f71c:      ldr x24, [x9, #0x8]
10074f720:      str x8, [x0]
10074f724:      ldr w28, [x24, #0x4]
10074f728:      ldr w26, [x26]
10074f72c:      cmp w26, #0x300
10074f730:      b.hs    0x10074fb2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10074f734:      ldr x8, [x25]
10074f738:      cmn x8, #0x1
10074f73c:      b.eq    0x10074fb1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x834>
10074f740:      mrs x9, TPIDRRO_EL0
10074f744:      and x9, x9, #0xfffffffffffffff8
10074f748:      ldr x0, [x9, x8, lsl #3]
10074f74c:      cbz x0, 0x10074fb1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x834>
10074f750:      add x8, x0, x26, lsl #3
10074f754:      ldr x0, [x8, #0x1e8]
10074f758:      cbz x0, 0x10074fb2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10074f75c:      str x22, [sp, #0x8]
10074f760:      ldr x0, [x0]
10074f764:      cbz x0, 0x10074fb44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x85c>
10074f768:      ldr x8, [x0, #0x5190]
10074f76c:      ubfx    x9, x28, #15, #15
10074f770:      ubfx    x10, x28, #5, #10
10074f774:      and x11, x28, #0x1f
10074f778:      ldr x8, [x8, x9, lsl #3]
10074f77c:      ldr x8, [x8, x10, lsl #3]
10074f780:      lsl x9, x11, #5
10074f784:      ldr x26, [x8, x9]
10074f788:      stp w27, w21, [x20]
10074f78c:      stp wzr, wzr, [x20, #0xc]
10074f790:      str w21, [x20, #0x8]
10074f794:      mov w8, #0x7b               ; =123
10074f798:      mov x2, x23
10074f79c:      strb    w8, [x2], #0x1
10074f7a0:      ldr x1, [x26, #0x8]
10074f7a4:      add x21, sp, #0x10
10074f7a8:      add x0, sp, #0x10
10074f7ac:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f7b0:      add x8, x23, x0
10074f7b4:      mov w27, #0x3a              ; =58
10074f7b8:      strb    w27, [x8, #0x1]
10074f7bc:      add x22, x0, #0x2
10074f7c0:      ldr x1, [x24, #0x10]
10074f7c4:      add x28, sp, #0xb0
10074f7c8:      add x0, sp, #0xb0
10074f7cc:      add x2, x23, x22
10074f7d0:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f7d4:      add x8, x0, x22
10074f7d8:      cmp x19, #0x1
10074f7dc:      b.eq    0x10074f8b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
10074f7e0:      mov w9, #0x2c               ; =44
10074f7e4:      strb    w9, [x23, x8]
10074f7e8:      add x22, x8, #0x1
10074f7ec:      ldr x1, [x26, #0x10]
10074f7f0:      add x0, x21, #0x28
10074f7f4:      add x2, x23, x22
10074f7f8:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f7fc:      add x8, x0, x22
10074f800:      strb    w27, [x23, x8]
10074f804:      add x21, x8, #0x1
10074f808:      ldr x1, [x24, #0x18]
10074f80c:      add x0, x28, #0x28
10074f810:      add x2, x23, x21
10074f814:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f818:      add x8, x0, x21
10074f81c:      cmp x19, #0x2
10074f820:      b.eq    0x10074f8b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
10074f824:      mov w9, #0x2c               ; =44
10074f828:      strb    w9, [x23, x8]
10074f82c:      add x22, x8, #0x1
10074f830:      add x21, sp, #0x10
10074f834:      ldr x1, [x26, #0x18]
10074f838:      add x0, x21, #0x50
10074f83c:      add x2, x23, x22
10074f840:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f844:      add x8, x0, x22
10074f848:      mov w28, #0x3a              ; =58
10074f84c:      strb    w28, [x23, x8]
10074f850:      add x22, x8, #0x1
10074f854:      add x27, sp, #0xb0
10074f858:      ldr x1, [x24, #0x20]
10074f85c:      add x0, x27, #0x50
10074f860:      add x2, x23, x22
10074f864:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f868:      add x8, x0, x22
10074f86c:      cmp x19, #0x3
10074f870:      b.eq    0x10074f8b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
10074f874:      mov w9, #0x2c               ; =44
10074f878:      strb    w9, [x23, x8]
10074f87c:      add x19, x8, #0x1
10074f880:      ldr x1, [x26, #0x20]
10074f884:      add x0, x21, #0x78
10074f888:      add x2, x23, x19
10074f88c:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f890:      add x8, x0, x19
10074f894:      strb    w28, [x23, x8]
10074f898:      add x19, x8, #0x1
10074f89c:      ldr x1, [x24, #0x28]
10074f8a0:      add x0, x27, #0x78
10074f8a4:      add x2, x23, x19
10074f8a8:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10074f8ac:      add x8, x0, x19
10074f8b0:      ldr x10, [sp, #0x8]
10074f8b4:      mov w9, #0x7d               ; =125
10074f8b8:      strb    w9, [x23, x8]
10074f8bc:      ldr x8, [x25]
10074f8c0:      cmn x8, #0x1
10074f8c4:      b.eq    0x10074f934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x64c>
10074f8c8:      mrs x9, TPIDRRO_EL0
10074f8cc:      and x9, x9, #0xfffffffffffffff8
10074f8d0:      ldr x8, [x9, x8, lsl #3]
10074f8d4:      cbz x8, 0x10074f934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x64c>
10074f8d8:      ldr x8, [x8, #0x19e8]
10074f8dc:      cbz x8, 0x10074fafc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x814>
10074f8e0:      ldr x9, [x8]
10074f8e4:      cbnz    x9, 0x10074fd30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa48>
10074f8e8:      ldr x9, [x8, #0x18]
10074f8ec:      cmp x10, x9
10074f8f0:      b.hi    0x10074f8f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x610>
10074f8f4:      str x10, [x8, #0x18]
10074f8f8:      str xzr, [x8]
10074f8fc:      b   0x10074fb0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
10074f900:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
10074f904:      add x0, x0, #0x5f8
10074f908:      ldr x8, [x0]
10074f90c:      blr x8
10074f910:      ldrb    w8, [x0, #0x20]
10074f914:      cbnz    w8, 0x10074fd80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa98>
10074f918:      ldr x8, [x0]
10074f91c:      cbnz    x8, 0x10074fe00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
10074f920:      ldr x8, [x0, #0x18]
10074f924:      cmp x22, x8
10074f928:      b.hi    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f92c:      str x22, [x0, #0x18]
10074f930:      b   0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f934:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
10074f938:      add x0, x0, #0x5f8
10074f93c:      ldr x8, [x0]
10074f940:      blr x8
10074f944:      ldrb    w8, [x0, #0x20]
10074f948:      cbnz    w8, 0x10074fdac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xac4>
10074f94c:      ldr x8, [x0]
10074f950:      cbnz    x8, 0x10074fe00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
10074f954:      ldr x8, [x0, #0x18]
10074f958:      cmp x10, x8
10074f95c:      b.hi    0x10074fb0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
10074f960:      str x10, [x0, #0x18]
10074f964:      b   0x10074fb0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
10074f968:      bl  0x100cc8104 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10074f96c:      add x8, x0, x22, lsl #3
10074f970:      ldr x0, [x8, #0x1e8]
10074f974:      cbnz    x0, 0x10074f39c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb4>
10074f978:      adrp    x0, 0x1010d9000 <_anon.72cde5cdc14742b721629e115e16bf6f.1612+0x158>
10074f97c:      add x0, x0, #0x288
10074f980:      bl  0x100cc7950 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10074f984:      ldr x0, [x0]
10074f988:      cbnz    x0, 0x10074f3a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xbc>
10074f98c:      bl  0x100cc7e88 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
10074f990:      b   0x10074f3a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xbc>
10074f994:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074f998:      add x0, x0, #0xb90
10074f99c:      bl  0x1001354ac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
10074f9a0:      mov x22, x0
10074f9a4:      b   0x10074f5b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
10074f9a8:      add w10, w8, #0x2
10074f9ac:      add w8, w9, #0x2
10074f9b0:      mov x9, x10
10074f9b4:      b   0x10074f9bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d4>
10074f9b8:      mov x9, x8
10074f9bc:      ldp w12, w10, [sp, #0xd8]
10074f9c0:      ldr w11, [sp, #0xe0]
10074f9c4:      cbz w12, 0x10074f9d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6f0>
10074f9c8:      cmp w12, #0x1
10074f9cc:      b.ne    0x10074f9e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x700>
10074f9d0:      ldr w10, [sp, #0xe4]
10074f9d4:      b   0x10074f9ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x704>
10074f9d8:      add w12, w10, #0x2
10074f9dc:      add w10, w11, #0x2
10074f9e0:      mov x11, x12
10074f9e4:      b   0x10074f9ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x704>
10074f9e8:      mov x11, x10
10074f9ec:      adds    w9, w9, w21
10074f9f0:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f9f4:      adds    w9, w11, w9
10074f9f8:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074f9fc:      cmn w9, #0x3
10074fa00:      b.hi    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fa04:      add w8, w8, w27
10074fa08:      cmp w8, w24
10074fa0c:      b.ls    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fa10:      mov x0, #0x0                ; =0
10074fa14:      adds    w8, w10, w8
10074fa18:      b.hs    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fa1c:      cmn w8, #0x3
10074fa20:      b.hi    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fa24:      add w21, w9, #0x2
10074fa28:      add w27, w8, #0x2
10074fa2c:      cmp x19, #0x2
10074fa30:      b.eq    0x10074f4d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
10074fa34:      ldr x1, [x22, #0x18]
10074fa38:      sub x0, x29, #0x90
10074fa3c:      bl  0x1007500d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10074fa40:      ldur    w8, [x29, #-0x90]
10074fa44:      cmn w8, #0x1
10074fa48:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fa4c:      ldur    x8, [x29, #-0x70]
10074fa50:      ldp q1, q0, [x29, #-0x90]
10074fa54:      stp q1, q0, [sp, #0x60]
10074fa58:      str x8, [sp, #0x80]
10074fa5c:      ldr x1, [x20, #0x20]
10074fa60:      sub x0, x29, #0x90
10074fa64:      bl  0x10074fe0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10074fa68:      ldur    w8, [x29, #-0x90]
10074fa6c:      cmn w8, #0x1
10074fa70:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fa74:      ldur    x8, [x29, #-0x70]
10074fa78:      ldp q1, q0, [x29, #-0x90]
10074fa7c:      stp q1, q0, [sp, #0x100]
10074fa80:      str x8, [sp, #0x120]
10074fa84:      ldp w10, w8, [sp, #0x60]
10074fa88:      ldr w9, [sp, #0x68]
10074fa8c:      cbz w10, 0x10074fb4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x864>
10074fa90:      cmp w10, #0x1
10074fa94:      b.ne    0x10074fb5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x874>
10074fa98:      ldr w8, [sp, #0x6c]
10074fa9c:      b   0x10074fb60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x878>
10074faa0:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074faa4:      add x0, x0, #0xb90
10074faa8:      sub x1, x29, #0xc0
10074faac:      bl  0x1001352d0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10074fab0:      mov x24, x0
10074fab4:      ldr w28, [x0, #0x4]
10074fab8:      ldr w26, [x26]
10074fabc:      cmp w26, #0x300
10074fac0:      b.lo    0x10074f734 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x44c>
10074fac4:      b   0x10074fb2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10074fac8:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074facc:      add x0, x0, #0xb90
10074fad0:      sub x1, x29, #0x68
10074fad4:      bl  0x100135888 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
10074fad8:      mov x0, #0x0                ; =0
10074fadc:      add sp, sp, #0x1c0
10074fae0:      ldp x29, x30, [sp, #0x50]
10074fae4:      ldp x20, x19, [sp, #0x40]
10074fae8:      ldp x22, x21, [sp, #0x30]
10074faec:      ldp x24, x23, [sp, #0x20]
10074faf0:      ldp x26, x25, [sp, #0x10]
10074faf4:      ldp x28, x27, [sp], #0x60
10074faf8:      ret
10074fafc:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074fb00:      add x0, x0, #0xb90
10074fb04:      sub x1, x29, #0x68
10074fb08:      bl  0x100135888 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
10074fb0c:      mov x1, #0x7fff000000000000 ; =9223090561878065152
10074fb10:      bfxil   x1, x20, #0, #48
10074fb14:      mov w0, #0x1                ; =1
10074fb18:      b   0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fb1c:      bl  0x100cc8104 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10074fb20:      add x8, x0, x26, lsl #3
10074fb24:      ldr x0, [x8, #0x1e8]
10074fb28:      cbnz    x0, 0x10074f75c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x474>
10074fb2c:      adrp    x0, 0x1010d9000 <_anon.72cde5cdc14742b721629e115e16bf6f.1612+0x158>
10074fb30:      add x0, x0, #0x288
10074fb34:      bl  0x100cc7950 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10074fb38:      str x22, [sp, #0x8]
10074fb3c:      ldr x0, [x0]
10074fb40:      cbnz    x0, 0x10074f768 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x480>
10074fb44:      bl  0x100cc7e88 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
10074fb48:      b   0x10074f768 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x480>
10074fb4c:      add w10, w8, #0x2
10074fb50:      add w8, w9, #0x2
10074fb54:      mov x9, x10
10074fb58:      b   0x10074fb60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x878>
10074fb5c:      mov x9, x8
10074fb60:      ldr w12, [sp, #0x100]
10074fb64:      ldr w10, [sp, #0x104]
10074fb68:      ldr w11, [sp, #0x108]
10074fb6c:      cbz w12, 0x10074fb80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x898>
10074fb70:      cmp w12, #0x1
10074fb74:      b.ne    0x10074fb90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8a8>
10074fb78:      ldr w10, [sp, #0x10c]
10074fb7c:      b   0x10074fb94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8ac>
10074fb80:      add w12, w10, #0x2
10074fb84:      add w10, w11, #0x2
10074fb88:      mov x11, x12
10074fb8c:      b   0x10074fb94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8ac>
10074fb90:      mov x11, x10
10074fb94:      adds    w9, w9, w21
10074fb98:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fb9c:      adds    w9, w11, w9
10074fba0:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fba4:      cmn w9, #0x3
10074fba8:      b.hi    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fbac:      adds    w8, w8, w27
10074fbb0:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fbb4:      mov x0, #0x0                ; =0
10074fbb8:      adds    w8, w10, w8
10074fbbc:      b.hs    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fbc0:      cmn w8, #0x3
10074fbc4:      b.hi    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fbc8:      add w21, w9, #0x2
10074fbcc:      add w27, w8, #0x2
10074fbd0:      cmp x19, #0x3
10074fbd4:      b.eq    0x10074f4d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
10074fbd8:      ldr x1, [x22, #0x20]
10074fbdc:      sub x0, x29, #0x90
10074fbe0:      bl  0x1007500d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10074fbe4:      ldur    w8, [x29, #-0x90]
10074fbe8:      cmn w8, #0x1
10074fbec:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fbf0:      ldur    x8, [x29, #-0x70]
10074fbf4:      ldp q1, q0, [x29, #-0x90]
10074fbf8:      stur    q1, [sp, #0x88]
10074fbfc:      stur    q0, [sp, #0x98]
10074fc00:      str x8, [sp, #0xa8]
10074fc04:      ldr x1, [x20, #0x28]
10074fc08:      sub x0, x29, #0x90
10074fc0c:      bl  0x10074fe0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10074fc10:      ldur    w8, [x29, #-0x90]
10074fc14:      cmn w8, #0x1
10074fc18:      b.eq    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fc1c:      ldur    x8, [x29, #-0x70]
10074fc20:      ldp q1, q0, [x29, #-0x90]
10074fc24:      stur    q1, [x23, #0x78]
10074fc28:      stur    q0, [x23, #0x88]
10074fc2c:      str x8, [sp, #0x148]
10074fc30:      ldp w10, w8, [sp, #0x88]
10074fc34:      ldr w9, [sp, #0x90]
10074fc38:      cbz w10, 0x10074fc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x964>
10074fc3c:      cmp w10, #0x1
10074fc40:      b.ne    0x10074fc5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x974>
10074fc44:      ldr w8, [sp, #0x94]
10074fc48:      b   0x10074fc60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x978>
10074fc4c:      add w10, w8, #0x2
10074fc50:      add w8, w9, #0x2
10074fc54:      mov x9, x10
10074fc58:      b   0x10074fc60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x978>
10074fc5c:      mov x9, x8
10074fc60:      ldr w12, [sp, #0x128]
10074fc64:      ldr w10, [sp, #0x12c]
10074fc68:      ldr w11, [sp, #0x130]
10074fc6c:      cbz w12, 0x10074fc80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x998>
10074fc70:      cmp w12, #0x1
10074fc74:      b.ne    0x10074fc90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9a8>
10074fc78:      ldr w10, [sp, #0x134]
10074fc7c:      b   0x10074fc94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ac>
10074fc80:      add w12, w10, #0x2
10074fc84:      add w10, w11, #0x2
10074fc88:      mov x11, x12
10074fc8c:      b   0x10074fc94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ac>
10074fc90:      mov x11, x10
10074fc94:      adds    w9, w9, w21
10074fc98:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fc9c:      adds    w9, w11, w9
10074fca0:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fca4:      cmn w9, #0x3
10074fca8:      b.hi    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fcac:      adds    w8, w8, w27
10074fcb0:      b.hs    0x10074fad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10074fcb4:      mov x0, #0x0                ; =0
10074fcb8:      adds    w8, w10, w8
10074fcbc:      b.hs    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fcc0:      cmn w8, #0x3
10074fcc4:      b.hi    0x10074fadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10074fcc8:      add w21, w9, #0x2
10074fccc:      add w27, w8, #0x2
10074fcd0:      b   0x10074f4d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
10074fcd4:      bl  0x100cb7368 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
10074fcd8:      cmp w8, #0x1
10074fcdc:      b.ne    0x10074fdb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
10074fce0:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
10074fce4:      add x1, x1, #0x850
10074fce8:      mov x22, x0
10074fcec:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10074fcf0:      mov x0, x22
10074fcf4:      strb    wzr, [x22, #0x20]
10074fcf8:      ldr x8, [x22]
10074fcfc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10074fd00:      cmp x8, x9
10074fd04:      b.lo    0x10074f5b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2c8>
10074fd08:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
10074fd0c:      add x0, x0, #0x468
10074fd10:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10074fd14:      adrp    x0, 0x100dba000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x26e0>
10074fd18:      add x0, x0, #0x7b0
10074fd1c:      mov w1, #0xb                ; =11
10074fd20:      bl  0x100cb7330 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10074fd24:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074fd28:      add x0, x0, #0xd08
10074fd2c:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10074fd30:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074fd34:      add x0, x0, #0xe00
10074fd38:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10074fd3c:      cmp w8, #0x2
10074fd40:      b.eq    0x10074fdb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
10074fd44:      mov x28, x25
10074fd48:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
10074fd4c:      add x1, x1, #0x850
10074fd50:      mov x25, x0
10074fd54:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10074fd58:      mov x0, x25
10074fd5c:      strb    wzr, [x25, #0x20]
10074fd60:      mov x25, x28
10074fd64:      ldr x8, [x0]
10074fd68:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10074fd6c:      cmp x8, x9
10074fd70:      b.lo    0x10074f6f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x408>
10074fd74:      adrp    x0, 0x101097000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10074fd78:      add x0, x0, #0xf70
10074fd7c:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10074fd80:      cmp w8, #0x2
10074fd84:      b.eq    0x10074fdb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
10074fd88:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
10074fd8c:      add x1, x1, #0x850
10074fd90:      mov x19, x0
10074fd94:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10074fd98:      mov x0, x19
10074fd9c:      strb    wzr, [x19, #0x20]
10074fda0:      ldr x8, [x19]
10074fda4:      cbz x8, 0x10074f920 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x638>
10074fda8:      b   0x10074fe00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
10074fdac:      cmp w8, #0x2
10074fdb0:      b.ne    0x10074fddc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf4>
10074fdb4:      adrp    x0, 0x101097000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10074fdb8:      add x0, x0, #0xed8
10074fdbc:      bl  0x100cd3f9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10074fdc0:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074fdc4:      add x0, x0, #0xc90
10074fdc8:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10074fdcc:      adrp    x0, 0x100dfd000 <_anon.4ff118d01ccdc9bd41517af7abf33093.1077+0xe2>
10074fdd0:      add x0, x0, #0xa9a
10074fdd4:      mov w1, #0xb                ; =11
10074fdd8:      bl  0x100cb7330 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10074fddc:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
10074fde0:      add x1, x1, #0x850
10074fde4:      mov x19, x0
10074fde8:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10074fdec:      mov x0, x19
10074fdf0:      strb    wzr, [x19, #0x20]
10074fdf4:      ldr x10, [sp, #0x8]
10074fdf8:      ldr x8, [x19]
10074fdfc:      cbz x8, 0x10074f954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x66c>
10074fe00:      adrp    x0, 0x10109d000 <_anon.68a532d94142320e15103d7866c451bd.1142>
10074fe04:      add x0, x0, #0x270
10074fe08:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
