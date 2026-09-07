/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003ba8e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>:
1003ba8e0:      sub sp, sp, #0x80
1003ba8e4:      stp d9, d8, [sp, #0x10]
1003ba8e8:      stp x28, x27, [sp, #0x20]
1003ba8ec:      stp x26, x25, [sp, #0x30]
1003ba8f0:      stp x24, x23, [sp, #0x40]
1003ba8f4:      stp x22, x21, [sp, #0x50]
1003ba8f8:      stp x20, x19, [sp, #0x60]
1003ba8fc:      stp x29, x30, [sp, #0x70]
1003ba900:      add x29, sp, #0x70
1003ba904:      str x2, [sp, #0x8]
1003ba908:      mov x23, x1
1003ba90c:      mov x21, x0
1003ba910:      mov x0, x23
1003ba914:      bl  0x1003fb814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
1003ba918:      mov x20, x0
1003ba91c:      bl  0x1003fd2b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
1003ba920:      cbz x23, 0x1003baaf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
1003ba924:      mov x22, #0x0               ; =0
1003ba928:      lsl x26, x23, #3
1003ba92c:      add x23, x20, #0x8
1003ba930:      mov w27, #0x8               ; =8
1003ba934:      mov w28, #0x7ffe            ; =32766
1003ba938:      mov x19, #0x7ff8ffffffffffff ; =9221401712017801215
1003ba93c:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
1003ba940:      fmov    d8, x8
1003ba944:      lsr x24, x20, #3
1003ba948:      b   0x1003ba974 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x94>
1003ba94c:      mov x0, x20
1003ba950:      mov x1, x23
1003ba954:      mov x2, x25
1003ba958:      mov w3, #0x0                ; =0
1003ba95c:      bl  0x10098f020 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1003ba960:      add x23, x23, #0x8
1003ba964:      add x27, x27, #0x8
1003ba968:      add x22, x22, #0x1
1003ba96c:      subs    x26, x26, #0x8
1003ba970:      b.eq    0x1003baaf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
1003ba974:      ldr x25, [x21, x22, lsl #3]
1003ba978:      str x25, [x20, x27]
1003ba97c:      mov x0, x20
1003ba980:      bl  0x1003fc3c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
1003ba984:      tbz w0, #0x0, 0x1003ba9a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xc4>
1003ba988:      cmp x28, x25, lsr #48
1003ba98c:      b.ne    0x1003ba9c4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xe4>
1003ba990:      mov x0, x25
1003ba994:      bl  0x10069fafc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1003ba998:      tbnz    w0, #0x0, 0x1003ba9dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
1003ba99c:      scvtf   d0, w25
1003ba9a0:      b   0x1003ba9d8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xf8>
1003ba9a4:      cmp x24, #0x201
1003ba9a8:      b.lo    0x1003ba9dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
1003ba9ac:      ldurb   w8, [x20, #-0x8]
1003ba9b0:      cmp w8, #0x1
1003ba9b4:      b.ne    0x1003ba9dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
1003ba9b8:      ldurh   w8, [x20, #-0x6]
1003ba9bc:      tbnz    w8, #0xc, 0x1003ba988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xa8>
1003ba9c0:      b   0x1003ba9dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
1003ba9c4:      cmp x25, x19
1003ba9c8:      b.gt    0x1003ba9dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
1003ba9cc:      fmov    d0, x25
1003ba9d0:      fcmp    d0, d0
1003ba9d4:      fcsel   d0, d8, d0, vs
1003ba9d8:      fmov    x25, d0
1003ba9dc:      str x25, [x20, x27]
1003ba9e0:      cmp x28, x25, lsr #48
1003ba9e4:      b.ne    0x1003baa04 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x124>
1003ba9e8:      mov x0, x25
1003ba9ec:      bl  0x10069fafc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1003ba9f0:      tbnz    w0, #0x0, 0x1003baa0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x12c>
1003ba9f4:      scvtf   d0, w25
1003ba9f8:      cmp x24, #0x201
1003ba9fc:      b.hs    0x1003baa5c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x17c>
1003baa00:      b   0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa04:      cmp x25, x19
1003baa08:      b.le    0x1003baa48 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x168>
1003baa0c:      cmp x24, #0x201
1003baa10:      b.lo    0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa14:      ldurb   w8, [x20, #-0x8]
1003baa18:      cmp w8, #0x1
1003baa1c:      b.ne    0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa20:      ldurh   w8, [x20, #-0x6]
1003baa24:      mov w9, #0xef7f             ; =61311
1003baa28:      and w9, w8, w9
1003baa2c:      sturh   w9, [x20, #-0x6]
1003baa30:      mov w9, #0x1080             ; =4224
1003baa34:      tst w8, w9
1003baa38:      b.eq    0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa3c:      mov x0, x20
1003baa40:      bl  0x10055a3e8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
1003baa44:      b   0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa48:      fmov    d0, x25
1003baa4c:      fcmp    d0, d0
1003baa50:      fcsel   d0, d8, d0, vs
1003baa54:      cmp x24, #0x201
1003baa58:      b.lo    0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa5c:      ldurb   w8, [x20, #-0x8]
1003baa60:      cmp w8, #0x1
1003baa64:      b.ne    0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa68:      ldurh   w8, [x20, #-0x6]
1003baa6c:      tbz w8, #0x7, 0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa70:      ldr w8, [x20]
1003baa74:      cmp x22, x8
1003baa78:      b.hs    0x1003baa80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1003baa7c:      str d0, [x20, x27]
1003baa80:      mov x0, x20
1003baa84:      mov x1, x22
1003baa88:      mov x2, x25
1003baa8c:      bl  0x10058a300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
1003baa90:      mov x0, x20
1003baa94:      bl  0x1003ead08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
1003baa98:      cbz w0, 0x1003ba960 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
1003baa9c:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003baaa0:      add x8, x8, #0xd50
1003baaa4:      ldapr   x8, [x8]
1003baaa8:      cbnz    x8, 0x1003baad4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1f4>
1003baaac:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003baab0:      ldrb    w8, [x8, #0xd58]
1003baab4:      tbnz    w8, #0x0, 0x1003ba94c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
1003baab8:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
1003baabc:      add x8, x8, #0xaa8
1003baac0:      ldr w8, [x8]
1003baac4:      cbz w8, 0x1003ba960 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
1003baac8:      mov x0, x25
1003baacc:      bl  0x1009904f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
1003baad0:      b   0x1003ba960 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
1003baad4:      adrp    x0, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003baad8:      add x0, x0, #0xd50
1003baadc:      bl  0x100cb63bc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1003baae0:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003baae4:      ldrb    w8, [x8, #0xd58]
1003baae8:      tbnz    w8, #0x0, 0x1003ba94c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
1003baaec:      b   0x1003baab8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1d8>
1003baaf0:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003baaf4:      add x0, x0, #0x8
1003baaf8:      ldr x8, [x0]
1003baafc:      blr x8
1003bab00:      ldrb    w8, [x0, #0x20]
1003bab04:      cbnz    w8, 0x1003bab50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x270>
1003bab08:      ldr x8, [x0]
1003bab0c:      cbnz    x8, 0x1003bab78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x298>
1003bab10:      ldr x8, [x0, #0x18]
1003bab14:      ldr x9, [sp, #0x8]
1003bab18:      cmp x9, x8
1003bab1c:      b.hi    0x1003bab24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x244>
1003bab20:      str x9, [x0, #0x18]
1003bab24:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1003bab28:      bfxil   x0, x20, #0, #48
1003bab2c:      ldp x29, x30, [sp, #0x70]
1003bab30:      ldp x20, x19, [sp, #0x60]
1003bab34:      ldp x22, x21, [sp, #0x50]
1003bab38:      ldp x24, x23, [sp, #0x40]
1003bab3c:      ldp x26, x25, [sp, #0x30]
1003bab40:      ldp x28, x27, [sp, #0x20]
1003bab44:      ldp d9, d8, [sp, #0x10]
1003bab48:      add sp, sp, #0x80
1003bab4c:      ret
1003bab50:      cmp w8, #0x1
1003bab54:      b.ne    0x1003bab84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x2a4>
1003bab58:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003bab5c:      add x1, x1, #0x824
1003bab60:      mov x21, x0
1003bab64:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003bab68:      mov x0, x21
1003bab6c:      strb    wzr, [x21, #0x20]
1003bab70:      ldr x8, [x21]
1003bab74:      cbz x8, 0x1003bab10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x230>
1003bab78:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003bab7c:      add x0, x0, #0xe58
1003bab80:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003bab84:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1003bab88:      add x0, x0, #0xed8
1003bab8c:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
