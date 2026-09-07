/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002beb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
1002beb14:      sub sp, sp, #0x190
1002beb18:      stp x26, x25, [sp, #0x140]
1002beb1c:      stp x24, x23, [sp, #0x150]
1002beb20:      stp x22, x21, [sp, #0x160]
1002beb24:      stp x20, x19, [sp, #0x170]
1002beb28:      stp x29, x30, [sp, #0x180]
1002beb2c:      add x29, sp, #0x180
1002beb30:      mov x20, x1
1002beb34:      mov x21, x0
1002beb38:      add x24, sp, #0x90
1002beb3c:      add x22, x0, #0x14
1002beb40:      cmp x1, #0x2
1002beb44:      b.ne    0x1002beb5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48>
1002beb48:      ldrh    w8, [x22]
1002beb4c:      mov w9, #0x7d7b             ; =32123
1002beb50:      cmp w8, w9
1002beb54:      b.eq    0x1002beb90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c>
1002beb58:      b   0x1002bebe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1002beb5c:      cmp x20, #0x3
1002beb60:      b.lo    0x1002bebe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1002beb64:      ldrb    w8, [x22]
1002beb68:      cmp w8, #0x20
1002beb6c:      b.hi    0x1002bebac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x98>
1002beb70:      mov x9, #0x2600             ; =9728
1002beb74:      movk    x9, #0x1, lsl #32
1002beb78:      lsr x9, x9, x8
1002beb7c:      tbz w9, #0x0, 0x1002bebac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x98>
1002beb80:      add x0, x21, #0x14
1002beb84:      mov x1, x20
1002beb88:      bl  0x1002b6354 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1002beb8c:      tbz w0, #0x0, 0x1002bebd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
1002beb90:      ldp x29, x30, [sp, #0x180]
1002beb94:      ldp x20, x19, [sp, #0x170]
1002beb98:      ldp x22, x21, [sp, #0x160]
1002beb9c:      ldp x24, x23, [sp, #0x150]
1002beba0:      ldp x26, x25, [sp, #0x140]
1002beba4:      add sp, sp, #0x190
1002beba8:      b   0x1002b6420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1002bebac:      cmp w8, #0x7b
1002bebb0:      b.ne    0x1002bebd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
1002bebb4:      ldrb    w8, [x21, #0x15]
1002bebb8:      cmp w8, #0x20
1002bebbc:      b.hi    0x1002bebd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xbc>
1002bebc0:      mov x9, #0x2600             ; =9728
1002bebc4:      movk    x9, #0x1, lsl #32
1002bebc8:      lsr x9, x9, x8
1002bebcc:      tbnz    w9, #0x0, 0x1002beb80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
1002bebd0:      cmp w8, #0x7d
1002bebd4:      b.eq    0x1002beb80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
1002bebd8:      cmp x20, #0x41
1002bebdc:      b.hs    0x1002bec44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x130>
1002bebe0:      add x0, sp, #0x90
1002bebe4:      add x1, x21, #0x14
1002bebe8:      mov x2, x20
1002bebec:      bl  0x1002b68ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1002bebf0:      ldr x8, [sp, #0x90]
1002bebf4:      cbz x8, 0x1002bed08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1002bebf8:      ldr x8, [sp, #0x118]
1002bebfc:      str x8, [sp, #0x80]
1002bec00:      ldur    q0, [x24, #0x48]
1002bec04:      ldur    q1, [x24, #0x58]
1002bec08:      stp q0, q1, [sp, #0x40]
1002bec0c:      ldur    q0, [x24, #0x68]
1002bec10:      ldur    q1, [x24, #0x78]
1002bec14:      stp q0, q1, [sp, #0x60]
1002bec18:      ldur    q0, [x24, #0x8]
1002bec1c:      ldur    q1, [x24, #0x18]
1002bec20:      stp q0, q1, [sp]
1002bec24:      ldur    q0, [x24, #0x28]
1002bec28:      ldur    q1, [x24, #0x38]
1002bec2c:      stp q0, q1, [sp, #0x20]
1002bec30:      mov x0, sp
1002bec34:      bl  0x1002b734c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1002bec38:      tbz w0, #0x0, 0x1002bed08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1002bec3c:      mov x22, x1
1002bec40:      b   0x1002bf11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1002bec44:      cmp x20, #0x3e9
1002bec48:      b.lo    0x1002bed08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1002bec4c:      add x0, x21, #0x14
1002bec50:      mov x1, x20
1002bec54:      mov w2, #0x3e8              ; =1000
1002bec58:      bl  0x1002b7e60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1002bec5c:      tbz w0, #0x0, 0x1002bed08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1002bec60:      add x0, x21, #0x14
1002bec64:      mov x1, x20
1002bec68:      mov w2, #0xa120             ; =41248
1002bec6c:      movk    w2, #0x7, lsl #16
1002bec70:      bl  0x1002b7e60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1002bec74:      tbnz    w0, #0x0, 0x1002bf290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x77c>
1002bec78:      stur    x20, [x29, #-0x58]
1002bec7c:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1002bec80:      bfxil   x8, x21, #0, #48
1002bec84:      str x8, [sp, #0x90]
1002bec88:      adrp    x19, 0x1010ab000 <_anon.b822d7b979bdf0233543f470364426b7.316+0x270>
1002bec8c:      add x19, x19, #0x540
1002bec90:      add x1, sp, #0x90
1002bec94:      mov x0, x19
1002bec98:      bl  0x100137690 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1002bec9c:      mov x21, x0
1002beca0:      stp x0, x22, [x29, #-0x50]
1002beca4:      str x20, [sp]
1002beca8:      sub x8, x29, #0x48
1002becac:      mov x9, sp
1002becb0:      stp x8, x9, [sp, #0x90]
1002becb4:      sub x8, x29, #0x50
1002becb8:      sub x9, x29, #0x58
1002becbc:      stp x8, x9, [sp, #0xa0]
1002becc0:      adrp    x0, 0x1010a9000 <_anon.2d62e9c08ab2025701038807088a1a53.1055+0x5b8>
1002becc4:      add x0, x0, #0xb48
1002becc8:      add x1, sp, #0x90
1002beccc:      bl  0x10012c5d8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1002becd0:      mov x20, x0
1002becd4:      mov x22, x1
1002becd8:      str x21, [sp, #0x90]
1002becdc:      add x1, sp, #0x90
1002bece0:      mov x0, x19
1002bece4:      bl  0x100137728 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1002bece8:      adrp    x0, 0x1010ab000 <_anon.b822d7b979bdf0233543f470364426b7.316+0x270>
1002becec:      add x0, x0, #0x548
1002becf0:      bl  0x100139fb4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1002becf4:      tbnz    w20, #0x0, 0x1002bf11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1002becf8:      adrp    x0, 0x100dc6000 <_anon.2d62e9c08ab2025701038807088a1a53.881+0x128c>
1002becfc:      add x0, x0, #0x7fd
1002bed00:      mov w1, #0x29               ; =41
1002bed04:      bl  0x1002bff50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1002bed08:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
1002bed0c:      add x0, x0, #0x590
1002bed10:      ldr x8, [x0]
1002bed14:      blr x8
1002bed18:      mov x19, x0
1002bed1c:      ldrb    w8, [x0, #0x20]
1002bed20:      cbnz    w8, 0x1002bf15c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x648>
1002bed24:      ldr x8, [x19]
1002bed28:      cbnz    x8, 0x1002bf1d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
1002bed2c:      mov x22, #0x7fff000000000000 ; =9223090561878065152
1002bed30:      bfxil   x22, x21, #0, #48
1002bed34:      mov x8, #-0x1               ; =-1
1002bed38:      str x8, [x19]
1002bed3c:      mov x21, x19
1002bed40:      ldr x8, [x21, #0x8]!
1002bed44:      ldr x23, [x19, #0x18]
1002bed48:      cmp x23, x8
1002bed4c:      b.ne    0x1002bed58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x244>
1002bed50:      mov x0, x21
1002bed54:      bl  0x100ccd37c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1002bed58:      ldr x8, [x19, #0x10]
1002bed5c:      str x22, [x8, x23, lsl #3]
1002bed60:      add x8, x23, #0x1
1002bed64:      str x8, [x19, #0x18]
1002bed68:      ldr x8, [x19]
1002bed6c:      add x8, x8, #0x1
1002bed70:      str x8, [x19]
1002bed74:      mov x0, #0x0                ; =0
1002bed78:      bl  0x1008ae260 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1002bed7c:      ldrb    w8, [x0]
1002bed80:      strb    wzr, [x0]
1002bed84:      cbz w8, 0x1002bedbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a8>
1002bed88:      mov x22, x0
1002bed8c:      mov x0, #0x0                ; =0
1002bed90:      bl  0x1008ae280 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1002bed94:      ldrb    w8, [x0]
1002bed98:      tst w8, #0x3
1002bed9c:      b.ne    0x1002bedb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a0>
1002beda0:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
1002beda4:      add x8, x8, #0x5d8
1002beda8:      ldapr   w8, [x8]
1002bedac:      cmp w8, #0x0
1002bedb0:      b.le    0x1002bef14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x400>
1002bedb4:      mov w8, #0x1                ; =1
1002bedb8:      strb    w8, [x22]
1002bedbc:      ldrb    w8, [x19, #0x20]
1002bedc0:      cbnz    w8, 0x1002bef5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x448>
1002bedc4:      ldr x8, [x19]
1002bedc8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bedcc:      cmp x8, x9
1002bedd0:      b.hs    0x1002bf240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1002bedd4:      add x9, x8, #0x1
1002bedd8:      str x9, [x19]
1002beddc:      ldr x9, [x19, #0x18]
1002bede0:      cmp x23, x9
1002bede4:      b.hs    0x1002bedf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e4>
1002bede8:      ldr x9, [x19, #0x10]
1002bedec:      ldr x9, [x9, x23, lsl #3]
1002bedf0:      and x22, x9, #0xffffffffffff
1002bedf4:      b   0x1002bedfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e8>
1002bedf8:      mov w22, #0x1               ; =1
1002bedfc:      str x8, [x19]
1002bee00:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002bee04:      add x8, x8, #0x568
1002bee08:      ldapr   x8, [x8]
1002bee0c:      cbnz    x8, 0x1002bf13c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x628>
1002bee10:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002bee14:      ldrb    w8, [x8, #0x570]
1002bee18:      cmp w8, #0x2
1002bee1c:      b.eq    0x1002bef94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1002bee20:      cmp w8, #0x1
1002bee24:      b.ne    0x1002bee68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x354>
1002bee28:      stp x23, x20, [x29, #-0x58]
1002bee2c:      ldrb    w8, [x19, #0x20]
1002bee30:      cbnz    w8, 0x1002bf214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
1002bee34:      ldr x8, [x19]
1002bee38:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bee3c:      cmp x8, x9
1002bee40:      b.hs    0x1002bf240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1002bee44:      add x9, x8, #0x1
1002bee48:      str x9, [x19]
1002bee4c:      ldr x9, [x19, #0x18]
1002bee50:      cmp x23, x9
1002bee54:      b.hs    0x1002beea8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x394>
1002bee58:      ldr x9, [x19, #0x10]
1002bee5c:      ldr x9, [x9, x23, lsl #3]
1002bee60:      and x9, x9, #0xffffffffffff
1002bee64:      b   0x1002beeac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x398>
1002bee68:      add x8, x22, #0x14
1002bee6c:      sub x9, x20, #0x400
1002bee70:      mov w10, #0xfffc00          ; =16776192
1002bee74:      cmp x9, x10
1002bee78:      b.hi    0x1002bef94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1002bee7c:      mov x9, #0x2600             ; =9728
1002bee80:      movk    x9, #0x1, lsl #32
1002bee84:      mov x10, x20
1002bee88:      ldrb    w11, [x8], #0x1
1002bee8c:      cmp w11, #0x20
1002bee90:      b.hi    0x1002bef8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x478>
1002bee94:      lsr x12, x9, x11
1002bee98:      tbz w12, #0x0, 0x1002bef8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x478>
1002bee9c:      subs    x10, x10, #0x1
1002beea0:      b.ne    0x1002bee88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x374>
1002beea4:      b   0x1002bef94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1002beea8:      mov w9, #0x1                ; =1
1002beeac:      str x8, [x19]
1002beeb0:      add x8, x9, #0x14
1002beeb4:      stur    x8, [x29, #-0x48]
1002beeb8:      str x20, [sp]
1002beebc:      sub x8, x29, #0x48
1002beec0:      mov x9, sp
1002beec4:      stp x8, x9, [sp, #0x90]
1002beec8:      sub x8, x29, #0x58
1002beecc:      sub x9, x29, #0x50
1002beed0:      stp x8, x9, [sp, #0xa0]
1002beed4:      adrp    x0, 0x1010a9000 <_anon.2d62e9c08ab2025701038807088a1a53.1055+0x5b8>
1002beed8:      add x0, x0, #0xb48
1002beedc:      add x1, sp, #0x90
1002beee0:      bl  0x10012c9a4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
1002beee4:      cmp x0, #0x1
1002beee8:      b.ne    0x1002bef94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1002beeec:      mov x22, x1
1002beef0:      ldrb    w8, [x19, #0x20]
1002beef4:      cbnz    w8, 0x1002bf24c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
1002beef8:      ldr x8, [x19]
1002beefc:      cbnz    x8, 0x1002bf208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
1002bef00:      ldr x8, [x19, #0x18]
1002bef04:      cmp x23, x8
1002bef08:      b.hi    0x1002bf11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1002bef0c:      str x23, [x19, #0x18]
1002bef10:      b   0x1002bf11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1002bef14:      mov x0, #0x0                ; =0
1002bef18:      bl  0x1008ae3c8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1002bef1c:      ldr x22, [x0]
1002bef20:      mov x0, #0x0                ; =0
1002bef24:      bl  0x1008ae160 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1002bef28:      ldr x8, [x0]
1002bef2c:      cmp x8, x22
1002bef30:      b.ls    0x1002bef50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x43c>
1002bef34:      str x22, [x0]
1002bef38:      adrp    x0, 0x101138000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy36GC_LAST_COLLECTION_POST_IN_USE_BYTES0s_023___RUST_STD_INTERNAL_VAL>
1002bef3c:      add x0, x0, #0x228
1002bef40:      ldr x8, [x0]
1002bef44:      blr x8
1002bef48:      mov w8, #0x1                ; =1
1002bef4c:      strb    w8, [x0]
1002bef50:      bl  0x100875f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1002bef54:      ldrb    w8, [x19, #0x20]
1002bef58:      cbz w8, 0x1002bedc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b0>
1002bef5c:      cmp w8, #0x2
1002bef60:      b.eq    0x1002bf254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1002bef64:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bef68:      add x1, x1, #0xf78
1002bef6c:      mov x0, x19
1002bef70:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bef74:      strb    wzr, [x19, #0x20]
1002bef78:      ldr x8, [x19]
1002bef7c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bef80:      cmp x8, x9
1002bef84:      b.lo    0x1002bedd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c0>
1002bef88:      b   0x1002bf240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1002bef8c:      cmp w11, #0x5b
1002bef90:      b.eq    0x1002bee28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x314>
1002bef94:      bl  0x100875f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1002bef98:      bl  0x100875970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1002bef9c:      ldrb    w8, [x19, #0x20]
1002befa0:      cbnz    w8, 0x1002bf184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x670>
1002befa4:      ldr x8, [x19]
1002befa8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002befac:      cmp x8, x9
1002befb0:      b.hs    0x1002bf240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1002befb4:      add x9, x8, #0x1
1002befb8:      str x9, [x19]
1002befbc:      ldr x10, [x19, #0x18]
1002befc0:      mov w9, #0x1                ; =1
1002befc4:      cmp x23, x10
1002befc8:      b.hs    0x1002befdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4c8>
1002befcc:      ldr x10, [x19, #0x10]
1002befd0:      ldr x10, [x10, x23, lsl #3]
1002befd4:      and x10, x10, #0xffffffffffff
1002befd8:      b   0x1002befe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4cc>
1002befdc:      mov w10, #0x1               ; =1
1002befe0:      str x8, [x19]
1002befe4:      add x8, x10, #0x14
1002befe8:      movi.2d v0, #0000000000000000
1002befec:      stur    q0, [x24, #0x78]
1002beff0:      stur    q0, [x24, #0x68]
1002beff4:      stur    q0, [x24, #0x58]
1002beff8:      stur    q0, [x24, #0x48]
1002beffc:      strb    w9, [sp, #0x120]
1002bf000:      mov x9, #-0x1               ; =-1
1002bf004:      stp x8, x20, [sp, #0xb8]
1002bf008:      str x9, [sp, #0x90]
1002bf00c:      stp xzr, xzr, [sp, #0xc8]
1002bf010:      str xzr, [sp, #0x118]
1002bf014:      add x0, sp, #0x90
1002bf018:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1002bf01c:      mov x22, x0
1002bf020:      ldp x8, x9, [sp, #0xc0]
1002bf024:      cmp x9, x8
1002bf028:      b.hs    0x1002bf05c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
1002bf02c:      ldr x10, [sp, #0xb8]
1002bf030:      mov x11, #0x2600            ; =9728
1002bf034:      movk    x11, #0x1, lsl #32
1002bf038:      ldrb    w12, [x10, x9]
1002bf03c:      cmp w12, #0x20
1002bf040:      b.hi    0x1002bf05c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
1002bf044:      lsr x12, x11, x12
1002bf048:      tbz w12, #0x0, 0x1002bf05c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
1002bf04c:      add x9, x9, #0x1
1002bf050:      cmp x8, x9
1002bf054:      b.ne    0x1002bf038 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x524>
1002bf058:      mov x9, x8
1002bf05c:      ldrb    w20, [sp, #0x120]
1002bf060:      cmp x9, x8
1002bf064:      cset    w24, eq
1002bf068:      ldrb    w8, [x19, #0x20]
1002bf06c:      cbnz    w8, 0x1002bf1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6a0>
1002bf070:      ldr x8, [x19]
1002bf074:      cbnz    x8, 0x1002bf1d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
1002bf078:      mov x8, #-0x1               ; =-1
1002bf07c:      str x8, [x19]
1002bf080:      ldr x25, [x19, #0x18]
1002bf084:      ldr x8, [x19, #0x8]
1002bf088:      cmp x25, x8
1002bf08c:      b.ne    0x1002bf098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x584>
1002bf090:      mov x0, x21
1002bf094:      bl  0x100ccd37c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1002bf098:      ldr x8, [x19, #0x10]
1002bf09c:      str x22, [x8, x25, lsl #3]
1002bf0a0:      add x8, x25, #0x1
1002bf0a4:      str x8, [x19, #0x18]
1002bf0a8:      ldr x8, [x19]
1002bf0ac:      add x8, x8, #0x1
1002bf0b0:      str x8, [x19]
1002bf0b4:      mov x0, #0x0                ; =0
1002bf0b8:      bl  0x1008ae280 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1002bf0bc:      ldrb    w8, [x0]
1002bf0c0:      and w8, w8, #0xfffffffd
1002bf0c4:      strb    w8, [x0]
1002bf0c8:      bl  0x1008768bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1002bf0cc:      bl  0x10087ace8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
1002bf0d0:      ldrb    w8, [x19, #0x20]
1002bf0d4:      cbnz    w8, 0x1002bf1e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
1002bf0d8:      ldr x8, [x19]
1002bf0dc:      cbnz    x8, 0x1002bf208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
1002bf0e0:      and w20, w24, w20
1002bf0e4:      ldr x8, [x19, #0x18]
1002bf0e8:      cmp x23, x8
1002bf0ec:      b.hi    0x1002bf0f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5e0>
1002bf0f0:      str x23, [x19, #0x18]
1002bf0f4:      adrp    x0, 0x1010ab000 <_anon.b822d7b979bdf0233543f470364426b7.316+0x270>
1002bf0f8:      add x0, x0, #0x548
1002bf0fc:      bl  0x100139ce4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
1002bf100:      tbz w20, #0x0, 0x1002bf280 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x76c>
1002bf104:      ldr x8, [sp, #0x90]
1002bf108:      cmn x8, #0x1
1002bf10c:      b.eq    0x1002bf11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1002bf110:      cbz x8, 0x1002bf11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1002bf114:      ldr x0, [sp, #0x98]
1002bf118:      bl  0x100ce1540 <_mi_free>
1002bf11c:      mov x0, x22
1002bf120:      ldp x29, x30, [sp, #0x180]
1002bf124:      ldp x20, x19, [sp, #0x170]
1002bf128:      ldp x22, x21, [sp, #0x160]
1002bf12c:      ldp x24, x23, [sp, #0x150]
1002bf130:      ldp x26, x25, [sp, #0x140]
1002bf134:      add sp, sp, #0x190
1002bf138:      ret
1002bf13c:      adrp    x0, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002bf140:      add x0, x0, #0x568
1002bf144:      bl  0x100cc57e8 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
1002bf148:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002bf14c:      ldrb    w8, [x8, #0x570]
1002bf150:      cmp w8, #0x2
1002bf154:      b.ne    0x1002bee20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x30c>
1002bf158:      b   0x1002bef94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1002bf15c:      cmp w8, #0x1
1002bf160:      b.ne    0x1002bf254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1002bf164:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf168:      add x1, x1, #0xf78
1002bf16c:      mov x0, x19
1002bf170:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf174:      strb    wzr, [x19, #0x20]
1002bf178:      ldr x8, [x19]
1002bf17c:      cbz x8, 0x1002bed2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x218>
1002bf180:      b   0x1002bf1d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
1002bf184:      cmp w8, #0x2
1002bf188:      b.eq    0x1002bf254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1002bf18c:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf190:      add x1, x1, #0xf78
1002bf194:      mov x0, x19
1002bf198:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf19c:      strb    wzr, [x19, #0x20]
1002bf1a0:      ldr x8, [x19]
1002bf1a4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bf1a8:      cmp x8, x9
1002bf1ac:      b.lo    0x1002befb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a0>
1002bf1b0:      b   0x1002bf240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1002bf1b4:      cmp w8, #0x2
1002bf1b8:      b.eq    0x1002bf254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1002bf1bc:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf1c0:      add x1, x1, #0xf78
1002bf1c4:      mov x0, x19
1002bf1c8:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf1cc:      strb    wzr, [x19, #0x20]
1002bf1d0:      ldr x8, [x19]
1002bf1d4:      cbz x8, 0x1002bf078 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x564>
1002bf1d8:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1002bf1dc:      add x0, x0, #0xdf8
1002bf1e0:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1002bf1e4:      cmp w8, #0x2
1002bf1e8:      b.eq    0x1002bf254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1002bf1ec:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf1f0:      add x1, x1, #0xf78
1002bf1f4:      mov x0, x19
1002bf1f8:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf1fc:      strb    wzr, [x19, #0x20]
1002bf200:      ldr x8, [x19]
1002bf204:      cbz x8, 0x1002bf0e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5cc>
1002bf208:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1002bf20c:      add x0, x0, #0xe58
1002bf210:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1002bf214:      cmp w8, #0x2
1002bf218:      b.eq    0x1002bf254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1002bf21c:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf220:      add x1, x1, #0xf78
1002bf224:      mov x0, x19
1002bf228:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf22c:      strb    wzr, [x19, #0x20]
1002bf230:      ldr x8, [x19]
1002bf234:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002bf238:      cmp x8, x9
1002bf23c:      b.lo    0x1002bee44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x330>
1002bf240:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1002bf244:      add x0, x0, #0xdc8
1002bf248:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1002bf24c:      cmp w8, #0x2
1002bf250:      b.ne    0x1002bf260 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x74c>
1002bf254:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1002bf258:      add x0, x0, #0xed8
1002bf25c:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1002bf260:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1002bf264:      add x1, x1, #0xf78
1002bf268:      mov x0, x19
1002bf26c:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002bf270:      strb    wzr, [x19, #0x20]
1002bf274:      ldr x8, [x19]
1002bf278:      cbz x8, 0x1002bef00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3ec>
1002bf27c:      b   0x1002bf208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
1002bf280:      adrp    x0, 0x100dca000 <_anon.b822d7b979bdf0233543f470364426b7.519+0xab>
1002bf284:      add x0, x0, #0x1e2
1002bf288:      mov w1, #0x21               ; =33
1002bf28c:      bl  0x1002bff50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1002bf290:      add x0, sp, #0x90
1002bf294:      bl  0x1002bfff8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
1002bf298:      ldp x0, x1, [sp, #0x98]
1002bf29c:      bl  0x1002bf94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
