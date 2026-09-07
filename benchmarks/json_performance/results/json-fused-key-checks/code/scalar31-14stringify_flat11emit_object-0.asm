/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bcbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object>:
1004bcbc0:      stp x28, x27, [sp, #-0x60]!
1004bcbc4:      stp x26, x25, [sp, #0x10]
1004bcbc8:      stp x24, x23, [sp, #0x20]
1004bcbcc:      stp x22, x21, [sp, #0x30]
1004bcbd0:      stp x20, x19, [sp, #0x40]
1004bcbd4:      stp x29, x30, [sp, #0x50]
1004bcbd8:      add x29, sp, #0x50
1004bcbdc:      sub sp, sp, #0x1a0
1004bcbe0:      mov x19, x1
1004bcbe4:      mov x20, x0
1004bcbe8:      movi.2d v0, #0000000000000000
1004bcbec:      str d0, [sp]
1004bcbf0:      str wzr, [sp, #0x8]
1004bcbf4:      str d0, [sp, #0x28]
1004bcbf8:      str wzr, [sp, #0x30]
1004bcbfc:      str d0, [sp, #0x50]
1004bcc00:      str wzr, [sp, #0x58]
1004bcc04:      str d0, [sp, #0x78]
1004bcc08:      str wzr, [sp, #0x80]
1004bcc0c:      str d0, [sp, #0xa0]
1004bcc10:      str wzr, [sp, #0xa8]
1004bcc14:      str d0, [sp, #0xc8]
1004bcc18:      str wzr, [sp, #0xd0]
1004bcc1c:      str d0, [sp, #0xf0]
1004bcc20:      str wzr, [sp, #0xf8]
1004bcc24:      str d0, [sp, #0x118]
1004bcc28:      mov w8, #-0x40000000        ; =-1073741824
1004bcc2c:      ldr w9, [x0, #0x4]
1004bcc30:      cmp w9, w8
1004bcc34:      csel    w21, w9, w8, lt
1004bcc38:      str wzr, [sp, #0x120]
1004bcc3c:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bcc40:      adrp    x25, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bcc44:      ldr w22, [x25, #0x634]
1004bcc48:      cmp w22, #0x300
1004bcc4c:      b.hs    0x1004bd0fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x53c>
1004bcc50:      ldr x8, [x24, #0x48]
1004bcc54:      cmn x8, #0x1
1004bcc58:      b.eq    0x1004bd0ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x52c>
1004bcc5c:      mrs x9, TPIDRRO_EL0
1004bcc60:      and x9, x9, #0xfffffffffffffff8
1004bcc64:      ldr x0, [x9, x8, lsl #3]
1004bcc68:      cbz x0, 0x1004bd0ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x52c>
1004bcc6c:      add x8, x0, x22, lsl #3
1004bcc70:      ldr x0, [x8, #0x1e8]
1004bcc74:      cbz x0, 0x1004bd0fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x53c>
1004bcc78:      ldr x0, [x0]
1004bcc7c:      cbz x0, 0x1004bd110 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x550>
1004bcc80:      ldr x8, [x0, #0x5190]
1004bcc84:      and w9, w21, #0x3fffffff
1004bcc88:      lsr x10, x9, #15
1004bcc8c:      ubfx    x11, x9, #5, #10
1004bcc90:      and x9, x9, #0x1f
1004bcc94:      ldr x8, [x8, x10, lsl #3]
1004bcc98:      ldr x8, [x8, x11, lsl #3]
1004bcc9c:      lsl x9, x9, #5
1004bcca0:      ldr x27, [x8, x9]
1004bcca4:      ldr x1, [x27, #0x8]
1004bcca8:      sub x0, x29, #0x88
1004bccac:      bl  0x1004bd710 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece>
1004bccb0:      ldur    w8, [x29, #-0x88]
1004bccb4:      cmn w8, #0x1
1004bccb8:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bccbc:      add x23, sp, #0xa0
1004bccc0:      ldur    x8, [x29, #-0x68]
1004bccc4:      ldur    q0, [x23, #0xd8]
1004bccc8:      ldur    q1, [x23, #0xc8]
1004bcccc:      stp q1, q0, [sp]
1004bccd0:      str x8, [sp, #0x20]
1004bccd4:      ldr x1, [x20, #0x10]
1004bccd8:      sub x0, x29, #0x88
1004bccdc:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bcce0:      ldur    w8, [x29, #-0x88]
1004bcce4:      cmn w8, #0x1
1004bcce8:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bccec:      ldur    x8, [x29, #-0x68]
1004bccf0:      ldur    q0, [x23, #0xd8]
1004bccf4:      ldur    q1, [x23, #0xc8]
1004bccf8:      stp q1, q0, [sp, #0xa0]
1004bccfc:      str x8, [sp, #0xc0]
1004bcd00:      ldp w10, w8, [sp]
1004bcd04:      ldr w9, [sp, #0x8]
1004bcd08:      cbz w10, 0x1004bcd34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x174>
1004bcd0c:      cmp w10, #0x1
1004bcd10:      b.ne    0x1004bcd5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x19c>
1004bcd14:      ldr w8, [sp, #0xc]
1004bcd18:      ldp w12, w10, [sp, #0xa0]
1004bcd1c:      ldr w11, [sp, #0xa8]
1004bcd20:      cbz w12, 0x1004bcd4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x18c>
1004bcd24:      cmp w12, #0x1
1004bcd28:      b.ne    0x1004bcd70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x1b0>
1004bcd2c:      ldr w10, [sp, #0xac]
1004bcd30:      b   0x1004bcd74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x1b4>
1004bcd34:      add w10, w8, #0x2
1004bcd38:      add w8, w9, #0x2
1004bcd3c:      mov x9, x10
1004bcd40:      ldp w12, w10, [sp, #0xa0]
1004bcd44:      ldr w11, [sp, #0xa8]
1004bcd48:      cbnz    w12, 0x1004bcd24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x164>
1004bcd4c:      add w12, w10, #0x2
1004bcd50:      add w10, w11, #0x2
1004bcd54:      mov x11, x12
1004bcd58:      b   0x1004bcd74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x1b4>
1004bcd5c:      mov x9, x8
1004bcd60:      ldp w12, w10, [sp, #0xa0]
1004bcd64:      ldr w11, [sp, #0xa8]
1004bcd68:      cbnz    w12, 0x1004bcd24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x164>
1004bcd6c:      b   0x1004bcd4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x18c>
1004bcd70:      mov x11, x10
1004bcd74:      cmn w9, #0x3
1004bcd78:      b.hi    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bcd7c:      add w9, w9, #0x2
1004bcd80:      adds    w9, w11, w9
1004bcd84:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bcd88:      mov x0, #0x0                ; =0
1004bcd8c:      adds    w21, w9, #0x1
1004bcd90:      b.hs    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bcd94:      cmn w8, #0x3
1004bcd98:      b.hi    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bcd9c:      mov x0, #0x0                ; =0
1004bcda0:      add w8, w8, #0x2
1004bcda4:      adds    w28, w10, w8
1004bcda8:      b.hs    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bcdac:      cmn w28, #0x1
1004bcdb0:      b.eq    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bcdb4:      add w26, w28, #0x1
1004bcdb8:      cmp x19, #0x1
1004bcdbc:      b.ne    0x1004bce80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x2c0>
1004bcdc0:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004bcdc4:      stur    x0, [x29, #-0xb0]
1004bcdc8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1004bcdcc:      stp x20, x8, [x29, #-0x80]
1004bcdd0:      mov w8, #0x1                ; =1
1004bcdd4:      stur    x8, [x29, #-0x88]
1004bcdd8:      sub x0, x29, #0x88
1004bcddc:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004bcde0:      mov x23, x0
1004bcde4:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004bcde8:      add x0, x0, #0xbf0
1004bcdec:      ldr x8, [x0]
1004bcdf0:      blr x8
1004bcdf4:      strb    wzr, [x0]
1004bcdf8:      mov x0, x20
1004bcdfc:      bl  0x1004c2b40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
1004bce00:      tbz w0, #0x0, 0x1004bcefc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x33c>
1004bce04:      mov x0, x21
1004bce08:      bl  0x10032570c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004bce0c:      mov x20, x0
1004bce10:      mov x22, x1
1004bce14:      ldr x8, [x24, #0x48]
1004bce18:      cmn x8, #0x1
1004bce1c:      b.eq    0x1004bcf2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x36c>
1004bce20:      mrs x9, TPIDRRO_EL0
1004bce24:      and x9, x9, #0xfffffffffffffff8
1004bce28:      ldr x8, [x9, x8, lsl #3]
1004bce2c:      cbz x8, 0x1004bcf2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x36c>
1004bce30:      ldr x8, [x8, #0x19e8]
1004bce34:      cbz x8, 0x1004bcf2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x36c>
1004bce38:      ldr x9, [x8]
1004bce3c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004bce40:      cmp x9, x10
1004bce44:      b.hs    0x1004bd3d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x810>
1004bce48:      add x10, x9, #0x1
1004bce4c:      str x10, [x8]
1004bce50:      ldr x10, [x8, #0x18]
1004bce54:      cmp x23, x10
1004bce58:      b.hs    0x1004bd3dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x81c>
1004bce5c:      ldr x10, [x8, #0x10]
1004bce60:      mov w11, #0x18              ; =24
1004bce64:      madd    x10, x23, x11, x10
1004bce68:      ldr x11, [x10]
1004bce6c:      cmp x11, #0x1
1004bce70:      b.ne    0x1004bd3e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x820>
1004bce74:      ldr x23, [x10, #0x8]
1004bce78:      str x9, [x8]
1004bce7c:      b   0x1004bcf38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x378>
1004bce80:      ldr x1, [x27, #0x10]
1004bce84:      sub x0, x29, #0x88
1004bce88:      bl  0x1004bd710 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece>
1004bce8c:      ldur    w8, [x29, #-0x88]
1004bce90:      cmn w8, #0x1
1004bce94:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bce98:      ldur    x8, [x29, #-0x68]
1004bce9c:      ldur    q0, [x23, #0xd8]
1004bcea0:      ldur    q1, [x23, #0xc8]
1004bcea4:      stur    q1, [sp, #0x28]
1004bcea8:      stur    q0, [sp, #0x38]
1004bceac:      str x8, [sp, #0x48]
1004bceb0:      ldr x1, [x20, #0x18]
1004bceb4:      sub x0, x29, #0x88
1004bceb8:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bcebc:      ldur    w8, [x29, #-0x88]
1004bcec0:      cmn w8, #0x1
1004bcec4:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bcec8:      ldur    x8, [x29, #-0x68]
1004bcecc:      ldur    q0, [x23, #0xd8]
1004bced0:      ldur    q1, [x23, #0xc8]
1004bced4:      stur    q1, [x23, #0x28]
1004bced8:      stur    q0, [x23, #0x38]
1004bcedc:      str x8, [sp, #0xe8]
1004bcee0:      ldp w10, w8, [sp, #0x28]
1004bcee4:      ldr w9, [sp, #0x30]
1004bcee8:      cbz w10, 0x1004bd118 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x558>
1004bceec:      cmp w10, #0x1
1004bcef0:      b.ne    0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x568>
1004bcef4:      ldr w8, [sp, #0x34]
1004bcef8:      b   0x1004bd12c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x56c>
1004bcefc:      sub x0, x29, #0xb0
1004bcf00:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004bcf04:      mov x0, #0x0                ; =0
1004bcf08:      mov x1, x22
1004bcf0c:      add sp, sp, #0x1a0
1004bcf10:      ldp x29, x30, [sp, #0x50]
1004bcf14:      ldp x20, x19, [sp, #0x40]
1004bcf18:      ldp x22, x21, [sp, #0x30]
1004bcf1c:      ldp x24, x23, [sp, #0x20]
1004bcf20:      ldp x26, x25, [sp, #0x10]
1004bcf24:      ldp x28, x27, [sp], #0x60
1004bcf28:      ret
1004bcf2c:      mov x0, x23
1004bcf30:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004bcf34:      mov x23, x0
1004bcf38:      ldr w8, [x23, #0x4]
1004bcf3c:      mov w9, #-0x40000000        ; =-1073741824
1004bcf40:      cmp w8, w9
1004bcf44:      csel    w27, w8, w9, lt
1004bcf48:      ldr w25, [x25, #0x634]
1004bcf4c:      cmp w25, #0x300
1004bcf50:      b.hs    0x1004bd228 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x668>
1004bcf54:      ldr x8, [x24, #0x48]
1004bcf58:      cmn x8, #0x1
1004bcf5c:      b.eq    0x1004bd218 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x658>
1004bcf60:      mrs x9, TPIDRRO_EL0
1004bcf64:      and x9, x9, #0xfffffffffffffff8
1004bcf68:      ldr x0, [x9, x8, lsl #3]
1004bcf6c:      cbz x0, 0x1004bd218 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x658>
1004bcf70:      add x8, x0, x25, lsl #3
1004bcf74:      ldr x0, [x8, #0x1e8]
1004bcf78:      cbz x0, 0x1004bd228 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x668>
1004bcf7c:      ldr x0, [x0]
1004bcf80:      cbz x0, 0x1004bd23c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x67c>
1004bcf84:      ldr x8, [x0, #0x5190]
1004bcf88:      and w9, w27, #0x3fffffff
1004bcf8c:      lsr x10, x9, #15
1004bcf90:      ubfx    x11, x9, #5, #10
1004bcf94:      and x9, x9, #0x1f
1004bcf98:      ldr x8, [x8, x10, lsl #3]
1004bcf9c:      ldr x8, [x8, x11, lsl #3]
1004bcfa0:      lsl x9, x9, #5
1004bcfa4:      ldr x24, [x8, x9]
1004bcfa8:      stp w26, w21, [x20]
1004bcfac:      stp wzr, wzr, [x20, #0xc]
1004bcfb0:      str w21, [x20, #0x8]
1004bcfb4:      mov w8, #0x7b               ; =123
1004bcfb8:      mov x2, x22
1004bcfbc:      strb    w8, [x2], #0x1
1004bcfc0:      ldr x1, [x24, #0x8]
1004bcfc4:      mov x21, sp
1004bcfc8:      mov x0, sp
1004bcfcc:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bcfd0:      add x8, x22, x0
1004bcfd4:      mov w25, #0x3a              ; =58
1004bcfd8:      strb    w25, [x8, #0x1]
1004bcfdc:      add x27, x0, #0x2
1004bcfe0:      ldr x1, [x23, #0x10]
1004bcfe4:      add x26, sp, #0xa0
1004bcfe8:      add x0, sp, #0xa0
1004bcfec:      add x2, x22, x27
1004bcff0:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bcff4:      add x8, x0, x27
1004bcff8:      cmp x19, #0x1
1004bcffc:      b.eq    0x1004bd0cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x50c>
1004bd000:      mov w27, #0x2c              ; =44
1004bd004:      strb    w27, [x22, x8]
1004bd008:      add x28, x8, #0x1
1004bd00c:      ldr x1, [x24, #0x10]
1004bd010:      add x0, x21, #0x28
1004bd014:      add x2, x22, x28
1004bd018:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd01c:      add x8, x0, x28
1004bd020:      strb    w25, [x22, x8]
1004bd024:      add x21, x8, #0x1
1004bd028:      ldr x1, [x23, #0x18]
1004bd02c:      add x0, x26, #0x28
1004bd030:      add x2, x22, x21
1004bd034:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd038:      add x8, x0, x21
1004bd03c:      cmp x19, #0x2
1004bd040:      b.eq    0x1004bd0cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x50c>
1004bd044:      strb    w27, [x22, x8]
1004bd048:      add x25, x8, #0x1
1004bd04c:      mov x21, sp
1004bd050:      ldr x1, [x24, #0x18]
1004bd054:      add x0, x21, #0x50
1004bd058:      add x2, x22, x25
1004bd05c:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd060:      add x8, x0, x25
1004bd064:      mov w26, #0x3a              ; =58
1004bd068:      strb    w26, [x22, x8]
1004bd06c:      add x27, x8, #0x1
1004bd070:      add x25, sp, #0xa0
1004bd074:      ldr x1, [x23, #0x20]
1004bd078:      add x0, x25, #0x50
1004bd07c:      add x2, x22, x27
1004bd080:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd084:      add x8, x0, x27
1004bd088:      cmp x19, #0x3
1004bd08c:      b.eq    0x1004bd0cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x50c>
1004bd090:      mov w9, #0x2c               ; =44
1004bd094:      strb    w9, [x22, x8]
1004bd098:      add x19, x8, #0x1
1004bd09c:      ldr x1, [x24, #0x20]
1004bd0a0:      add x0, x21, #0x78
1004bd0a4:      add x2, x22, x19
1004bd0a8:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd0ac:      add x8, x0, x19
1004bd0b0:      strb    w26, [x22, x8]
1004bd0b4:      add x19, x8, #0x1
1004bd0b8:      ldr x1, [x23, #0x28]
1004bd0bc:      add x0, x25, #0x78
1004bd0c0:      add x2, x22, x19
1004bd0c4:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004bd0c8:      add x8, x0, x19
1004bd0cc:      mov w9, #0x7d               ; =125
1004bd0d0:      strb    w9, [x22, x8]
1004bd0d4:      mov x22, #0x7fff000000000000 ; =9223090561878065152
1004bd0d8:      bfxil   x22, x20, #0, #48
1004bd0dc:      sub x0, x29, #0xb0
1004bd0e0:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004bd0e4:      mov w0, #0x1                ; =1
1004bd0e8:      b   0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd0ec:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004bd0f0:      add x8, x0, x22, lsl #3
1004bd0f4:      ldr x0, [x8, #0x1e8]
1004bd0f8:      cbnz    x0, 0x1004bcc78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0xb8>
1004bd0fc:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
1004bd100:      add x0, x0, #0x330
1004bd104:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004bd108:      ldr x0, [x0]
1004bd10c:      cbnz    x0, 0x1004bcc80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0xc0>
1004bd110:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004bd114:      b   0x1004bcc80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0xc0>
1004bd118:      add w10, w8, #0x2
1004bd11c:      add w8, w9, #0x2
1004bd120:      mov x9, x10
1004bd124:      b   0x1004bd12c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x56c>
1004bd128:      mov x9, x8
1004bd12c:      ldp w12, w10, [sp, #0xc8]
1004bd130:      ldr w11, [sp, #0xd0]
1004bd134:      cbz w12, 0x1004bd148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x588>
1004bd138:      cmp w12, #0x1
1004bd13c:      b.ne    0x1004bd158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x598>
1004bd140:      ldr w10, [sp, #0xd4]
1004bd144:      b   0x1004bd15c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x59c>
1004bd148:      add w12, w10, #0x2
1004bd14c:      add w10, w11, #0x2
1004bd150:      mov x11, x12
1004bd154:      b   0x1004bd15c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x59c>
1004bd158:      mov x11, x10
1004bd15c:      adds    w9, w9, w21
1004bd160:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd164:      adds    w9, w11, w9
1004bd168:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd16c:      cmn w9, #0x3
1004bd170:      b.hi    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd174:      add w8, w8, w26
1004bd178:      cmp w8, w28
1004bd17c:      b.ls    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd180:      mov x0, #0x0                ; =0
1004bd184:      adds    w8, w10, w8
1004bd188:      b.hs    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd18c:      cmn w8, #0x3
1004bd190:      b.hi    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd194:      add w21, w9, #0x2
1004bd198:      add w26, w8, #0x2
1004bd19c:      cmp x19, #0x2
1004bd1a0:      b.eq    0x1004bcdc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x200>
1004bd1a4:      ldr x1, [x27, #0x18]
1004bd1a8:      sub x0, x29, #0x88
1004bd1ac:      bl  0x1004bd710 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece>
1004bd1b0:      ldur    w8, [x29, #-0x88]
1004bd1b4:      cmn w8, #0x1
1004bd1b8:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd1bc:      ldur    x8, [x29, #-0x68]
1004bd1c0:      ldur    q0, [x23, #0xd8]
1004bd1c4:      ldur    q1, [x23, #0xc8]
1004bd1c8:      stp q1, q0, [sp, #0x50]
1004bd1cc:      str x8, [sp, #0x70]
1004bd1d0:      ldr x1, [x20, #0x20]
1004bd1d4:      sub x0, x29, #0x88
1004bd1d8:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bd1dc:      ldur    w8, [x29, #-0x88]
1004bd1e0:      cmn w8, #0x1
1004bd1e4:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd1e8:      ldur    x8, [x29, #-0x68]
1004bd1ec:      ldur    q0, [x23, #0xd8]
1004bd1f0:      ldur    q1, [x23, #0xc8]
1004bd1f4:      stp q1, q0, [sp, #0xf0]
1004bd1f8:      str x8, [sp, #0x110]
1004bd1fc:      ldp w10, w8, [sp, #0x50]
1004bd200:      ldr w9, [sp, #0x58]
1004bd204:      cbz w10, 0x1004bd244 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x684>
1004bd208:      cmp w10, #0x1
1004bd20c:      b.ne    0x1004bd254 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x694>
1004bd210:      ldr w8, [sp, #0x5c]
1004bd214:      b   0x1004bd258 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x698>
1004bd218:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004bd21c:      add x8, x0, x25, lsl #3
1004bd220:      ldr x0, [x8, #0x1e8]
1004bd224:      cbnz    x0, 0x1004bcf7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x3bc>
1004bd228:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
1004bd22c:      add x0, x0, #0x330
1004bd230:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004bd234:      ldr x0, [x0]
1004bd238:      cbnz    x0, 0x1004bcf84 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x3c4>
1004bd23c:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004bd240:      b   0x1004bcf84 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x3c4>
1004bd244:      add w10, w8, #0x2
1004bd248:      add w8, w9, #0x2
1004bd24c:      mov x9, x10
1004bd250:      b   0x1004bd258 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x698>
1004bd254:      mov x9, x8
1004bd258:      ldp w12, w10, [sp, #0xf0]
1004bd25c:      ldr w11, [sp, #0xf8]
1004bd260:      cbz w12, 0x1004bd274 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x6b4>
1004bd264:      cmp w12, #0x1
1004bd268:      b.ne    0x1004bd284 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x6c4>
1004bd26c:      ldr w10, [sp, #0xfc]
1004bd270:      b   0x1004bd288 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x6c8>
1004bd274:      add w12, w10, #0x2
1004bd278:      add w10, w11, #0x2
1004bd27c:      mov x11, x12
1004bd280:      b   0x1004bd288 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x6c8>
1004bd284:      mov x11, x10
1004bd288:      adds    w9, w9, w21
1004bd28c:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd290:      adds    w9, w11, w9
1004bd294:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd298:      cmn w9, #0x3
1004bd29c:      b.hi    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd2a0:      adds    w8, w8, w26
1004bd2a4:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd2a8:      mov x0, #0x0                ; =0
1004bd2ac:      adds    w8, w10, w8
1004bd2b0:      b.hs    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd2b4:      cmn w8, #0x3
1004bd2b8:      b.hi    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd2bc:      add w21, w9, #0x2
1004bd2c0:      add w26, w8, #0x2
1004bd2c4:      cmp x19, #0x3
1004bd2c8:      b.eq    0x1004bcdc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x200>
1004bd2cc:      ldr x1, [x27, #0x20]
1004bd2d0:      sub x0, x29, #0x88
1004bd2d4:      bl  0x1004bd710 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12string_piece>
1004bd2d8:      ldur    w8, [x29, #-0x88]
1004bd2dc:      cmn w8, #0x1
1004bd2e0:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd2e4:      ldur    x8, [x29, #-0x68]
1004bd2e8:      ldur    q0, [x23, #0xd8]
1004bd2ec:      ldur    q1, [x23, #0xc8]
1004bd2f0:      stur    q1, [sp, #0x78]
1004bd2f4:      stur    q0, [sp, #0x88]
1004bd2f8:      str x8, [sp, #0x98]
1004bd2fc:      ldr x1, [x20, #0x28]
1004bd300:      sub x0, x29, #0x88
1004bd304:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004bd308:      ldur    w8, [x29, #-0x88]
1004bd30c:      cmn w8, #0x1
1004bd310:      b.eq    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd314:      ldur    x8, [x29, #-0x68]
1004bd318:      ldur    q0, [x23, #0xd8]
1004bd31c:      ldur    q1, [x23, #0xc8]
1004bd320:      stur    q1, [x23, #0x78]
1004bd324:      stur    q0, [x23, #0x88]
1004bd328:      str x8, [sp, #0x138]
1004bd32c:      ldp w10, w8, [sp, #0x78]
1004bd330:      ldr w9, [sp, #0x80]
1004bd334:      cbz w10, 0x1004bd348 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x788>
1004bd338:      cmp w10, #0x1
1004bd33c:      b.ne    0x1004bd358 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x798>
1004bd340:      ldr w8, [sp, #0x84]
1004bd344:      b   0x1004bd35c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x79c>
1004bd348:      add w10, w8, #0x2
1004bd34c:      add w8, w9, #0x2
1004bd350:      mov x9, x10
1004bd354:      b   0x1004bd35c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x79c>
1004bd358:      mov x9, x8
1004bd35c:      ldr w12, [sp, #0x118]
1004bd360:      ldr w10, [sp, #0x11c]
1004bd364:      ldr w11, [sp, #0x120]
1004bd368:      cbz w12, 0x1004bd37c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x7bc>
1004bd36c:      cmp w12, #0x1
1004bd370:      b.ne    0x1004bd38c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x7cc>
1004bd374:      ldr w10, [sp, #0x124]
1004bd378:      b   0x1004bd390 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x7d0>
1004bd37c:      add w12, w10, #0x2
1004bd380:      add w10, w11, #0x2
1004bd384:      mov x11, x12
1004bd388:      b   0x1004bd390 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x7d0>
1004bd38c:      mov x11, x10
1004bd390:      adds    w9, w9, w21
1004bd394:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd398:      adds    w9, w11, w9
1004bd39c:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd3a0:      cmn w9, #0x3
1004bd3a4:      b.hi    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd3a8:      adds    w8, w8, w26
1004bd3ac:      b.hs    0x1004bcf04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x344>
1004bd3b0:      mov x0, #0x0                ; =0
1004bd3b4:      adds    w8, w10, w8
1004bd3b8:      b.hs    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd3bc:      cmn w8, #0x3
1004bd3c0:      b.hi    0x1004bcf08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x348>
1004bd3c4:      add w21, w9, #0x2
1004bd3c8:      add w26, w8, #0x2
1004bd3cc:      b   0x1004bcdc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat11emit_object+0x200>
1004bd3d0:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004bd3d4:      add x0, x0, #0x8a8
1004bd3d8:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004bd3dc:      bl  0x100ae2488 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004bd3e0:      adrp    x0, 0x100bd8000 <_PERRY_EMPTY_STRING+0x2f0>
1004bd3e4:      add x0, x0, #0x96c
1004bd3e8:      mov w1, #0xb                ; =11
1004bd3ec:      bl  0x100ae2450 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
