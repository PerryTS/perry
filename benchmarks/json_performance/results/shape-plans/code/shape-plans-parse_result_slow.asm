/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002bf2a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
1002bf2a0:      sub sp, sp, #0x1a0
1002bf2a4:      stp x28, x27, [sp, #0x140]
1002bf2a8:      stp x26, x25, [sp, #0x150]
1002bf2ac:      stp x24, x23, [sp, #0x160]
1002bf2b0:      stp x22, x21, [sp, #0x170]
1002bf2b4:      stp x20, x19, [sp, #0x180]
1002bf2b8:      stp x29, x30, [sp, #0x190]
1002bf2bc:      add x29, sp, #0x190
1002bf2c0:      mov x20, x2
1002bf2c4:      mov x22, x1
1002bf2c8:      mov x19, x0
1002bf2cc:      add x24, sp, #0x90
1002bf2d0:      add x23, x1, #0x14
1002bf2d4:      cmp x2, #0x2
1002bf2d8:      b.ne    0x1002bf2f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50>
1002bf2dc:      ldrh    w8, [x23]
1002bf2e0:      mov w9, #0x7d7b             ; =32123
1002bf2e4:      cmp w8, w9
1002bf2e8:      b.eq    0x1002bf324 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
1002bf2ec:      b   0x1002bf364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1002bf2f0:      cmp x20, #0x3
1002bf2f4:      b.lo    0x1002bf364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1002bf2f8:      ldrb    w8, [x23]
1002bf2fc:      cmp w8, #0x20
1002bf300:      b.hi    0x1002bf330 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1002bf304:      mov x9, #0x2600             ; =9728
1002bf308:      movk    x9, #0x1, lsl #32
1002bf30c:      lsr x9, x9, x8
1002bf310:      tbz w9, #0x0, 0x1002bf330 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1002bf314:      add x0, x22, #0x14
1002bf318:      mov x1, x20
1002bf31c:      bl  0x1002b6354 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1002bf320:      tbz w0, #0x0, 0x1002bf35c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1002bf324:      bl  0x1002b6420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1002bf328:      stp xzr, x0, [x19]
1002bf32c:      b   0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf330:      cmp w8, #0x7b
1002bf334:      b.ne    0x1002bf35c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1002bf338:      ldrb    w8, [x22, #0x15]
1002bf33c:      cmp w8, #0x20
1002bf340:      b.hi    0x1002bf354 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
1002bf344:      mov x9, #0x2600             ; =9728
1002bf348:      movk    x9, #0x1, lsl #32
1002bf34c:      lsr x9, x9, x8
1002bf350:      tbnz    w9, #0x0, 0x1002bf314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1002bf354:      cmp w8, #0x7d
1002bf358:      b.eq    0x1002bf314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1002bf35c:      cmp x20, #0x41
1002bf360:      b.hs    0x1002bf3c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x128>
1002bf364:      add x0, sp, #0x90
1002bf368:      add x1, x22, #0x14
1002bf36c:      mov x2, x20
1002bf370:      bl  0x1002b68ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1002bf374:      ldr x8, [sp, #0x90]
1002bf378:      cbz x8, 0x1002bf474 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1002bf37c:      ldr x8, [sp, #0x118]
1002bf380:      str x8, [sp, #0x80]
1002bf384:      ldur    q0, [x24, #0x48]
1002bf388:      ldur    q1, [x24, #0x58]
1002bf38c:      stp q0, q1, [sp, #0x40]
1002bf390:      ldur    q0, [x24, #0x68]
1002bf394:      ldur    q1, [x24, #0x78]
1002bf398:      stp q0, q1, [sp, #0x60]
1002bf39c:      ldur    q0, [x24, #0x8]
1002bf3a0:      ldur    q1, [x24, #0x18]
1002bf3a4:      stp q0, q1, [sp]
1002bf3a8:      ldur    q0, [x24, #0x28]
1002bf3ac:      ldur    q1, [x24, #0x38]
1002bf3b0:      stp q0, q1, [sp, #0x20]
1002bf3b4:      mov x0, sp
1002bf3b8:      bl  0x1002b734c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1002bf3bc:      tbz w0, #0x0, 0x1002bf474 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1002bf3c0:      stp xzr, x1, [x19]
1002bf3c4:      b   0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf3c8:      cmp x20, #0x3e9
1002bf3cc:      b.lo    0x1002bf474 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1002bf3d0:      add x0, x22, #0x14
1002bf3d4:      mov x1, x20
1002bf3d8:      mov w2, #0x3e8              ; =1000
1002bf3dc:      bl  0x1002b7e60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1002bf3e0:      tbz w0, #0x0, 0x1002bf474 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1002bf3e4:      add x0, x22, #0x14
1002bf3e8:      mov x1, x20
1002bf3ec:      mov w2, #0xa120             ; =41248
1002bf3f0:      movk    w2, #0x7, lsl #16
1002bf3f4:      bl  0x1002b7e60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1002bf3f8:      tbz w0, #0x0, 0x1002bf7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50c>
1002bf3fc:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1002bf400:      add x8, x8, #0xf80
1002bf404:      adrp    x9, 0x100dcd000 <_anon.b822d7b979bdf0233543f470364426b7.1647+0x343>
1002bf408:      add x9, x9, #0x8
1002bf40c:      stp x9, x8, [sp]
1002bf410:      adrp    x0, 0x100ef1000 <_anon.b822d7b979bdf0233543f470364426b7.605+0xa6>
1002bf414:      add x0, x0, #0xef
1002bf418:      add x8, sp, #0x90
1002bf41c:      mov x1, sp
1002bf420:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
1002bf424:      ldr x20, [sp, #0x98]
1002bf428:      ldr w1, [sp, #0xa0]
1002bf42c:      mov x0, x20
1002bf430:      mov x2, x1
1002bf434:      bl  0x10097fb80 <_js_string_from_bytes_with_capacity>
1002bf438:      mov x3, x0
1002bf43c:      adrp    x1, 0x100dc6000 <_anon.2d62e9c08ab2025701038807088a1a53.881+0x128c>
1002bf440:      add x1, x1, #0x834
1002bf444:      mov w21, #0x1               ; =1
1002bf448:      mov w0, #0x2                ; =2
1002bf44c:      mov w2, #0xa                ; =10
1002bf450:      mov w4, #0x1                ; =1
1002bf454:      bl  0x1002a54fc <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1002bf458:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1002bf45c:      bfxil   x8, x0, #0, #48
1002bf460:      stp x21, x8, [x19]
1002bf464:      ldr x8, [sp, #0x90]
1002bf468:      cbz x8, 0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf46c:      mov x0, x20
1002bf470:      b   0x1002bf700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x460>
1002bf474:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
1002bf478:      add x0, x0, #0x590
1002bf47c:      ldr x8, [x0]
1002bf480:      blr x8
1002bf484:      mov x21, x0
1002bf488:      ldrb    w8, [x0, #0x20]
1002bf48c:      cbnz    w8, 0x1002bf878 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5d8>
1002bf490:      ldr x8, [x21]
1002bf494:      cbnz    x8, 0x1002bf8c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
1002bf498:      mov x23, #0x7fff000000000000 ; =9223090561878065152
1002bf49c:      bfxil   x23, x22, #0, #48
1002bf4a0:      mov x8, #-0x1               ; =-1
1002bf4a4:      str x8, [x21]
1002bf4a8:      mov x22, x21
1002bf4ac:      ldr x8, [x22, #0x8]!
1002bf4b0:      ldr x25, [x21, #0x18]
1002bf4b4:      cmp x25, x8
1002bf4b8:      b.ne    0x1002bf4c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x224>
1002bf4bc:      mov x0, x22
1002bf4c0:      bl  0x100ccd37c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1002bf4c4:      ldr x8, [x21, #0x10]
1002bf4c8:      str x23, [x8, x25, lsl #3]
1002bf4cc:      add x8, x25, #0x1
1002bf4d0:      str x8, [x21, #0x18]
1002bf4d4:      ldr x8, [x21]
1002bf4d8:      add x8, x8, #0x1
1002bf4dc:      str x8, [x21]
1002bf4e0:      mov x0, #0x0                ; =0
1002bf4e4:      bl  0x1008ae260 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1002bf4e8:      ldrb    w8, [x0]
1002bf4ec:      strb    wzr, [x0]
1002bf4f0:      cbz w8, 0x1002bf528 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x288>
1002bf4f4:      mov x23, x0
1002bf4f8:      mov x0, #0x0                ; =0
1002bf4fc:      bl  0x1008ae280 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1002bf500:      ldrb    w8, [x0]
1002bf504:      tst w8, #0x3
1002bf508:      b.ne    0x1002bf520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x280>
1002bf50c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
1002bf510:      add x8, x8, #0x5d8
1002bf514:      ldapr   w8, [x8]
1002bf518:      cmp w8, #0x0
1002bf51c:      b.le    0x1002bf724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x484>
1002bf520:      mov w8, #0x1                ; =1
1002bf524:      strb    w8, [x23]
1002bf528:      bl  0x100875f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1002bf52c:      bl  0x100875970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1002bf530:      ldrb    w8, [x21, #0x20]
1002bf534:      cbnz    w8, 0x1002bf774 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4d4>
1002bf538:      ldr x8, [x21]
1002bf53c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bf540:      cmp x8, x9
1002bf544:      b.hs    0x1002bf7a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x500>
1002bf548:      add x9, x8, #0x1
1002bf54c:      str x9, [x21]
1002bf550:      ldr x10, [x21, #0x18]
1002bf554:      mov w9, #0x1                ; =1
1002bf558:      cmp x25, x10
1002bf55c:      b.hs    0x1002bf570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d0>
1002bf560:      ldr x10, [x21, #0x10]
1002bf564:      ldr x10, [x10, x25, lsl #3]
1002bf568:      and x10, x10, #0xffffffffffff
1002bf56c:      b   0x1002bf574 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d4>
1002bf570:      mov w10, #0x1               ; =1
1002bf574:      str x8, [x21]
1002bf578:      add x8, x10, #0x14
1002bf57c:      movi.2d v0, #0000000000000000
1002bf580:      stur    q0, [x24, #0x78]
1002bf584:      stur    q0, [x24, #0x68]
1002bf588:      stur    q0, [x24, #0x58]
1002bf58c:      stur    q0, [x24, #0x48]
1002bf590:      strb    w9, [sp, #0x120]
1002bf594:      mov x9, #-0x1               ; =-1
1002bf598:      stp x8, x20, [sp, #0xb8]
1002bf59c:      str x9, [sp, #0x90]
1002bf5a0:      stp xzr, xzr, [sp, #0xc8]
1002bf5a4:      str xzr, [sp, #0x118]
1002bf5a8:      add x0, sp, #0x90
1002bf5ac:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1002bf5b0:      mov x20, x0
1002bf5b4:      ldp x8, x9, [sp, #0xc0]
1002bf5b8:      cmp x9, x8
1002bf5bc:      b.hs    0x1002bf5f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1002bf5c0:      ldr x10, [sp, #0xb8]
1002bf5c4:      mov x11, #0x2600            ; =9728
1002bf5c8:      movk    x11, #0x1, lsl #32
1002bf5cc:      ldrb    w12, [x10, x9]
1002bf5d0:      cmp w12, #0x20
1002bf5d4:      b.hi    0x1002bf5f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1002bf5d8:      lsr x12, x11, x12
1002bf5dc:      tbz w12, #0x0, 0x1002bf5f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1002bf5e0:      add x9, x9, #0x1
1002bf5e4:      cmp x8, x9
1002bf5e8:      b.ne    0x1002bf5cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x32c>
1002bf5ec:      mov x9, x8
1002bf5f0:      ldrb    w23, [sp, #0x120]
1002bf5f4:      cmp x9, x8
1002bf5f8:      cset    w24, eq
1002bf5fc:      ldrb    w8, [x21, #0x20]
1002bf600:      cbnz    w8, 0x1002bf8a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x600>
1002bf604:      ldr x8, [x21]
1002bf608:      cbnz    x8, 0x1002bf8c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
1002bf60c:      mov x8, #-0x1               ; =-1
1002bf610:      str x8, [x21]
1002bf614:      ldr x26, [x21, #0x18]
1002bf618:      ldr x8, [x21, #0x8]
1002bf61c:      cmp x26, x8
1002bf620:      b.ne    0x1002bf62c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x38c>
1002bf624:      mov x0, x22
1002bf628:      bl  0x100ccd37c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1002bf62c:      ldr x8, [x21, #0x10]
1002bf630:      str x20, [x8, x26, lsl #3]
1002bf634:      add x8, x26, #0x1
1002bf638:      str x8, [x21, #0x18]
1002bf63c:      ldr x8, [x21]
1002bf640:      add x8, x8, #0x1
1002bf644:      str x8, [x21]
1002bf648:      mov x0, #0x0                ; =0
1002bf64c:      bl  0x1008ae280 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1002bf650:      ldrb    w8, [x0]
1002bf654:      and w8, w8, #0xfffffffd
1002bf658:      strb    w8, [x0]
1002bf65c:      bl  0x1008768bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1002bf660:      bl  0x10087ace8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
1002bf664:      ldrb    w8, [x21, #0x20]
1002bf668:      cbnz    w8, 0x1002bf8d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x630>
1002bf66c:      ldr x8, [x21]
1002bf670:      cbnz    x8, 0x1002bf900 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x660>
1002bf674:      and w22, w24, w23
1002bf678:      ldr x8, [x21, #0x18]
1002bf67c:      cmp x25, x8
1002bf680:      b.hi    0x1002bf688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3e8>
1002bf684:      str x25, [x21, #0x18]
1002bf688:      adrp    x0, 0x1010ab000 <_anon.b822d7b979bdf0233543f470364426b7.316+0x270>
1002bf68c:      add x0, x0, #0x548
1002bf690:      bl  0x100139e4c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
1002bf694:      tbz w22, #0x0, 0x1002bf6ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x40c>
1002bf698:      stp xzr, x20, [x19]
1002bf69c:      ldr x8, [sp, #0x90]
1002bf6a0:      cmn x8, #0x1
1002bf6a4:      b.ne    0x1002bf6f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x458>
1002bf6a8:      b   0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf6ac:      adrp    x0, 0x100dca000 <_anon.b822d7b979bdf0233543f470364426b7.519+0xab>
1002bf6b0:      add x0, x0, #0x1e2
1002bf6b4:      mov w1, #0x21               ; =33
1002bf6b8:      mov w2, #0x21               ; =33
1002bf6bc:      bl  0x10097fb80 <_js_string_from_bytes_with_capacity>
1002bf6c0:      mov x3, x0
1002bf6c4:      adrp    x1, 0x100dc6000 <_anon.2d62e9c08ab2025701038807088a1a53.881+0x128c>
1002bf6c8:      add x1, x1, #0x84c
1002bf6cc:      mov w20, #0x1               ; =1
1002bf6d0:      mov w0, #0x4                ; =4
1002bf6d4:      mov w2, #0xb                ; =11
1002bf6d8:      mov w4, #0x1                ; =1
1002bf6dc:      bl  0x1002a54fc <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1002bf6e0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1002bf6e4:      bfxil   x8, x0, #0, #48
1002bf6e8:      stp x20, x8, [x19]
1002bf6ec:      ldr x8, [sp, #0x90]
1002bf6f0:      cmn x8, #0x1
1002bf6f4:      b.eq    0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf6f8:      cbz x8, 0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf6fc:      ldr x0, [sp, #0x98]
1002bf700:      bl  0x100ce1540 <_mi_free>
1002bf704:      ldp x29, x30, [sp, #0x190]
1002bf708:      ldp x20, x19, [sp, #0x180]
1002bf70c:      ldp x22, x21, [sp, #0x170]
1002bf710:      ldp x24, x23, [sp, #0x160]
1002bf714:      ldp x26, x25, [sp, #0x150]
1002bf718:      ldp x28, x27, [sp, #0x140]
1002bf71c:      add sp, sp, #0x1a0
1002bf720:      ret
1002bf724:      mov x0, #0x0                ; =0
1002bf728:      bl  0x1008ae3c8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1002bf72c:      ldr x23, [x0]
1002bf730:      mov x0, #0x0                ; =0
1002bf734:      bl  0x1008ae160 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1002bf738:      ldr x8, [x0]
1002bf73c:      cmp x8, x23
1002bf740:      b.ls    0x1002bf760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4c0>
1002bf744:      str x23, [x0]
1002bf748:      adrp    x0, 0x101138000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy36GC_LAST_COLLECTION_POST_IN_USE_BYTES0s_023___RUST_STD_INTERNAL_VAL>
1002bf74c:      add x0, x0, #0x228
1002bf750:      ldr x8, [x0]
1002bf754:      blr x8
1002bf758:      mov w8, #0x1                ; =1
1002bf75c:      strb    w8, [x0]
1002bf760:      bl  0x100875f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1002bf764:      bl  0x100875f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1002bf768:      bl  0x100875970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1002bf76c:      ldrb    w8, [x21, #0x20]
1002bf770:      cbz w8, 0x1002bf538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x298>
1002bf774:      cmp w8, #0x2
1002bf778:      b.eq    0x1002bf8d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
1002bf77c:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf780:      add x1, x1, #0xf78
1002bf784:      mov x0, x21
1002bf788:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf78c:      strb    wzr, [x21, #0x20]
1002bf790:      ldr x8, [x21]
1002bf794:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bf798:      cmp x8, x9
1002bf79c:      b.lo    0x1002bf548 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2a8>
1002bf7a0:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1002bf7a4:      add x0, x0, #0xdc8
1002bf7a8:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1002bf7ac:      stur    x20, [x29, #-0x68]
1002bf7b0:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1002bf7b4:      bfxil   x8, x22, #0, #48
1002bf7b8:      str x8, [sp, #0x90]
1002bf7bc:      adrp    x21, 0x1010ab000 <_anon.b822d7b979bdf0233543f470364426b7.316+0x270>
1002bf7c0:      add x21, x21, #0x540
1002bf7c4:      add x1, sp, #0x90
1002bf7c8:      mov x0, x21
1002bf7cc:      bl  0x100137690 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1002bf7d0:      mov x22, x0
1002bf7d4:      stp x0, x23, [x29, #-0x60]
1002bf7d8:      str x20, [sp]
1002bf7dc:      sub x8, x29, #0x58
1002bf7e0:      mov x9, sp
1002bf7e4:      stp x8, x9, [sp, #0x90]
1002bf7e8:      sub x8, x29, #0x60
1002bf7ec:      sub x9, x29, #0x68
1002bf7f0:      stp x8, x9, [sp, #0xa0]
1002bf7f4:      adrp    x0, 0x1010a9000 <_anon.2d62e9c08ab2025701038807088a1a53.1055+0x5b8>
1002bf7f8:      add x0, x0, #0xb48
1002bf7fc:      add x1, sp, #0x90
1002bf800:      bl  0x10012c5d8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1002bf804:      mov x23, x0
1002bf808:      mov x20, x1
1002bf80c:      str x22, [sp, #0x90]
1002bf810:      add x1, sp, #0x90
1002bf814:      mov x0, x21
1002bf818:      bl  0x100137728 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1002bf81c:      adrp    x0, 0x1010ab000 <_anon.b822d7b979bdf0233543f470364426b7.316+0x270>
1002bf820:      add x0, x0, #0x548
1002bf824:      bl  0x100139fb4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1002bf828:      tbnz    w23, #0x0, 0x1002bf86c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5cc>
1002bf82c:      adrp    x0, 0x100dc6000 <_anon.2d62e9c08ab2025701038807088a1a53.881+0x128c>
1002bf830:      add x0, x0, #0x7fd
1002bf834:      mov w1, #0x29               ; =41
1002bf838:      mov w2, #0x29               ; =41
1002bf83c:      bl  0x10097fb80 <_js_string_from_bytes_with_capacity>
1002bf840:      mov x3, x0
1002bf844:      adrp    x1, 0x100dc6000 <_anon.2d62e9c08ab2025701038807088a1a53.881+0x128c>
1002bf848:      add x1, x1, #0x84c
1002bf84c:      mov w21, #0x1               ; =1
1002bf850:      mov w0, #0x4                ; =4
1002bf854:      mov w2, #0xb                ; =11
1002bf858:      mov w4, #0x1                ; =1
1002bf85c:      bl  0x1002a54fc <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1002bf860:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
1002bf864:      bfxil   x20, x0, #0, #48
1002bf868:      b   0x1002bf870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5d0>
1002bf86c:      mov x21, #0x0               ; =0
1002bf870:      stp x21, x20, [x19]
1002bf874:      b   0x1002bf704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1002bf878:      cmp w8, #0x1
1002bf87c:      b.ne    0x1002bf8d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
1002bf880:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf884:      add x1, x1, #0xf78
1002bf888:      mov x0, x21
1002bf88c:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf890:      strb    wzr, [x21, #0x20]
1002bf894:      ldr x8, [x21]
1002bf898:      cbz x8, 0x1002bf498 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1f8>
1002bf89c:      b   0x1002bf8c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
1002bf8a0:      cmp w8, #0x2
1002bf8a4:      b.eq    0x1002bf8d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
1002bf8a8:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf8ac:      add x1, x1, #0xf78
1002bf8b0:      mov x0, x21
1002bf8b4:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf8b8:      strb    wzr, [x21, #0x20]
1002bf8bc:      ldr x8, [x21]
1002bf8c0:      cbz x8, 0x1002bf60c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x36c>
1002bf8c4:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1002bf8c8:      add x0, x0, #0xdf8
1002bf8cc:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1002bf8d0:      cmp w8, #0x2
1002bf8d4:      b.ne    0x1002bf8e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x644>
1002bf8d8:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1002bf8dc:      add x0, x0, #0xed8
1002bf8e0:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1002bf8e4:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf8e8:      add x1, x1, #0xf78
1002bf8ec:      mov x0, x21
1002bf8f0:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf8f4:      strb    wzr, [x21, #0x20]
1002bf8f8:      ldr x8, [x21]
1002bf8fc:      cbz x8, 0x1002bf674 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3d4>
1002bf900:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1002bf904:      add x0, x0, #0xe58
1002bf908:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
