/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/inline-object-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001007da974 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array>:
1007da974:      stp d9, d8, [sp, #-0x70]!
1007da978:      stp x28, x27, [sp, #0x10]
1007da97c:      stp x26, x25, [sp, #0x20]
1007da980:      stp x24, x23, [sp, #0x30]
1007da984:      stp x22, x21, [sp, #0x40]
1007da988:      stp x20, x19, [sp, #0x50]
1007da98c:      stp x29, x30, [sp, #0x60]
1007da990:      add x29, sp, #0x60
1007da994:      mov x20, x0
1007da998:      ldp x22, x8, [x0, #0x30]
1007da99c:      add x21, x8, #0x1
1007da9a0:      str x21, [x0, #0x38]
1007da9a4:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
1007da9a8:      add x0, x0, #0x1b0
1007da9ac:      cmp x21, x22
1007da9b0:      b.hs    0x1007daa18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xa4>
1007da9b4:      ldr x8, [x20, #0x28]
1007da9b8:      mov x9, #0x2600             ; =9728
1007da9bc:      movk    x9, #0x1, lsl #32
1007da9c0:      ldrb    w10, [x8, x21]
1007da9c4:      cmp w10, #0x20
1007da9c8:      b.hi    0x1007daa18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xa4>
1007da9cc:      lsr x10, x9, x10
1007da9d0:      tbz w10, #0x0, 0x1007daa18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xa4>
1007da9d4:      add x21, x21, #0x1
1007da9d8:      str x21, [x20, #0x38]
1007da9dc:      cmp x22, x21
1007da9e0:      b.ne    0x1007da9c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x4c>
1007da9e4:      ldr x8, [x0]
1007da9e8:      blr x8
1007da9ec:      mov x19, x0
1007da9f0:      ldrb    w8, [x0, #0x20]
1007da9f4:      cbnz    w8, 0x1007db014 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6a0>
1007da9f8:      ldr x8, [x19]
1007da9fc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007daa00:      cmp x8, x9
1007daa04:      b.hs    0x1007db040 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6cc>
1007daa08:      mov w23, #0x0               ; =0
1007daa0c:      ldr x27, [x19, #0x18]
1007daa10:      mov w0, #0x10               ; =16
1007daa14:      b   0x1007daa9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x128>
1007daa18:      ldr x8, [x0]
1007daa1c:      blr x8
1007daa20:      mov x19, x0
1007daa24:      ldrb    w8, [x0, #0x20]
1007daa28:      cbnz    w8, 0x1007dafe4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x670>
1007daa2c:      ldr x8, [x19]
1007daa30:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007daa34:      cmp x8, x9
1007daa38:      b.hs    0x1007db040 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6cc>
1007daa3c:      ldr x27, [x19, #0x18]
1007daa40:      subs    x8, x22, x21
1007daa44:      b.ls    0x1007daa90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x11c>
1007daa48:      ldr x9, [x20, #0x28]
1007daa4c:      ldrb    w9, [x9, x21]
1007daa50:      mov w10, #0x10              ; =16
1007daa54:      mov x11, #-0x5555555555555556 ; =-6148914691236517206
1007daa58:      movk    x11, #0xaaab
1007daa5c:      umulh   x8, x8, x11
1007daa60:      lsr x8, x8, #6
1007daa64:      mov w11, #0x10              ; =16
1007daa68:      cmp x8, #0x10
1007daa6c:      csel    x8, x8, x11, hi
1007daa70:      mov w11, #0x4000            ; =16384
1007daa74:      cmp x8, #0x4, lsl #12       ; =0x4000
1007daa78:      csel    x8, x8, x11, lo
1007daa7c:      mov w11, #0x1               ; =1
1007daa80:      cmp w9, #0x7b
1007daa84:      csel    w0, w10, w8, ne
1007daa88:      csinc   w23, w11, wzr, eq
1007daa8c:      b   0x1007daa98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x124>
1007daa90:      mov w23, #0x0               ; =0
1007daa94:      mov w0, #0x10               ; =16
1007daa98:      mov x22, x21
1007daa9c:      bl  0x10029aaac <_js_array_alloc>
1007daaa0:      ldrb    w8, [x19, #0x20]
1007daaa4:      cbnz    w8, 0x1007daf74 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x600>
1007daaa8:      ldr x8, [x19]
1007daaac:      cbnz    x8, 0x1007dafa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x62c>
1007daab0:      mov x21, #0x7ffd000000000000 ; =9222527611924643840
1007daab4:      bfxil   x21, x0, #0, #48
1007daab8:      mov x8, #-0x1               ; =-1
1007daabc:      str x8, [x19]
1007daac0:      mov x0, x19
1007daac4:      ldr x8, [x0, #0x8]!
1007daac8:      ldr x28, [x19, #0x18]
1007daacc:      cmp x28, x8
1007daad0:      b.ne    0x1007daad8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x164>
1007daad4:      bl  0x100cbaee0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1007daad8:      ldr x8, [x19, #0x10]
1007daadc:      str x21, [x8, x28, lsl #3]
1007daae0:      add x8, x28, #0x1
1007daae4:      str x8, [x19, #0x18]
1007daae8:      ldr x8, [x19]
1007daaec:      add x8, x8, #0x1
1007daaf0:      str x8, [x19]
1007daaf4:      tbz w23, #0x0, 0x1007dab28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x1b4>
1007daaf8:      ldr x9, [x20, #0x28]
1007daafc:      ldrb    w9, [x9, x22]
1007dab00:      cmp w9, #0x5d
1007dab04:      b.ne    0x1007dab28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x1b4>
1007dab08:      add x9, x22, #0x1
1007dab0c:      str x9, [x20, #0x38]
1007dab10:      ldrb    w9, [x19, #0x20]
1007dab14:      cbnz    w9, 0x1007db04c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6d8>
1007dab18:      cbz x8, 0x1007daf40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x5cc>
1007dab1c:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1007dab20:      add x0, x0, #0xe58
1007dab24:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1007dab28:      mov x0, x20
1007dab2c:      bl  0x1007db080 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1007dab30:      ldrb    w8, [x20, #0x90]
1007dab34:      cbz w8, 0x1007dae98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x524>
1007dab38:      mov x23, x0
1007dab3c:      mov x26, #0x7fffffffffffffff ; =9223372036854775807
1007dab40:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
1007dab44:      fmov    d8, x8
1007dab48:      mov x21, #-0x1              ; =-1
1007dab4c:      mov x22, #0x2600            ; =9728
1007dab50:      movk    x22, #0x1, lsl #32
1007dab54:      ldrb    w8, [x19, #0x20]
1007dab58:      cbnz    w8, 0x1007dad3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x3c8>
1007dab5c:      ldr x8, [x19]
1007dab60:      cmp x8, x26
1007dab64:      b.hs    0x1007dafd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x664>
1007dab68:      add x9, x8, #0x1
1007dab6c:      str x9, [x19]
1007dab70:      ldr x9, [x19, #0x18]
1007dab74:      cmp x28, x9
1007dab78:      b.hs    0x1007dabac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x238>
1007dab7c:      ldr x9, [x19, #0x10]
1007dab80:      ldr x9, [x9, x28, lsl #3]
1007dab84:      and x24, x9, #0xffffffffffff
1007dab88:      str x8, [x19]
1007dab8c:      ldp w25, w8, [x24]
1007dab90:      cmp w25, w8
1007dab94:      b.lo    0x1007dabc0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x24c>
1007dab98:      fmov    d0, x23
1007dab9c:      mov x0, x24
1007daba0:      bl  0x1006ed3dc <_js_array_push_f64>
1007daba4:      and x24, x0, #0xffffffffffff
1007daba8:      b   0x1007dada4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x430>
1007dabac:      mov w24, #0x1               ; =1
1007dabb0:      str x8, [x19]
1007dabb4:      ldp w25, w8, [x24]
1007dabb8:      cmp w25, w8
1007dabbc:      b.hs    0x1007dab98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x224>
1007dabc0:      add x26, x24, x25, lsl #3
1007dabc4:      str x23, [x26, #0x8]!
1007dabc8:      mov x0, x24
1007dabcc:      bl  0x10083f51c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
1007dabd0:      tbz w0, #0x0, 0x1007dabf4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x280>
1007dabd4:      mov w8, #0x7ffe             ; =32766
1007dabd8:      cmp x8, x23, lsr #48
1007dabdc:      b.ne    0x1007dac18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2a4>
1007dabe0:      mov x0, x23
1007dabe4:      bl  0x1004665f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1007dabe8:      tbnz    w0, #0x0, 0x1007dac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2c0>
1007dabec:      scvtf   d0, w23
1007dabf0:      b   0x1007dac30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2bc>
1007dabf4:      lsr x8, x24, #3
1007dabf8:      cmp x8, #0x201
1007dabfc:      b.lo    0x1007dac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2c0>
1007dac00:      ldurb   w8, [x24, #-0x8]
1007dac04:      cmp w8, #0x1
1007dac08:      b.ne    0x1007dac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2c0>
1007dac0c:      ldurh   w8, [x24, #-0x6]
1007dac10:      tbnz    w8, #0xc, 0x1007dabd4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x260>
1007dac14:      b   0x1007dac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2c0>
1007dac18:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1007dac1c:      cmp x23, x8
1007dac20:      b.gt    0x1007dac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2c0>
1007dac24:      fmov    d0, x23
1007dac28:      fcmp    d0, d0
1007dac2c:      fcsel   d0, d8, d0, vs
1007dac30:      fmov    x23, d0
1007dac34:      str x23, [x26]
1007dac38:      mov w8, #0x7ffe             ; =32766
1007dac3c:      cmp x8, x23, lsr #48
1007dac40:      b.ne    0x1007dac64 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2f0>
1007dac44:      mov x0, x23
1007dac48:      bl  0x1004665f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1007dac4c:      tbnz    w0, #0x0, 0x1007dac70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x2fc>
1007dac50:      scvtf   d0, w23
1007dac54:      lsr x8, x24, #3
1007dac58:      cmp x8, #0x201
1007dac5c:      b.hs    0x1007dacc8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x354>
1007dac60:      b   0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dac64:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1007dac68:      cmp x23, x8
1007dac6c:      b.le    0x1007dacb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x33c>
1007dac70:      lsr x8, x24, #3
1007dac74:      cmp x8, #0x201
1007dac78:      b.lo    0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dac7c:      ldurb   w8, [x24, #-0x8]
1007dac80:      cmp w8, #0x1
1007dac84:      b.ne    0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dac88:      ldurh   w8, [x24, #-0x6]
1007dac8c:      mov w9, #0xef7f             ; =61311
1007dac90:      and w9, w8, w9
1007dac94:      sturh   w9, [x24, #-0x6]
1007dac98:      mov w9, #0x1080             ; =4224
1007dac9c:      tst w8, w9
1007daca0:      b.eq    0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007daca4:      mov x0, x24
1007daca8:      bl  0x10067dfe0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
1007dacac:      b   0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dacb0:      fmov    d0, x23
1007dacb4:      fcmp    d0, d0
1007dacb8:      fcsel   d0, d8, d0, vs
1007dacbc:      lsr x8, x24, #3
1007dacc0:      cmp x8, #0x201
1007dacc4:      b.lo    0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dacc8:      ldurb   w8, [x24, #-0x8]
1007daccc:      cmp w8, #0x1
1007dacd0:      b.ne    0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dacd4:      ldurh   w8, [x24, #-0x6]
1007dacd8:      tbz w8, #0x7, 0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dacdc:      ldr w8, [x24]
1007dace0:      cmp w25, w8
1007dace4:      b.hs    0x1007dacec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x378>
1007dace8:      str d0, [x26]
1007dacec:      mov x0, x24
1007dacf0:      mov x1, x25
1007dacf4:      mov x2, x23
1007dacf8:      bl  0x10019f740 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
1007dacfc:      mov x0, x24
1007dad00:      bl  0x10083f1cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
1007dad04:      cbz w0, 0x1007dad98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x424>
1007dad08:      adrp    x8, 0x101124000 <_perry_global_baseline_worker_ts__1>
1007dad0c:      add x8, x8, #0x770
1007dad10:      ldapr   x8, [x8]
1007dad14:      cbnz    x8, 0x1007dad68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x3f4>
1007dad18:      adrp    x8, 0x101124000 <_perry_global_baseline_worker_ts__1>
1007dad1c:      ldrb    w8, [x8, #0x778]
1007dad20:      tbz w8, #0x0, 0x1007dad80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x40c>
1007dad24:      mov x0, x24
1007dad28:      mov x1, x26
1007dad2c:      mov x2, x23
1007dad30:      mov w3, #0x0                ; =0
1007dad34:      bl  0x10034a3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1007dad38:      b   0x1007dad98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x424>
1007dad3c:      cmp w8, #0x2
1007dad40:      b.eq    0x1007db054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6e0>
1007dad44:      mov x0, x19
1007dad48:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007dad4c:      add x1, x1, #0x36c
1007dad50:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007dad54:      strb    wzr, [x19, #0x20]
1007dad58:      ldr x8, [x19]
1007dad5c:      cmp x8, x26
1007dad60:      b.lo    0x1007dab68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x1f4>
1007dad64:      b   0x1007dafd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x664>
1007dad68:      adrp    x0, 0x101124000 <_perry_global_baseline_worker_ts__1>
1007dad6c:      add x0, x0, #0x770
1007dad70:      bl  0x100cb7490 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1007dad74:      adrp    x8, 0x101124000 <_perry_global_baseline_worker_ts__1>
1007dad78:      ldrb    w8, [x8, #0x778]
1007dad7c:      tbnz    w8, #0x0, 0x1007dad24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x3b0>
1007dad80:      adrp    x8, 0x1011f0000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fc30>
1007dad84:      add x8, x8, #0x5a0
1007dad88:      ldr w8, [x8]
1007dad8c:      cbz w8, 0x1007dad98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x424>
1007dad90:      mov x0, x23
1007dad94:      bl  0x10034b378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
1007dad98:      add w8, w25, #0x1
1007dad9c:      str w8, [x24]
1007dada0:      mov x26, #0x7fffffffffffffff ; =9223372036854775807
1007dada4:      ldrb    w8, [x19, #0x20]
1007dada8:      cbnz    w8, 0x1007dae68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x4f4>
1007dadac:      ldr x8, [x19]
1007dadb0:      cbnz    x8, 0x1007dae8c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x518>
1007dadb4:      str x21, [x19]
1007dadb8:      ldr x8, [x19, #0x18]
1007dadbc:      cmp x28, x8
1007dadc0:      b.hs    0x1007dadf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x47c>
1007dadc4:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1007dadc8:      orr x8, x24, x8
1007dadcc:      ldr x9, [x19, #0x10]
1007dadd0:      str x8, [x9, x28, lsl #3]
1007dadd4:      ldr x8, [x19]
1007dadd8:      add x8, x8, #0x1
1007daddc:      str x8, [x19]
1007dade0:      ldp x9, x8, [x20, #0x30]
1007dade4:      cmp x8, x9
1007dade8:      b.lo    0x1007dae04 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x490>
1007dadec:      b   0x1007dae30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x4bc>
1007dadf0:      mov x8, #0x0                ; =0
1007dadf4:      str x8, [x19]
1007dadf8:      ldp x9, x8, [x20, #0x30]
1007dadfc:      cmp x8, x9
1007dae00:      b.hs    0x1007dae30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x4bc>
1007dae04:      ldr x10, [x20, #0x28]
1007dae08:      ldrb    w11, [x10, x8]
1007dae0c:      cmp w11, #0x20
1007dae10:      b.hi    0x1007dae30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x4bc>
1007dae14:      lsr x11, x22, x11
1007dae18:      tbz w11, #0x0, 0x1007dae30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x4bc>
1007dae1c:      add x8, x8, #0x1
1007dae20:      str x8, [x20, #0x38]
1007dae24:      cmp x9, x8
1007dae28:      b.ne    0x1007dae08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x494>
1007dae2c:      b   0x1007daeec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x578>
1007dae30:      cmp x8, x9
1007dae34:      b.hs    0x1007dae9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x528>
1007dae38:      ldr x10, [x20, #0x28]
1007dae3c:      ldrb    w10, [x10, x8]
1007dae40:      cmp w10, #0x2c
1007dae44:      b.ne    0x1007dae9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x528>
1007dae48:      add x8, x8, #0x1
1007dae4c:      str x8, [x20, #0x38]
1007dae50:      mov x0, x20
1007dae54:      bl  0x1007db080 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1007dae58:      mov x23, x0
1007dae5c:      ldrb    w8, [x20, #0x90]
1007dae60:      tbnz    w8, #0x0, 0x1007dab54 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x1e0>
1007dae64:      b   0x1007dae98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x524>
1007dae68:      cmp w8, #0x2
1007dae6c:      b.eq    0x1007db054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6e0>
1007dae70:      mov x0, x19
1007dae74:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007dae78:      add x1, x1, #0x36c
1007dae7c:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007dae80:      strb    wzr, [x19, #0x20]
1007dae84:      ldr x8, [x19]
1007dae88:      cbz x8, 0x1007dadb4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x440>
1007dae8c:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1007dae90:      add x0, x0, #0xde0
1007dae94:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1007dae98:      ldp x9, x8, [x20, #0x30]
1007dae9c:      cmp x8, x9
1007daea0:      b.hs    0x1007daeec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x578>
1007daea4:      ldr x10, [x20, #0x28]
1007daea8:      mov x11, #0x2600            ; =9728
1007daeac:      movk    x11, #0x1, lsl #32
1007daeb0:      ldrb    w12, [x10, x8]
1007daeb4:      cmp w12, #0x20
1007daeb8:      b.hi    0x1007daed8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x564>
1007daebc:      lsr x13, x11, x12
1007daec0:      tbz w13, #0x0, 0x1007daed8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x564>
1007daec4:      add x8, x8, #0x1
1007daec8:      str x8, [x20, #0x38]
1007daecc:      cmp x9, x8
1007daed0:      b.ne    0x1007daeb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x53c>
1007daed4:      b   0x1007daeec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x578>
1007daed8:      cmp w12, #0x5d
1007daedc:      b.ne    0x1007daeec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x578>
1007daee0:      add x8, x8, #0x1
1007daee4:      str x8, [x20, #0x38]
1007daee8:      b   0x1007daef0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x57c>
1007daeec:      strb    wzr, [x20, #0x90]
1007daef0:      ldrb    w8, [x19, #0x20]
1007daef4:      cbnz    w8, 0x1007dafac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x638>
1007daef8:      ldr x8, [x19]
1007daefc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007daf00:      cmp x8, x9
1007daf04:      b.hs    0x1007dafd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x664>
1007daf08:      add x9, x8, #0x1
1007daf0c:      str x9, [x19]
1007daf10:      ldr x9, [x19, #0x18]
1007daf14:      cmp x28, x9
1007daf18:      b.hs    0x1007daf30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x5bc>
1007daf1c:      ldr x9, [x19, #0x10]
1007daf20:      ldr x9, [x9, x28, lsl #3]
1007daf24:      mov x21, #0x7ffd000000000000 ; =9222527611924643840
1007daf28:      bfxil   x21, x9, #0, #48
1007daf2c:      b   0x1007daf38 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x5c4>
1007daf30:      mov x21, #0x1               ; =1
1007daf34:      movk    x21, #0x7ffd, lsl #48
1007daf38:      str x8, [x19]
1007daf3c:      cbnz    x8, 0x1007dab1c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x1a8>
1007daf40:      ldr x8, [x19, #0x18]
1007daf44:      cmp x27, x8
1007daf48:      b.hi    0x1007daf50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x5dc>
1007daf4c:      str x27, [x19, #0x18]
1007daf50:      mov x0, x21
1007daf54:      ldp x29, x30, [sp, #0x60]
1007daf58:      ldp x20, x19, [sp, #0x50]
1007daf5c:      ldp x22, x21, [sp, #0x40]
1007daf60:      ldp x24, x23, [sp, #0x30]
1007daf64:      ldp x26, x25, [sp, #0x20]
1007daf68:      ldp x28, x27, [sp, #0x10]
1007daf6c:      ldp d9, d8, [sp], #0x70
1007daf70:      ret
1007daf74:      cmp w8, #0x2
1007daf78:      b.eq    0x1007db054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6e0>
1007daf7c:      mov x21, x0
1007daf80:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007daf84:      add x1, x1, #0x36c
1007daf88:      mov x0, x19
1007daf8c:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007daf90:      strb    wzr, [x19, #0x20]
1007daf94:      mov x0, x21
1007daf98:      ldr x8, [x19]
1007daf9c:      cbz x8, 0x1007daab0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x13c>
1007dafa0:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1007dafa4:      add x0, x0, #0xdf8
1007dafa8:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1007dafac:      cmp w8, #0x2
1007dafb0:      b.eq    0x1007db054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6e0>
1007dafb4:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007dafb8:      add x1, x1, #0x36c
1007dafbc:      mov x0, x19
1007dafc0:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007dafc4:      strb    wzr, [x19, #0x20]
1007dafc8:      ldr x8, [x19]
1007dafcc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007dafd0:      cmp x8, x9
1007dafd4:      b.lo    0x1007daf08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x594>
1007dafd8:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1007dafdc:      add x0, x0, #0xdc8
1007dafe0:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1007dafe4:      cmp w8, #0x2
1007dafe8:      b.eq    0x1007db054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6e0>
1007dafec:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007daff0:      add x1, x1, #0x36c
1007daff4:      mov x0, x19
1007daff8:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007daffc:      strb    wzr, [x19, #0x20]
1007db000:      ldr x8, [x19]
1007db004:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007db008:      cmp x8, x9
1007db00c:      b.lo    0x1007daa3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xc8>
1007db010:      b   0x1007db040 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6cc>
1007db014:      cmp w8, #0x1
1007db018:      b.ne    0x1007db054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6e0>
1007db01c:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007db020:      add x1, x1, #0x36c
1007db024:      mov x0, x19
1007db028:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007db02c:      strb    wzr, [x19, #0x20]
1007db030:      ldr x8, [x19]
1007db034:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007db038:      cmp x8, x9
1007db03c:      b.lo    0x1007daa08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x94>
1007db040:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1007db044:      add x0, x0, #0xe70
1007db048:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1007db04c:      cmp w9, #0x2
1007db050:      b.ne    0x1007db060 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x6ec>
1007db054:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1007db058:      add x0, x0, #0xed8
1007db05c:      bl  0x100cd1edc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1007db060:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1007db064:      add x1, x1, #0x36c
1007db068:      mov x0, x19
1007db06c:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007db070:      strb    wzr, [x19, #0x20]
1007db074:      ldr x8, [x19]
1007db078:      cbz x8, 0x1007daf40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x5cc>
1007db07c:      b   0x1007dab1c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x1a8>
