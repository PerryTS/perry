/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-tail-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008d65a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object>:
1008d65a4:      stp x28, x27, [sp, #-0x60]!
1008d65a8:      stp x26, x25, [sp, #0x10]
1008d65ac:      stp x24, x23, [sp, #0x20]
1008d65b0:      stp x22, x21, [sp, #0x30]
1008d65b4:      stp x20, x19, [sp, #0x40]
1008d65b8:      stp x29, x30, [sp, #0x50]
1008d65bc:      add x29, sp, #0x50
1008d65c0:      sub sp, sp, #0x1c0
1008d65c4:      mov x19, x1
1008d65c8:      mov x20, x0
1008d65cc:      movi.2d v0, #0000000000000000
1008d65d0:      str d0, [sp, #0x10]
1008d65d4:      str wzr, [sp, #0x18]
1008d65d8:      str d0, [sp, #0x38]
1008d65dc:      str wzr, [sp, #0x40]
1008d65e0:      str d0, [sp, #0x60]
1008d65e4:      str wzr, [sp, #0x68]
1008d65e8:      str d0, [sp, #0x88]
1008d65ec:      str wzr, [sp, #0x90]
1008d65f0:      str d0, [sp, #0xb0]
1008d65f4:      str wzr, [sp, #0xb8]
1008d65f8:      str d0, [sp, #0xd8]
1008d65fc:      str wzr, [sp, #0xe0]
1008d6600:      str d0, [sp, #0x100]
1008d6604:      str wzr, [sp, #0x108]
1008d6608:      str d0, [sp, #0x128]
1008d660c:      str wzr, [sp, #0x130]
1008d6610:      ldr w21, [x0, #0x4]
1008d6614:      adrp    x26, 0x101121000 <__MergedGlobals>
1008d6618:      add x26, x26, #0xcc4
1008d661c:      ldr w22, [x26]
1008d6620:      adrp    x25, 0x101120000 <_perry_global_baseline_worker_ts__1>
1008d6624:      add x25, x25, #0x980
1008d6628:      cmp w22, #0x300
1008d662c:      b.hs    0x1008d6c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x690>
1008d6630:      ldr x8, [x25]
1008d6634:      cmn x8, #0x1
1008d6638:      b.eq    0x1008d6c24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x680>
1008d663c:      mrs x9, TPIDRRO_EL0
1008d6640:      and x9, x9, #0xfffffffffffffff8
1008d6644:      ldr x0, [x9, x8, lsl #3]
1008d6648:      cbz x0, 0x1008d6c24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x680>
1008d664c:      add x8, x0, x22, lsl #3
1008d6650:      ldr x0, [x8, #0x1e8]
1008d6654:      cbz x0, 0x1008d6c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x690>
1008d6658:      ldr x0, [x0]
1008d665c:      cbz x0, 0x1008d6c48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6a4>
1008d6660:      ldr x8, [x0, #0x5190]
1008d6664:      ubfx    x9, x21, #15, #15
1008d6668:      ubfx    x10, x21, #5, #10
1008d666c:      and x11, x21, #0x1f
1008d6670:      ldr x8, [x8, x9, lsl #3]
1008d6674:      ldr x8, [x8, x10, lsl #3]
1008d6678:      lsl x9, x11, #5
1008d667c:      ldr x22, [x8, x9]
1008d6680:      ldr x1, [x22, #0x8]
1008d6684:      sub x0, x29, #0x90
1008d6688:      bl  0x1008d738c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
1008d668c:      ldur    w8, [x29, #-0x90]
1008d6690:      cmn w8, #0x1
1008d6694:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6698:      ldur    x8, [x29, #-0x70]
1008d669c:      ldp q1, q0, [x29, #-0x90]
1008d66a0:      stp q1, q0, [sp, #0x10]
1008d66a4:      str x8, [sp, #0x30]
1008d66a8:      ldr x1, [x20, #0x10]
1008d66ac:      sub x0, x29, #0x90
1008d66b0:      bl  0x1008d70c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1008d66b4:      ldur    w8, [x29, #-0x90]
1008d66b8:      cmn w8, #0x1
1008d66bc:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d66c0:      ldur    x8, [x29, #-0x70]
1008d66c4:      ldp q1, q0, [x29, #-0x90]
1008d66c8:      stp q1, q0, [sp, #0xb0]
1008d66cc:      str x8, [sp, #0xd0]
1008d66d0:      ldp w10, w8, [sp, #0x10]
1008d66d4:      ldr w9, [sp, #0x18]
1008d66d8:      cbz w10, 0x1008d6704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x160>
1008d66dc:      cmp w10, #0x1
1008d66e0:      b.ne    0x1008d672c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x188>
1008d66e4:      ldr w8, [sp, #0x1c]
1008d66e8:      ldp w12, w10, [sp, #0xb0]
1008d66ec:      ldr w11, [sp, #0xb8]
1008d66f0:      cbz w12, 0x1008d671c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x178>
1008d66f4:      cmp w12, #0x1
1008d66f8:      b.ne    0x1008d6740 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x19c>
1008d66fc:      ldr w10, [sp, #0xbc]
1008d6700:      b   0x1008d6744 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a0>
1008d6704:      add w10, w8, #0x2
1008d6708:      add w8, w9, #0x2
1008d670c:      mov x9, x10
1008d6710:      ldp w12, w10, [sp, #0xb0]
1008d6714:      ldr w11, [sp, #0xb8]
1008d6718:      cbnz    w12, 0x1008d66f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x150>
1008d671c:      add w12, w10, #0x2
1008d6720:      add w10, w11, #0x2
1008d6724:      mov x11, x12
1008d6728:      b   0x1008d6744 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a0>
1008d672c:      mov x9, x8
1008d6730:      ldp w12, w10, [sp, #0xb0]
1008d6734:      ldr w11, [sp, #0xb8]
1008d6738:      cbnz    w12, 0x1008d66f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x150>
1008d673c:      b   0x1008d671c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x178>
1008d6740:      mov x11, x10
1008d6744:      cmn w9, #0x3
1008d6748:      b.hi    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d674c:      add w9, w9, #0x2
1008d6750:      adds    w9, w11, w9
1008d6754:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6758:      mov x0, #0x0                ; =0
1008d675c:      adds    w21, w9, #0x1
1008d6760:      b.hs    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6764:      cmn w8, #0x3
1008d6768:      b.hi    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d676c:      mov x0, #0x0                ; =0
1008d6770:      add w8, w8, #0x2
1008d6774:      adds    w24, w10, w8
1008d6778:      b.hs    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d677c:      cmn w24, #0x1
1008d6780:      b.eq    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6784:      add w27, w24, #0x1
1008d6788:      cmp x19, #0x1
1008d678c:      b.ne    0x1008d67cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x228>
1008d6790:      ldr x8, [x25]
1008d6794:      cmn x8, #0x1
1008d6798:      b.eq    0x1008d6844 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2a0>
1008d679c:      mrs x9, TPIDRRO_EL0
1008d67a0:      and x9, x9, #0xfffffffffffffff8
1008d67a4:      ldr x8, [x9, x8, lsl #3]
1008d67a8:      cbz x8, 0x1008d6844 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2a0>
1008d67ac:      ldr x8, [x8, #0x19e8]
1008d67b0:      cbz x8, 0x1008d6c50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6ac>
1008d67b4:      ldr x9, [x8]
1008d67b8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008d67bc:      cmp x9, x10
1008d67c0:      b.hs    0x1008d6fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa3c>
1008d67c4:      ldr x22, [x8, #0x18]
1008d67c8:      b   0x1008d6870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
1008d67cc:      ldr x1, [x22, #0x10]
1008d67d0:      sub x0, x29, #0x90
1008d67d4:      bl  0x1008d738c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
1008d67d8:      ldur    w8, [x29, #-0x90]
1008d67dc:      cmn w8, #0x1
1008d67e0:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d67e4:      ldur    x8, [x29, #-0x70]
1008d67e8:      ldp q1, q0, [x29, #-0x90]
1008d67ec:      stur    q1, [sp, #0x38]
1008d67f0:      stur    q0, [sp, #0x48]
1008d67f4:      str x8, [sp, #0x58]
1008d67f8:      ldr x1, [x20, #0x18]
1008d67fc:      sub x0, x29, #0x90
1008d6800:      bl  0x1008d70c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1008d6804:      ldur    w8, [x29, #-0x90]
1008d6808:      cmn w8, #0x1
1008d680c:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6810:      add x23, sp, #0xb0
1008d6814:      ldur    x8, [x29, #-0x70]
1008d6818:      ldp q1, q0, [x29, #-0x90]
1008d681c:      stur    q1, [x23, #0x28]
1008d6820:      stur    q0, [x23, #0x38]
1008d6824:      str x8, [sp, #0xf8]
1008d6828:      ldp w10, w8, [sp, #0x38]
1008d682c:      ldr w9, [sp, #0x40]
1008d6830:      cbz w10, 0x1008d6c64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6c0>
1008d6834:      cmp w10, #0x1
1008d6838:      b.ne    0x1008d6c74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d0>
1008d683c:      ldr w8, [sp, #0x44]
1008d6840:      b   0x1008d6c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d4>
1008d6844:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008d6848:      add x0, x0, #0x8f8
1008d684c:      ldr x8, [x0]
1008d6850:      blr x8
1008d6854:      ldrb    w8, [x0, #0x20]
1008d6858:      cbnz    w8, 0x1008d6f94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9f0>
1008d685c:      ldr x8, [x0]
1008d6860:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008d6864:      cmp x8, x9
1008d6868:      b.hs    0x1008d6fc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa20>
1008d686c:      ldr x22, [x0, #0x18]
1008d6870:      stur    x22, [x29, #-0x68]
1008d6874:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008d6878:      stp x20, x8, [x29, #-0x88]
1008d687c:      mov w8, #0x1                ; =1
1008d6880:      stur    x8, [x29, #-0x90]
1008d6884:      sub x0, x29, #0x90
1008d6888:      bl  0x10089a0c8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1008d688c:      mov x24, x0
1008d6890:      stur    x0, [x29, #-0xc0]
1008d6894:      adrp    x0, 0x101128000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10perf_hooks13LOOP_START_MS0s_023___RUST_STD_INTERNAL_VAL>
1008d6898:      add x0, x0, #0x5e8
1008d689c:      ldr x8, [x0]
1008d68a0:      blr x8
1008d68a4:      strb    wzr, [x0]
1008d68a8:      mov x0, x20
1008d68ac:      bl  0x1002da440 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
1008d68b0:      tbz w0, #0x0, 0x1008d6940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x39c>
1008d68b4:      mov x0, x21
1008d68b8:      bl  0x1008a726c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
1008d68bc:      mov x20, x0
1008d68c0:      mov x23, x1
1008d68c4:      ldr x8, [x25]
1008d68c8:      cmn x8, #0x1
1008d68cc:      b.eq    0x1008d6984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3e0>
1008d68d0:      mrs x9, TPIDRRO_EL0
1008d68d4:      and x9, x9, #0xfffffffffffffff8
1008d68d8:      ldr x8, [x9, x8, lsl #3]
1008d68dc:      cbz x8, 0x1008d6984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3e0>
1008d68e0:      ldr x8, [x8, #0x19e8]
1008d68e4:      cbz x8, 0x1008d6d5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7b8>
1008d68e8:      ldr x9, [x8]
1008d68ec:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008d68f0:      cmp x9, x10
1008d68f4:      b.hs    0x1008d707c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xad8>
1008d68f8:      add x10, x9, #0x1
1008d68fc:      str x10, [x8]
1008d6900:      ldr x10, [x8, #0x18]
1008d6904:      cmp x24, x10
1008d6908:      b.hs    0x1008d6f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ec>
1008d690c:      ldr x10, [x8, #0x10]
1008d6910:      mov w11, #0x18              ; =24
1008d6914:      madd    x10, x24, x11, x10
1008d6918:      ldr x11, [x10]
1008d691c:      cmp x11, #0x1
1008d6920:      b.ne    0x1008d7088 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xae4>
1008d6924:      ldr x24, [x10, #0x8]
1008d6928:      str x9, [x8]
1008d692c:      ldr w28, [x24, #0x4]
1008d6930:      ldr w26, [x26]
1008d6934:      cmp w26, #0x300
1008d6938:      b.lo    0x1008d69f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x44c>
1008d693c:      b   0x1008d6de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
1008d6940:      ldr x8, [x25]
1008d6944:      cmn x8, #0x1
1008d6948:      b.eq    0x1008d6bbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x618>
1008d694c:      mrs x9, TPIDRRO_EL0
1008d6950:      and x9, x9, #0xfffffffffffffff8
1008d6954:      ldr x8, [x9, x8, lsl #3]
1008d6958:      cbz x8, 0x1008d6bbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x618>
1008d695c:      ldr x8, [x8, #0x19e8]
1008d6960:      cbz x8, 0x1008d6d84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7e0>
1008d6964:      ldr x9, [x8]
1008d6968:      cbnz    x9, 0x1008d6fec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa48>
1008d696c:      ldr x9, [x8, #0x18]
1008d6970:      cmp x22, x9
1008d6974:      b.hi    0x1008d697c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3d8>
1008d6978:      str x22, [x8, #0x18]
1008d697c:      str xzr, [x8]
1008d6980:      b   0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6984:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008d6988:      add x0, x0, #0x8f8
1008d698c:      ldr x8, [x0]
1008d6990:      blr x8
1008d6994:      ldrb    w8, [x0, #0x20]
1008d6998:      cbnz    w8, 0x1008d6ff8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa54>
1008d699c:      ldr x8, [x0]
1008d69a0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008d69a4:      cmp x8, x9
1008d69a8:      b.hs    0x1008d7030 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa8c>
1008d69ac:      add x9, x8, #0x1
1008d69b0:      str x9, [x0]
1008d69b4:      ldr x9, [x0, #0x18]
1008d69b8:      cmp x24, x9
1008d69bc:      b.hs    0x1008d6f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ec>
1008d69c0:      ldr x9, [x0, #0x10]
1008d69c4:      mov w10, #0x18              ; =24
1008d69c8:      madd    x9, x24, x10, x9
1008d69cc:      ldr x10, [x9]
1008d69d0:      cmp x10, #0x1
1008d69d4:      b.ne    0x1008d6fd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa2c>
1008d69d8:      ldr x24, [x9, #0x8]
1008d69dc:      str x8, [x0]
1008d69e0:      ldr w28, [x24, #0x4]
1008d69e4:      ldr w26, [x26]
1008d69e8:      cmp w26, #0x300
1008d69ec:      b.hs    0x1008d6de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
1008d69f0:      ldr x8, [x25]
1008d69f4:      cmn x8, #0x1
1008d69f8:      b.eq    0x1008d6dd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x834>
1008d69fc:      mrs x9, TPIDRRO_EL0
1008d6a00:      and x9, x9, #0xfffffffffffffff8
1008d6a04:      ldr x0, [x9, x8, lsl #3]
1008d6a08:      cbz x0, 0x1008d6dd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x834>
1008d6a0c:      add x8, x0, x26, lsl #3
1008d6a10:      ldr x0, [x8, #0x1e8]
1008d6a14:      cbz x0, 0x1008d6de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
1008d6a18:      str x22, [sp, #0x8]
1008d6a1c:      ldr x0, [x0]
1008d6a20:      cbz x0, 0x1008d6e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x85c>
1008d6a24:      ldr x8, [x0, #0x5190]
1008d6a28:      ubfx    x9, x28, #15, #15
1008d6a2c:      ubfx    x10, x28, #5, #10
1008d6a30:      and x11, x28, #0x1f
1008d6a34:      ldr x8, [x8, x9, lsl #3]
1008d6a38:      ldr x8, [x8, x10, lsl #3]
1008d6a3c:      lsl x9, x11, #5
1008d6a40:      ldr x26, [x8, x9]
1008d6a44:      stp w27, w21, [x20]
1008d6a48:      stp wzr, wzr, [x20, #0xc]
1008d6a4c:      str w21, [x20, #0x8]
1008d6a50:      mov w8, #0x7b               ; =123
1008d6a54:      mov x2, x23
1008d6a58:      strb    w8, [x2], #0x1
1008d6a5c:      ldr x1, [x26, #0x8]
1008d6a60:      add x21, sp, #0x10
1008d6a64:      add x0, sp, #0x10
1008d6a68:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6a6c:      add x8, x23, x0
1008d6a70:      mov w27, #0x3a              ; =58
1008d6a74:      strb    w27, [x8, #0x1]
1008d6a78:      add x22, x0, #0x2
1008d6a7c:      ldr x1, [x24, #0x10]
1008d6a80:      add x28, sp, #0xb0
1008d6a84:      add x0, sp, #0xb0
1008d6a88:      add x2, x23, x22
1008d6a8c:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6a90:      add x8, x0, x22
1008d6a94:      cmp x19, #0x1
1008d6a98:      b.eq    0x1008d6b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
1008d6a9c:      mov w9, #0x2c               ; =44
1008d6aa0:      strb    w9, [x23, x8]
1008d6aa4:      add x22, x8, #0x1
1008d6aa8:      ldr x1, [x26, #0x10]
1008d6aac:      add x0, x21, #0x28
1008d6ab0:      add x2, x23, x22
1008d6ab4:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6ab8:      add x8, x0, x22
1008d6abc:      strb    w27, [x23, x8]
1008d6ac0:      add x21, x8, #0x1
1008d6ac4:      ldr x1, [x24, #0x18]
1008d6ac8:      add x0, x28, #0x28
1008d6acc:      add x2, x23, x21
1008d6ad0:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6ad4:      add x8, x0, x21
1008d6ad8:      cmp x19, #0x2
1008d6adc:      b.eq    0x1008d6b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
1008d6ae0:      mov w9, #0x2c               ; =44
1008d6ae4:      strb    w9, [x23, x8]
1008d6ae8:      add x22, x8, #0x1
1008d6aec:      add x21, sp, #0x10
1008d6af0:      ldr x1, [x26, #0x18]
1008d6af4:      add x0, x21, #0x50
1008d6af8:      add x2, x23, x22
1008d6afc:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6b00:      add x8, x0, x22
1008d6b04:      mov w28, #0x3a              ; =58
1008d6b08:      strb    w28, [x23, x8]
1008d6b0c:      add x22, x8, #0x1
1008d6b10:      add x27, sp, #0xb0
1008d6b14:      ldr x1, [x24, #0x20]
1008d6b18:      add x0, x27, #0x50
1008d6b1c:      add x2, x23, x22
1008d6b20:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6b24:      add x8, x0, x22
1008d6b28:      cmp x19, #0x3
1008d6b2c:      b.eq    0x1008d6b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
1008d6b30:      mov w9, #0x2c               ; =44
1008d6b34:      strb    w9, [x23, x8]
1008d6b38:      add x19, x8, #0x1
1008d6b3c:      ldr x1, [x26, #0x20]
1008d6b40:      add x0, x21, #0x78
1008d6b44:      add x2, x23, x19
1008d6b48:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6b4c:      add x8, x0, x19
1008d6b50:      strb    w28, [x23, x8]
1008d6b54:      add x19, x8, #0x1
1008d6b58:      ldr x1, [x24, #0x28]
1008d6b5c:      add x0, x27, #0x78
1008d6b60:      add x2, x23, x19
1008d6b64:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008d6b68:      add x8, x0, x19
1008d6b6c:      ldr x10, [sp, #0x8]
1008d6b70:      mov w9, #0x7d               ; =125
1008d6b74:      strb    w9, [x23, x8]
1008d6b78:      ldr x8, [x25]
1008d6b7c:      cmn x8, #0x1
1008d6b80:      b.eq    0x1008d6bf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x64c>
1008d6b84:      mrs x9, TPIDRRO_EL0
1008d6b88:      and x9, x9, #0xfffffffffffffff8
1008d6b8c:      ldr x8, [x9, x8, lsl #3]
1008d6b90:      cbz x8, 0x1008d6bf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x64c>
1008d6b94:      ldr x8, [x8, #0x19e8]
1008d6b98:      cbz x8, 0x1008d6db8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x814>
1008d6b9c:      ldr x9, [x8]
1008d6ba0:      cbnz    x9, 0x1008d6fec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa48>
1008d6ba4:      ldr x9, [x8, #0x18]
1008d6ba8:      cmp x10, x9
1008d6bac:      b.hi    0x1008d6bb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x610>
1008d6bb0:      str x10, [x8, #0x18]
1008d6bb4:      str xzr, [x8]
1008d6bb8:      b   0x1008d6dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
1008d6bbc:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008d6bc0:      add x0, x0, #0x8f8
1008d6bc4:      ldr x8, [x0]
1008d6bc8:      blr x8
1008d6bcc:      ldrb    w8, [x0, #0x20]
1008d6bd0:      cbnz    w8, 0x1008d703c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa98>
1008d6bd4:      ldr x8, [x0]
1008d6bd8:      cbnz    x8, 0x1008d70bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
1008d6bdc:      ldr x8, [x0, #0x18]
1008d6be0:      cmp x22, x8
1008d6be4:      b.hi    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6be8:      str x22, [x0, #0x18]
1008d6bec:      b   0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6bf0:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008d6bf4:      add x0, x0, #0x8f8
1008d6bf8:      ldr x8, [x0]
1008d6bfc:      blr x8
1008d6c00:      ldrb    w8, [x0, #0x20]
1008d6c04:      cbnz    w8, 0x1008d7068 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xac4>
1008d6c08:      ldr x8, [x0]
1008d6c0c:      cbnz    x8, 0x1008d70bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
1008d6c10:      ldr x8, [x0, #0x18]
1008d6c14:      cmp x10, x8
1008d6c18:      b.hi    0x1008d6dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
1008d6c1c:      str x10, [x0, #0x18]
1008d6c20:      b   0x1008d6dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
1008d6c24:      bl  0x100c8a348 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008d6c28:      add x8, x0, x22, lsl #3
1008d6c2c:      ldr x0, [x8, #0x1e8]
1008d6c30:      cbnz    x0, 0x1008d6658 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb4>
1008d6c34:      adrp    x0, 0x1010c8000 <_anon.534fe2791366adc85564f9268bfdb267.1230+0x448>
1008d6c38:      add x0, x0, #0xd18
1008d6c3c:      bl  0x100c89a58 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008d6c40:      ldr x0, [x0]
1008d6c44:      cbnz    x0, 0x1008d6660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xbc>
1008d6c48:      bl  0x100cbd098 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008d6c4c:      b   0x1008d6660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xbc>
1008d6c50:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6c54:      add x0, x0, #0x600
1008d6c58:      bl  0x10013546c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1008d6c5c:      mov x22, x0
1008d6c60:      b   0x1008d6870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
1008d6c64:      add w10, w8, #0x2
1008d6c68:      add w8, w9, #0x2
1008d6c6c:      mov x9, x10
1008d6c70:      b   0x1008d6c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d4>
1008d6c74:      mov x9, x8
1008d6c78:      ldp w12, w10, [sp, #0xd8]
1008d6c7c:      ldr w11, [sp, #0xe0]
1008d6c80:      cbz w12, 0x1008d6c94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6f0>
1008d6c84:      cmp w12, #0x1
1008d6c88:      b.ne    0x1008d6ca4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x700>
1008d6c8c:      ldr w10, [sp, #0xe4]
1008d6c90:      b   0x1008d6ca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x704>
1008d6c94:      add w12, w10, #0x2
1008d6c98:      add w10, w11, #0x2
1008d6c9c:      mov x11, x12
1008d6ca0:      b   0x1008d6ca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x704>
1008d6ca4:      mov x11, x10
1008d6ca8:      adds    w9, w9, w21
1008d6cac:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6cb0:      adds    w9, w11, w9
1008d6cb4:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6cb8:      cmn w9, #0x3
1008d6cbc:      b.hi    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6cc0:      add w8, w8, w27
1008d6cc4:      cmp w8, w24
1008d6cc8:      b.ls    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6ccc:      mov x0, #0x0                ; =0
1008d6cd0:      adds    w8, w10, w8
1008d6cd4:      b.hs    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6cd8:      cmn w8, #0x3
1008d6cdc:      b.hi    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6ce0:      add w21, w9, #0x2
1008d6ce4:      add w27, w8, #0x2
1008d6ce8:      cmp x19, #0x2
1008d6cec:      b.eq    0x1008d6790 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
1008d6cf0:      ldr x1, [x22, #0x18]
1008d6cf4:      sub x0, x29, #0x90
1008d6cf8:      bl  0x1008d738c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
1008d6cfc:      ldur    w8, [x29, #-0x90]
1008d6d00:      cmn w8, #0x1
1008d6d04:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6d08:      ldur    x8, [x29, #-0x70]
1008d6d0c:      ldp q1, q0, [x29, #-0x90]
1008d6d10:      stp q1, q0, [sp, #0x60]
1008d6d14:      str x8, [sp, #0x80]
1008d6d18:      ldr x1, [x20, #0x20]
1008d6d1c:      sub x0, x29, #0x90
1008d6d20:      bl  0x1008d70c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1008d6d24:      ldur    w8, [x29, #-0x90]
1008d6d28:      cmn w8, #0x1
1008d6d2c:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6d30:      ldur    x8, [x29, #-0x70]
1008d6d34:      ldp q1, q0, [x29, #-0x90]
1008d6d38:      stp q1, q0, [sp, #0x100]
1008d6d3c:      str x8, [sp, #0x120]
1008d6d40:      ldp w10, w8, [sp, #0x60]
1008d6d44:      ldr w9, [sp, #0x68]
1008d6d48:      cbz w10, 0x1008d6e08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x864>
1008d6d4c:      cmp w10, #0x1
1008d6d50:      b.ne    0x1008d6e18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x874>
1008d6d54:      ldr w8, [sp, #0x6c]
1008d6d58:      b   0x1008d6e1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x878>
1008d6d5c:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6d60:      add x0, x0, #0x600
1008d6d64:      sub x1, x29, #0xc0
1008d6d68:      bl  0x100135290 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1008d6d6c:      mov x24, x0
1008d6d70:      ldr w28, [x0, #0x4]
1008d6d74:      ldr w26, [x26]
1008d6d78:      cmp w26, #0x300
1008d6d7c:      b.lo    0x1008d69f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x44c>
1008d6d80:      b   0x1008d6de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
1008d6d84:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6d88:      add x0, x0, #0x600
1008d6d8c:      sub x1, x29, #0x68
1008d6d90:      bl  0x100135848 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1008d6d94:      mov x0, #0x0                ; =0
1008d6d98:      add sp, sp, #0x1c0
1008d6d9c:      ldp x29, x30, [sp, #0x50]
1008d6da0:      ldp x20, x19, [sp, #0x40]
1008d6da4:      ldp x22, x21, [sp, #0x30]
1008d6da8:      ldp x24, x23, [sp, #0x20]
1008d6dac:      ldp x26, x25, [sp, #0x10]
1008d6db0:      ldp x28, x27, [sp], #0x60
1008d6db4:      ret
1008d6db8:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6dbc:      add x0, x0, #0x600
1008d6dc0:      sub x1, x29, #0x68
1008d6dc4:      bl  0x100135848 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1008d6dc8:      mov x1, #0x7fff000000000000 ; =9223090561878065152
1008d6dcc:      bfxil   x1, x20, #0, #48
1008d6dd0:      mov w0, #0x1                ; =1
1008d6dd4:      b   0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6dd8:      bl  0x100c8a348 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008d6ddc:      add x8, x0, x26, lsl #3
1008d6de0:      ldr x0, [x8, #0x1e8]
1008d6de4:      cbnz    x0, 0x1008d6a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x474>
1008d6de8:      adrp    x0, 0x1010c8000 <_anon.534fe2791366adc85564f9268bfdb267.1230+0x448>
1008d6dec:      add x0, x0, #0xd18
1008d6df0:      bl  0x100c89a58 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008d6df4:      str x22, [sp, #0x8]
1008d6df8:      ldr x0, [x0]
1008d6dfc:      cbnz    x0, 0x1008d6a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x480>
1008d6e00:      bl  0x100cbd098 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008d6e04:      b   0x1008d6a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x480>
1008d6e08:      add w10, w8, #0x2
1008d6e0c:      add w8, w9, #0x2
1008d6e10:      mov x9, x10
1008d6e14:      b   0x1008d6e1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x878>
1008d6e18:      mov x9, x8
1008d6e1c:      ldr w12, [sp, #0x100]
1008d6e20:      ldr w10, [sp, #0x104]
1008d6e24:      ldr w11, [sp, #0x108]
1008d6e28:      cbz w12, 0x1008d6e3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x898>
1008d6e2c:      cmp w12, #0x1
1008d6e30:      b.ne    0x1008d6e4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8a8>
1008d6e34:      ldr w10, [sp, #0x10c]
1008d6e38:      b   0x1008d6e50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8ac>
1008d6e3c:      add w12, w10, #0x2
1008d6e40:      add w10, w11, #0x2
1008d6e44:      mov x11, x12
1008d6e48:      b   0x1008d6e50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8ac>
1008d6e4c:      mov x11, x10
1008d6e50:      adds    w9, w9, w21
1008d6e54:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6e58:      adds    w9, w11, w9
1008d6e5c:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6e60:      cmn w9, #0x3
1008d6e64:      b.hi    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6e68:      adds    w8, w8, w27
1008d6e6c:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6e70:      mov x0, #0x0                ; =0
1008d6e74:      adds    w8, w10, w8
1008d6e78:      b.hs    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6e7c:      cmn w8, #0x3
1008d6e80:      b.hi    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6e84:      add w21, w9, #0x2
1008d6e88:      add w27, w8, #0x2
1008d6e8c:      cmp x19, #0x3
1008d6e90:      b.eq    0x1008d6790 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
1008d6e94:      ldr x1, [x22, #0x20]
1008d6e98:      sub x0, x29, #0x90
1008d6e9c:      bl  0x1008d738c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
1008d6ea0:      ldur    w8, [x29, #-0x90]
1008d6ea4:      cmn w8, #0x1
1008d6ea8:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6eac:      ldur    x8, [x29, #-0x70]
1008d6eb0:      ldp q1, q0, [x29, #-0x90]
1008d6eb4:      stur    q1, [sp, #0x88]
1008d6eb8:      stur    q0, [sp, #0x98]
1008d6ebc:      str x8, [sp, #0xa8]
1008d6ec0:      ldr x1, [x20, #0x28]
1008d6ec4:      sub x0, x29, #0x90
1008d6ec8:      bl  0x1008d70c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1008d6ecc:      ldur    w8, [x29, #-0x90]
1008d6ed0:      cmn w8, #0x1
1008d6ed4:      b.eq    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6ed8:      ldur    x8, [x29, #-0x70]
1008d6edc:      ldp q1, q0, [x29, #-0x90]
1008d6ee0:      stur    q1, [x23, #0x78]
1008d6ee4:      stur    q0, [x23, #0x88]
1008d6ee8:      str x8, [sp, #0x148]
1008d6eec:      ldp w10, w8, [sp, #0x88]
1008d6ef0:      ldr w9, [sp, #0x90]
1008d6ef4:      cbz w10, 0x1008d6f08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x964>
1008d6ef8:      cmp w10, #0x1
1008d6efc:      b.ne    0x1008d6f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x974>
1008d6f00:      ldr w8, [sp, #0x94]
1008d6f04:      b   0x1008d6f1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x978>
1008d6f08:      add w10, w8, #0x2
1008d6f0c:      add w8, w9, #0x2
1008d6f10:      mov x9, x10
1008d6f14:      b   0x1008d6f1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x978>
1008d6f18:      mov x9, x8
1008d6f1c:      ldr w12, [sp, #0x128]
1008d6f20:      ldr w10, [sp, #0x12c]
1008d6f24:      ldr w11, [sp, #0x130]
1008d6f28:      cbz w12, 0x1008d6f3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x998>
1008d6f2c:      cmp w12, #0x1
1008d6f30:      b.ne    0x1008d6f4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9a8>
1008d6f34:      ldr w10, [sp, #0x134]
1008d6f38:      b   0x1008d6f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ac>
1008d6f3c:      add w12, w10, #0x2
1008d6f40:      add w10, w11, #0x2
1008d6f44:      mov x11, x12
1008d6f48:      b   0x1008d6f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ac>
1008d6f4c:      mov x11, x10
1008d6f50:      adds    w9, w9, w21
1008d6f54:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6f58:      adds    w9, w11, w9
1008d6f5c:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6f60:      cmn w9, #0x3
1008d6f64:      b.hi    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6f68:      adds    w8, w8, w27
1008d6f6c:      b.hs    0x1008d6d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
1008d6f70:      mov x0, #0x0                ; =0
1008d6f74:      adds    w8, w10, w8
1008d6f78:      b.hs    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6f7c:      cmn w8, #0x3
1008d6f80:      b.hi    0x1008d6d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
1008d6f84:      add w21, w9, #0x2
1008d6f88:      add w27, w8, #0x2
1008d6f8c:      b   0x1008d6790 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
1008d6f90:      bl  0x100cb1470 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1008d6f94:      cmp w8, #0x1
1008d6f98:      b.ne    0x1008d7070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
1008d6f9c:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008d6fa0:      add x1, x1, #0xc0
1008d6fa4:      mov x22, x0
1008d6fa8:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008d6fac:      mov x0, x22
1008d6fb0:      strb    wzr, [x22, #0x20]
1008d6fb4:      ldr x8, [x22]
1008d6fb8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008d6fbc:      cmp x8, x9
1008d6fc0:      b.lo    0x1008d686c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2c8>
1008d6fc4:      adrp    x0, 0x101090000 <_anon.68a532d94142320e15103d7866c451bd.21>
1008d6fc8:      add x0, x0, #0x468
1008d6fcc:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008d6fd0:      adrp    x0, 0x100db1000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2b20>
1008d6fd4:      add x0, x0, #0x370
1008d6fd8:      mov w1, #0xb                ; =11
1008d6fdc:      bl  0x100cb1438 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008d6fe0:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6fe4:      add x0, x0, #0x790
1008d6fe8:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008d6fec:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6ff0:      add x0, x0, #0x888
1008d6ff4:      bl  0x100c83c6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008d6ff8:      cmp w8, #0x2
1008d6ffc:      b.eq    0x1008d7070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
1008d7000:      mov x28, x25
1008d7004:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008d7008:      add x1, x1, #0xc0
1008d700c:      mov x25, x0
1008d7010:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008d7014:      mov x0, x25
1008d7018:      strb    wzr, [x25, #0x20]
1008d701c:      mov x25, x28
1008d7020:      ldr x8, [x0]
1008d7024:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008d7028:      cmp x8, x9
1008d702c:      b.lo    0x1008d69ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x408>
1008d7030:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008d7034:      add x0, x0, #0xf70
1008d7038:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008d703c:      cmp w8, #0x2
1008d7040:      b.eq    0x1008d7070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
1008d7044:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008d7048:      add x1, x1, #0xc0
1008d704c:      mov x19, x0
1008d7050:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008d7054:      mov x0, x19
1008d7058:      strb    wzr, [x19, #0x20]
1008d705c:      ldr x8, [x19]
1008d7060:      cbz x8, 0x1008d6bdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x638>
1008d7064:      b   0x1008d70bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
1008d7068:      cmp w8, #0x2
1008d706c:      b.ne    0x1008d7098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf4>
1008d7070:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008d7074:      add x0, x0, #0xed8
1008d7078:      bl  0x100ccab5c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008d707c:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d7080:      add x0, x0, #0x700
1008d7084:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008d7088:      adrp    x0, 0x100e03000 <_anon.2faa2ae5fa73ebf7e6102d50cd6666c0.1847+0xc23>
1008d708c:      add x0, x0, #0x78f
1008d7090:      mov w1, #0xb                ; =11
1008d7094:      bl  0x100cb1438 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008d7098:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008d709c:      add x1, x1, #0xc0
1008d70a0:      mov x19, x0
1008d70a4:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008d70a8:      mov x0, x19
1008d70ac:      strb    wzr, [x19, #0x20]
1008d70b0:      ldr x10, [sp, #0x8]
1008d70b4:      ldr x8, [x19]
1008d70b8:      cbz x8, 0x1008d6c10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x66c>
1008d70bc:      adrp    x0, 0x101095000 <_anon.68a532d94142320e15103d7866c451bd.1142>
1008d70c0:      add x0, x0, #0x270
1008d70c4:      bl  0x100c83c6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
