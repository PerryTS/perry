/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081b7c0 <_js_object_alloc_class_dynamic_parent>:
10081b7c0:      sub sp, sp, #0xa0
10081b7c4:      stp x28, x27, [sp, #0x40]
10081b7c8:      stp x26, x25, [sp, #0x50]
10081b7cc:      stp x24, x23, [sp, #0x60]
10081b7d0:      stp x22, x21, [sp, #0x70]
10081b7d4:      stp x20, x19, [sp, #0x80]
10081b7d8:      stp x29, x30, [sp, #0x90]
10081b7dc:      add x29, sp, #0x90
10081b7e0:      mov x27, x3
10081b7e4:      mov x22, x2
10081b7e8:      mov x21, x1
10081b7ec:      mov x19, x0
10081b7f0:      bl  0x10057d920 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object19class_meta_registry19get_parent_class_id>
10081b7f4:      mov x20, x1
10081b7f8:      mov w1, #0x0                ; =0
10081b7fc:      cmp w0, #0x1
10081b800:      b.ne    0x10081b8a8 <_js_object_alloc_class_dynamic_parent+0xe8>
10081b804:      cbz w20, 0x10081b8a8 <_js_object_alloc_class_dynamic_parent+0xe8>
10081b808:      add x0, sp, #0x10
10081b80c:      mov x1, x20
10081b810:      bl  0x100591c98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc27registered_class_keys_array>
10081b814:      ldr w8, [sp, #0x10]
10081b818:      tbz w8, #0x0, 0x10081b8a4 <_js_object_alloc_class_dynamic_parent+0xe4>
10081b81c:      ldr x26, [sp, #0x18]
10081b820:      ldr w24, [x26]
10081b824:      mov w21, #0x2717            ; =10007
10081b828:      mov w23, #0x8480            ; =33920
10081b82c:      movk    w23, #0x1e, lsl #16
10081b830:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081b834:      ldr w25, [x8, #0x634]
10081b838:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081b83c:      cmp w25, #0x300
10081b840:      b.hs    0x10081b8e8 <_js_object_alloc_class_dynamic_parent+0x128>
10081b844:      ldr x8, [x28, #0x48]
10081b848:      cmn x8, #0x1
10081b84c:      b.eq    0x10081b8d8 <_js_object_alloc_class_dynamic_parent+0x118>
10081b850:      mrs x9, TPIDRRO_EL0
10081b854:      and x9, x9, #0xfffffffffffffff8
10081b858:      ldr x0, [x9, x8, lsl #3]
10081b85c:      cbz x0, 0x10081b8d8 <_js_object_alloc_class_dynamic_parent+0x118>
10081b860:      add x8, x0, x25, lsl #3
10081b864:      ldr x0, [x8, #0x1e8]
10081b868:      cbz x0, 0x10081b8e8 <_js_object_alloc_class_dynamic_parent+0x128>
10081b86c:      madd    w23, w19, w21, w23
10081b870:      ldr x0, [x0]
10081b874:      cbz x0, 0x10081b900 <_js_object_alloc_class_dynamic_parent+0x140>
10081b878:      and w8, w23, #0xff
10081b87c:      add x8, x0, w8, uxtw #4
10081b880:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081b884:      ldr w9, [x8]
10081b888:      cmp w9, w23
10081b88c:      b.ne    0x10081b91c <_js_object_alloc_class_dynamic_parent+0x15c>
10081b890:      ldr x21, [x8, #0x8]
10081b894:      ldr w25, [x8, #0x4]
10081b898:      cbz x21, 0x10081b9f8 <_js_object_alloc_class_dynamic_parent+0x238>
10081b89c:      ldr w22, [x21]
10081b8a0:      b   0x10081bd48 <_js_object_alloc_class_dynamic_parent+0x588>
10081b8a4:      mov x1, x20
10081b8a8:      mov x0, x19
10081b8ac:      mov x2, x21
10081b8b0:      mov x3, x22
10081b8b4:      mov x4, x27
10081b8b8:      ldp x29, x30, [sp, #0x90]
10081b8bc:      ldp x20, x19, [sp, #0x80]
10081b8c0:      ldp x22, x21, [sp, #0x70]
10081b8c4:      ldp x24, x23, [sp, #0x60]
10081b8c8:      ldp x26, x25, [sp, #0x50]
10081b8cc:      ldp x28, x27, [sp, #0x40]
10081b8d0:      add sp, sp, #0xa0
10081b8d4:      b   0x10081c380 <_js_object_alloc_class_with_keys>
10081b8d8:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081b8dc:      add x8, x0, x25, lsl #3
10081b8e0:      ldr x0, [x8, #0x1e8]
10081b8e4:      cbnz    x0, 0x10081b86c <_js_object_alloc_class_dynamic_parent+0xac>
10081b8e8:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081b8ec:      add x0, x0, #0x330
10081b8f0:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081b8f4:      madd    w23, w19, w21, w23
10081b8f8:      ldr x0, [x0]
10081b8fc:      cbnz    x0, 0x10081b878 <_js_object_alloc_class_dynamic_parent+0xb8>
10081b900:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081b904:      and w8, w23, #0xff
10081b908:      add x8, x0, w8, uxtw #4
10081b90c:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081b910:      ldr w9, [x8]
10081b914:      cmp w9, w23
10081b918:      b.eq    0x10081b890 <_js_object_alloc_class_dynamic_parent+0xd0>
10081b91c:      ldr x8, [x0, #0x5088]
10081b920:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081b924:      cmp x8, x9
10081b928:      b.hs    0x10081c2a4 <_js_object_alloc_class_dynamic_parent+0xae4>
10081b92c:      add x9, x8, #0x1
10081b930:      str x9, [x0, #0x5088]
10081b934:      ldr x9, [x0, #0x50a8]
10081b938:      cbz x9, 0x10081b9e8 <_js_object_alloc_class_dynamic_parent+0x228>
10081b93c:      mov x9, #0x0                ; =0
10081b940:      mov x10, #0x7c15            ; =31765
10081b944:      movk    x10, #0x7f4a, lsl #16
10081b948:      movk    x10, #0x79b9, lsl #32
10081b94c:      movk    x10, #0x9e37, lsl #48
10081b950:      mul x10, x23, x10
10081b954:      eor x13, x10, x10, lsr #32
10081b958:      lsr x12, x10, #57
10081b95c:      ldr x10, [x0, #0x5098]
10081b960:      ldr x11, [x0, #0x5090]
10081b964:      dup.8b  v0, w12
10081b968:      movi.2d v1, #0xffffffffffffffff
10081b96c:      mov w12, #0x18              ; =24
10081b970:      and x13, x13, x10
10081b974:      ldr d2, [x11, x13]
10081b978:      cmeq.8b v3, v2, v0
10081b97c:      fmov    x14, d3
10081b980:      ands    x14, x14, #0x8080808080808080
10081b984:      b.eq    0x10081b9b8 <_js_object_alloc_class_dynamic_parent+0x1f8>
10081b988:      rbit    x15, x14
10081b98c:      clz x15, x15
10081b990:      add x15, x13, x15, lsr #3
10081b994:      and x15, x15, x10
10081b998:      mneg    x15, x15, x12
10081b99c:      add x15, x11, x15
10081b9a0:      ldur    w16, [x15, #-0x18]
10081b9a4:      cmp w23, w16
10081b9a8:      b.eq    0x10081ba88 <_js_object_alloc_class_dynamic_parent+0x2c8>
10081b9ac:      sub x15, x14, #0x2
10081b9b0:      ands    x14, x15, x14
10081b9b4:      b.ne    0x10081b988 <_js_object_alloc_class_dynamic_parent+0x1c8>
10081b9b8:      cmeq.8b v2, v2, v1
10081b9bc:      fmov    x14, d2
10081b9c0:      cbnz    x14, 0x10081b9e8 <_js_object_alloc_class_dynamic_parent+0x228>
10081b9c4:      add x9, x9, #0x8
10081b9c8:      add x13, x13, x9
10081b9cc:      and x13, x13, x10
10081b9d0:      ldr d2, [x11, x13]
10081b9d4:      cmeq.8b v3, v2, v0
10081b9d8:      fmov    x14, d3
10081b9dc:      ands    x14, x14, #0x8080808080808080
10081b9e0:      b.ne    0x10081b988 <_js_object_alloc_class_dynamic_parent+0x1c8>
10081b9e4:      b   0x10081b9b8 <_js_object_alloc_class_dynamic_parent+0x1f8>
10081b9e8:      mov w25, #0x0               ; =0
10081b9ec:      mov x21, #0x0               ; =0
10081b9f0:      str x8, [x0, #0x5088]
10081b9f4:      cbnz    x21, 0x10081b89c <_js_object_alloc_class_dynamic_parent+0xdc>
10081b9f8:      mov x25, #0x0               ; =0
10081b9fc:      mov w9, #0x1                ; =1
10081ba00:      mov w8, #0x8                ; =8
10081ba04:      str x8, [sp, #0x8]
10081ba08:      cbz x22, 0x10081bb94 <_js_object_alloc_class_dynamic_parent+0x3d4>
10081ba0c:      cbz w27, 0x10081bb94 <_js_object_alloc_class_dynamic_parent+0x3d4>
10081ba10:      str x26, [sp]
10081ba14:      mov w26, #0x0               ; =0
10081ba18:      mov w8, #0x0                ; =0
10081ba1c:      mov w9, #0x8                ; =8
10081ba20:      str x9, [sp, #0x8]
10081ba24:      mov w21, w27
10081ba28:      b   0x10081ba3c <_js_object_alloc_class_dynamic_parent+0x27c>
10081ba2c:      mov w26, #0x1               ; =1
10081ba30:      mov x22, x25
10081ba34:      mov w8, #0x1                ; =1
10081ba38:      cbnz    x27, 0x10081baa8 <_js_object_alloc_class_dynamic_parent+0x2e8>
10081ba3c:      tbnz    w8, #0x0, 0x10081ba9c <_js_object_alloc_class_dynamic_parent+0x2dc>
10081ba40:      mov x25, x22
10081ba44:      mov x27, #0x0               ; =0
10081ba48:      cbz x21, 0x10081ba2c <_js_object_alloc_class_dynamic_parent+0x26c>
10081ba4c:      ldrb    w8, [x25, x27]
10081ba50:      cbz w8, 0x10081ba74 <_js_object_alloc_class_dynamic_parent+0x2b4>
10081ba54:      add x27, x27, #0x1
10081ba58:      cmp x21, x27
10081ba5c:      b.ne    0x10081ba4c <_js_object_alloc_class_dynamic_parent+0x28c>
10081ba60:      mov w26, #0x1               ; =1
10081ba64:      mov x22, x25
10081ba68:      mov w8, #0x1                ; =1
10081ba6c:      mov x27, x21
10081ba70:      b   0x10081ba38 <_js_object_alloc_class_dynamic_parent+0x278>
10081ba74:      mvn x9, x27
10081ba78:      add x21, x9, x21
10081ba7c:      add x9, x25, x27
10081ba80:      add x22, x9, #0x1
10081ba84:      b   0x10081ba38 <_js_object_alloc_class_dynamic_parent+0x278>
10081ba88:      ldur    x21, [x15, #-0x10]
10081ba8c:      ldur    w25, [x15, #-0x8]
10081ba90:      str x8, [x0, #0x5088]
10081ba94:      cbnz    x21, 0x10081b89c <_js_object_alloc_class_dynamic_parent+0xdc>
10081ba98:      b   0x10081b9f8 <_js_object_alloc_class_dynamic_parent+0x238>
10081ba9c:      mov x25, #0x0               ; =0
10081baa0:      mov w9, #0x1                ; =1
10081baa4:      b   0x10081bb90 <_js_object_alloc_class_dynamic_parent+0x3d0>
10081baa8:      mov w0, #0x40               ; =64
10081baac:      mov w1, #0x8                ; =8
10081bab0:      bl  0x100af5c08 <_mi_malloc_aligned>
10081bab4:      cbz x0, 0x10081c340 <_js_object_alloc_class_dynamic_parent+0xb80>
10081bab8:      stp x25, x27, [x0]
10081babc:      mov w8, #0x4                ; =4
10081bac0:      stp x8, x0, [sp, #0x28]
10081bac4:      mov w25, #0x1               ; =1
10081bac8:      str x25, [sp, #0x38]
10081bacc:      mov x8, x21
10081bad0:      mov x10, x22
10081bad4:      mov x9, x26
10081bad8:      b   0x10081baec <_js_object_alloc_class_dynamic_parent+0x32c>
10081badc:      mov w26, #0x1               ; =1
10081bae0:      mov x10, x27
10081bae4:      mov w9, #0x1                ; =1
10081bae8:      cbnz    x28, 0x10081bb40 <_js_object_alloc_class_dynamic_parent+0x380>
10081baec:      tbnz    w9, #0x0, 0x10081bb7c <_js_object_alloc_class_dynamic_parent+0x3bc>
10081baf0:      mov x27, x10
10081baf4:      mov x28, #0x0               ; =0
10081baf8:      cbz x8, 0x10081badc <_js_object_alloc_class_dynamic_parent+0x31c>
10081bafc:      ldrb    w9, [x27, x28]
10081bb00:      cbz w9, 0x10081bb24 <_js_object_alloc_class_dynamic_parent+0x364>
10081bb04:      add x28, x28, #0x1
10081bb08:      cmp x8, x28
10081bb0c:      b.ne    0x10081bafc <_js_object_alloc_class_dynamic_parent+0x33c>
10081bb10:      mov w26, #0x1               ; =1
10081bb14:      mov x10, x27
10081bb18:      mov w9, #0x1                ; =1
10081bb1c:      mov x28, x8
10081bb20:      b   0x10081bae8 <_js_object_alloc_class_dynamic_parent+0x328>
10081bb24:      mvn x10, x28
10081bb28:      add x21, x10, x8
10081bb2c:      add x8, x27, x28
10081bb30:      add x22, x8, #0x1
10081bb34:      mov x8, x21
10081bb38:      mov x10, x22
10081bb3c:      b   0x10081bae8 <_js_object_alloc_class_dynamic_parent+0x328>
10081bb40:      ldr x8, [sp, #0x28]
10081bb44:      cmp x25, x8
10081bb48:      b.eq    0x10081bb5c <_js_object_alloc_class_dynamic_parent+0x39c>
10081bb4c:      add x8, x0, x25, lsl #4
10081bb50:      stp x27, x28, [x8]
10081bb54:      add x25, x25, #0x1
10081bb58:      b   0x10081bac8 <_js_object_alloc_class_dynamic_parent+0x308>
10081bb5c:      add x0, sp, #0x28
10081bb60:      mov x1, x25
10081bb64:      mov w2, #0x1                ; =1
10081bb68:      mov w3, #0x8                ; =8
10081bb6c:      mov w4, #0x10               ; =16
10081bb70:      bl  0x100ad5290 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081bb74:      ldr x0, [sp, #0x30]
10081bb78:      b   0x10081bb4c <_js_object_alloc_class_dynamic_parent+0x38c>
10081bb7c:      ldp x8, x9, [sp, #0x28]
10081bb80:      str x9, [sp, #0x8]
10081bb84:      cmp x8, #0x0
10081bb88:      cset    w9, eq
10081bb8c:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081bb90:      ldr x26, [sp]
10081bb94:      str w9, [sp]
10081bb98:      add w22, w24, w25
10081bb9c:      ubfiz   x8, x22, #3, #32
10081bba0:      add x0, x8, #0x8
10081bba4:      mov w1, #0x8                ; =8
10081bba8:      mov w2, #0x1                ; =1
10081bbac:      bl  0x1004da9a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081bbb0:      mov x21, x0
10081bbb4:      stp w22, w22, [x0]
10081bbb8:      lsr x8, x0, #3
10081bbbc:      cmp x8, #0x201
10081bbc0:      b.lo    0x10081bc50 <_js_object_alloc_class_dynamic_parent+0x490>
10081bbc4:      ldurb   w8, [x21, #-0x8]
10081bbc8:      cmp w8, #0x1
10081bbcc:      b.ne    0x10081bbf8 <_js_object_alloc_class_dynamic_parent+0x438>
10081bbd0:      ldurh   w8, [x21, #-0x6]
10081bbd4:      mov w9, #0x1080             ; =4224
10081bbd8:      mov w10, #0xef7f            ; =61311
10081bbdc:      and w10, w8, w10
10081bbe0:      sturh   w10, [x21, #-0x6]
10081bbe4:      tst w8, w9
10081bbe8:      b.eq    0x10081bc08 <_js_object_alloc_class_dynamic_parent+0x448>
10081bbec:      mov x0, x21
10081bbf0:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081bbf4:      ldurb   w8, [x21, #-0x8]
10081bbf8:      sub w9, w8, #0x15
10081bbfc:      cmn w9, #0x14
10081bc00:      b.hs    0x10081bc0c <_js_object_alloc_class_dynamic_parent+0x44c>
10081bc04:      b   0x10081bc50 <_js_object_alloc_class_dynamic_parent+0x490>
10081bc08:      mov w8, #0x1                ; =1
10081bc0c:      mov w8, w8
10081bc10:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081bc14:      add x9, x9, #0xb10
10081bc18:      add x8, x9, x8, lsl #5
10081bc1c:      ldrb    w8, [x8, #0x14]
10081bc20:      mov w9, #0x16               ; =22
10081bc24:      lsr w8, w9, w8
10081bc28:      tbz w8, #0x0, 0x10081bc50 <_js_object_alloc_class_dynamic_parent+0x490>
10081bc2c:      ldurh   w8, [x21, #-0x6]
10081bc30:      mov w9, #0x4000             ; =16384
10081bc34:      bfxil   w9, w8, #0, #13
10081bc38:      sturh   w9, [x21, #-0x6]
10081bc3c:      mov x0, x21
10081bc40:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081bc44:      ldurh   w8, [x21, #-0x6]
10081bc48:      and w8, w8, #0xffffefff
10081bc4c:      sturh   w8, [x21, #-0x6]
10081bc50:      cbz w24, 0x10081bc88 <_js_object_alloc_class_dynamic_parent+0x4c8>
10081bc54:      mov x1, #0x0                ; =0
10081bc58:      add x26, x26, #0x8
10081bc5c:      add x27, x1, #0x1
10081bc60:      lsl x8, x1, #3
10081bc64:      ldr d0, [x26, x8]
10081bc68:      add x8, x21, x8
10081bc6c:      str d0, [x8, #0x8]
10081bc70:      fmov    x2, d0
10081bc74:      mov x0, x21
10081bc78:      bl  0x1004f079c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081bc7c:      mov x1, x27
10081bc80:      cmp x24, x27
10081bc84:      b.ne    0x10081bc5c <_js_object_alloc_class_dynamic_parent+0x49c>
10081bc88:      ldr x27, [sp, #0x8]
10081bc8c:      cbz x25, 0x10081bcd0 <_js_object_alloc_class_dynamic_parent+0x510>
10081bc90:      add x25, x27, x25, lsl #4
10081bc94:      mov x26, x27
10081bc98:      ldr x0, [x26]
10081bc9c:      ldr w1, [x26, #0x8]
10081bca0:      bl  0x100884854 <_js_string_from_bytes_longlived>
10081bca4:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081bca8:      bfxil   x2, x0, #0, #48
10081bcac:      add x8, x21, x24, lsl #3
10081bcb0:      str x2, [x8, #0x8]
10081bcb4:      mov x0, x21
10081bcb8:      mov x1, x24
10081bcbc:      bl  0x1004f079c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081bcc0:      add x24, x24, #0x1
10081bcc4:      add x26, x26, #0x10
10081bcc8:      cmp x26, x25
10081bccc:      b.ne    0x10081bc98 <_js_object_alloc_class_dynamic_parent+0x4d8>
10081bcd0:      mov x0, x23
10081bcd4:      mov x1, x21
10081bcd8:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081bcdc:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081bce0:      ldr w24, [x8, #0x634]
10081bce4:      cmp w24, #0x300
10081bce8:      and w10, w23, #0xff
10081bcec:      b.hs    0x10081c13c <_js_object_alloc_class_dynamic_parent+0x97c>
10081bcf0:      ldr x8, [x28, #0x48]
10081bcf4:      cmn x8, #0x1
10081bcf8:      b.eq    0x10081c128 <_js_object_alloc_class_dynamic_parent+0x968>
10081bcfc:      mrs x9, TPIDRRO_EL0
10081bd00:      and x9, x9, #0xfffffffffffffff8
10081bd04:      ldr x0, [x9, x8, lsl #3]
10081bd08:      cbz x0, 0x10081c128 <_js_object_alloc_class_dynamic_parent+0x968>
10081bd0c:      add x8, x0, x24, lsl #3
10081bd10:      ldr x0, [x8, #0x1e8]
10081bd14:      cbz x0, 0x10081c13c <_js_object_alloc_class_dynamic_parent+0x97c>
10081bd18:      ldr x0, [x0]
10081bd1c:      cbz x0, 0x10081c154 <_js_object_alloc_class_dynamic_parent+0x994>
10081bd20:      add x8, x0, x10, lsl #4
10081bd24:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081bd28:      ldr w9, [x8]
10081bd2c:      cmp w9, w23
10081bd30:      b.ne    0x10081c170 <_js_object_alloc_class_dynamic_parent+0x9b0>
10081bd34:      ldr w25, [x8, #0x4]
10081bd38:      ldr w8, [sp]
10081bd3c:      tbnz    w8, #0x0, 0x10081bd48 <_js_object_alloc_class_dynamic_parent+0x588>
10081bd40:      mov x0, x27
10081bd44:      bl  0x100af5980 <_mi_free>
10081bd48:      mov w8, #0x2                ; =2
10081bd4c:      cmp w22, #0x2
10081bd50:      csel    w27, w22, w8, hi
10081bd54:      ubfiz   x8, x27, #3, #32
10081bd58:      mov w9, #0x3ffd             ; =16381
10081bd5c:      cmp w22, w9
10081bd60:      b.ls    0x10081bd88 <_js_object_alloc_class_dynamic_parent+0x5c8>
10081bd64:      add x0, x8, #0x10
10081bd68:      mov w1, #0x8                ; =8
10081bd6c:      mov w2, #0x2                ; =2
10081bd70:      bl  0x1004d9f18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081bd74:      mov x24, x0
10081bd78:      ldurb   w8, [x0, #-0x7]
10081bd7c:      orr w8, w8, #0x20
10081bd80:      sturb   w8, [x0, #-0x7]
10081bd84:      b   0x10081bef4 <_js_object_alloc_class_dynamic_parent+0x734>
10081bd88:      add x23, x8, #0x18
10081bd8c:      ldr x8, [x28, #0x48]
10081bd90:      cmn x8, #0x1
10081bd94:      b.eq    0x10081c0e0 <_js_object_alloc_class_dynamic_parent+0x920>
10081bd98:      mrs x9, TPIDRRO_EL0
10081bd9c:      and x9, x9, #0xfffffffffffffff8
10081bda0:      ldr x0, [x9, x8, lsl #3]
10081bda4:      cbz x0, 0x10081c0e0 <_js_object_alloc_class_dynamic_parent+0x920>
10081bda8:      ldr x8, [x0, #0x28]
10081bdac:      ldrb    w8, [x8]
10081bdb0:      tbz w8, #0x0, 0x10081be1c <_js_object_alloc_class_dynamic_parent+0x65c>
10081bdb4:      ldr x8, [x28, #0x48]
10081bdb8:      cmn x8, #0x1
10081bdbc:      b.eq    0x10081c264 <_js_object_alloc_class_dynamic_parent+0xaa4>
10081bdc0:      mrs x9, TPIDRRO_EL0
10081bdc4:      and x9, x9, #0xfffffffffffffff8
10081bdc8:      ldr x0, [x9, x8, lsl #3]
10081bdcc:      cbz x0, 0x10081c264 <_js_object_alloc_class_dynamic_parent+0xaa4>
10081bdd0:      ldr x26, [x0, #0x20]
10081bdd4:      ldr x8, [x26]
10081bdd8:      cbnz    x8, 0x10081c274 <_js_object_alloc_class_dynamic_parent+0xab4>
10081bddc:      mov x8, #-0x1               ; =-1
10081bde0:      str x8, [x26]
10081bde4:      ldr x1, [x26, #0x18]
10081bde8:      cbz x1, 0x10081be18 <_js_object_alloc_class_dynamic_parent+0x658>
10081bdec:      mov x0, #0x0                ; =0
10081bdf0:      ldr x8, [x26, #0x10]
10081bdf4:      lsl x10, x1, #4
10081bdf8:      mov x9, x8
10081bdfc:      ldr x11, [x9, #0x8]
10081be00:      cmp x11, x23
10081be04:      b.eq    0x10081bfc0 <_js_object_alloc_class_dynamic_parent+0x800>
10081be08:      add x0, x0, #0x1
10081be0c:      add x9, x9, #0x10
10081be10:      subs    x10, x10, #0x10
10081be14:      b.ne    0x10081bdfc <_js_object_alloc_class_dynamic_parent+0x63c>
10081be18:      str xzr, [x26]
10081be1c:      mov x0, x23
10081be20:      bl  0x1004d9b5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081be24:      mov w8, #0x2                ; =2
10081be28:      strb    w8, [x0]
10081be2c:      ldr x8, [x28, #0x48]
10081be30:      cmn x8, #0x1
10081be34:      b.eq    0x10081c0f4 <_js_object_alloc_class_dynamic_parent+0x934>
10081be38:      mrs x9, TPIDRRO_EL0
10081be3c:      and x9, x9, #0xfffffffffffffff8
10081be40:      ldr x8, [x9, x8, lsl #3]
10081be44:      cbz x8, 0x10081c0f4 <_js_object_alloc_class_dynamic_parent+0x934>
10081be48:      ldr x8, [x8, #0x30]
10081be4c:      ldrb    w8, [x8]
10081be50:      orr w8, w8, #0x2
10081be54:      strb    w8, [x0, #0x1]
10081be58:      ldr x8, [x28, #0x48]
10081be5c:      cmn x8, #0x1
10081be60:      b.eq    0x10081c108 <_js_object_alloc_class_dynamic_parent+0x948>
10081be64:      mrs x9, TPIDRRO_EL0
10081be68:      and x9, x9, #0xfffffffffffffff8
10081be6c:      ldr x8, [x9, x8, lsl #3]
10081be70:      cbz x8, 0x10081c108 <_js_object_alloc_class_dynamic_parent+0x948>
10081be74:      ldr x8, [x8, #0x30]
10081be78:      ldrb    w8, [x8]
10081be7c:      tbz w8, #0x0, 0x10081bee8 <_js_object_alloc_class_dynamic_parent+0x728>
10081be80:      ldrb    w8, [x0]
10081be84:      sub w9, w8, #0x15
10081be88:      cmn w9, #0x14
10081be8c:      b.lo    0x10081bee8 <_js_object_alloc_class_dynamic_parent+0x728>
10081be90:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081be94:      add x9, x9, #0xb10
10081be98:      add x8, x9, x8, lsl #5
10081be9c:      ldrb    w8, [x8, #0x1b]
10081bea0:      tbnz    w8, #0x0, 0x10081bee8 <_js_object_alloc_class_dynamic_parent+0x728>
10081bea4:      mov x26, x0
10081bea8:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081beac:      add x0, x0, #0x658
10081beb0:      ldr x8, [x0]
10081beb4:      blr x8
10081beb8:      mov x24, x0
10081bebc:      ldrb    w8, [x0, #0x18]
10081bec0:      cbnz    w8, 0x10081c2b4 <_js_object_alloc_class_dynamic_parent+0xaf4>
10081bec4:      ldr x28, [x24, #0x10]
10081bec8:      ldr x8, [x24]
10081becc:      cmp x28, x8
10081bed0:      mov x0, x26
10081bed4:      b.eq    0x10081c2e4 <_js_object_alloc_class_dynamic_parent+0xb24>
10081bed8:      ldr x8, [x24, #0x8]
10081bedc:      str x0, [x8, x28, lsl #3]
10081bee0:      add x8, x28, #0x1
10081bee4:      str x8, [x24, #0x10]
10081bee8:      strh    wzr, [x0, #0x2]
10081beec:      str w23, [x0, #0x4]
10081bef0:      add x24, x0, #0x8
10081bef4:      stp w19, w20, [x24]
10081bef8:      lsl x2, x27, #3
10081befc:      str xzr, [x24, #0x8]
10081bf00:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x61e8>
10081bf04:      add x1, x1, #0x3b0
10081bf08:      add x0, x24, #0x10
10081bf0c:      bl  0x100af8850 <_writev+0x100af8850>
10081bf10:      mov x0, x24
10081bf14:      mov x1, x21
10081bf18:      mov x2, x22
10081bf1c:      bl  0x100324600 <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object31set_object_keys_array_with_live>
10081bf20:      lsr x8, x24, #3
10081bf24:      cmp x8, #0x201
10081bf28:      b.lo    0x10081bf7c <_js_object_alloc_class_dynamic_parent+0x7bc>
10081bf2c:      ldurb   w8, [x24, #-0x8]
10081bf30:      sub w9, w8, #0x15
10081bf34:      cmn w9, #0x14
10081bf38:      b.lo    0x10081bf7c <_js_object_alloc_class_dynamic_parent+0x7bc>
10081bf3c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081bf40:      add x9, x9, #0xb10
10081bf44:      add x8, x9, x8, lsl #5
10081bf48:      ldrb    w8, [x8, #0x14]
10081bf4c:      mov w9, #0x16               ; =22
10081bf50:      lsr w8, w9, w8
10081bf54:      tbz w8, #0x0, 0x10081bf7c <_js_object_alloc_class_dynamic_parent+0x7bc>
10081bf58:      ldurh   w8, [x24, #-0x6]
10081bf5c:      mov w9, #0x4000             ; =16384
10081bf60:      bfxil   w9, w8, #0, #13
10081bf64:      sturh   w9, [x24, #-0x6]
10081bf68:      mov x0, x24
10081bf6c:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081bf70:      ldurh   w8, [x24, #-0x6]
10081bf74:      and w8, w8, #0xffffefff
10081bf78:      sturh   w8, [x24, #-0x6]
10081bf7c:      mov x0, x24
10081bf80:      mov x1, x25
10081bf84:      mov x2, x22
10081bf88:      bl  0x100597880 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081bf8c:      mov x0, x19
10081bf90:      mov x1, x22
10081bf94:      mov x2, x21
10081bf98:      bl  0x100591500 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc25remember_class_keys_array>
10081bf9c:      mov x0, x24
10081bfa0:      ldp x29, x30, [sp, #0x90]
10081bfa4:      ldp x20, x19, [sp, #0x80]
10081bfa8:      ldp x22, x21, [sp, #0x70]
10081bfac:      ldp x24, x23, [sp, #0x60]
10081bfb0:      ldp x26, x25, [sp, #0x50]
10081bfb4:      ldp x28, x27, [sp, #0x40]
10081bfb8:      add sp, sp, #0xa0
10081bfbc:      ret
10081bfc0:      cmp x0, x1
10081bfc4:      b.hs    0x10081c2b0 <_js_object_alloc_class_dynamic_parent+0xaf0>
10081bfc8:      ldr x24, [x9]
10081bfcc:      subs    x10, x1, #0x1
10081bfd0:      ldr q0, [x8, x10, lsl #4]
10081bfd4:      str q0, [x9]
10081bfd8:      str x10, [x26, #0x18]
10081bfdc:      b.ne    0x10081c004 <_js_object_alloc_class_dynamic_parent+0x844>
10081bfe0:      ldr x8, [x28, #0x48]
10081bfe4:      cmn x8, #0x1
10081bfe8:      b.eq    0x10081c29c <_js_object_alloc_class_dynamic_parent+0xadc>
10081bfec:      mrs x9, TPIDRRO_EL0
10081bff0:      and x9, x9, #0xfffffffffffffff8
10081bff4:      ldr x0, [x9, x8, lsl #3]
10081bff8:      cbz x0, 0x10081c29c <_js_object_alloc_class_dynamic_parent+0xadc>
10081bffc:      ldr x8, [x0, #0x28]
10081c000:      strb    wzr, [x8]
10081c004:      ldr x8, [x26]
10081c008:      add x8, x8, #0x1
10081c00c:      str x8, [x26]
10081c010:      mov w8, #0x2                ; =2
10081c014:      mov x9, x28
10081c018:      mov x28, x24
10081c01c:      strb    w8, [x28, #-0x8]!
10081c020:      mov x26, x9
10081c024:      ldr x8, [x9, #0x48]
10081c028:      cmn x8, #0x1
10081c02c:      b.eq    0x10081c280 <_js_object_alloc_class_dynamic_parent+0xac0>
10081c030:      mrs x9, TPIDRRO_EL0
10081c034:      and x9, x9, #0xfffffffffffffff8
10081c038:      ldr x0, [x9, x8, lsl #3]
10081c03c:      cbz x0, 0x10081c280 <_js_object_alloc_class_dynamic_parent+0xac0>
10081c040:      ldr x8, [x0, #0x30]
10081c044:      ldrb    w8, [x8]
10081c048:      orr w8, w8, #0x2
10081c04c:      sturb   w8, [x24, #-0x7]
10081c050:      ldr x8, [x26, #0x48]
10081c054:      cmn x8, #0x1
10081c058:      b.eq    0x10081c288 <_js_object_alloc_class_dynamic_parent+0xac8>
10081c05c:      mrs x9, TPIDRRO_EL0
10081c060:      and x9, x9, #0xfffffffffffffff8
10081c064:      ldr x0, [x9, x8, lsl #3]
10081c068:      cbz x0, 0x10081c288 <_js_object_alloc_class_dynamic_parent+0xac8>
10081c06c:      ldr x8, [x0, #0x30]
10081c070:      ldrb    w8, [x8]
10081c074:      tbz w8, #0x0, 0x10081c0d4 <_js_object_alloc_class_dynamic_parent+0x914>
10081c078:      ldrb    w8, [x28]
10081c07c:      sub w9, w8, #0x15
10081c080:      cmn w9, #0x14
10081c084:      b.lo    0x10081c0d4 <_js_object_alloc_class_dynamic_parent+0x914>
10081c088:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c08c:      add x9, x9, #0xb10
10081c090:      add x8, x9, x8, lsl #5
10081c094:      ldrb    w8, [x8, #0x1b]
10081c098:      tbnz    w8, #0x0, 0x10081c0d4 <_js_object_alloc_class_dynamic_parent+0x914>
10081c09c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c0a0:      add x0, x0, #0x658
10081c0a4:      ldr x8, [x0]
10081c0a8:      blr x8
10081c0ac:      ldrb    w8, [x0, #0x18]
10081c0b0:      cbnz    w8, 0x10081c2f4 <_js_object_alloc_class_dynamic_parent+0xb34>
10081c0b4:      ldr x26, [x0, #0x10]
10081c0b8:      ldr x8, [x0]
10081c0bc:      cmp x26, x8
10081c0c0:      b.eq    0x10081c330 <_js_object_alloc_class_dynamic_parent+0xb70>
10081c0c4:      ldr x8, [x0, #0x8]
10081c0c8:      str x28, [x8, x26, lsl #3]
10081c0cc:      add x8, x26, #0x1
10081c0d0:      str x8, [x0, #0x10]
10081c0d4:      sturh   wzr, [x24, #-0x6]
10081c0d8:      stur    w23, [x24, #-0x4]
10081c0dc:      b   0x10081bef4 <_js_object_alloc_class_dynamic_parent+0x734>
10081c0e0:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c0e4:      ldr x8, [x0, #0x28]
10081c0e8:      ldrb    w8, [x8]
10081c0ec:      tbnz    w8, #0x0, 0x10081bdb4 <_js_object_alloc_class_dynamic_parent+0x5f4>
10081c0f0:      b   0x10081be1c <_js_object_alloc_class_dynamic_parent+0x65c>
10081c0f4:      mov x24, x0
10081c0f8:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c0fc:      mov x8, x0
10081c100:      mov x0, x24
10081c104:      b   0x10081be48 <_js_object_alloc_class_dynamic_parent+0x688>
10081c108:      mov x24, x0
10081c10c:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c110:      mov x8, x0
10081c114:      mov x0, x24
10081c118:      ldr x8, [x8, #0x30]
10081c11c:      ldrb    w8, [x8]
10081c120:      tbnz    w8, #0x0, 0x10081be80 <_js_object_alloc_class_dynamic_parent+0x6c0>
10081c124:      b   0x10081bee8 <_js_object_alloc_class_dynamic_parent+0x728>
10081c128:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c12c:      and w10, w23, #0xff
10081c130:      add x8, x0, x24, lsl #3
10081c134:      ldr x0, [x8, #0x1e8]
10081c138:      cbnz    x0, 0x10081bd18 <_js_object_alloc_class_dynamic_parent+0x558>
10081c13c:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081c140:      add x0, x0, #0x330
10081c144:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081c148:      and w10, w23, #0xff
10081c14c:      ldr x0, [x0]
10081c150:      cbnz    x0, 0x10081bd20 <_js_object_alloc_class_dynamic_parent+0x560>
10081c154:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081c158:      and w10, w23, #0xff
10081c15c:      add x8, x0, x10, lsl #4
10081c160:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c164:      ldr w9, [x8]
10081c168:      cmp w9, w23
10081c16c:      b.eq    0x10081bd34 <_js_object_alloc_class_dynamic_parent+0x574>
10081c170:      ldr x8, [x0, #0x5088]
10081c174:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081c178:      cmp x8, x9
10081c17c:      b.hs    0x10081c2a4 <_js_object_alloc_class_dynamic_parent+0xae4>
10081c180:      add x9, x8, #0x1
10081c184:      str x9, [x0, #0x5088]
10081c188:      ldr x9, [x0, #0x50a8]
10081c18c:      cbz x9, 0x10081c23c <_js_object_alloc_class_dynamic_parent+0xa7c>
10081c190:      mov x9, #0x0                ; =0
10081c194:      mov x10, #0x7c15            ; =31765
10081c198:      movk    x10, #0x7f4a, lsl #16
10081c19c:      movk    x10, #0x79b9, lsl #32
10081c1a0:      movk    x10, #0x9e37, lsl #48
10081c1a4:      mul x10, x23, x10
10081c1a8:      eor x13, x10, x10, lsr #32
10081c1ac:      lsr x12, x10, #57
10081c1b0:      ldr x10, [x0, #0x5098]
10081c1b4:      ldr x11, [x0, #0x5090]
10081c1b8:      dup.8b  v0, w12
10081c1bc:      movi.2d v1, #0xffffffffffffffff
10081c1c0:      mov w12, #0x18              ; =24
10081c1c4:      and x13, x13, x10
10081c1c8:      ldr d2, [x11, x13]
10081c1cc:      cmeq.8b v3, v2, v0
10081c1d0:      fmov    x14, d3
10081c1d4:      ands    x14, x14, #0x8080808080808080
10081c1d8:      b.eq    0x10081c20c <_js_object_alloc_class_dynamic_parent+0xa4c>
10081c1dc:      rbit    x15, x14
10081c1e0:      clz x15, x15
10081c1e4:      add x15, x13, x15, lsr #3
10081c1e8:      and x15, x15, x10
10081c1ec:      mneg    x15, x15, x12
10081c1f0:      add x15, x11, x15
10081c1f4:      ldur    w16, [x15, #-0x18]
10081c1f8:      cmp w23, w16
10081c1fc:      b.eq    0x10081c250 <_js_object_alloc_class_dynamic_parent+0xa90>
10081c200:      sub x15, x14, #0x2
10081c204:      ands    x14, x15, x14
10081c208:      b.ne    0x10081c1dc <_js_object_alloc_class_dynamic_parent+0xa1c>
10081c20c:      cmeq.8b v2, v2, v1
10081c210:      fmov    x14, d2
10081c214:      cbnz    x14, 0x10081c23c <_js_object_alloc_class_dynamic_parent+0xa7c>
10081c218:      add x9, x9, #0x8
10081c21c:      add x13, x13, x9
10081c220:      and x13, x13, x10
10081c224:      ldr d2, [x11, x13]
10081c228:      cmeq.8b v3, v2, v0
10081c22c:      fmov    x14, d3
10081c230:      ands    x14, x14, #0x8080808080808080
10081c234:      b.ne    0x10081c1dc <_js_object_alloc_class_dynamic_parent+0xa1c>
10081c238:      b   0x10081c20c <_js_object_alloc_class_dynamic_parent+0xa4c>
10081c23c:      mov w25, #0x0               ; =0
10081c240:      str x8, [x0, #0x5088]
10081c244:      ldr w8, [sp]
10081c248:      tbz w8, #0x0, 0x10081bd40 <_js_object_alloc_class_dynamic_parent+0x580>
10081c24c:      b   0x10081bd48 <_js_object_alloc_class_dynamic_parent+0x588>
10081c250:      ldur    w25, [x15, #-0x8]
10081c254:      str x8, [x0, #0x5088]
10081c258:      ldr w8, [sp]
10081c25c:      tbz w8, #0x0, 0x10081bd40 <_js_object_alloc_class_dynamic_parent+0x580>
10081c260:      b   0x10081bd48 <_js_object_alloc_class_dynamic_parent+0x588>
10081c264:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c268:      ldr x26, [x0, #0x20]
10081c26c:      ldr x8, [x26]
10081c270:      cbz x8, 0x10081bddc <_js_object_alloc_class_dynamic_parent+0x61c>
10081c274:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081c278:      add x0, x0, #0x120
10081c27c:      bl  0x100ab0bac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081c280:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c284:      b   0x10081c040 <_js_object_alloc_class_dynamic_parent+0x880>
10081c288:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c28c:      ldr x8, [x0, #0x30]
10081c290:      ldrb    w8, [x8]
10081c294:      tbnz    w8, #0x0, 0x10081c078 <_js_object_alloc_class_dynamic_parent+0x8b8>
10081c298:      b   0x10081c0d4 <_js_object_alloc_class_dynamic_parent+0x914>
10081c29c:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c2a0:      b   0x10081bffc <_js_object_alloc_class_dynamic_parent+0x83c>
10081c2a4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081c2a8:      add x0, x0, #0x798
10081c2ac:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081c2b0:      bl  0x100ab087c <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081c2b4:      cmp w8, #0x1
10081c2b8:      b.ne    0x10081c2fc <_js_object_alloc_class_dynamic_parent+0xb3c>
10081c2bc:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081c2c0:      add x1, x1, #0xbb8
10081c2c4:      mov x0, x24
10081c2c8:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081c2cc:      strb    wzr, [x24, #0x18]
10081c2d0:      ldr x28, [x24, #0x10]
10081c2d4:      ldr x8, [x24]
10081c2d8:      cmp x28, x8
10081c2dc:      mov x0, x26
10081c2e0:      b.ne    0x10081bed8 <_js_object_alloc_class_dynamic_parent+0x718>
10081c2e4:      mov x0, x24
10081c2e8:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081c2ec:      mov x0, x26
10081c2f0:      b   0x10081bed8 <_js_object_alloc_class_dynamic_parent+0x718>
10081c2f4:      cmp w8, #0x2
10081c2f8:      b.ne    0x10081c308 <_js_object_alloc_class_dynamic_parent+0xb48>
10081c2fc:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081c300:      add x0, x0, #0x850
10081c304:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081c308:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081c30c:      add x1, x1, #0xbb8
10081c310:      mov x26, x0
10081c314:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081c318:      mov x0, x26
10081c31c:      strb    wzr, [x26, #0x18]
10081c320:      ldr x26, [x26, #0x10]
10081c324:      ldr x8, [x0]
10081c328:      cmp x26, x8
10081c32c:      b.ne    0x10081c0c4 <_js_object_alloc_class_dynamic_parent+0x904>
10081c330:      str x0, [sp, #0x8]
10081c334:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081c338:      ldr x0, [sp, #0x8]
10081c33c:      b   0x10081c0c4 <_js_object_alloc_class_dynamic_parent+0x904>
10081c340:      mov w0, #0x8                ; =8
10081c344:      mov w1, #0x40               ; =64
10081c348:      bl  0x100ab0824 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
        ...
