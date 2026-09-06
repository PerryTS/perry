/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100767388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
100767388:      sub sp, sp, #0x120
10076738c:      stp x28, x27, [sp, #0xc0]
100767390:      stp x26, x25, [sp, #0xd0]
100767394:      stp x24, x23, [sp, #0xe0]
100767398:      stp x22, x21, [sp, #0xf0]
10076739c:      stp x20, x19, [sp, #0x100]
1007673a0:      stp x29, x30, [sp, #0x110]
1007673a4:      add x29, sp, #0x110
1007673a8:      mov x20, x1
1007673ac:      mov x21, x0
1007673b0:      add x22, x0, #0x14
1007673b4:      cmp x1, #0x2
1007673b8:      b.ne    0x1007673d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48>
1007673bc:      ldrh    w8, [x22]
1007673c0:      mov w9, #0x7d7b             ; =32123
1007673c4:      cmp w8, w9
1007673c8:      b.eq    0x100767404 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c>
1007673cc:      b   0x100767568 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1e0>
1007673d0:      cmp x20, #0x3
1007673d4:      b.lo    0x100767568 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1e0>
1007673d8:      ldrb    w8, [x22]
1007673dc:      cmp w8, #0x20
1007673e0:      b.hi    0x100767424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x9c>
1007673e4:      mov x9, #0x2600             ; =9728
1007673e8:      movk    x9, #0x1, lsl #32
1007673ec:      lsr x9, x9, x8
1007673f0:      tbz w9, #0x0, 0x100767424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x9c>
1007673f4:      add x0, x21, #0x14
1007673f8:      mov x1, x20
1007673fc:      bl  0x10074e65c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
100767400:      tbz w0, #0x0, 0x100767450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc8>
100767404:      ldp x29, x30, [sp, #0x110]
100767408:      ldp x20, x19, [sp, #0x100]
10076740c:      ldp x22, x21, [sp, #0xf0]
100767410:      ldp x24, x23, [sp, #0xe0]
100767414:      ldp x26, x25, [sp, #0xd0]
100767418:      ldp x28, x27, [sp, #0xc0]
10076741c:      add sp, sp, #0x120
100767420:      b   0x10074e728 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
100767424:      cmp w8, #0x7b
100767428:      b.ne    0x100767450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc8>
10076742c:      ldrb    w8, [x21, #0x15]
100767430:      cmp w8, #0x20
100767434:      b.hi    0x100767448 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc0>
100767438:      mov x9, #0x2600             ; =9728
10076743c:      movk    x9, #0x1, lsl #32
100767440:      lsr x9, x9, x8
100767444:      tbnz    w9, #0x0, 0x1007673f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
100767448:      cmp w8, #0x7d
10076744c:      b.eq    0x1007673f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
100767450:      cmp x20, #0x3e9
100767454:      b.lo    0x100767568 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1e0>
100767458:      add x0, x21, #0x14
10076745c:      mov x1, x20
100767460:      mov w2, #0x3e8              ; =1000
100767464:      bl  0x100758e14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100767468:      tbz w0, #0x0, 0x100767568 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1e0>
10076746c:      add x0, x21, #0x14
100767470:      mov x1, x20
100767474:      mov w2, #0xa120             ; =41248
100767478:      movk    w2, #0x7, lsl #16
10076747c:      bl  0x100758e14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100767480:      tbnz    w0, #0x0, 0x100767b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7d8>
100767484:      stur    x20, [x29, #-0x70]
100767488:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
10076748c:      add x0, x0, #0xc0
100767490:      ldr x8, [x0]
100767494:      blr x8
100767498:      mov x19, x0
10076749c:      ldrb    w8, [x0, #0x20]
1007674a0:      cbnz    w8, 0x100767af4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x76c>
1007674a4:      ldr x8, [x19]
1007674a8:      cbnz    x8, 0x100767a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
1007674ac:      mov x23, #0x7fff000000000000 ; =9223090561878065152
1007674b0:      bfxil   x23, x21, #0, #48
1007674b4:      mov x8, #-0x1               ; =-1
1007674b8:      str x8, [x19]
1007674bc:      mov x0, x19
1007674c0:      ldr x8, [x0, #0x8]!
1007674c4:      ldr x21, [x19, #0x18]
1007674c8:      cmp x21, x8
1007674cc:      b.ne    0x1007674d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x14c>
1007674d0:      bl  0x100cb2c30 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1007674d4:      ldr x8, [x19, #0x10]
1007674d8:      str x23, [x8, x21, lsl #3]
1007674dc:      add x8, x21, #0x1
1007674e0:      str x8, [x19, #0x18]
1007674e4:      ldr x8, [x19]
1007674e8:      add x8, x8, #0x1
1007674ec:      str x8, [x19]
1007674f0:      stp x21, x22, [x29, #-0x68]
1007674f4:      stur    x20, [x29, #-0x58]
1007674f8:      sub x8, x29, #0x60
1007674fc:      sub x9, x29, #0x58
100767500:      stp x8, x9, [sp, #0x8]
100767504:      sub x8, x29, #0x68
100767508:      sub x9, x29, #0x70
10076750c:      stp x8, x9, [sp, #0x18]
100767510:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100767514:      add x0, x0, #0xa68
100767518:      add x1, sp, #0x8
10076751c:      bl  0x10012c630 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
100767520:      mov x20, x0
100767524:      mov x23, x1
100767528:      ldrb    w8, [x19, #0x20]
10076752c:      cbnz    w8, 0x100767b1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x794>
100767530:      ldr x8, [x19]
100767534:      cbnz    x8, 0x100767a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
100767538:      ldr x8, [x19, #0x18]
10076753c:      cmp x21, x8
100767540:      b.hi    0x100767548 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1c0>
100767544:      str x21, [x19, #0x18]
100767548:      adrp    x0, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
10076754c:      add x0, x0, #0x318
100767550:      bl  0x1001399c4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
100767554:      tbnz    w20, #0x0, 0x100767998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x610>
100767558:      adrp    x0, 0x100dfd000 <_anon.4ff118d01ccdc9bd41517af7abf33093.1077+0xe2>
10076755c:      add x0, x0, #0xaf6
100767560:      mov w1, #0x29               ; =41
100767564:      bl  0x10076887c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
100767568:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
10076756c:      add x0, x0, #0xc0
100767570:      ldr x8, [x0]
100767574:      blr x8
100767578:      mov x19, x0
10076757c:      ldrb    w8, [x0, #0x20]
100767580:      cbnz    w8, 0x1007679dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x654>
100767584:      ldr x8, [x19]
100767588:      cbnz    x8, 0x100767a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
10076758c:      mov x22, #0x7fff000000000000 ; =9223090561878065152
100767590:      bfxil   x22, x21, #0, #48
100767594:      mov x8, #-0x1               ; =-1
100767598:      str x8, [x19]
10076759c:      mov x21, x19
1007675a0:      ldr x8, [x21, #0x8]!
1007675a4:      ldr x24, [x19, #0x18]
1007675a8:      cmp x24, x8
1007675ac:      b.ne    0x1007675b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x230>
1007675b0:      mov x0, x21
1007675b4:      bl  0x100cb2c30 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1007675b8:      ldr x8, [x19, #0x10]
1007675bc:      str x22, [x8, x24, lsl #3]
1007675c0:      add x8, x24, #0x1
1007675c4:      str x8, [x19, #0x18]
1007675c8:      ldr x8, [x19]
1007675cc:      add x8, x8, #0x1
1007675d0:      str x8, [x19]
1007675d4:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
1007675d8:      add x0, x0, #0xfa0
1007675dc:      ldr x8, [x0]
1007675e0:      blr x8
1007675e4:      ldrb    w9, [x0]
1007675e8:      strb    wzr, [x0]
1007675ec:      adrp    x22, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
1007675f0:      add x22, x22, #0x228
1007675f4:      cbz w9, 0x100767630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a8>
1007675f8:      mov x8, x0
1007675fc:      ldr x9, [x22]
100767600:      mov x0, x22
100767604:      blr x9
100767608:      ldrb    w9, [x0]
10076760c:      tst w9, #0x3
100767610:      b.ne    0x100767628 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a0>
100767614:      adrp    x9, 0x101205000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x120>
100767618:      add x9, x9, #0x8a8
10076761c:      ldapr   w9, [x9]
100767620:      cmp w9, #0x0
100767624:      b.le    0x100767784 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3fc>
100767628:      mov w9, #0x1                ; =1
10076762c:      strb    w9, [x8]
100767630:      ldrb    w8, [x19, #0x20]
100767634:      cbnz    w8, 0x1007677dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x454>
100767638:      ldr x8, [x19]
10076763c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100767640:      cmp x8, x9
100767644:      b.hs    0x100767ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
100767648:      add x9, x8, #0x1
10076764c:      str x9, [x19]
100767650:      ldr x9, [x19, #0x18]
100767654:      cmp x24, x9
100767658:      b.hs    0x10076766c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e4>
10076765c:      ldr x9, [x19, #0x10]
100767660:      ldr x9, [x9, x24, lsl #3]
100767664:      and x23, x9, #0xffffffffffff
100767668:      b   0x100767670 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e8>
10076766c:      mov w23, #0x1               ; =1
100767670:      str x8, [x19]
100767674:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
100767678:      add x8, x8, #0x4c0
10076767c:      ldapr   x8, [x8]
100767680:      cbnz    x8, 0x1007679bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x634>
100767684:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
100767688:      ldrb    w8, [x8, #0x4c8]
10076768c:      cmp w8, #0x2
100767690:      b.eq    0x100767814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48c>
100767694:      cmp w8, #0x1
100767698:      b.ne    0x1007676dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x354>
10076769c:      stp x24, x20, [x29, #-0x70]
1007676a0:      ldrb    w8, [x19, #0x20]
1007676a4:      cbnz    w8, 0x100767a94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70c>
1007676a8:      ldr x8, [x19]
1007676ac:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007676b0:      cmp x8, x9
1007676b4:      b.hs    0x100767ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
1007676b8:      add x9, x8, #0x1
1007676bc:      str x9, [x19]
1007676c0:      ldr x9, [x19, #0x18]
1007676c4:      cmp x24, x9
1007676c8:      b.hs    0x10076771c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x394>
1007676cc:      ldr x9, [x19, #0x10]
1007676d0:      ldr x9, [x9, x24, lsl #3]
1007676d4:      and x9, x9, #0xffffffffffff
1007676d8:      b   0x100767720 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x398>
1007676dc:      add x8, x23, #0x14
1007676e0:      sub x9, x20, #0x400
1007676e4:      mov w10, #0xfffc00          ; =16776192
1007676e8:      cmp x9, x10
1007676ec:      b.hi    0x100767814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48c>
1007676f0:      mov x9, #0x2600             ; =9728
1007676f4:      movk    x9, #0x1, lsl #32
1007676f8:      mov x10, x20
1007676fc:      ldrb    w11, [x8], #0x1
100767700:      cmp w11, #0x20
100767704:      b.hi    0x10076780c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x484>
100767708:      lsr x12, x9, x11
10076770c:      tbz w12, #0x0, 0x10076780c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x484>
100767710:      subs    x10, x10, #0x1
100767714:      b.ne    0x1007676fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x374>
100767718:      b   0x100767814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48c>
10076771c:      mov w9, #0x1                ; =1
100767720:      str x8, [x19]
100767724:      add x8, x9, #0x14
100767728:      stp x8, x20, [x29, #-0x60]
10076772c:      sub x8, x29, #0x60
100767730:      sub x9, x29, #0x58
100767734:      stp x8, x9, [sp, #0x8]
100767738:      sub x8, x29, #0x70
10076773c:      sub x9, x29, #0x68
100767740:      stp x8, x9, [sp, #0x18]
100767744:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100767748:      add x0, x0, #0xa68
10076774c:      add x1, sp, #0x8
100767750:      bl  0x10012c9fc <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
100767754:      cmp x0, #0x1
100767758:      b.ne    0x100767814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48c>
10076775c:      mov x23, x1
100767760:      ldrb    w8, [x19, #0x20]
100767764:      cbnz    w8, 0x100767acc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x744>
100767768:      ldr x8, [x19]
10076776c:      cbnz    x8, 0x100767a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
100767770:      ldr x8, [x19, #0x18]
100767774:      cmp x24, x8
100767778:      b.hi    0x100767998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x610>
10076777c:      str x24, [x19, #0x18]
100767780:      b   0x100767998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x610>
100767784:      adrp    x0, 0x10112e000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime6object40ASYNC_GENERATOR_INTRINSIC_PROTO_PTR_SLOT7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100767788:      add x0, x0, #0x8d8
10076778c:      ldr x8, [x0]
100767790:      blr x8
100767794:      ldr x8, [x0]
100767798:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
10076779c:      add x0, x0, #0x1b0
1007677a0:      ldr x9, [x0]
1007677a4:      blr x9
1007677a8:      ldr x9, [x0]
1007677ac:      cmp x9, x8
1007677b0:      b.ls    0x1007677d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x448>
1007677b4:      str x8, [x0]
1007677b8:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
1007677bc:      add x0, x0, #0xf0
1007677c0:      ldr x8, [x0]
1007677c4:      blr x8
1007677c8:      mov w8, #0x1                ; =1
1007677cc:      strb    w8, [x0]
1007677d0:      bl  0x100747fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1007677d4:      ldrb    w8, [x19, #0x20]
1007677d8:      cbz w8, 0x100767638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b0>
1007677dc:      cmp w8, #0x2
1007677e0:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
1007677e4:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
1007677e8:      add x1, x1, #0x850
1007677ec:      mov x0, x19
1007677f0:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007677f4:      strb    wzr, [x19, #0x20]
1007677f8:      ldr x8, [x19]
1007677fc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100767800:      cmp x8, x9
100767804:      b.lo    0x100767648 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c0>
100767808:      b   0x100767ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
10076780c:      cmp w11, #0x5b
100767810:      b.eq    0x10076769c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x314>
100767814:      bl  0x100747fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100767818:      bl  0x1007479d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
10076781c:      ldrb    w8, [x19, #0x20]
100767820:      cbnz    w8, 0x100767a04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x67c>
100767824:      ldr x8, [x19]
100767828:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10076782c:      cmp x8, x9
100767830:      b.hs    0x100767ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
100767834:      add x9, x8, #0x1
100767838:      str x9, [x19]
10076783c:      ldr x10, [x19, #0x18]
100767840:      mov w9, #0x1                ; =1
100767844:      cmp x24, x10
100767848:      b.hs    0x10076785c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4d4>
10076784c:      ldr x10, [x19, #0x10]
100767850:      ldr x10, [x10, x24, lsl #3]
100767854:      and x10, x10, #0xffffffffffff
100767858:      b   0x100767860 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4d8>
10076785c:      mov w10, #0x1               ; =1
100767860:      str x8, [x19]
100767864:      add x8, x10, #0x14
100767868:      movi.2d v0, #0000000000000000
10076786c:      stp q0, q0, [sp, #0x70]
100767870:      stp q0, q0, [sp, #0x50]
100767874:      strb    w9, [sp, #0x98]
100767878:      mov x9, #-0x1               ; =-1
10076787c:      stp x8, x20, [sp, #0x30]
100767880:      str x9, [sp, #0x8]
100767884:      stp xzr, xzr, [sp, #0x40]
100767888:      str xzr, [sp, #0x90]
10076788c:      add x0, sp, #0x8
100767890:      bl  0x10070e500 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100767894:      mov x23, x0
100767898:      ldp x8, x9, [sp, #0x38]
10076789c:      cmp x9, x8
1007678a0:      b.hs    0x1007678d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x54c>
1007678a4:      ldr x10, [sp, #0x30]
1007678a8:      mov x11, #0x2600            ; =9728
1007678ac:      movk    x11, #0x1, lsl #32
1007678b0:      ldrb    w12, [x10, x9]
1007678b4:      cmp w12, #0x20
1007678b8:      b.hi    0x1007678d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x54c>
1007678bc:      lsr x12, x11, x12
1007678c0:      tbz w12, #0x0, 0x1007678d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x54c>
1007678c4:      add x9, x9, #0x1
1007678c8:      cmp x8, x9
1007678cc:      b.ne    0x1007678b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x528>
1007678d0:      mov x9, x8
1007678d4:      ldrb    w20, [sp, #0x98]
1007678d8:      cmp x9, x8
1007678dc:      cset    w25, eq
1007678e0:      ldrb    w8, [x19, #0x20]
1007678e4:      cbnz    w8, 0x100767a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6ac>
1007678e8:      ldr x8, [x19]
1007678ec:      cbnz    x8, 0x100767a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
1007678f0:      mov x8, #-0x1               ; =-1
1007678f4:      str x8, [x19]
1007678f8:      ldr x26, [x19, #0x18]
1007678fc:      ldr x8, [x19, #0x8]
100767900:      cmp x26, x8
100767904:      b.ne    0x100767910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x588>
100767908:      mov x0, x21
10076790c:      bl  0x100cb2c30 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100767910:      ldr x8, [x19, #0x10]
100767914:      str x23, [x8, x26, lsl #3]
100767918:      add x8, x26, #0x1
10076791c:      str x8, [x19, #0x18]
100767920:      ldr x8, [x19]
100767924:      add x8, x8, #0x1
100767928:      str x8, [x19]
10076792c:      ldr x8, [x22]
100767930:      mov x0, x22
100767934:      blr x8
100767938:      ldrb    w8, [x0]
10076793c:      and w8, w8, #0xfffffffd
100767940:      strb    w8, [x0]
100767944:      bl  0x100748930 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
100767948:      bl  0x10074d9fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
10076794c:      ldrb    w8, [x19, #0x20]
100767950:      cbnz    w8, 0x100767a64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6dc>
100767954:      ldr x8, [x19]
100767958:      cbnz    x8, 0x100767a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
10076795c:      and w20, w25, w20
100767960:      ldr x8, [x19, #0x18]
100767964:      cmp x24, x8
100767968:      b.hi    0x100767970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5e8>
10076796c:      str x24, [x19, #0x18]
100767970:      adrp    x0, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
100767974:      add x0, x0, #0x318
100767978:      bl  0x1001396f4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
10076797c:      tbz w20, #0x0, 0x100767b50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c8>
100767980:      ldr x8, [sp, #0x8]
100767984:      cmn x8, #0x1
100767988:      b.eq    0x100767998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x610>
10076798c:      cbz x8, 0x100767998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x610>
100767990:      ldr x0, [sp, #0x10]
100767994:      bl  0x100cda940 <_mi_free>
100767998:      mov x0, x23
10076799c:      ldp x29, x30, [sp, #0x110]
1007679a0:      ldp x20, x19, [sp, #0x100]
1007679a4:      ldp x22, x21, [sp, #0xf0]
1007679a8:      ldp x24, x23, [sp, #0xe0]
1007679ac:      ldp x26, x25, [sp, #0xd0]
1007679b0:      ldp x28, x27, [sp, #0xc0]
1007679b4:      add sp, sp, #0x120
1007679b8:      ret
1007679bc:      adrp    x0, 0x101129000 <__MergedGlobals+0x38>
1007679c0:      add x0, x0, #0x4c0
1007679c4:      bl  0x100cbd200 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
1007679c8:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
1007679cc:      ldrb    w8, [x8, #0x4c8]
1007679d0:      cmp w8, #0x2
1007679d4:      b.ne    0x100767694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x30c>
1007679d8:      b   0x100767814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48c>
1007679dc:      cmp w8, #0x1
1007679e0:      b.ne    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
1007679e4:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
1007679e8:      add x1, x1, #0x850
1007679ec:      mov x0, x19
1007679f0:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007679f4:      strb    wzr, [x19, #0x20]
1007679f8:      ldr x8, [x19]
1007679fc:      cbz x8, 0x10076758c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x204>
100767a00:      b   0x100767a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
100767a04:      cmp w8, #0x2
100767a08:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
100767a0c:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767a10:      add x1, x1, #0x850
100767a14:      mov x0, x19
100767a18:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767a1c:      strb    wzr, [x19, #0x20]
100767a20:      ldr x8, [x19]
100767a24:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100767a28:      cmp x8, x9
100767a2c:      b.lo    0x100767834 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4ac>
100767a30:      b   0x100767ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
100767a34:      cmp w8, #0x2
100767a38:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
100767a3c:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767a40:      add x1, x1, #0x850
100767a44:      mov x0, x19
100767a48:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767a4c:      strb    wzr, [x19, #0x20]
100767a50:      ldr x8, [x19]
100767a54:      cbz x8, 0x1007678f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x568>
100767a58:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
100767a5c:      add x0, x0, #0xdc8
100767a60:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100767a64:      cmp w8, #0x2
100767a68:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
100767a6c:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767a70:      add x1, x1, #0x850
100767a74:      mov x0, x19
100767a78:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767a7c:      strb    wzr, [x19, #0x20]
100767a80:      ldr x8, [x19]
100767a84:      cbz x8, 0x10076795c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5d4>
100767a88:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
100767a8c:      add x0, x0, #0xe28
100767a90:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100767a94:      cmp w8, #0x2
100767a98:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
100767a9c:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767aa0:      add x1, x1, #0x850
100767aa4:      mov x0, x19
100767aa8:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767aac:      strb    wzr, [x19, #0x20]
100767ab0:      ldr x8, [x19]
100767ab4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100767ab8:      cmp x8, x9
100767abc:      b.lo    0x1007676b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x330>
100767ac0:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
100767ac4:      add x0, x0, #0xd98
100767ac8:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100767acc:      cmp w8, #0x2
100767ad0:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
100767ad4:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767ad8:      add x1, x1, #0x850
100767adc:      mov x0, x19
100767ae0:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767ae4:      strb    wzr, [x19, #0x20]
100767ae8:      ldr x8, [x19]
100767aec:      cbz x8, 0x100767770 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3e8>
100767af0:      b   0x100767a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
100767af4:      cmp w8, #0x2
100767af8:      b.eq    0x100767b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x79c>
100767afc:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767b00:      add x1, x1, #0x850
100767b04:      mov x0, x19
100767b08:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767b0c:      strb    wzr, [x19, #0x20]
100767b10:      ldr x8, [x19]
100767b14:      cbz x8, 0x1007674ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x124>
100767b18:      b   0x100767a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
100767b1c:      cmp w8, #0x2
100767b20:      b.ne    0x100767b30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7a8>
100767b24:      adrp    x0, 0x101097000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100767b28:      add x0, x0, #0xed8
100767b2c:      bl  0x100cd3f9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100767b30:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100767b34:      add x1, x1, #0x850
100767b38:      mov x0, x19
100767b3c:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100767b40:      strb    wzr, [x19, #0x20]
100767b44:      ldr x8, [x19]
100767b48:      cbz x8, 0x100767538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1b0>
100767b4c:      b   0x100767a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
100767b50:      adrp    x0, 0x100e04000 <_anon.fd7e678389f6d6013308189123b84ec8.899+0x2>
100767b54:      add x0, x0, #0x5a1
100767b58:      mov w1, #0x21               ; =33
100767b5c:      bl  0x10076887c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
100767b60:      add x0, sp, #0x8
100767b64:      bl  0x100768924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
100767b68:      ldp x0, x1, [sp, #0x10]
100767b6c:      bl  0x1007681e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
