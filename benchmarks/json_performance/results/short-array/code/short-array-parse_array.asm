/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008996d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array>:
1008996d4:      sub sp, sp, #0x30
1008996d8:      stp x20, x19, [sp, #0x10]
1008996dc:      stp x29, x30, [sp, #0x20]
1008996e0:      add x29, sp, #0x20
1008996e4:      mov x19, x0
1008996e8:      ldp x8, x9, [x0, #0x30]
1008996ec:      add x20, x9, #0x1
1008996f0:      str x20, [x0, #0x38]
1008996f4:      cmp x20, x8
1008996f8:      b.hs    0x10089975c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
1008996fc:      ldr x9, [x19, #0x28]
100899700:      mov x10, #0x2600            ; =9728
100899704:      movk    x10, #0x1, lsl #32
100899708:      ldrb    w11, [x9, x20]
10089970c:      cmp w11, #0x20
100899710:      b.hi    0x10089975c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
100899714:      lsr x11, x10, x11
100899718:      tbz w11, #0x0, 0x10089975c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10089971c:      add x20, x20, #0x1
100899720:      str x20, [x19, #0x38]
100899724:      cmp x8, x20
100899728:      b.ne    0x100899708 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x34>
10089972c:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
100899730:      add x0, x0, #0x660
100899734:      ldr x8, [x0]
100899738:      blr x8
10089973c:      ldrb    w8, [x0, #0x20]
100899740:      cbnz    w8, 0x100899834 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x160>
100899744:      ldr x8, [x0]
100899748:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10089974c:      cmp x8, x9
100899750:      b.hs    0x100899864 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
100899754:      ldr x1, [x0, #0x18]
100899758:      b   0x1008997ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10089975c:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
100899760:      add x0, x0, #0x660
100899764:      ldr x9, [x0]
100899768:      blr x9
10089976c:      ldrb    w9, [x0, #0x20]
100899770:      cbnz    w9, 0x100899800 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x12c>
100899774:      ldr x9, [x0]
100899778:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10089977c:      cmp x9, x10
100899780:      b.hs    0x100899864 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
100899784:      ldr x1, [x0, #0x18]
100899788:      subs    x8, x8, x20
10089978c:      b.ls    0x1008997ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
100899790:      ldr x9, [x19, #0x28]
100899794:      ldrb    w9, [x9, x20]
100899798:      cmp w9, #0x7b
10089979c:      b.ne    0x1008997ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
1008997a0:      mov x9, #-0x5555555555555556 ; =-6148914691236517206
1008997a4:      movk    x9, #0xaaab
1008997a8:      umulh   x8, x8, x9
1008997ac:      lsr x8, x8, #6
1008997b0:      mov w9, #0x10               ; =16
1008997b4:      cmp x8, #0x10
1008997b8:      csel    x8, x8, x9, hi
1008997bc:      mov w9, #0x4000             ; =16384
1008997c0:      cmp x8, #0x4, lsl #12       ; =0x4000
1008997c4:      csel    x0, x8, x9, lo
1008997c8:      mov x20, x1
1008997cc:      bl  0x10091817c <_js_array_alloc>
1008997d0:      mov x1, x0
1008997d4:      mov x0, x19
1008997d8:      mov x2, x20
1008997dc:      ldp x29, x30, [sp, #0x20]
1008997e0:      ldp x20, x19, [sp, #0x10]
1008997e4:      add sp, sp, #0x30
1008997e8:      b   0x100899df4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
1008997ec:      mov x0, x19
1008997f0:      ldp x29, x30, [sp, #0x20]
1008997f4:      ldp x20, x19, [sp, #0x10]
1008997f8:      add sp, sp, #0x30
1008997fc:      b   0x10089a600 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>
100899800:      cmp w9, #0x2
100899804:      b.eq    0x100899870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
100899808:      stp x8, x0, [sp]
10089980c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
100899810:      add x1, x1, #0xd0
100899814:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100899818:      ldp x8, x0, [sp]
10089981c:      strb    wzr, [x0, #0x20]
100899820:      ldr x9, [x0]
100899824:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100899828:      cmp x9, x10
10089982c:      b.lo    0x100899784 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xb0>
100899830:      b   0x100899864 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
100899834:      cmp w8, #0x1
100899838:      b.ne    0x100899870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
10089983c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
100899840:      add x1, x1, #0xd0
100899844:      str x0, [sp, #0x8]
100899848:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089984c:      ldr x0, [sp, #0x8]
100899850:      strb    wzr, [x0, #0x20]
100899854:      ldr x8, [x0]
100899858:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10089985c:      cmp x8, x9
100899860:      b.lo    0x100899754 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x80>
100899864:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100899868:      add x0, x0, #0xe70
10089986c:      bl  0x100c99adc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100899870:      adrp    x0, 0x10109b000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100899874:      add x0, x0, #0xed8
100899878:      bl  0x100cdb71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
