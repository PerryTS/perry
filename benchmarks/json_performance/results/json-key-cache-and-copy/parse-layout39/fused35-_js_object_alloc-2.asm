/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081d5c0 <_js_object_alloc_with_parent>:
10081d5c0:      sub sp, sp, #0x90
10081d5c4:      stp x28, x27, [sp, #0x30]
10081d5c8:      stp x26, x25, [sp, #0x40]
10081d5cc:      stp x24, x23, [sp, #0x50]
10081d5d0:      stp x22, x21, [sp, #0x60]
10081d5d4:      stp x20, x19, [sp, #0x70]
10081d5d8:      stp x29, x30, [sp, #0x80]
10081d5dc:      add x29, sp, #0x80
10081d5e0:      mov x19, x2
10081d5e4:      mov x20, x1
10081d5e8:      mov x21, x0
10081d5ec:      cbz w1, 0x10081d5fc <_js_object_alloc_with_parent+0x3c>
10081d5f0:      mov x0, x21
10081d5f4:      mov x1, x20
10081d5f8:      bl  0x1006d1a30 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry13parent_static14register_class>
10081d5fc:      mov w8, #0x2                ; =2
10081d600:      cmp w19, #0x2
10081d604:      csel    w25, w19, w8, hi
10081d608:      ubfiz   x8, x25, #3, #32
10081d60c:      mov w9, #0x3ffd             ; =16381
10081d610:      cmp w19, w9
10081d614:      b.ls    0x10081d63c <_js_object_alloc_with_parent+0x7c>
10081d618:      add x0, x8, #0x10
10081d61c:      mov w1, #0x8                ; =8
10081d620:      mov w2, #0x2                ; =2
10081d624:      bl  0x1004da658 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081d628:      mov x23, x0
10081d62c:      ldurb   w8, [x0, #-0x7]
10081d630:      orr w8, w8, #0x20
10081d634:      sturb   w8, [x0, #-0x7]
10081d638:      b   0x10081d7ac <_js_object_alloc_with_parent+0x1ec>
10081d63c:      add x22, x8, #0x18
10081d640:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d644:      ldr x8, [x24, #0x48]
10081d648:      cmn x8, #0x1
10081d64c:      b.eq    0x10081d97c <_js_object_alloc_with_parent+0x3bc>
10081d650:      mrs x9, TPIDRRO_EL0
10081d654:      and x9, x9, #0xfffffffffffffff8
10081d658:      ldr x0, [x9, x8, lsl #3]
10081d65c:      cbz x0, 0x10081d97c <_js_object_alloc_with_parent+0x3bc>
10081d660:      ldr x8, [x0, #0x28]
10081d664:      ldrb    w8, [x8]
10081d668:      tbz w8, #0x0, 0x10081d6d4 <_js_object_alloc_with_parent+0x114>
10081d66c:      ldr x8, [x24, #0x48]
10081d670:      cmn x8, #0x1
10081d674:      b.eq    0x10081d9c4 <_js_object_alloc_with_parent+0x404>
10081d678:      mrs x9, TPIDRRO_EL0
10081d67c:      and x9, x9, #0xfffffffffffffff8
10081d680:      ldr x0, [x9, x8, lsl #3]
10081d684:      cbz x0, 0x10081d9c4 <_js_object_alloc_with_parent+0x404>
10081d688:      ldr x26, [x0, #0x20]
10081d68c:      ldr x8, [x26]
10081d690:      cbnz    x8, 0x10081d9d4 <_js_object_alloc_with_parent+0x414>
10081d694:      mov x8, #-0x1               ; =-1
10081d698:      str x8, [x26]
10081d69c:      ldr x1, [x26, #0x18]
10081d6a0:      cbz x1, 0x10081d6d0 <_js_object_alloc_with_parent+0x110>
10081d6a4:      mov x0, #0x0                ; =0
10081d6a8:      ldr x8, [x26, #0x10]
10081d6ac:      lsl x10, x1, #4
10081d6b0:      mov x9, x8
10081d6b4:      ldr x11, [x9, #0x8]
10081d6b8:      cmp x11, x22
10081d6bc:      b.eq    0x10081d864 <_js_object_alloc_with_parent+0x2a4>
10081d6c0:      add x9, x9, #0x10
10081d6c4:      add x0, x0, #0x1
10081d6c8:      subs    x10, x10, #0x10
10081d6cc:      b.ne    0x10081d6b4 <_js_object_alloc_with_parent+0xf4>
10081d6d0:      str xzr, [x26]
10081d6d4:      mov x0, x22
10081d6d8:      bl  0x1004da29c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081d6dc:      mov w8, #0x2                ; =2
10081d6e0:      strb    w8, [x0]
10081d6e4:      ldr x8, [x24, #0x48]
10081d6e8:      cmn x8, #0x1
10081d6ec:      b.eq    0x10081d990 <_js_object_alloc_with_parent+0x3d0>
10081d6f0:      mrs x9, TPIDRRO_EL0
10081d6f4:      and x9, x9, #0xfffffffffffffff8
10081d6f8:      ldr x8, [x9, x8, lsl #3]
10081d6fc:      cbz x8, 0x10081d990 <_js_object_alloc_with_parent+0x3d0>
10081d700:      ldr x8, [x8, #0x30]
10081d704:      ldrb    w8, [x8]
10081d708:      orr w8, w8, #0x2
10081d70c:      strb    w8, [x0, #0x1]
10081d710:      ldr x8, [x24, #0x48]
10081d714:      cmn x8, #0x1
10081d718:      b.eq    0x10081d9a4 <_js_object_alloc_with_parent+0x3e4>
10081d71c:      mrs x9, TPIDRRO_EL0
10081d720:      and x9, x9, #0xfffffffffffffff8
10081d724:      ldr x8, [x9, x8, lsl #3]
10081d728:      cbz x8, 0x10081d9a4 <_js_object_alloc_with_parent+0x3e4>
10081d72c:      ldr x8, [x8, #0x30]
10081d730:      ldrb    w8, [x8]
10081d734:      tbz w8, #0x0, 0x10081d7a0 <_js_object_alloc_with_parent+0x1e0>
10081d738:      ldrb    w8, [x0]
10081d73c:      sub w9, w8, #0x15
10081d740:      cmn w9, #0x14
10081d744:      b.lo    0x10081d7a0 <_js_object_alloc_with_parent+0x1e0>
10081d748:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d74c:      add x9, x9, #0xb10
10081d750:      add x8, x9, x8, lsl #5
10081d754:      ldrb    w8, [x8, #0x1b]
10081d758:      tbnz    w8, #0x0, 0x10081d7a0 <_js_object_alloc_with_parent+0x1e0>
10081d75c:      mov x24, x0
10081d760:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d764:      add x0, x0, #0x658
10081d768:      ldr x8, [x0]
10081d76c:      blr x8
10081d770:      mov x23, x0
10081d774:      ldrb    w8, [x0, #0x18]
10081d778:      cbnz    w8, 0x10081da08 <_js_object_alloc_with_parent+0x448>
10081d77c:      ldr x26, [x23, #0x10]
10081d780:      ldr x8, [x23]
10081d784:      cmp x26, x8
10081d788:      mov x0, x24
10081d78c:      b.eq    0x10081da38 <_js_object_alloc_with_parent+0x478>
10081d790:      ldr x8, [x23, #0x8]
10081d794:      str x0, [x8, x26, lsl #3]
10081d798:      add x8, x26, #0x1
10081d79c:      str x8, [x23, #0x10]
10081d7a0:      strh    wzr, [x0, #0x2]
10081d7a4:      str w22, [x0, #0x4]
10081d7a8:      add x23, x0, #0x8
10081d7ac:      stp w21, w20, [x23]
10081d7b0:      lsl x2, x25, #3
10081d7b4:      str xzr, [x23, #0x8]
10081d7b8:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5aa8>
10081d7bc:      add x1, x1, #0xaf0
10081d7c0:      add x0, x23, #0x10
10081d7c4:      bl  0x100af8f90 <_writev+0x100af8f90>
10081d7c8:      lsr x8, x23, #3
10081d7cc:      cmp x8, #0x201
10081d7d0:      b.lo    0x10081d824 <_js_object_alloc_with_parent+0x264>
10081d7d4:      ldurb   w8, [x23, #-0x8]
10081d7d8:      sub w9, w8, #0x15
10081d7dc:      cmn w9, #0x14
10081d7e0:      b.lo    0x10081d824 <_js_object_alloc_with_parent+0x264>
10081d7e4:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d7e8:      add x9, x9, #0xb10
10081d7ec:      add x8, x9, x8, lsl #5
10081d7f0:      ldrb    w8, [x8, #0x14]
10081d7f4:      mov w9, #0x16               ; =22
10081d7f8:      lsr w8, w9, w8
10081d7fc:      tbz w8, #0x0, 0x10081d824 <_js_object_alloc_with_parent+0x264>
10081d800:      ldurh   w8, [x23, #-0x6]
10081d804:      mov w9, #0x4000             ; =16384
10081d808:      bfxil   w9, w8, #0, #13
10081d80c:      sturh   w9, [x23, #-0x6]
10081d810:      mov x0, x23
10081d814:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081d818:      ldurh   w8, [x23, #-0x6]
10081d81c:      and w8, w8, #0xffffefff
10081d820:      sturh   w8, [x23, #-0x6]
10081d824:      mov w8, #0x2                ; =2
10081d828:      strb    w8, [sp, #0x2e]
10081d82c:      add x1, sp, #0x8
10081d830:      mov x0, x23
10081d834:      mov x2, #0x0                ; =0
10081d838:      mov x3, x19
10081d83c:      bl  0x10059876c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25publish_object_shape_from>
10081d840:      mov x0, x23
10081d844:      ldp x29, x30, [sp, #0x80]
10081d848:      ldp x20, x19, [sp, #0x70]
10081d84c:      ldp x22, x21, [sp, #0x60]
10081d850:      ldp x24, x23, [sp, #0x50]
10081d854:      ldp x26, x25, [sp, #0x40]
10081d858:      ldp x28, x27, [sp, #0x30]
10081d85c:      add sp, sp, #0x90
10081d860:      ret
10081d864:      cmp x0, x1
10081d868:      b.hs    0x10081da04 <_js_object_alloc_with_parent+0x444>
10081d86c:      ldr x23, [x9]
10081d870:      subs    x10, x1, #0x1
10081d874:      ldr q0, [x8, x10, lsl #4]
10081d878:      str q0, [x9]
10081d87c:      str x10, [x26, #0x18]
10081d880:      b.ne    0x10081d8a8 <_js_object_alloc_with_parent+0x2e8>
10081d884:      ldr x8, [x24, #0x48]
10081d888:      cmn x8, #0x1
10081d88c:      b.eq    0x10081d9fc <_js_object_alloc_with_parent+0x43c>
10081d890:      mrs x9, TPIDRRO_EL0
10081d894:      and x9, x9, #0xfffffffffffffff8
10081d898:      ldr x0, [x9, x8, lsl #3]
10081d89c:      cbz x0, 0x10081d9fc <_js_object_alloc_with_parent+0x43c>
10081d8a0:      ldr x8, [x0, #0x28]
10081d8a4:      strb    wzr, [x8]
10081d8a8:      ldr x8, [x26]
10081d8ac:      add x8, x8, #0x1
10081d8b0:      str x8, [x26]
10081d8b4:      mov w8, #0x2                ; =2
10081d8b8:      mov x26, x23
10081d8bc:      strb    w8, [x26, #-0x8]!
10081d8c0:      ldr x8, [x24, #0x48]
10081d8c4:      cmn x8, #0x1
10081d8c8:      b.eq    0x10081d9e0 <_js_object_alloc_with_parent+0x420>
10081d8cc:      mrs x9, TPIDRRO_EL0
10081d8d0:      and x9, x9, #0xfffffffffffffff8
10081d8d4:      ldr x0, [x9, x8, lsl #3]
10081d8d8:      cbz x0, 0x10081d9e0 <_js_object_alloc_with_parent+0x420>
10081d8dc:      ldr x8, [x0, #0x30]
10081d8e0:      ldrb    w8, [x8]
10081d8e4:      orr w8, w8, #0x2
10081d8e8:      sturb   w8, [x23, #-0x7]
10081d8ec:      ldr x8, [x24, #0x48]
10081d8f0:      cmn x8, #0x1
10081d8f4:      b.eq    0x10081d9e8 <_js_object_alloc_with_parent+0x428>
10081d8f8:      mrs x9, TPIDRRO_EL0
10081d8fc:      and x9, x9, #0xfffffffffffffff8
10081d900:      ldr x0, [x9, x8, lsl #3]
10081d904:      cbz x0, 0x10081d9e8 <_js_object_alloc_with_parent+0x428>
10081d908:      ldr x8, [x0, #0x30]
10081d90c:      ldrb    w8, [x8]
10081d910:      tbz w8, #0x0, 0x10081d970 <_js_object_alloc_with_parent+0x3b0>
10081d914:      ldrb    w8, [x26]
10081d918:      sub w9, w8, #0x15
10081d91c:      cmn w9, #0x14
10081d920:      b.lo    0x10081d970 <_js_object_alloc_with_parent+0x3b0>
10081d924:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d928:      add x9, x9, #0xb10
10081d92c:      add x8, x9, x8, lsl #5
10081d930:      ldrb    w8, [x8, #0x1b]
10081d934:      tbnz    w8, #0x0, 0x10081d970 <_js_object_alloc_with_parent+0x3b0>
10081d938:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d93c:      add x0, x0, #0x658
10081d940:      ldr x8, [x0]
10081d944:      blr x8
10081d948:      ldrb    w8, [x0, #0x18]
10081d94c:      cbnz    w8, 0x10081da48 <_js_object_alloc_with_parent+0x488>
10081d950:      ldr x27, [x0, #0x10]
10081d954:      ldr x8, [x0]
10081d958:      cmp x27, x8
10081d95c:      b.eq    0x10081da84 <_js_object_alloc_with_parent+0x4c4>
10081d960:      ldr x8, [x0, #0x8]
10081d964:      str x26, [x8, x27, lsl #3]
10081d968:      add x8, x27, #0x1
10081d96c:      str x8, [x0, #0x10]
10081d970:      sturh   wzr, [x23, #-0x6]
10081d974:      stur    w22, [x23, #-0x4]
10081d978:      b   0x10081d7ac <_js_object_alloc_with_parent+0x1ec>
10081d97c:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d980:      ldr x8, [x0, #0x28]
10081d984:      ldrb    w8, [x8]
10081d988:      tbnz    w8, #0x0, 0x10081d66c <_js_object_alloc_with_parent+0xac>
10081d98c:      b   0x10081d6d4 <_js_object_alloc_with_parent+0x114>
10081d990:      mov x23, x0
10081d994:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d998:      mov x8, x0
10081d99c:      mov x0, x23
10081d9a0:      b   0x10081d700 <_js_object_alloc_with_parent+0x140>
10081d9a4:      mov x23, x0
10081d9a8:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d9ac:      mov x8, x0
10081d9b0:      mov x0, x23
10081d9b4:      ldr x8, [x8, #0x30]
10081d9b8:      ldrb    w8, [x8]
10081d9bc:      tbnz    w8, #0x0, 0x10081d738 <_js_object_alloc_with_parent+0x178>
10081d9c0:      b   0x10081d7a0 <_js_object_alloc_with_parent+0x1e0>
10081d9c4:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d9c8:      ldr x26, [x0, #0x20]
10081d9cc:      ldr x8, [x26]
10081d9d0:      cbz x8, 0x10081d694 <_js_object_alloc_with_parent+0xd4>
10081d9d4:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081d9d8:      add x0, x0, #0x120
10081d9dc:      bl  0x100ab12ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081d9e0:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d9e4:      b   0x10081d8dc <_js_object_alloc_with_parent+0x31c>
10081d9e8:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d9ec:      ldr x8, [x0, #0x30]
10081d9f0:      ldrb    w8, [x8]
10081d9f4:      tbnz    w8, #0x0, 0x10081d914 <_js_object_alloc_with_parent+0x354>
10081d9f8:      b   0x10081d970 <_js_object_alloc_with_parent+0x3b0>
10081d9fc:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081da00:      b   0x10081d8a0 <_js_object_alloc_with_parent+0x2e0>
10081da04:      bl  0x100ab0fbc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081da08:      cmp w8, #0x1
10081da0c:      b.ne    0x10081da50 <_js_object_alloc_with_parent+0x490>
10081da10:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081da14:      add x1, x1, #0xbb8
10081da18:      mov x0, x23
10081da1c:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081da20:      strb    wzr, [x23, #0x18]
10081da24:      ldr x26, [x23, #0x10]
10081da28:      ldr x8, [x23]
10081da2c:      cmp x26, x8
10081da30:      mov x0, x24
10081da34:      b.ne    0x10081d790 <_js_object_alloc_with_parent+0x1d0>
10081da38:      mov x0, x23
10081da3c:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081da40:      mov x0, x24
10081da44:      b   0x10081d790 <_js_object_alloc_with_parent+0x1d0>
10081da48:      cmp w8, #0x2
10081da4c:      b.ne    0x10081da5c <_js_object_alloc_with_parent+0x49c>
10081da50:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081da54:      add x0, x0, #0x850
10081da58:      bl  0x100aef71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081da5c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081da60:      add x1, x1, #0xbb8
10081da64:      mov x24, x0
10081da68:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081da6c:      mov x0, x24
10081da70:      strb    wzr, [x24, #0x18]
10081da74:      ldr x27, [x24, #0x10]
10081da78:      ldr x8, [x24]
10081da7c:      cmp x27, x8
10081da80:      b.ne    0x10081d960 <_js_object_alloc_with_parent+0x3a0>
10081da84:      mov x24, x0
10081da88:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081da8c:      mov x0, x24
10081da90:      b   0x10081d960 <_js_object_alloc_with_parent+0x3a0>
10081da94:      nop
10081da98:      nop
10081da9c:      nop
10081daa0:      nop
10081daa4:      nop
10081daa8:      nop
10081daac:      nop
10081dab0:      nop
10081dab4:      nop
10081dab8:      nop
10081dabc:      nop
