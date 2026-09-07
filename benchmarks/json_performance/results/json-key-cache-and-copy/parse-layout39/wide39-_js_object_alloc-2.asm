/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081d7c0 <_js_object_alloc_with_parent>:
10081d7c0:      sub sp, sp, #0x90
10081d7c4:      stp x28, x27, [sp, #0x30]
10081d7c8:      stp x26, x25, [sp, #0x40]
10081d7cc:      stp x24, x23, [sp, #0x50]
10081d7d0:      stp x22, x21, [sp, #0x60]
10081d7d4:      stp x20, x19, [sp, #0x70]
10081d7d8:      stp x29, x30, [sp, #0x80]
10081d7dc:      add x29, sp, #0x80
10081d7e0:      mov x19, x2
10081d7e4:      mov x20, x1
10081d7e8:      mov x21, x0
10081d7ec:      cbz w1, 0x10081d7fc <_js_object_alloc_with_parent+0x3c>
10081d7f0:      mov x0, x21
10081d7f4:      mov x1, x20
10081d7f8:      bl  0x1006d1c30 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry13parent_static14register_class>
10081d7fc:      mov w8, #0x2                ; =2
10081d800:      cmp w19, #0x2
10081d804:      csel    w25, w19, w8, hi
10081d808:      ubfiz   x8, x25, #3, #32
10081d80c:      mov w9, #0x3ffd             ; =16381
10081d810:      cmp w19, w9
10081d814:      b.ls    0x10081d83c <_js_object_alloc_with_parent+0x7c>
10081d818:      add x0, x8, #0x10
10081d81c:      mov w1, #0x8                ; =8
10081d820:      mov w2, #0x2                ; =2
10081d824:      bl  0x1004da858 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081d828:      mov x23, x0
10081d82c:      ldurb   w8, [x0, #-0x7]
10081d830:      orr w8, w8, #0x20
10081d834:      sturb   w8, [x0, #-0x7]
10081d838:      b   0x10081d9ac <_js_object_alloc_with_parent+0x1ec>
10081d83c:      add x22, x8, #0x18
10081d840:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d844:      ldr x8, [x24, #0x48]
10081d848:      cmn x8, #0x1
10081d84c:      b.eq    0x10081db7c <_js_object_alloc_with_parent+0x3bc>
10081d850:      mrs x9, TPIDRRO_EL0
10081d854:      and x9, x9, #0xfffffffffffffff8
10081d858:      ldr x0, [x9, x8, lsl #3]
10081d85c:      cbz x0, 0x10081db7c <_js_object_alloc_with_parent+0x3bc>
10081d860:      ldr x8, [x0, #0x28]
10081d864:      ldrb    w8, [x8]
10081d868:      tbz w8, #0x0, 0x10081d8d4 <_js_object_alloc_with_parent+0x114>
10081d86c:      ldr x8, [x24, #0x48]
10081d870:      cmn x8, #0x1
10081d874:      b.eq    0x10081dbc4 <_js_object_alloc_with_parent+0x404>
10081d878:      mrs x9, TPIDRRO_EL0
10081d87c:      and x9, x9, #0xfffffffffffffff8
10081d880:      ldr x0, [x9, x8, lsl #3]
10081d884:      cbz x0, 0x10081dbc4 <_js_object_alloc_with_parent+0x404>
10081d888:      ldr x26, [x0, #0x20]
10081d88c:      ldr x8, [x26]
10081d890:      cbnz    x8, 0x10081dbd4 <_js_object_alloc_with_parent+0x414>
10081d894:      mov x8, #-0x1               ; =-1
10081d898:      str x8, [x26]
10081d89c:      ldr x1, [x26, #0x18]
10081d8a0:      cbz x1, 0x10081d8d0 <_js_object_alloc_with_parent+0x110>
10081d8a4:      mov x0, #0x0                ; =0
10081d8a8:      ldr x8, [x26, #0x10]
10081d8ac:      lsl x10, x1, #4
10081d8b0:      mov x9, x8
10081d8b4:      ldr x11, [x9, #0x8]
10081d8b8:      cmp x11, x22
10081d8bc:      b.eq    0x10081da64 <_js_object_alloc_with_parent+0x2a4>
10081d8c0:      add x9, x9, #0x10
10081d8c4:      add x0, x0, #0x1
10081d8c8:      subs    x10, x10, #0x10
10081d8cc:      b.ne    0x10081d8b4 <_js_object_alloc_with_parent+0xf4>
10081d8d0:      str xzr, [x26]
10081d8d4:      mov x0, x22
10081d8d8:      bl  0x1004da49c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081d8dc:      mov w8, #0x2                ; =2
10081d8e0:      strb    w8, [x0]
10081d8e4:      ldr x8, [x24, #0x48]
10081d8e8:      cmn x8, #0x1
10081d8ec:      b.eq    0x10081db90 <_js_object_alloc_with_parent+0x3d0>
10081d8f0:      mrs x9, TPIDRRO_EL0
10081d8f4:      and x9, x9, #0xfffffffffffffff8
10081d8f8:      ldr x8, [x9, x8, lsl #3]
10081d8fc:      cbz x8, 0x10081db90 <_js_object_alloc_with_parent+0x3d0>
10081d900:      ldr x8, [x8, #0x30]
10081d904:      ldrb    w8, [x8]
10081d908:      orr w8, w8, #0x2
10081d90c:      strb    w8, [x0, #0x1]
10081d910:      ldr x8, [x24, #0x48]
10081d914:      cmn x8, #0x1
10081d918:      b.eq    0x10081dba4 <_js_object_alloc_with_parent+0x3e4>
10081d91c:      mrs x9, TPIDRRO_EL0
10081d920:      and x9, x9, #0xfffffffffffffff8
10081d924:      ldr x8, [x9, x8, lsl #3]
10081d928:      cbz x8, 0x10081dba4 <_js_object_alloc_with_parent+0x3e4>
10081d92c:      ldr x8, [x8, #0x30]
10081d930:      ldrb    w8, [x8]
10081d934:      tbz w8, #0x0, 0x10081d9a0 <_js_object_alloc_with_parent+0x1e0>
10081d938:      ldrb    w8, [x0]
10081d93c:      sub w9, w8, #0x15
10081d940:      cmn w9, #0x14
10081d944:      b.lo    0x10081d9a0 <_js_object_alloc_with_parent+0x1e0>
10081d948:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081d94c:      add x9, x9, #0xb40
10081d950:      add x8, x9, x8, lsl #5
10081d954:      ldrb    w8, [x8, #0x1b]
10081d958:      tbnz    w8, #0x0, 0x10081d9a0 <_js_object_alloc_with_parent+0x1e0>
10081d95c:      mov x24, x0
10081d960:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d964:      add x0, x0, #0x658
10081d968:      ldr x8, [x0]
10081d96c:      blr x8
10081d970:      mov x23, x0
10081d974:      ldrb    w8, [x0, #0x18]
10081d978:      cbnz    w8, 0x10081dc08 <_js_object_alloc_with_parent+0x448>
10081d97c:      ldr x26, [x23, #0x10]
10081d980:      ldr x8, [x23]
10081d984:      cmp x26, x8
10081d988:      mov x0, x24
10081d98c:      b.eq    0x10081dc38 <_js_object_alloc_with_parent+0x478>
10081d990:      ldr x8, [x23, #0x8]
10081d994:      str x0, [x8, x26, lsl #3]
10081d998:      add x8, x26, #0x1
10081d99c:      str x8, [x23, #0x10]
10081d9a0:      strh    wzr, [x0, #0x2]
10081d9a4:      str w22, [x0, #0x4]
10081d9a8:      add x23, x0, #0x8
10081d9ac:      stp w21, w20, [x23]
10081d9b0:      lsl x2, x25, #3
10081d9b4:      str xzr, [x23, #0x8]
10081d9b8:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x58a8>
10081d9bc:      add x1, x1, #0xcf0
10081d9c0:      add x0, x23, #0x10
10081d9c4:      bl  0x100af9190 <_writev+0x100af9190>
10081d9c8:      lsr x8, x23, #3
10081d9cc:      cmp x8, #0x201
10081d9d0:      b.lo    0x10081da24 <_js_object_alloc_with_parent+0x264>
10081d9d4:      ldurb   w8, [x23, #-0x8]
10081d9d8:      sub w9, w8, #0x15
10081d9dc:      cmn w9, #0x14
10081d9e0:      b.lo    0x10081da24 <_js_object_alloc_with_parent+0x264>
10081d9e4:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081d9e8:      add x9, x9, #0xb40
10081d9ec:      add x8, x9, x8, lsl #5
10081d9f0:      ldrb    w8, [x8, #0x14]
10081d9f4:      mov w9, #0x16               ; =22
10081d9f8:      lsr w8, w9, w8
10081d9fc:      tbz w8, #0x0, 0x10081da24 <_js_object_alloc_with_parent+0x264>
10081da00:      ldurh   w8, [x23, #-0x6]
10081da04:      mov w9, #0x4000             ; =16384
10081da08:      bfxil   w9, w8, #0, #13
10081da0c:      sturh   w9, [x23, #-0x6]
10081da10:      mov x0, x23
10081da14:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081da18:      ldurh   w8, [x23, #-0x6]
10081da1c:      and w8, w8, #0xffffefff
10081da20:      sturh   w8, [x23, #-0x6]
10081da24:      mov w8, #0x2                ; =2
10081da28:      strb    w8, [sp, #0x2e]
10081da2c:      add x1, sp, #0x8
10081da30:      mov x0, x23
10081da34:      mov x2, #0x0                ; =0
10081da38:      mov x3, x19
10081da3c:      bl  0x10059896c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25publish_object_shape_from>
10081da40:      mov x0, x23
10081da44:      ldp x29, x30, [sp, #0x80]
10081da48:      ldp x20, x19, [sp, #0x70]
10081da4c:      ldp x22, x21, [sp, #0x60]
10081da50:      ldp x24, x23, [sp, #0x50]
10081da54:      ldp x26, x25, [sp, #0x40]
10081da58:      ldp x28, x27, [sp, #0x30]
10081da5c:      add sp, sp, #0x90
10081da60:      ret
10081da64:      cmp x0, x1
10081da68:      b.hs    0x10081dc04 <_js_object_alloc_with_parent+0x444>
10081da6c:      ldr x23, [x9]
10081da70:      subs    x10, x1, #0x1
10081da74:      ldr q0, [x8, x10, lsl #4]
10081da78:      str q0, [x9]
10081da7c:      str x10, [x26, #0x18]
10081da80:      b.ne    0x10081daa8 <_js_object_alloc_with_parent+0x2e8>
10081da84:      ldr x8, [x24, #0x48]
10081da88:      cmn x8, #0x1
10081da8c:      b.eq    0x10081dbfc <_js_object_alloc_with_parent+0x43c>
10081da90:      mrs x9, TPIDRRO_EL0
10081da94:      and x9, x9, #0xfffffffffffffff8
10081da98:      ldr x0, [x9, x8, lsl #3]
10081da9c:      cbz x0, 0x10081dbfc <_js_object_alloc_with_parent+0x43c>
10081daa0:      ldr x8, [x0, #0x28]
10081daa4:      strb    wzr, [x8]
10081daa8:      ldr x8, [x26]
10081daac:      add x8, x8, #0x1
10081dab0:      str x8, [x26]
10081dab4:      mov w8, #0x2                ; =2
10081dab8:      mov x26, x23
10081dabc:      strb    w8, [x26, #-0x8]!
10081dac0:      ldr x8, [x24, #0x48]
10081dac4:      cmn x8, #0x1
10081dac8:      b.eq    0x10081dbe0 <_js_object_alloc_with_parent+0x420>
10081dacc:      mrs x9, TPIDRRO_EL0
10081dad0:      and x9, x9, #0xfffffffffffffff8
10081dad4:      ldr x0, [x9, x8, lsl #3]
10081dad8:      cbz x0, 0x10081dbe0 <_js_object_alloc_with_parent+0x420>
10081dadc:      ldr x8, [x0, #0x30]
10081dae0:      ldrb    w8, [x8]
10081dae4:      orr w8, w8, #0x2
10081dae8:      sturb   w8, [x23, #-0x7]
10081daec:      ldr x8, [x24, #0x48]
10081daf0:      cmn x8, #0x1
10081daf4:      b.eq    0x10081dbe8 <_js_object_alloc_with_parent+0x428>
10081daf8:      mrs x9, TPIDRRO_EL0
10081dafc:      and x9, x9, #0xfffffffffffffff8
10081db00:      ldr x0, [x9, x8, lsl #3]
10081db04:      cbz x0, 0x10081dbe8 <_js_object_alloc_with_parent+0x428>
10081db08:      ldr x8, [x0, #0x30]
10081db0c:      ldrb    w8, [x8]
10081db10:      tbz w8, #0x0, 0x10081db70 <_js_object_alloc_with_parent+0x3b0>
10081db14:      ldrb    w8, [x26]
10081db18:      sub w9, w8, #0x15
10081db1c:      cmn w9, #0x14
10081db20:      b.lo    0x10081db70 <_js_object_alloc_with_parent+0x3b0>
10081db24:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081db28:      add x9, x9, #0xb40
10081db2c:      add x8, x9, x8, lsl #5
10081db30:      ldrb    w8, [x8, #0x1b]
10081db34:      tbnz    w8, #0x0, 0x10081db70 <_js_object_alloc_with_parent+0x3b0>
10081db38:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081db3c:      add x0, x0, #0x658
10081db40:      ldr x8, [x0]
10081db44:      blr x8
10081db48:      ldrb    w8, [x0, #0x18]
10081db4c:      cbnz    w8, 0x10081dc48 <_js_object_alloc_with_parent+0x488>
10081db50:      ldr x27, [x0, #0x10]
10081db54:      ldr x8, [x0]
10081db58:      cmp x27, x8
10081db5c:      b.eq    0x10081dc84 <_js_object_alloc_with_parent+0x4c4>
10081db60:      ldr x8, [x0, #0x8]
10081db64:      str x26, [x8, x27, lsl #3]
10081db68:      add x8, x27, #0x1
10081db6c:      str x8, [x0, #0x10]
10081db70:      sturh   wzr, [x23, #-0x6]
10081db74:      stur    w22, [x23, #-0x4]
10081db78:      b   0x10081d9ac <_js_object_alloc_with_parent+0x1ec>
10081db7c:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081db80:      ldr x8, [x0, #0x28]
10081db84:      ldrb    w8, [x8]
10081db88:      tbnz    w8, #0x0, 0x10081d86c <_js_object_alloc_with_parent+0xac>
10081db8c:      b   0x10081d8d4 <_js_object_alloc_with_parent+0x114>
10081db90:      mov x23, x0
10081db94:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081db98:      mov x8, x0
10081db9c:      mov x0, x23
10081dba0:      b   0x10081d900 <_js_object_alloc_with_parent+0x140>
10081dba4:      mov x23, x0
10081dba8:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081dbac:      mov x8, x0
10081dbb0:      mov x0, x23
10081dbb4:      ldr x8, [x8, #0x30]
10081dbb8:      ldrb    w8, [x8]
10081dbbc:      tbnz    w8, #0x0, 0x10081d938 <_js_object_alloc_with_parent+0x178>
10081dbc0:      b   0x10081d9a0 <_js_object_alloc_with_parent+0x1e0>
10081dbc4:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081dbc8:      ldr x26, [x0, #0x20]
10081dbcc:      ldr x8, [x26]
10081dbd0:      cbz x8, 0x10081d894 <_js_object_alloc_with_parent+0xd4>
10081dbd4:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
10081dbd8:      add x0, x0, #0x150
10081dbdc:      bl  0x100ab14ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081dbe0:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081dbe4:      b   0x10081dadc <_js_object_alloc_with_parent+0x31c>
10081dbe8:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081dbec:      ldr x8, [x0, #0x30]
10081dbf0:      ldrb    w8, [x8]
10081dbf4:      tbnz    w8, #0x0, 0x10081db14 <_js_object_alloc_with_parent+0x354>
10081dbf8:      b   0x10081db70 <_js_object_alloc_with_parent+0x3b0>
10081dbfc:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081dc00:      b   0x10081daa0 <_js_object_alloc_with_parent+0x2e0>
10081dc04:      bl  0x100ab11bc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081dc08:      cmp w8, #0x1
10081dc0c:      b.ne    0x10081dc50 <_js_object_alloc_with_parent+0x490>
10081dc10:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081dc14:      add x1, x1, #0xbb8
10081dc18:      mov x0, x23
10081dc1c:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081dc20:      strb    wzr, [x23, #0x18]
10081dc24:      ldr x26, [x23, #0x10]
10081dc28:      ldr x8, [x23]
10081dc2c:      cmp x26, x8
10081dc30:      mov x0, x24
10081dc34:      b.ne    0x10081d990 <_js_object_alloc_with_parent+0x1d0>
10081dc38:      mov x0, x23
10081dc3c:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081dc40:      mov x0, x24
10081dc44:      b   0x10081d990 <_js_object_alloc_with_parent+0x1d0>
10081dc48:      cmp w8, #0x2
10081dc4c:      b.ne    0x10081dc5c <_js_object_alloc_with_parent+0x49c>
10081dc50:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081dc54:      add x0, x0, #0x850
10081dc58:      bl  0x100aef91c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081dc5c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081dc60:      add x1, x1, #0xbb8
10081dc64:      mov x24, x0
10081dc68:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081dc6c:      mov x0, x24
10081dc70:      strb    wzr, [x24, #0x18]
10081dc74:      ldr x27, [x24, #0x10]
10081dc78:      ldr x8, [x24]
10081dc7c:      cmp x27, x8
10081dc80:      b.ne    0x10081db60 <_js_object_alloc_with_parent+0x3a0>
10081dc84:      mov x24, x0
10081dc88:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081dc8c:      mov x0, x24
10081dc90:      b   0x10081db60 <_js_object_alloc_with_parent+0x3a0>
10081dc94:      nop
10081dc98:      nop
10081dc9c:      nop
10081dca0:      nop
10081dca4:      nop
10081dca8:      nop
10081dcac:      nop
10081dcb0:      nop
10081dcb4:      nop
10081dcb8:      nop
10081dcbc:      nop
