/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081dac0 <_js_object_alloc_with_shape>:
10081dac0:      sub sp, sp, #0xb0
10081dac4:      stp x28, x27, [sp, #0x50]
10081dac8:      stp x26, x25, [sp, #0x60]
10081dacc:      stp x24, x23, [sp, #0x70]
10081dad0:      stp x22, x21, [sp, #0x80]
10081dad4:      stp x20, x19, [sp, #0x90]
10081dad8:      stp x29, x30, [sp, #0xa0]
10081dadc:      add x29, sp, #0xa0
10081dae0:      mov x23, x3
10081dae4:      mov x22, x2
10081dae8:      mov x19, x1
10081daec:      mov x20, x0
10081daf0:      mov w8, #0x2                ; =2
10081daf4:      cmp w1, #0x2
10081daf8:      csel    w26, w1, w8, hi
10081dafc:      ubfiz   x8, x26, #3, #32
10081db00:      mov w9, #0x3ffd             ; =16381
10081db04:      adrp    x27, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081db08:      cmp w1, w9
10081db0c:      b.ls    0x10081db34 <_js_object_alloc_with_shape+0x74>
10081db10:      add x0, x8, #0x10
10081db14:      mov w1, #0x8                ; =8
10081db18:      mov w2, #0x2                ; =2
10081db1c:      bl  0x1004da658 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081db20:      mov x24, x0
10081db24:      ldurb   w8, [x0, #-0x7]
10081db28:      orr w8, w8, #0x20
10081db2c:      sturb   w8, [x0, #-0x7]
10081db30:      b   0x10081dca0 <_js_object_alloc_with_shape+0x1e0>
10081db34:      add x21, x8, #0x18
10081db38:      ldr x8, [x27, #0x48]
10081db3c:      cmn x8, #0x1
10081db40:      b.eq    0x10081e698 <_js_object_alloc_with_shape+0xbd8>
10081db44:      mrs x9, TPIDRRO_EL0
10081db48:      and x9, x9, #0xfffffffffffffff8
10081db4c:      ldr x0, [x9, x8, lsl #3]
10081db50:      cbz x0, 0x10081e698 <_js_object_alloc_with_shape+0xbd8>
10081db54:      ldr x8, [x0, #0x28]
10081db58:      ldrb    w8, [x8]
10081db5c:      tbz w8, #0x0, 0x10081dbc8 <_js_object_alloc_with_shape+0x108>
10081db60:      ldr x8, [x27, #0x48]
10081db64:      cmn x8, #0x1
10081db68:      b.eq    0x10081e828 <_js_object_alloc_with_shape+0xd68>
10081db6c:      mrs x9, TPIDRRO_EL0
10081db70:      and x9, x9, #0xfffffffffffffff8
10081db74:      ldr x0, [x9, x8, lsl #3]
10081db78:      cbz x0, 0x10081e828 <_js_object_alloc_with_shape+0xd68>
10081db7c:      ldr x25, [x0, #0x20]
10081db80:      ldr x8, [x25]
10081db84:      cbnz    x8, 0x10081e838 <_js_object_alloc_with_shape+0xd78>
10081db88:      mov x8, #-0x1               ; =-1
10081db8c:      str x8, [x25]
10081db90:      ldr x1, [x25, #0x18]
10081db94:      cbz x1, 0x10081dbc4 <_js_object_alloc_with_shape+0x104>
10081db98:      mov x0, #0x0                ; =0
10081db9c:      ldr x8, [x25, #0x10]
10081dba0:      lsl x10, x1, #4
10081dba4:      mov x9, x8
10081dba8:      ldr x11, [x9, #0x8]
10081dbac:      cmp x11, x21
10081dbb0:      b.eq    0x10081dda0 <_js_object_alloc_with_shape+0x2e0>
10081dbb4:      add x0, x0, #0x1
10081dbb8:      add x9, x9, #0x10
10081dbbc:      subs    x10, x10, #0x10
10081dbc0:      b.ne    0x10081dba8 <_js_object_alloc_with_shape+0xe8>
10081dbc4:      str xzr, [x25]
10081dbc8:      mov x0, x21
10081dbcc:      bl  0x1004da29c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081dbd0:      mov w8, #0x2                ; =2
10081dbd4:      strb    w8, [x0]
10081dbd8:      ldr x8, [x27, #0x48]
10081dbdc:      cmn x8, #0x1
10081dbe0:      b.eq    0x10081e6ac <_js_object_alloc_with_shape+0xbec>
10081dbe4:      mrs x9, TPIDRRO_EL0
10081dbe8:      and x9, x9, #0xfffffffffffffff8
10081dbec:      ldr x8, [x9, x8, lsl #3]
10081dbf0:      cbz x8, 0x10081e6ac <_js_object_alloc_with_shape+0xbec>
10081dbf4:      ldr x8, [x8, #0x30]
10081dbf8:      ldrb    w8, [x8]
10081dbfc:      orr w8, w8, #0x2
10081dc00:      strb    w8, [x0, #0x1]
10081dc04:      ldr x8, [x27, #0x48]
10081dc08:      cmn x8, #0x1
10081dc0c:      b.eq    0x10081e6c0 <_js_object_alloc_with_shape+0xc00>
10081dc10:      mrs x9, TPIDRRO_EL0
10081dc14:      and x9, x9, #0xfffffffffffffff8
10081dc18:      ldr x8, [x9, x8, lsl #3]
10081dc1c:      cbz x8, 0x10081e6c0 <_js_object_alloc_with_shape+0xc00>
10081dc20:      ldr x8, [x8, #0x30]
10081dc24:      ldrb    w8, [x8]
10081dc28:      tbz w8, #0x0, 0x10081dc94 <_js_object_alloc_with_shape+0x1d4>
10081dc2c:      ldrb    w8, [x0]
10081dc30:      sub w9, w8, #0x15
10081dc34:      cmn w9, #0x14
10081dc38:      b.lo    0x10081dc94 <_js_object_alloc_with_shape+0x1d4>
10081dc3c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081dc40:      add x9, x9, #0xb10
10081dc44:      add x8, x9, x8, lsl #5
10081dc48:      ldrb    w8, [x8, #0x1b]
10081dc4c:      tbnz    w8, #0x0, 0x10081dc94 <_js_object_alloc_with_shape+0x1d4>
10081dc50:      mov x25, x0
10081dc54:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081dc58:      add x0, x0, #0x658
10081dc5c:      ldr x8, [x0]
10081dc60:      blr x8
10081dc64:      mov x24, x0
10081dc68:      ldrb    w8, [x0, #0x18]
10081dc6c:      cbnz    w8, 0x10081e968 <_js_object_alloc_with_shape+0xea8>
10081dc70:      ldr x28, [x24, #0x10]
10081dc74:      ldr x8, [x24]
10081dc78:      cmp x28, x8
10081dc7c:      mov x0, x25
10081dc80:      b.eq    0x10081e998 <_js_object_alloc_with_shape+0xed8>
10081dc84:      ldr x8, [x24, #0x8]
10081dc88:      str x0, [x8, x28, lsl #3]
10081dc8c:      add x8, x28, #0x1
10081dc90:      str x8, [x24, #0x10]
10081dc94:      strh    wzr, [x0, #0x2]
10081dc98:      str w21, [x0, #0x4]
10081dc9c:      add x24, x0, #0x8
10081dca0:      stp xzr, xzr, [x24]
10081dca4:      lsl x2, x26, #3
10081dca8:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5aa8>
10081dcac:      add x1, x1, #0xaf0
10081dcb0:      add x0, x24, #0x10
10081dcb4:      bl  0x100af8f90 <_writev+0x100af8f90>
10081dcb8:      lsr x8, x24, #3
10081dcbc:      cmp x8, #0x201
10081dcc0:      b.lo    0x10081dd14 <_js_object_alloc_with_shape+0x254>
10081dcc4:      ldurb   w8, [x24, #-0x8]
10081dcc8:      sub w9, w8, #0x15
10081dccc:      cmn w9, #0x14
10081dcd0:      b.lo    0x10081dd14 <_js_object_alloc_with_shape+0x254>
10081dcd4:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081dcd8:      add x9, x9, #0xb10
10081dcdc:      add x8, x9, x8, lsl #5
10081dce0:      ldrb    w8, [x8, #0x14]
10081dce4:      mov w9, #0x16               ; =22
10081dce8:      lsr w8, w9, w8
10081dcec:      tbz w8, #0x0, 0x10081dd14 <_js_object_alloc_with_shape+0x254>
10081dcf0:      ldurh   w8, [x24, #-0x6]
10081dcf4:      mov w9, #0x4000             ; =16384
10081dcf8:      bfxil   w9, w8, #0, #13
10081dcfc:      sturh   w9, [x24, #-0x6]
10081dd00:      mov x0, x24
10081dd04:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081dd08:      ldurh   w8, [x24, #-0x6]
10081dd0c:      and w8, w8, #0xffffefff
10081dd10:      sturh   w8, [x24, #-0x6]
10081dd14:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
10081dd18:      str x0, [sp, #0x18]
10081dd1c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10081dd20:      stp x24, x8, [sp, #0x30]
10081dd24:      mov w8, #0x1                ; =1
10081dd28:      str x8, [sp, #0x28]
10081dd2c:      add x0, sp, #0x28
10081dd30:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10081dd34:      mov x28, x0
10081dd38:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081dd3c:      ldr w21, [x24, #0x634]
10081dd40:      cmp w21, #0x300
10081dd44:      b.hs    0x10081dec8 <_js_object_alloc_with_shape+0x408>
10081dd48:      ldr x8, [x27, #0x48]
10081dd4c:      cmn x8, #0x1
10081dd50:      b.eq    0x10081deb8 <_js_object_alloc_with_shape+0x3f8>
10081dd54:      mrs x9, TPIDRRO_EL0
10081dd58:      and x9, x9, #0xfffffffffffffff8
10081dd5c:      ldr x0, [x9, x8, lsl #3]
10081dd60:      cbz x0, 0x10081deb8 <_js_object_alloc_with_shape+0x3f8>
10081dd64:      add x8, x0, x21, lsl #3
10081dd68:      ldr x0, [x8, #0x1e8]
10081dd6c:      cbz x0, 0x10081dec8 <_js_object_alloc_with_shape+0x408>
10081dd70:      ldr x0, [x0]
10081dd74:      cbz x0, 0x10081dedc <_js_object_alloc_with_shape+0x41c>
10081dd78:      and w8, w20, #0xff
10081dd7c:      add x8, x0, w8, uxtw #4
10081dd80:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081dd84:      ldr w9, [x8]
10081dd88:      cmp w9, w20
10081dd8c:      b.ne    0x10081def8 <_js_object_alloc_with_shape+0x438>
10081dd90:      ldr x25, [x8, #0x8]
10081dd94:      ldr w26, [x8, #0x4]
10081dd98:      cbnz    x25, 0x10081e45c <_js_object_alloc_with_shape+0x99c>
10081dd9c:      b   0x10081dfec <_js_object_alloc_with_shape+0x52c>
10081dda0:      cmp x0, x1
10081dda4:      b.hs    0x10081e964 <_js_object_alloc_with_shape+0xea4>
10081dda8:      ldr x24, [x9]
10081ddac:      subs    x10, x1, #0x1
10081ddb0:      ldr q0, [x8, x10, lsl #4]
10081ddb4:      str q0, [x9]
10081ddb8:      str x10, [x25, #0x18]
10081ddbc:      b.ne    0x10081dde4 <_js_object_alloc_with_shape+0x324>
10081ddc0:      ldr x8, [x27, #0x48]
10081ddc4:      cmn x8, #0x1
10081ddc8:      b.eq    0x10081e930 <_js_object_alloc_with_shape+0xe70>
10081ddcc:      mrs x9, TPIDRRO_EL0
10081ddd0:      and x9, x9, #0xfffffffffffffff8
10081ddd4:      ldr x0, [x9, x8, lsl #3]
10081ddd8:      cbz x0, 0x10081e930 <_js_object_alloc_with_shape+0xe70>
10081dddc:      ldr x8, [x0, #0x28]
10081dde0:      strb    wzr, [x8]
10081dde4:      ldr x8, [x25]
10081dde8:      add x8, x8, #0x1
10081ddec:      str x8, [x25]
10081ddf0:      mov w8, #0x2                ; =2
10081ddf4:      mov x28, x24
10081ddf8:      strb    w8, [x28, #-0x8]!
10081ddfc:      ldr x8, [x27, #0x48]
10081de00:      cmn x8, #0x1
10081de04:      b.eq    0x10081e914 <_js_object_alloc_with_shape+0xe54>
10081de08:      mrs x9, TPIDRRO_EL0
10081de0c:      and x9, x9, #0xfffffffffffffff8
10081de10:      ldr x0, [x9, x8, lsl #3]
10081de14:      cbz x0, 0x10081e914 <_js_object_alloc_with_shape+0xe54>
10081de18:      ldr x8, [x0, #0x30]
10081de1c:      ldrb    w8, [x8]
10081de20:      orr w8, w8, #0x2
10081de24:      sturb   w8, [x24, #-0x7]
10081de28:      ldr x8, [x27, #0x48]
10081de2c:      cmn x8, #0x1
10081de30:      b.eq    0x10081e91c <_js_object_alloc_with_shape+0xe5c>
10081de34:      mrs x9, TPIDRRO_EL0
10081de38:      and x9, x9, #0xfffffffffffffff8
10081de3c:      ldr x0, [x9, x8, lsl #3]
10081de40:      cbz x0, 0x10081e91c <_js_object_alloc_with_shape+0xe5c>
10081de44:      ldr x8, [x0, #0x30]
10081de48:      ldrb    w8, [x8]
10081de4c:      tbz w8, #0x0, 0x10081deac <_js_object_alloc_with_shape+0x3ec>
10081de50:      ldrb    w8, [x28]
10081de54:      sub w9, w8, #0x15
10081de58:      cmn w9, #0x14
10081de5c:      b.lo    0x10081deac <_js_object_alloc_with_shape+0x3ec>
10081de60:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081de64:      add x9, x9, #0xb10
10081de68:      add x8, x9, x8, lsl #5
10081de6c:      ldrb    w8, [x8, #0x1b]
10081de70:      tbnz    w8, #0x0, 0x10081deac <_js_object_alloc_with_shape+0x3ec>
10081de74:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081de78:      add x0, x0, #0x658
10081de7c:      ldr x8, [x0]
10081de80:      blr x8
10081de84:      ldrb    w8, [x0, #0x18]
10081de88:      cbnz    w8, 0x10081e9a8 <_js_object_alloc_with_shape+0xee8>
10081de8c:      ldr x25, [x0, #0x10]
10081de90:      ldr x8, [x0]
10081de94:      cmp x25, x8
10081de98:      b.eq    0x10081e9e4 <_js_object_alloc_with_shape+0xf24>
10081de9c:      ldr x8, [x0, #0x8]
10081dea0:      str x28, [x8, x25, lsl #3]
10081dea4:      add x8, x25, #0x1
10081dea8:      str x8, [x0, #0x10]
10081deac:      sturh   wzr, [x24, #-0x6]
10081deb0:      stur    w21, [x24, #-0x4]
10081deb4:      b   0x10081dca0 <_js_object_alloc_with_shape+0x1e0>
10081deb8:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081debc:      add x8, x0, x21, lsl #3
10081dec0:      ldr x0, [x8, #0x1e8]
10081dec4:      cbnz    x0, 0x10081dd70 <_js_object_alloc_with_shape+0x2b0>
10081dec8:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081decc:      add x0, x0, #0x330
10081ded0:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081ded4:      ldr x0, [x0]
10081ded8:      cbnz    x0, 0x10081dd78 <_js_object_alloc_with_shape+0x2b8>
10081dedc:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081dee0:      and w8, w20, #0xff
10081dee4:      add x8, x0, w8, uxtw #4
10081dee8:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081deec:      ldr w9, [x8]
10081def0:      cmp w9, w20
10081def4:      b.eq    0x10081dd90 <_js_object_alloc_with_shape+0x2d0>
10081def8:      ldr x8, [x0, #0x5088]
10081defc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081df00:      cmp x8, x9
10081df04:      b.hs    0x10081e958 <_js_object_alloc_with_shape+0xe98>
10081df08:      add x9, x8, #0x1
10081df0c:      str x9, [x0, #0x5088]
10081df10:      ldr x9, [x0, #0x50a8]
10081df14:      cbz x9, 0x10081dfc8 <_js_object_alloc_with_shape+0x508>
10081df18:      mov x9, #0x0                ; =0
10081df1c:      mov w10, w20
10081df20:      mov x11, #0x7c15            ; =31765
10081df24:      movk    x11, #0x7f4a, lsl #16
10081df28:      movk    x11, #0x79b9, lsl #32
10081df2c:      movk    x11, #0x9e37, lsl #48
10081df30:      mul x10, x10, x11
10081df34:      eor x13, x10, x10, lsr #32
10081df38:      lsr x12, x10, #57
10081df3c:      ldr x10, [x0, #0x5098]
10081df40:      ldr x11, [x0, #0x5090]
10081df44:      dup.8b  v0, w12
10081df48:      movi.2d v1, #0xffffffffffffffff
10081df4c:      mov w12, #0x18              ; =24
10081df50:      and x13, x13, x10
10081df54:      ldr d2, [x11, x13]
10081df58:      cmeq.8b v3, v2, v0
10081df5c:      fmov    x14, d3
10081df60:      ands    x14, x14, #0x8080808080808080
10081df64:      b.eq    0x10081df98 <_js_object_alloc_with_shape+0x4d8>
10081df68:      rbit    x15, x14
10081df6c:      clz x15, x15
10081df70:      add x15, x13, x15, lsr #3
10081df74:      and x15, x15, x10
10081df78:      mneg    x15, x15, x12
10081df7c:      add x15, x11, x15
10081df80:      ldur    w16, [x15, #-0x18]
10081df84:      cmp w20, w16
10081df88:      b.eq    0x10081dfdc <_js_object_alloc_with_shape+0x51c>
10081df8c:      sub x15, x14, #0x2
10081df90:      ands    x14, x15, x14
10081df94:      b.ne    0x10081df68 <_js_object_alloc_with_shape+0x4a8>
10081df98:      cmeq.8b v2, v2, v1
10081df9c:      fmov    x14, d2
10081dfa0:      cbnz    x14, 0x10081dfc8 <_js_object_alloc_with_shape+0x508>
10081dfa4:      add x9, x9, #0x8
10081dfa8:      add x13, x13, x9
10081dfac:      and x13, x13, x10
10081dfb0:      ldr d2, [x11, x13]
10081dfb4:      cmeq.8b v3, v2, v0
10081dfb8:      fmov    x14, d3
10081dfbc:      ands    x14, x14, #0x8080808080808080
10081dfc0:      b.ne    0x10081df68 <_js_object_alloc_with_shape+0x4a8>
10081dfc4:      b   0x10081df98 <_js_object_alloc_with_shape+0x4d8>
10081dfc8:      mov w26, #0x0               ; =0
10081dfcc:      mov x25, #0x0               ; =0
10081dfd0:      str x8, [x0, #0x5088]
10081dfd4:      cbnz    x25, 0x10081e45c <_js_object_alloc_with_shape+0x99c>
10081dfd8:      b   0x10081dfec <_js_object_alloc_with_shape+0x52c>
10081dfdc:      ldur    x25, [x15, #-0x10]
10081dfe0:      ldur    w26, [x15, #-0x8]
10081dfe4:      str x8, [x0, #0x5088]
10081dfe8:      cbnz    x25, 0x10081e45c <_js_object_alloc_with_shape+0x99c>
10081dfec:      and w8, w20, #0xff
10081dff0:      str x8, [sp, #0x8]
10081dff4:      mov w26, #0x0               ; =0
10081dff8:      mov w8, #0x0                ; =0
10081dffc:      mov w9, #0x8                ; =8
10081e000:      str x9, [sp, #0x10]
10081e004:      mov w25, w23
10081e008:      b   0x10081e01c <_js_object_alloc_with_shape+0x55c>
10081e00c:      mov w26, #0x1               ; =1
10081e010:      mov x22, x21
10081e014:      mov w8, #0x1                ; =1
10081e018:      cbnz    x23, 0x10081e074 <_js_object_alloc_with_shape+0x5b4>
10081e01c:      tbnz    w8, #0x0, 0x10081e068 <_js_object_alloc_with_shape+0x5a8>
10081e020:      mov x21, x22
10081e024:      mov x23, #0x0               ; =0
10081e028:      cbz x25, 0x10081e00c <_js_object_alloc_with_shape+0x54c>
10081e02c:      ldrb    w8, [x21, x23]
10081e030:      cbz w8, 0x10081e054 <_js_object_alloc_with_shape+0x594>
10081e034:      add x23, x23, #0x1
10081e038:      cmp x25, x23
10081e03c:      b.ne    0x10081e02c <_js_object_alloc_with_shape+0x56c>
10081e040:      mov w26, #0x1               ; =1
10081e044:      mov x22, x21
10081e048:      mov w8, #0x1                ; =1
10081e04c:      mov x23, x25
10081e050:      b   0x10081e018 <_js_object_alloc_with_shape+0x558>
10081e054:      mvn x9, x23
10081e058:      add x25, x9, x25
10081e05c:      add x9, x21, x23
10081e060:      add x22, x9, #0x1
10081e064:      b   0x10081e018 <_js_object_alloc_with_shape+0x558>
10081e068:      mov x23, #0x0               ; =0
10081e06c:      mov w21, #0x1               ; =1
10081e070:      b   0x10081e174 <_js_object_alloc_with_shape+0x6b4>
10081e074:      cbz x21, 0x10081e164 <_js_object_alloc_with_shape+0x6a4>
10081e078:      mov w0, #0x40               ; =64
10081e07c:      mov w1, #0x8                ; =8
10081e080:      bl  0x100af6348 <_mi_malloc_aligned>
10081e084:      cbz x0, 0x10081e9f4 <_js_object_alloc_with_shape+0xf34>
10081e088:      stp x21, x23, [x0]
10081e08c:      mov w8, #0x4                ; =4
10081e090:      stp x8, x0, [sp, #0x28]
10081e094:      mov w23, #0x1               ; =1
10081e098:      str x23, [sp, #0x38]
10081e09c:      mov x8, x25
10081e0a0:      mov x10, x22
10081e0a4:      mov x9, x26
10081e0a8:      b   0x10081e0bc <_js_object_alloc_with_shape+0x5fc>
10081e0ac:      mov w26, #0x1               ; =1
10081e0b0:      mov x10, x24
10081e0b4:      mov w9, #0x1                ; =1
10081e0b8:      cbnz    x21, 0x10081e110 <_js_object_alloc_with_shape+0x650>
10081e0bc:      tbnz    w9, #0x0, 0x10081e150 <_js_object_alloc_with_shape+0x690>
10081e0c0:      mov x24, x10
10081e0c4:      mov x21, #0x0               ; =0
10081e0c8:      cbz x8, 0x10081e0ac <_js_object_alloc_with_shape+0x5ec>
10081e0cc:      ldrb    w9, [x24, x21]
10081e0d0:      cbz w9, 0x10081e0f4 <_js_object_alloc_with_shape+0x634>
10081e0d4:      add x21, x21, #0x1
10081e0d8:      cmp x8, x21
10081e0dc:      b.ne    0x10081e0cc <_js_object_alloc_with_shape+0x60c>
10081e0e0:      mov w26, #0x1               ; =1
10081e0e4:      mov x10, x24
10081e0e8:      mov w9, #0x1                ; =1
10081e0ec:      mov x21, x8
10081e0f0:      b   0x10081e0b8 <_js_object_alloc_with_shape+0x5f8>
10081e0f4:      mvn x10, x21
10081e0f8:      add x25, x10, x8
10081e0fc:      add x8, x24, x21
10081e100:      add x22, x8, #0x1
10081e104:      mov x8, x25
10081e108:      mov x10, x22
10081e10c:      b   0x10081e0b8 <_js_object_alloc_with_shape+0x5f8>
10081e110:      cbz x24, 0x10081e150 <_js_object_alloc_with_shape+0x690>
10081e114:      ldr x8, [sp, #0x28]
10081e118:      cmp x23, x8
10081e11c:      b.eq    0x10081e130 <_js_object_alloc_with_shape+0x670>
10081e120:      add x8, x0, x23, lsl #4
10081e124:      stp x24, x21, [x8]
10081e128:      add x23, x23, #0x1
10081e12c:      b   0x10081e098 <_js_object_alloc_with_shape+0x5d8>
10081e130:      add x0, sp, #0x28
10081e134:      mov x1, x23
10081e138:      mov w2, #0x1                ; =1
10081e13c:      mov w3, #0x8                ; =8
10081e140:      mov w4, #0x10               ; =16
10081e144:      bl  0x100ad59d0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081e148:      ldr x0, [sp, #0x30]
10081e14c:      b   0x10081e120 <_js_object_alloc_with_shape+0x660>
10081e150:      ldp x8, x9, [sp, #0x28]
10081e154:      str x9, [sp, #0x10]
10081e158:      cmp x8, #0x0
10081e15c:      cset    w21, eq
10081e160:      b   0x10081e174 <_js_object_alloc_with_shape+0x6b4>
10081e164:      mov x23, #0x0               ; =0
10081e168:      mov w21, #0x1               ; =1
10081e16c:      mov w8, #0x8                ; =8
10081e170:      str x8, [sp, #0x10]
10081e174:      ubfiz   x8, x23, #3, #32
10081e178:      add x0, x8, #0x8
10081e17c:      mov w1, #0x8                ; =8
10081e180:      mov w2, #0x1                ; =1
10081e184:      bl  0x1004db0e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081e188:      mov x22, x0
10081e18c:      stp w23, w23, [x0]
10081e190:      lsr x8, x0, #3
10081e194:      cmp x8, #0x201
10081e198:      b.lo    0x10081e228 <_js_object_alloc_with_shape+0x768>
10081e19c:      ldurb   w8, [x22, #-0x8]
10081e1a0:      cmp w8, #0x1
10081e1a4:      b.ne    0x10081e1d0 <_js_object_alloc_with_shape+0x710>
10081e1a8:      ldurh   w8, [x22, #-0x6]
10081e1ac:      mov w9, #0x1080             ; =4224
10081e1b0:      mov w10, #0xef7f            ; =61311
10081e1b4:      and w10, w8, w10
10081e1b8:      sturh   w10, [x22, #-0x6]
10081e1bc:      tst w8, w9
10081e1c0:      b.eq    0x10081e1e0 <_js_object_alloc_with_shape+0x720>
10081e1c4:      mov x0, x22
10081e1c8:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081e1cc:      ldurb   w8, [x22, #-0x8]
10081e1d0:      sub w9, w8, #0x15
10081e1d4:      cmn w9, #0x14
10081e1d8:      b.hs    0x10081e1e4 <_js_object_alloc_with_shape+0x724>
10081e1dc:      b   0x10081e228 <_js_object_alloc_with_shape+0x768>
10081e1e0:      mov w8, #0x1                ; =1
10081e1e4:      mov w8, w8
10081e1e8:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081e1ec:      add x9, x9, #0xb10
10081e1f0:      add x8, x9, x8, lsl #5
10081e1f4:      ldrb    w8, [x8, #0x14]
10081e1f8:      mov w9, #0x16               ; =22
10081e1fc:      lsr w8, w9, w8
10081e200:      tbz w8, #0x0, 0x10081e228 <_js_object_alloc_with_shape+0x768>
10081e204:      ldurh   w8, [x22, #-0x6]
10081e208:      mov w9, #0x4000             ; =16384
10081e20c:      bfxil   w9, w8, #0, #13
10081e210:      sturh   w9, [x22, #-0x6]
10081e214:      mov x0, x22
10081e218:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081e21c:      ldurh   w8, [x22, #-0x6]
10081e220:      and w8, w8, #0xffffefff
10081e224:      sturh   w8, [x22, #-0x6]
10081e228:      stp w21, w19, [sp]
10081e22c:      mov x19, x20
10081e230:      mov x20, x28
10081e234:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
10081e238:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10081e23c:      stp x22, x8, [sp, #0x30]
10081e240:      mov w8, #0x1                ; =1
10081e244:      stp x0, x8, [sp, #0x20]
10081e248:      add x0, sp, #0x28
10081e24c:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10081e250:      mov x22, x0
10081e254:      cbz x23, 0x10081e318 <_js_object_alloc_with_shape+0x858>
10081e258:      mov x25, #0x0               ; =0
10081e25c:      mov x26, #0x7fffffffffffffff ; =9223372036854775807
10081e260:      mov w28, #0x18              ; =24
10081e264:      ldr x8, [sp, #0x10]
10081e268:      mov x24, x8
10081e26c:      add x21, x8, x23, lsl #4
10081e270:      b   0x10081e2a4 <_js_object_alloc_with_shape+0x7e4>
10081e274:      mov x0, x22
10081e278:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e27c:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081e280:      bfxil   x2, x23, #0, #48
10081e284:      add x8, x0, x25, lsl #3
10081e288:      str x2, [x8, #0x8]
10081e28c:      mov x1, x25
10081e290:      bl  0x1004f0edc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081e294:      add x24, x24, #0x10
10081e298:      add x25, x25, #0x1
10081e29c:      cmp x24, x21
10081e2a0:      b.eq    0x10081e318 <_js_object_alloc_with_shape+0x858>
10081e2a4:      ldr x0, [x24]
10081e2a8:      ldr w1, [x24, #0x8]
10081e2ac:      bl  0x100884f94 <_js_string_from_bytes_longlived>
10081e2b0:      mov x23, x0
10081e2b4:      ldr x8, [x27, #0x48]
10081e2b8:      cmn x8, #0x1
10081e2bc:      b.eq    0x10081e274 <_js_object_alloc_with_shape+0x7b4>
10081e2c0:      mrs x9, TPIDRRO_EL0
10081e2c4:      and x9, x9, #0xfffffffffffffff8
10081e2c8:      ldr x8, [x9, x8, lsl #3]
10081e2cc:      cbz x8, 0x10081e274 <_js_object_alloc_with_shape+0x7b4>
10081e2d0:      ldr x8, [x8, #0x19e8]
10081e2d4:      cbz x8, 0x10081e274 <_js_object_alloc_with_shape+0x7b4>
10081e2d8:      ldr x9, [x8]
10081e2dc:      cmp x9, x26
10081e2e0:      b.hs    0x10081e938 <_js_object_alloc_with_shape+0xe78>
10081e2e4:      add x10, x9, #0x1
10081e2e8:      str x10, [x8]
10081e2ec:      ldr x10, [x8, #0x18]
10081e2f0:      cmp x22, x10
10081e2f4:      b.hs    0x10081e944 <_js_object_alloc_with_shape+0xe84>
10081e2f8:      ldr x10, [x8, #0x10]
10081e2fc:      madd    x10, x22, x28, x10
10081e300:      ldr x11, [x10]
10081e304:      cmp x11, #0x1
10081e308:      b.ne    0x10081e948 <_js_object_alloc_with_shape+0xe88>
10081e30c:      ldr x0, [x10, #0x8]
10081e310:      str x9, [x8]
10081e314:      b   0x10081e27c <_js_object_alloc_with_shape+0x7bc>
10081e318:      ldr x8, [x27, #0x48]
10081e31c:      cmn x8, #0x1
10081e320:      b.eq    0x10081e390 <_js_object_alloc_with_shape+0x8d0>
10081e324:      mrs x9, TPIDRRO_EL0
10081e328:      and x9, x9, #0xfffffffffffffff8
10081e32c:      ldr x8, [x9, x8, lsl #3]
10081e330:      cbz x8, 0x10081e390 <_js_object_alloc_with_shape+0x8d0>
10081e334:      ldr x8, [x8, #0x19e8]
10081e338:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081e33c:      cbz x8, 0x10081e3c8 <_js_object_alloc_with_shape+0x908>
10081e340:      ldr x9, [x8]
10081e344:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081e348:      cmp x9, x10
10081e34c:      b.hs    0x10081e938 <_js_object_alloc_with_shape+0xe78>
10081e350:      add x10, x9, #0x1
10081e354:      str x10, [x8]
10081e358:      ldr x10, [x8, #0x18]
10081e35c:      cmp x22, x10
10081e360:      b.hs    0x10081e944 <_js_object_alloc_with_shape+0xe84>
10081e364:      ldr x10, [x8, #0x10]
10081e368:      mov w11, #0x18              ; =24
10081e36c:      madd    x10, x22, x11, x10
10081e370:      ldr x11, [x10]
10081e374:      cmp x11, #0x1
10081e378:      b.ne    0x10081e948 <_js_object_alloc_with_shape+0xe88>
10081e37c:      mov x28, x20
10081e380:      mov x20, x19
10081e384:      ldr x25, [x10, #0x8]
10081e388:      str x9, [x8]
10081e38c:      b   0x10081e3dc <_js_object_alloc_with_shape+0x91c>
10081e390:      mov x0, x22
10081e394:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e398:      mov x25, x0
10081e39c:      mov x28, x20
10081e3a0:      mov x20, x19
10081e3a4:      ldr w19, [sp, #0x4]
10081e3a8:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081e3ac:      mov x0, x20
10081e3b0:      mov x1, x25
10081e3b4:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081e3b8:      ldr w21, [x24, #0x634]
10081e3bc:      cmp w21, #0x300
10081e3c0:      b.lo    0x10081e3f8 <_js_object_alloc_with_shape+0x938>
10081e3c4:      b   0x10081e6f0 <_js_object_alloc_with_shape+0xc30>
10081e3c8:      mov x0, x22
10081e3cc:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e3d0:      mov x25, x0
10081e3d4:      mov x28, x20
10081e3d8:      mov x20, x19
10081e3dc:      ldr w19, [sp, #0x4]
10081e3e0:      mov x0, x20
10081e3e4:      mov x1, x25
10081e3e8:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081e3ec:      ldr w21, [x24, #0x634]
10081e3f0:      cmp w21, #0x300
10081e3f4:      b.hs    0x10081e6f0 <_js_object_alloc_with_shape+0xc30>
10081e3f8:      ldr x8, [x27, #0x48]
10081e3fc:      cmn x8, #0x1
10081e400:      b.eq    0x10081e6e0 <_js_object_alloc_with_shape+0xc20>
10081e404:      mrs x9, TPIDRRO_EL0
10081e408:      and x9, x9, #0xfffffffffffffff8
10081e40c:      ldr x0, [x9, x8, lsl #3]
10081e410:      cbz x0, 0x10081e6e0 <_js_object_alloc_with_shape+0xc20>
10081e414:      add x8, x0, x21, lsl #3
10081e418:      ldr x0, [x8, #0x1e8]
10081e41c:      cbz x0, 0x10081e6f0 <_js_object_alloc_with_shape+0xc30>
10081e420:      ldr x0, [x0]
10081e424:      cbz x0, 0x10081e704 <_js_object_alloc_with_shape+0xc44>
10081e428:      ldr x8, [sp, #0x8]
10081e42c:      add x8, x0, x8, lsl #4
10081e430:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081e434:      ldr w9, [x8]
10081e438:      cmp w9, w20
10081e43c:      b.ne    0x10081e720 <_js_object_alloc_with_shape+0xc60>
10081e440:      ldr w26, [x8, #0x4]
10081e444:      add x0, sp, #0x20
10081e448:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e44c:      ldr w8, [sp]
10081e450:      tbnz    w8, #0x0, 0x10081e45c <_js_object_alloc_with_shape+0x99c>
10081e454:      ldr x0, [sp, #0x10]
10081e458:      bl  0x100af60c0 <_mi_free>
10081e45c:      ldr x8, [x27, #0x48]
10081e460:      cmn x8, #0x1
10081e464:      b.eq    0x10081e4cc <_js_object_alloc_with_shape+0xa0c>
10081e468:      mrs x9, TPIDRRO_EL0
10081e46c:      and x9, x9, #0xfffffffffffffff8
10081e470:      ldr x8, [x9, x8, lsl #3]
10081e474:      cbz x8, 0x10081e4cc <_js_object_alloc_with_shape+0xa0c>
10081e478:      ldr x8, [x8, #0x19e8]
10081e47c:      cbz x8, 0x10081e4cc <_js_object_alloc_with_shape+0xa0c>
10081e480:      ldr x9, [x8]
10081e484:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081e488:      cmp x9, x10
10081e48c:      b.hs    0x10081e938 <_js_object_alloc_with_shape+0xe78>
10081e490:      add x10, x9, #0x1
10081e494:      str x10, [x8]
10081e498:      ldr x10, [x8, #0x18]
10081e49c:      cmp x28, x10
10081e4a0:      b.hs    0x10081e944 <_js_object_alloc_with_shape+0xe84>
10081e4a4:      ldr x10, [x8, #0x10]
10081e4a8:      mov w11, #0x18              ; =24
10081e4ac:      madd    x10, x28, x11, x10
10081e4b0:      ldr x11, [x10]
10081e4b4:      cmp x11, #0x1
10081e4b8:      b.ne    0x10081e948 <_js_object_alloc_with_shape+0xe88>
10081e4bc:      ldr x20, [x10, #0x8]
10081e4c0:      str x9, [x8]
10081e4c4:      cbnz    w26, 0x10081e4dc <_js_object_alloc_with_shape+0xa1c>
10081e4c8:      b   0x10081e4f4 <_js_object_alloc_with_shape+0xa34>
10081e4cc:      mov x0, x28
10081e4d0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e4d4:      mov x20, x0
10081e4d8:      cbz w26, 0x10081e4f4 <_js_object_alloc_with_shape+0xa34>
10081e4dc:      mov x0, x20
10081e4e0:      mov x1, x26
10081e4e4:      mov x2, x25
10081e4e8:      mov x3, x19
10081e4ec:      bl  0x10059bf3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34try_birth_stamp_preinstalled_shape>
10081e4f0:      tbnz    w0, #0x0, 0x10081e870 <_js_object_alloc_with_shape+0xdb0>
10081e4f4:      ldr w21, [x20, #0x4]
10081e4f8:      ldr w22, [x24, #0x634]
10081e4fc:      cmp w22, #0x300
10081e500:      b.hs    0x10081e5d0 <_js_object_alloc_with_shape+0xb10>
10081e504:      ldr x8, [x27, #0x48]
10081e508:      cmn x8, #0x1
10081e50c:      b.eq    0x10081e5c0 <_js_object_alloc_with_shape+0xb00>
10081e510:      mrs x9, TPIDRRO_EL0
10081e514:      and x9, x9, #0xfffffffffffffff8
10081e518:      ldr x0, [x9, x8, lsl #3]
10081e51c:      cbz x0, 0x10081e5c0 <_js_object_alloc_with_shape+0xb00>
10081e520:      add x8, x0, x22, lsl #3
10081e524:      ldr x0, [x8, #0x1e8]
10081e528:      cbz x0, 0x10081e5d0 <_js_object_alloc_with_shape+0xb10>
10081e52c:      ldr x0, [x0]
10081e530:      cbz x0, 0x10081e5e4 <_js_object_alloc_with_shape+0xb24>
10081e534:      mov w8, #-0x40000001        ; =-1073741825
10081e538:      cmp w21, w8
10081e53c:      b.gt    0x10081e5f4 <_js_object_alloc_with_shape+0xb34>
10081e540:      ldr x9, [x0, #0x5198]
10081e544:      ubfx    x8, x21, #15, #15
10081e548:      cmp x8, x9
10081e54c:      b.hs    0x10081e5f4 <_js_object_alloc_with_shape+0xb34>
10081e550:      ldr x9, [x0, #0x5190]
10081e554:      ldr x8, [x9, x8, lsl #3]
10081e558:      cbz x8, 0x10081e5f4 <_js_object_alloc_with_shape+0xb34>
10081e55c:      ubfx    x9, x21, #5, #10
10081e560:      ldr x8, [x8, x9, lsl #3]
10081e564:      cbz x8, 0x10081e5f4 <_js_object_alloc_with_shape+0xb34>
10081e568:      and x9, x21, #0x1f
10081e56c:      add x8, x8, x9, lsl #5
10081e570:      ldrb    w9, [x8, #0x1c]
10081e574:      tbz w9, #0x0, 0x10081e5f4 <_js_object_alloc_with_shape+0xb34>
10081e578:      mov w10, #0x90              ; =144
10081e57c:      tst w9, w10
10081e580:      cset    w10, ne
10081e584:      ldp x11, x12, [x8]
10081e588:      ubfx    w13, w9, #5, #1
10081e58c:      ldr w14, [x8, #0x18]
10081e590:      ubfx    w9, w9, #2, #1
10081e594:      stp x11, x8, [sp, #0x28]
10081e598:      str x12, [sp, #0x38]
10081e59c:      ldr d0, [x8, #0x10]
10081e5a0:      str d0, [sp, #0x40]
10081e5a4:      str w14, [sp, #0x48]
10081e5a8:      strb    w9, [sp, #0x4c]
10081e5ac:      strb    w10, [sp, #0x4d]
10081e5b0:      strb    w13, [sp, #0x4e]
10081e5b4:      cmp x11, x25
10081e5b8:      b.ne    0x10081e600 <_js_object_alloc_with_shape+0xb40>
10081e5bc:      b   0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e5c0:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e5c4:      add x8, x0, x22, lsl #3
10081e5c8:      ldr x0, [x8, #0x1e8]
10081e5cc:      cbnz    x0, 0x10081e52c <_js_object_alloc_with_shape+0xa6c>
10081e5d0:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081e5d4:      add x0, x0, #0x330
10081e5d8:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081e5dc:      ldr x0, [x0]
10081e5e0:      cbnz    x0, 0x10081e534 <_js_object_alloc_with_shape+0xa74>
10081e5e4:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081e5e8:      mov w8, #-0x40000001        ; =-1073741825
10081e5ec:      cmp w21, w8
10081e5f0:      b.le    0x10081e540 <_js_object_alloc_with_shape+0xa80>
10081e5f4:      mov w8, #0x2                ; =2
10081e5f8:      strb    w8, [sp, #0x4e]
10081e5fc:      cbz x25, 0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e600:      lsr x8, x20, #3
10081e604:      cmp x8, #0x201
10081e608:      b.lo    0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e60c:      ldursh  w8, [x20, #-0x6]
10081e610:      tst w8, #0x1000
10081e614:      mov w9, #-0x4001            ; =-16385
10081e618:      ccmp    w8, w9, #0x4, eq
10081e61c:      b.gt    0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e620:      lsr x9, x20, #3
10081e624:      cmp x9, #0x201
10081e628:      b.lo    0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e62c:      ldurb   w9, [x20, #-0x8]
10081e630:      sub w10, w9, #0x15
10081e634:      cmn w10, #0x14
10081e638:      b.lo    0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e63c:      and w8, w8, #0xffff
10081e640:      adrp    x10, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081e644:      add x10, x10, #0xb10
10081e648:      add x9, x10, x9, lsl #5
10081e64c:      ldrb    w9, [x9, #0x14]
10081e650:      mov w10, #0x16              ; =22
10081e654:      lsr w9, w10, w9
10081e658:      tbz w9, #0x0, 0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e65c:      and w9, w8, #0xffffefff
10081e660:      sturh   w9, [x20, #-0x6]
10081e664:      ands    w21, w8, #0xc000
10081e668:      b.eq    0x10081e844 <_js_object_alloc_with_shape+0xd84>
10081e66c:      and w8, w8, #0xfff
10081e670:      sturh   w8, [x20, #-0x6]
10081e674:      mov x0, x20
10081e678:      bl  0x1004141e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20typed_layouts_remove>
10081e67c:      cmp w21, #0x4, lsl #12      ; =0x4000
10081e680:      b.eq    0x10081e68c <_js_object_alloc_with_shape+0xbcc>
10081e684:      mov x0, x20
10081e688:      bl  0x1004138e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables17slot_masks_remove>
10081e68c:      mov x0, x20
10081e690:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081e694:      b   0x10081e84c <_js_object_alloc_with_shape+0xd8c>
10081e698:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e69c:      ldr x8, [x0, #0x28]
10081e6a0:      ldrb    w8, [x8]
10081e6a4:      tbnz    w8, #0x0, 0x10081db60 <_js_object_alloc_with_shape+0xa0>
10081e6a8:      b   0x10081dbc8 <_js_object_alloc_with_shape+0x108>
10081e6ac:      mov x24, x0
10081e6b0:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e6b4:      mov x8, x0
10081e6b8:      mov x0, x24
10081e6bc:      b   0x10081dbf4 <_js_object_alloc_with_shape+0x134>
10081e6c0:      mov x24, x0
10081e6c4:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e6c8:      mov x8, x0
10081e6cc:      mov x0, x24
10081e6d0:      ldr x8, [x8, #0x30]
10081e6d4:      ldrb    w8, [x8]
10081e6d8:      tbnz    w8, #0x0, 0x10081dc2c <_js_object_alloc_with_shape+0x16c>
10081e6dc:      b   0x10081dc94 <_js_object_alloc_with_shape+0x1d4>
10081e6e0:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e6e4:      add x8, x0, x21, lsl #3
10081e6e8:      ldr x0, [x8, #0x1e8]
10081e6ec:      cbnz    x0, 0x10081e420 <_js_object_alloc_with_shape+0x960>
10081e6f0:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081e6f4:      add x0, x0, #0x330
10081e6f8:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081e6fc:      ldr x0, [x0]
10081e700:      cbnz    x0, 0x10081e428 <_js_object_alloc_with_shape+0x968>
10081e704:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081e708:      ldr x8, [sp, #0x8]
10081e70c:      add x8, x0, x8, lsl #4
10081e710:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081e714:      ldr w9, [x8]
10081e718:      cmp w9, w20
10081e71c:      b.eq    0x10081e440 <_js_object_alloc_with_shape+0x980>
10081e720:      ldr x8, [x0, #0x5088]
10081e724:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081e728:      cmp x8, x9
10081e72c:      b.hs    0x10081e958 <_js_object_alloc_with_shape+0xe98>
10081e730:      add x9, x8, #0x1
10081e734:      str x9, [x0, #0x5088]
10081e738:      ldr x9, [x0, #0x50a8]
10081e73c:      cbz x9, 0x10081e7f0 <_js_object_alloc_with_shape+0xd30>
10081e740:      mov x9, #0x0                ; =0
10081e744:      mov w10, w20
10081e748:      mov x11, #0x7c15            ; =31765
10081e74c:      movk    x11, #0x7f4a, lsl #16
10081e750:      movk    x11, #0x79b9, lsl #32
10081e754:      movk    x11, #0x9e37, lsl #48
10081e758:      mul x10, x10, x11
10081e75c:      eor x13, x10, x10, lsr #32
10081e760:      lsr x12, x10, #57
10081e764:      ldr x10, [x0, #0x5098]
10081e768:      ldr x11, [x0, #0x5090]
10081e76c:      dup.8b  v0, w12
10081e770:      movi.2d v1, #0xffffffffffffffff
10081e774:      mov w12, #0x18              ; =24
10081e778:      and x13, x13, x10
10081e77c:      ldr d2, [x11, x13]
10081e780:      cmeq.8b v3, v2, v0
10081e784:      fmov    x14, d3
10081e788:      ands    x14, x14, #0x8080808080808080
10081e78c:      b.eq    0x10081e7c0 <_js_object_alloc_with_shape+0xd00>
10081e790:      rbit    x15, x14
10081e794:      clz x15, x15
10081e798:      add x15, x13, x15, lsr #3
10081e79c:      and x15, x15, x10
10081e7a0:      mneg    x15, x15, x12
10081e7a4:      add x15, x11, x15
10081e7a8:      ldur    w16, [x15, #-0x18]
10081e7ac:      cmp w20, w16
10081e7b0:      b.eq    0x10081e80c <_js_object_alloc_with_shape+0xd4c>
10081e7b4:      sub x15, x14, #0x2
10081e7b8:      ands    x14, x15, x14
10081e7bc:      b.ne    0x10081e790 <_js_object_alloc_with_shape+0xcd0>
10081e7c0:      cmeq.8b v2, v2, v1
10081e7c4:      fmov    x14, d2
10081e7c8:      cbnz    x14, 0x10081e7f0 <_js_object_alloc_with_shape+0xd30>
10081e7cc:      add x9, x9, #0x8
10081e7d0:      add x13, x13, x9
10081e7d4:      and x13, x13, x10
10081e7d8:      ldr d2, [x11, x13]
10081e7dc:      cmeq.8b v3, v2, v0
10081e7e0:      fmov    x14, d3
10081e7e4:      ands    x14, x14, #0x8080808080808080
10081e7e8:      b.ne    0x10081e790 <_js_object_alloc_with_shape+0xcd0>
10081e7ec:      b   0x10081e7c0 <_js_object_alloc_with_shape+0xd00>
10081e7f0:      mov w26, #0x0               ; =0
10081e7f4:      str x8, [x0, #0x5088]
10081e7f8:      add x0, sp, #0x20
10081e7fc:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e800:      ldr w8, [sp]
10081e804:      tbz w8, #0x0, 0x10081e454 <_js_object_alloc_with_shape+0x994>
10081e808:      b   0x10081e45c <_js_object_alloc_with_shape+0x99c>
10081e80c:      ldur    w26, [x15, #-0x8]
10081e810:      str x8, [x0, #0x5088]
10081e814:      add x0, sp, #0x20
10081e818:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e81c:      ldr w8, [sp]
10081e820:      tbz w8, #0x0, 0x10081e454 <_js_object_alloc_with_shape+0x994>
10081e824:      b   0x10081e45c <_js_object_alloc_with_shape+0x99c>
10081e828:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e82c:      ldr x25, [x0, #0x20]
10081e830:      ldr x8, [x25]
10081e834:      cbz x8, 0x10081db88 <_js_object_alloc_with_shape+0xc8>
10081e838:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081e83c:      add x0, x0, #0x120
10081e840:      bl  0x100ab12ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081e844:      mov x0, x20
10081e848:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081e84c:      add x1, sp, #0x28
10081e850:      mov x0, x20
10081e854:      mov x2, x25
10081e858:      mov x3, x19
10081e85c:      bl  0x10059876c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25publish_object_shape_from>
10081e860:      mov x0, x20
10081e864:      mov x1, x26
10081e868:      mov x2, x19
10081e86c:      bl  0x100597fc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081e870:      ldr x8, [x27, #0x48]
10081e874:      cmn x8, #0x1
10081e878:      b.eq    0x10081e8dc <_js_object_alloc_with_shape+0xe1c>
10081e87c:      mrs x9, TPIDRRO_EL0
10081e880:      and x9, x9, #0xfffffffffffffff8
10081e884:      ldr x8, [x9, x8, lsl #3]
10081e888:      cbz x8, 0x10081e8dc <_js_object_alloc_with_shape+0xe1c>
10081e88c:      ldr x8, [x8, #0x19e8]
10081e890:      cbz x8, 0x10081e8dc <_js_object_alloc_with_shape+0xe1c>
10081e894:      ldr x9, [x8]
10081e898:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081e89c:      cmp x9, x10
10081e8a0:      b.hs    0x10081e938 <_js_object_alloc_with_shape+0xe78>
10081e8a4:      add x10, x9, #0x1
10081e8a8:      str x10, [x8]
10081e8ac:      ldr x10, [x8, #0x18]
10081e8b0:      cmp x28, x10
10081e8b4:      b.hs    0x10081e944 <_js_object_alloc_with_shape+0xe84>
10081e8b8:      ldr x10, [x8, #0x10]
10081e8bc:      mov w11, #0x18              ; =24
10081e8c0:      madd    x10, x28, x11, x10
10081e8c4:      ldr x11, [x10]
10081e8c8:      cmp x11, #0x1
10081e8cc:      b.ne    0x10081e948 <_js_object_alloc_with_shape+0xe88>
10081e8d0:      ldr x19, [x10, #0x8]
10081e8d4:      str x9, [x8]
10081e8d8:      b   0x10081e8e8 <_js_object_alloc_with_shape+0xe28>
10081e8dc:      mov x0, x28
10081e8e0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e8e4:      mov x19, x0
10081e8e8:      add x0, sp, #0x18
10081e8ec:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e8f0:      mov x0, x19
10081e8f4:      ldp x29, x30, [sp, #0xa0]
10081e8f8:      ldp x20, x19, [sp, #0x90]
10081e8fc:      ldp x22, x21, [sp, #0x80]
10081e900:      ldp x24, x23, [sp, #0x70]
10081e904:      ldp x26, x25, [sp, #0x60]
10081e908:      ldp x28, x27, [sp, #0x50]
10081e90c:      add sp, sp, #0xb0
10081e910:      ret
10081e914:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e918:      b   0x10081de18 <_js_object_alloc_with_shape+0x358>
10081e91c:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e920:      ldr x8, [x0, #0x30]
10081e924:      ldrb    w8, [x8]
10081e928:      tbnz    w8, #0x0, 0x10081de50 <_js_object_alloc_with_shape+0x390>
10081e92c:      b   0x10081deac <_js_object_alloc_with_shape+0x3ec>
10081e930:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e934:      b   0x10081dddc <_js_object_alloc_with_shape+0x31c>
10081e938:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
10081e93c:      add x0, x0, #0x8a8
10081e940:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081e944:      bl  0x100ae2bc8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
10081e948:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbb0>
10081e94c:      add x0, x0, #0xac
10081e950:      mov w1, #0xb                ; =11
10081e954:      bl  0x100ae2b90 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10081e958:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081e95c:      add x0, x0, #0x798
10081e960:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081e964:      bl  0x100ab0fbc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081e968:      cmp w8, #0x1
10081e96c:      b.ne    0x10081e9b0 <_js_object_alloc_with_shape+0xef0>
10081e970:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081e974:      add x1, x1, #0xbb8
10081e978:      mov x0, x24
10081e97c:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081e980:      strb    wzr, [x24, #0x18]
10081e984:      ldr x28, [x24, #0x10]
10081e988:      ldr x8, [x24]
10081e98c:      cmp x28, x8
10081e990:      mov x0, x25
10081e994:      b.ne    0x10081dc84 <_js_object_alloc_with_shape+0x1c4>
10081e998:      mov x0, x24
10081e99c:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081e9a0:      mov x0, x25
10081e9a4:      b   0x10081dc84 <_js_object_alloc_with_shape+0x1c4>
10081e9a8:      cmp w8, #0x2
10081e9ac:      b.ne    0x10081e9bc <_js_object_alloc_with_shape+0xefc>
10081e9b0:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081e9b4:      add x0, x0, #0x850
10081e9b8:      bl  0x100aef71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081e9bc:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081e9c0:      add x1, x1, #0xbb8
10081e9c4:      mov x25, x0
10081e9c8:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081e9cc:      mov x0, x25
10081e9d0:      strb    wzr, [x25, #0x18]
10081e9d4:      ldr x25, [x25, #0x10]
10081e9d8:      ldr x8, [x0]
10081e9dc:      cmp x25, x8
10081e9e0:      b.ne    0x10081de9c <_js_object_alloc_with_shape+0x3dc>
10081e9e4:      str x0, [sp, #0x10]
10081e9e8:      bl  0x100ad8264 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081e9ec:      ldr x0, [sp, #0x10]
10081e9f0:      b   0x10081de9c <_js_object_alloc_with_shape+0x3dc>
10081e9f4:      mov w0, #0x8                ; =8
10081e9f8:      mov w1, #0x40               ; =64
10081e9fc:      bl  0x100ab0f64 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
