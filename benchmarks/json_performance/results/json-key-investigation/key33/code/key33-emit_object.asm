/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/key33-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bd440 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object>:
1004bd440:      sub sp, sp, #0x190
1004bd444:      stp x28, x27, [sp, #0x130]
1004bd448:      stp x26, x25, [sp, #0x140]
1004bd44c:      stp x24, x23, [sp, #0x150]
1004bd450:      stp x22, x21, [sp, #0x160]
1004bd454:      stp x20, x19, [sp, #0x170]
1004bd458:      stp x29, x30, [sp, #0x180]
1004bd45c:      add x29, sp, #0x180
1004bd460:      mov x19, x1
1004bd464:      mov x22, x0
1004bd468:      movi.2d v0, #0000000000000000
1004bd46c:      str d0, [sp, #0x30]
1004bd470:      str wzr, [sp, #0x38]
1004bd474:      str d0, [sp, #0x58]
1004bd478:      str wzr, [sp, #0x60]
1004bd47c:      str d0, [sp, #0x80]
1004bd480:      str wzr, [sp, #0x88]
1004bd484:      str d0, [sp, #0xa8]
1004bd488:      str wzr, [sp, #0xb0]
1004bd48c:      ldr w8, [x0, #0x4]
1004bd490:      mov w9, #-0x40000000        ; =-1073741824
1004bd494:      cmp w8, w9
1004bd498:      csel    w20, w8, w9, lt
1004bd49c:      adrp    x25, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bd4a0:      ldr w21, [x25, #0x634]
1004bd4a4:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bd4a8:      cmp w21, #0x300
1004bd4ac:      b.hs    0x1004bd924 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4e4>
1004bd4b0:      ldr x8, [x28, #0x48]
1004bd4b4:      cmn x8, #0x1
1004bd4b8:      b.eq    0x1004bd914 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4d4>
1004bd4bc:      mrs x9, TPIDRRO_EL0
1004bd4c0:      and x9, x9, #0xfffffffffffffff8
1004bd4c4:      ldr x0, [x9, x8, lsl #3]
1004bd4c8:      cbz x0, 0x1004bd914 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4d4>
1004bd4cc:      add x8, x0, x21, lsl #3
1004bd4d0:      ldr x0, [x8, #0x1e8]
1004bd4d4:      cbz x0, 0x1004bd924 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4e4>
1004bd4d8:      ldr x0, [x0]
1004bd4dc:      cbz x0, 0x1004bd938 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4f8>
1004bd4e0:      ldr x8, [x0, #0x5190]
1004bd4e4:      and w9, w20, #0x3fffffff
1004bd4e8:      lsr x10, x9, #15
1004bd4ec:      ubfx    x11, x9, #5, #10
1004bd4f0:      and x9, x9, #0x1f
1004bd4f4:      ldr x8, [x8, x10, lsl #3]
1004bd4f8:      ldr x8, [x8, x11, lsl #3]
1004bd4fc:      lsl x9, x9, #5
1004bd500:      ldr x23, [x8, x9]
1004bd504:      ldr x0, [x23, #0x8]
1004bd508:      bl  0x10021ad34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new>
1004bd50c:      cbz x0, 0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd510:      mov x20, x0
1004bd514:      str w1, [sp, #0x2c]
1004bd518:      ldr x1, [x22, #0x10]
1004bd51c:      sub x0, x29, #0x88
1004bd520:      bl  0x1004bdbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bd524:      ldur    w8, [x29, #-0x88]
1004bd528:      cmn w8, #0x1
1004bd52c:      b.eq    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd530:      add x27, sp, #0x30
1004bd534:      ldur    x8, [x29, #-0x68]
1004bd538:      ldur    q0, [x27, #0xd8]
1004bd53c:      ldur    q1, [x27, #0xc8]
1004bd540:      stp q1, q0, [sp, #0x30]
1004bd544:      str x8, [sp, #0x50]
1004bd548:      ldp w10, w8, [sp, #0x30]
1004bd54c:      ldr w9, [sp, #0x38]
1004bd550:      cbz w10, 0x1004bd564 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x124>
1004bd554:      cmp w10, #0x1
1004bd558:      b.ne    0x1004bd574 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x134>
1004bd55c:      ldr w8, [sp, #0x3c]
1004bd560:      b   0x1004bd578 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x138>
1004bd564:      add w10, w8, #0x2
1004bd568:      add w8, w9, #0x2
1004bd56c:      mov x9, x10
1004bd570:      b   0x1004bd578 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x138>
1004bd574:      mov x9, x8
1004bd578:      cmn w20, #0x3
1004bd57c:      b.hi    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd580:      add w10, w20, #0x2
1004bd584:      adds    w9, w9, w10
1004bd588:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd58c:      mov x0, #0x0                ; =0
1004bd590:      adds    w26, w9, #0x1
1004bd594:      cset    w9, hs
1004bd598:      mov x10, #-0x200000001      ; =-8589934593
1004bd59c:      cmp x20, x10
1004bd5a0:      b.hi    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd5a4:      tbnz    w9, #0x0, 0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd5a8:      mov x0, #0x0                ; =0
1004bd5ac:      lsr x9, x20, #32
1004bd5b0:      add w9, w9, #0x2
1004bd5b4:      adds    w21, w8, w9
1004bd5b8:      b.hs    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd5bc:      cmn w21, #0x1
1004bd5c0:      b.eq    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd5c4:      add w8, w21, #0x1
1004bd5c8:      cmp x19, #0x1
1004bd5cc:      b.ne    0x1004bd6b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x278>
1004bd5d0:      stp w8, wzr, [sp, #0x24]
1004bd5d4:      mov w21, #0x0               ; =0
1004bd5d8:      mov w27, #0x0               ; =0
1004bd5dc:      mov x23, #0x200000002       ; =8589934594
1004bd5e0:      mov x8, #0x200000002        ; =8589934594
1004bd5e4:      stp x8, x8, [sp, #0x10]
1004bd5e8:      bl  0x10026abd0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004bd5ec:      stur    x0, [x29, #-0xb0]
1004bd5f0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1004bd5f4:      stp x22, x8, [x29, #-0x80]
1004bd5f8:      mov w8, #0x1                ; =1
1004bd5fc:      stur    x8, [x29, #-0x88]
1004bd600:      sub x0, x29, #0x88
1004bd604:      bl  0x10026acd8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004bd608:      mov x24, x0
1004bd60c:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004bd610:      add x0, x0, #0xbf0
1004bd614:      ldr x8, [x0]
1004bd618:      blr x8
1004bd61c:      strb    wzr, [x0]
1004bd620:      mov x0, x22
1004bd624:      bl  0x1004c3280 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
1004bd628:      tbz w0, #0x0, 0x1004bd718 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2d8>
1004bd62c:      str w27, [sp]
1004bd630:      mov x0, x26
1004bd634:      bl  0x100325f8c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004bd638:      mov x22, x0
1004bd63c:      mov x27, x1
1004bd640:      ldr x8, [x28, #0x48]
1004bd644:      cmn x8, #0x1
1004bd648:      str w21, [sp, #0x4]
1004bd64c:      str x23, [sp, #0x8]
1004bd650:      b.eq    0x1004bd748 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x308>
1004bd654:      mrs x9, TPIDRRO_EL0
1004bd658:      and x9, x9, #0xfffffffffffffff8
1004bd65c:      ldr x8, [x9, x8, lsl #3]
1004bd660:      cbz x8, 0x1004bd748 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x308>
1004bd664:      ldr x8, [x8, #0x19e8]
1004bd668:      cbz x8, 0x1004bd748 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x308>
1004bd66c:      ldr x9, [x8]
1004bd670:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004bd674:      cmp x9, x10
1004bd678:      b.hs    0x1004bdba0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x760>
1004bd67c:      add x10, x9, #0x1
1004bd680:      str x10, [x8]
1004bd684:      ldr x10, [x8, #0x18]
1004bd688:      cmp x24, x10
1004bd68c:      ldr w1, [sp, #0x2c]
1004bd690:      b.hs    0x1004bdbac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x76c>
1004bd694:      ldr x10, [x8, #0x10]
1004bd698:      mov w11, #0x18              ; =24
1004bd69c:      madd    x10, x24, x11, x10
1004bd6a0:      ldr x11, [x10]
1004bd6a4:      cmp x11, #0x1
1004bd6a8:      b.ne    0x1004bdbb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x770>
1004bd6ac:      ldr x24, [x10, #0x8]
1004bd6b0:      str x9, [x8]
1004bd6b4:      b   0x1004bd758 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x318>
1004bd6b8:      ldr x0, [x23, #0x10]
1004bd6bc:      bl  0x10021ad34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new>
1004bd6c0:      cbz x0, 0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd6c4:      mov x24, x0
1004bd6c8:      str w1, [sp, #0x28]
1004bd6cc:      ldr x1, [x22, #0x18]
1004bd6d0:      sub x0, x29, #0x88
1004bd6d4:      bl  0x1004bdbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bd6d8:      ldur    w8, [x29, #-0x88]
1004bd6dc:      cmn w8, #0x1
1004bd6e0:      b.eq    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd6e4:      ldur    x8, [x29, #-0x68]
1004bd6e8:      ldur    q0, [x27, #0xd8]
1004bd6ec:      ldur    q1, [x27, #0xc8]
1004bd6f0:      stur    q1, [sp, #0x58]
1004bd6f4:      stur    q0, [sp, #0x68]
1004bd6f8:      str x8, [sp, #0x78]
1004bd6fc:      ldp w10, w8, [sp, #0x58]
1004bd700:      ldr w9, [sp, #0x60]
1004bd704:      cbz w10, 0x1004bd940 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x500>
1004bd708:      cmp w10, #0x1
1004bd70c:      b.ne    0x1004bd950 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x510>
1004bd710:      ldr w8, [sp, #0x64]
1004bd714:      b   0x1004bd954 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x514>
1004bd718:      sub x0, x29, #0xb0
1004bd71c:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004bd720:      mov x0, #0x0                ; =0
1004bd724:      mov x1, x24
1004bd728:      ldp x29, x30, [sp, #0x180]
1004bd72c:      ldp x20, x19, [sp, #0x170]
1004bd730:      ldp x22, x21, [sp, #0x160]
1004bd734:      ldp x24, x23, [sp, #0x150]
1004bd738:      ldp x26, x25, [sp, #0x140]
1004bd73c:      ldp x28, x27, [sp, #0x130]
1004bd740:      add sp, sp, #0x190
1004bd744:      ret
1004bd748:      mov x0, x24
1004bd74c:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004bd750:      mov x24, x0
1004bd754:      ldr w1, [sp, #0x2c]
1004bd758:      ldr w8, [x24, #0x4]
1004bd75c:      mov w9, #-0x40000000        ; =-1073741824
1004bd760:      cmp w8, w9
1004bd764:      csel    w21, w8, w9, lt
1004bd768:      ldr w23, [x25, #0x634]
1004bd76c:      cmp w23, #0x300
1004bd770:      b.hs    0x1004bda34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x5f4>
1004bd774:      ldr x8, [x28, #0x48]
1004bd778:      cmn x8, #0x1
1004bd77c:      b.eq    0x1004bda20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x5e0>
1004bd780:      mrs x9, TPIDRRO_EL0
1004bd784:      and x9, x9, #0xfffffffffffffff8
1004bd788:      ldr x0, [x9, x8, lsl #3]
1004bd78c:      cbz x0, 0x1004bda20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x5e0>
1004bd790:      add x8, x0, x23, lsl #3
1004bd794:      ldr x0, [x8, #0x1e8]
1004bd798:      cbz x0, 0x1004bda34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x5f4>
1004bd79c:      ldr x0, [x0]
1004bd7a0:      cbz x0, 0x1004bda4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x60c>
1004bd7a4:      ldr x8, [x0, #0x5190]
1004bd7a8:      and w9, w21, #0x3fffffff
1004bd7ac:      lsr x10, x9, #15
1004bd7b0:      ubfx    x11, x9, #5, #10
1004bd7b4:      and x9, x9, #0x1f
1004bd7b8:      ldr x8, [x8, x10, lsl #3]
1004bd7bc:      ldr x8, [x8, x11, lsl #3]
1004bd7c0:      lsl x9, x9, #5
1004bd7c4:      ldr x23, [x8, x9]
1004bd7c8:      ldr w8, [sp, #0x24]
1004bd7cc:      stp w8, w26, [x22]
1004bd7d0:      stp wzr, wzr, [x22, #0xc]
1004bd7d4:      str w26, [x22, #0x8]
1004bd7d8:      mov w8, #0x7b               ; =123
1004bd7dc:      mov x3, x27
1004bd7e0:      strb    w8, [x3], #0x1
1004bd7e4:      ldr x2, [x23, #0x8]
1004bd7e8:      mov x0, x20
1004bd7ec:      bl  0x10021b34c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write>
1004bd7f0:      add x8, x27, x0
1004bd7f4:      mov w20, #0x3a              ; =58
1004bd7f8:      strb    w20, [x8, #0x1]
1004bd7fc:      add x25, x0, #0x2
1004bd800:      ldr x1, [x24, #0x10]
1004bd804:      add x21, sp, #0x30
1004bd808:      add x0, sp, #0x30
1004bd80c:      add x2, x27, x25
1004bd810:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd814:      add x8, x0, x25
1004bd818:      cmp x19, #0x1
1004bd81c:      b.eq    0x1004bd8f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4b4>
1004bd820:      mov w25, #0x2c              ; =44
1004bd824:      strb    w25, [x27, x8]
1004bd828:      add x26, x8, #0x1
1004bd82c:      ldr x2, [x23, #0x10]
1004bd830:      add x3, x27, x26
1004bd834:      ldr x0, [sp, #0x18]
1004bd838:      ldr w1, [sp, #0x28]
1004bd83c:      bl  0x10021b34c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write>
1004bd840:      add x8, x0, x26
1004bd844:      strb    w20, [x27, x8]
1004bd848:      add x20, x8, #0x1
1004bd84c:      ldr x1, [x24, #0x18]
1004bd850:      add x0, x21, #0x28
1004bd854:      add x2, x27, x20
1004bd858:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd85c:      add x8, x0, x20
1004bd860:      cmp x19, #0x2
1004bd864:      b.eq    0x1004bd8f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4b4>
1004bd868:      strb    w25, [x27, x8]
1004bd86c:      add x20, x8, #0x1
1004bd870:      ldr x2, [x23, #0x18]
1004bd874:      add x3, x27, x20
1004bd878:      ldr x0, [sp, #0x8]
1004bd87c:      ldr w1, [sp, #0x4]
1004bd880:      bl  0x10021b34c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write>
1004bd884:      add x8, x0, x20
1004bd888:      mov w21, #0x3a              ; =58
1004bd88c:      strb    w21, [x27, x8]
1004bd890:      add x25, x8, #0x1
1004bd894:      add x20, sp, #0x30
1004bd898:      ldr x1, [x24, #0x20]
1004bd89c:      add x0, x20, #0x50
1004bd8a0:      add x2, x27, x25
1004bd8a4:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd8a8:      add x8, x0, x25
1004bd8ac:      cmp x19, #0x3
1004bd8b0:      b.eq    0x1004bd8f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x4b4>
1004bd8b4:      mov w9, #0x2c               ; =44
1004bd8b8:      strb    w9, [x27, x8]
1004bd8bc:      add x19, x8, #0x1
1004bd8c0:      ldr x2, [x23, #0x20]
1004bd8c4:      add x3, x27, x19
1004bd8c8:      ldr x0, [sp, #0x10]
1004bd8cc:      ldr w1, [sp]
1004bd8d0:      bl  0x10021b34c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write>
1004bd8d4:      add x8, x0, x19
1004bd8d8:      strb    w21, [x27, x8]
1004bd8dc:      add x19, x8, #0x1
1004bd8e0:      ldr x1, [x24, #0x28]
1004bd8e4:      add x0, x20, #0x78
1004bd8e8:      add x2, x27, x19
1004bd8ec:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd8f0:      add x8, x0, x19
1004bd8f4:      mov w9, #0x7d               ; =125
1004bd8f8:      strb    w9, [x27, x8]
1004bd8fc:      mov x24, #0x7fff000000000000 ; =9223090561878065152
1004bd900:      bfxil   x24, x22, #0, #48
1004bd904:      sub x0, x29, #0xb0
1004bd908:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004bd90c:      mov w0, #0x1                ; =1
1004bd910:      b   0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd914:      bl  0x100adbcb0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004bd918:      add x8, x0, x21, lsl #3
1004bd91c:      ldr x0, [x8, #0x1e8]
1004bd920:      cbnz    x0, 0x1004bd4d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x98>
1004bd924:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1c8>
1004bd928:      add x0, x0, #0x348
1004bd92c:      bl  0x100ad911c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004bd930:      ldr x0, [x0]
1004bd934:      cbnz    x0, 0x1004bd4e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0xa0>
1004bd938:      bl  0x100adadec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004bd93c:      b   0x1004bd4e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0xa0>
1004bd940:      add w10, w8, #0x2
1004bd944:      add w8, w9, #0x2
1004bd948:      mov x9, x10
1004bd94c:      b   0x1004bd954 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x514>
1004bd950:      mov x9, x8
1004bd954:      adds    w10, w26, w24
1004bd958:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd95c:      adds    w9, w9, w10
1004bd960:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd964:      cmn w9, #0x3
1004bd968:      b.hi    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd96c:      lsr x10, x24, #32
1004bd970:      add w11, w21, #0x1
1004bd974:      add w10, w11, w10
1004bd978:      cmp w10, w21
1004bd97c:      b.ls    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd980:      str x24, [sp, #0x18]
1004bd984:      mov x0, #0x0                ; =0
1004bd988:      adds    w8, w8, w10
1004bd98c:      b.hs    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd990:      cmn w8, #0x3
1004bd994:      b.hi    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd998:      add w26, w9, #0x2
1004bd99c:      add w24, w8, #0x2
1004bd9a0:      cmp x19, #0x2
1004bd9a4:      b.ne    0x1004bd9c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x584>
1004bd9a8:      str w24, [sp, #0x24]
1004bd9ac:      mov w21, #0x0               ; =0
1004bd9b0:      mov w27, #0x0               ; =0
1004bd9b4:      mov x23, #0x200000002       ; =8589934594
1004bd9b8:      mov x8, #0x200000002        ; =8589934594
1004bd9bc:      str x8, [sp, #0x10]
1004bd9c0:      b   0x1004bd5e8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x1a8>
1004bd9c4:      ldr x0, [x23, #0x18]
1004bd9c8:      bl  0x10021ad34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new>
1004bd9cc:      str x0, [sp, #0x8]
1004bd9d0:      cbz x0, 0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bd9d4:      mov x21, x1
1004bd9d8:      ldr x1, [x22, #0x20]
1004bd9dc:      sub x0, x29, #0x88
1004bd9e0:      bl  0x1004bdbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bd9e4:      ldur    w8, [x29, #-0x88]
1004bd9e8:      cmn w8, #0x1
1004bd9ec:      b.eq    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bd9f0:      ldur    x8, [x29, #-0x68]
1004bd9f4:      ldur    q0, [x27, #0xd8]
1004bd9f8:      ldur    q1, [x27, #0xc8]
1004bd9fc:      stp q1, q0, [sp, #0x80]
1004bda00:      str x8, [sp, #0xa0]
1004bda04:      ldp w10, w8, [sp, #0x80]
1004bda08:      ldr w9, [sp, #0x88]
1004bda0c:      cbz w10, 0x1004bda58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x618>
1004bda10:      cmp w10, #0x1
1004bda14:      b.ne    0x1004bda68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x628>
1004bda18:      ldr w8, [sp, #0x8c]
1004bda1c:      b   0x1004bda6c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x62c>
1004bda20:      bl  0x100adbcb0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004bda24:      ldr w1, [sp, #0x2c]
1004bda28:      add x8, x0, x23, lsl #3
1004bda2c:      ldr x0, [x8, #0x1e8]
1004bda30:      cbnz    x0, 0x1004bd79c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x35c>
1004bda34:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1c8>
1004bda38:      add x0, x0, #0x348
1004bda3c:      bl  0x100ad911c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004bda40:      ldr w1, [sp, #0x2c]
1004bda44:      ldr x0, [x0]
1004bda48:      cbnz    x0, 0x1004bd7a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x364>
1004bda4c:      bl  0x100adadec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004bda50:      ldr w1, [sp, #0x2c]
1004bda54:      b   0x1004bd7a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x364>
1004bda58:      add w10, w8, #0x2
1004bda5c:      add w8, w9, #0x2
1004bda60:      mov x9, x10
1004bda64:      b   0x1004bda6c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x62c>
1004bda68:      mov x9, x8
1004bda6c:      ldr x10, [sp, #0x8]
1004bda70:      adds    w10, w26, w10
1004bda74:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bda78:      adds    w9, w9, w10
1004bda7c:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bda80:      cmn w9, #0x3
1004bda84:      b.hi    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bda88:      ldr x10, [sp, #0x8]
1004bda8c:      lsr x10, x10, #32
1004bda90:      adds    w10, w24, w10
1004bda94:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bda98:      mov x0, #0x0                ; =0
1004bda9c:      adds    w8, w8, w10
1004bdaa0:      b.hs    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bdaa4:      cmn w8, #0x3
1004bdaa8:      b.hi    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bdaac:      add w26, w9, #0x2
1004bdab0:      add w24, w8, #0x2
1004bdab4:      cmp x19, #0x3
1004bdab8:      b.ne    0x1004bdad0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x690>
1004bdabc:      str w24, [sp, #0x24]
1004bdac0:      mov w27, #0x0               ; =0
1004bdac4:      mov x8, #0x200000002        ; =8589934594
1004bdac8:      str x8, [sp, #0x10]
1004bdacc:      b   0x1004bdb98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x758>
1004bdad0:      ldr x0, [x23, #0x20]
1004bdad4:      bl  0x10021ad34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new>
1004bdad8:      cbz x0, 0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bdadc:      mov x23, x1
1004bdae0:      str x0, [sp, #0x10]
1004bdae4:      ldr x1, [x22, #0x28]
1004bdae8:      sub x0, x29, #0x88
1004bdaec:      bl  0x1004bdbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bdaf0:      ldur    w8, [x29, #-0x88]
1004bdaf4:      cmn w8, #0x1
1004bdaf8:      b.eq    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bdafc:      ldur    x8, [x29, #-0x68]
1004bdb00:      ldur    q0, [x27, #0xd8]
1004bdb04:      ldur    q1, [x27, #0xc8]
1004bdb08:      stur    q1, [sp, #0xa8]
1004bdb0c:      stur    q0, [x27, #0x88]
1004bdb10:      str x8, [sp, #0xc8]
1004bdb14:      ldp w10, w8, [sp, #0xa8]
1004bdb18:      ldr w9, [sp, #0xb0]
1004bdb1c:      cbz w10, 0x1004bdb38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x6f8>
1004bdb20:      cmp w10, #0x1
1004bdb24:      mov x27, x23
1004bdb28:      ldr x11, [sp, #0x10]
1004bdb2c:      b.ne    0x1004bdb50 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x710>
1004bdb30:      ldr w8, [sp, #0xb4]
1004bdb34:      b   0x1004bdb54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x714>
1004bdb38:      add w10, w8, #0x2
1004bdb3c:      add w8, w9, #0x2
1004bdb40:      mov x9, x10
1004bdb44:      mov x27, x23
1004bdb48:      ldr x11, [sp, #0x10]
1004bdb4c:      b   0x1004bdb54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x714>
1004bdb50:      mov x9, x8
1004bdb54:      adds    w10, w26, w11
1004bdb58:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bdb5c:      adds    w9, w9, w10
1004bdb60:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bdb64:      cmn w9, #0x3
1004bdb68:      b.hi    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bdb6c:      lsr x10, x11, #32
1004bdb70:      adds    w10, w24, w10
1004bdb74:      b.hs    0x1004bd720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e0>
1004bdb78:      mov x0, #0x0                ; =0
1004bdb7c:      adds    w8, w8, w10
1004bdb80:      b.hs    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bdb84:      cmn w8, #0x3
1004bdb88:      b.hi    0x1004bd724 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2e4>
1004bdb8c:      add w26, w9, #0x2
1004bdb90:      add w8, w8, #0x2
1004bdb94:      str w8, [sp, #0x24]
1004bdb98:      ldr x23, [sp, #0x8]
1004bdb9c:      b   0x1004bd5e8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x1a8>
1004bdba0:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004bdba4:      add x0, x0, #0x8a8
1004bdba8:      bl  0x100ab12dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004bdbac:      bl  0x100ae2b88 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004bdbb0:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbf0>
1004bdbb4:      add x0, x0, #0x6c
1004bdbb8:      mov w1, #0xb                ; =11
1004bdbbc:      bl  0x100ae2b50 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
