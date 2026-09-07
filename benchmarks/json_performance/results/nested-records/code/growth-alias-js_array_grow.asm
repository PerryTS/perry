/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001005e38c0 <_js_array_grow>:
1005e38c0:      sub sp, sp, #0x90
1005e38c4:      stp x28, x27, [sp, #0x30]
1005e38c8:      stp x26, x25, [sp, #0x40]
1005e38cc:      stp x24, x23, [sp, #0x50]
1005e38d0:      stp x22, x21, [sp, #0x60]
1005e38d4:      stp x20, x19, [sp, #0x70]
1005e38d8:      stp x29, x30, [sp, #0x80]
1005e38dc:      add x29, sp, #0x80
1005e38e0:      cmp x0, #0xfff
1005e38e4:      b.ls    0x1005e3c34 <_js_array_grow+0x374>
1005e38e8:      mov x19, x0
1005e38ec:      lsr x8, x0, #51
1005e38f0:      cmp x8, #0xfff
1005e38f4:      b.lo    0x1005e390c <_js_array_grow+0x4c>
1005e38f8:      mov w8, #0x7ffc             ; =32764
1005e38fc:      cmp x8, x19, lsr #48
1005e3900:      b.eq    0x1005e3c34 <_js_array_grow+0x374>
1005e3904:      ands    x19, x19, #0xffffffffffff
1005e3908:      b.eq    0x1005e3c34 <_js_array_grow+0x374>
1005e390c:      and x8, x19, #0xfffffffffff00000
1005e3910:      lsr x9, x19, #47
1005e3914:      cmp x9, #0x0
1005e3918:      ccmp    x8, #0x0, #0x4, eq
1005e391c:      b.eq    0x1005e3c34 <_js_array_grow+0x374>
1005e3920:      tst x19, #0x3
1005e3924:      ccmp    x19, #0x7, #0x0, eq
1005e3928:      mov x22, x1
1005e392c:      b.ls    0x1005e3a34 <_js_array_grow+0x174>
1005e3930:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1005e3934:      add x8, x8, #0xfe0
1005e3938:      ldr x8, [x8]
1005e393c:      cmn x8, #0x1
1005e3940:      b.eq    0x1005e3e18 <_js_array_grow+0x558>
1005e3944:      mrs x9, TPIDRRO_EL0
1005e3948:      and x9, x9, #0xfffffffffffffff8
1005e394c:      ldr x0, [x9, x8, lsl #3]
1005e3950:      cbz x0, 0x1005e3e18 <_js_array_grow+0x558>
1005e3954:      lsr x1, x19, #20
1005e3958:      ldr x8, [x0, #0x10]
1005e395c:      ldrb    w9, [x8, #0x28]
1005e3960:      tbz w9, #0x0, 0x1005e3980 <_js_array_grow+0xc0>
1005e3964:      ldr x9, [x8, #0x20]
1005e3968:      cmp x9, x1
1005e396c:      b.ne    0x1005e3980 <_js_array_grow+0xc0>
1005e3970:      ldp x9, x10, [x8]
1005e3974:      cmp x9, x19
1005e3978:      ccmp    x10, x19, #0x0, ls
1005e397c:      b.hi    0x1005e39fc <_js_array_grow+0x13c>
1005e3980:      ldrb    w9, [x8, #0x58]
1005e3984:      cbz w9, 0x1005e39a4 <_js_array_grow+0xe4>
1005e3988:      ldr x9, [x8, #0x50]
1005e398c:      cmp x9, x1
1005e3990:      b.ne    0x1005e39a4 <_js_array_grow+0xe4>
1005e3994:      ldp x9, x10, [x8, #0x30]
1005e3998:      cmp x9, x19
1005e399c:      ccmp    x10, x19, #0x0, ls
1005e39a0:      b.hi    0x1005e39f0 <_js_array_grow+0x130>
1005e39a4:      ldrb    w9, [x8, #0x88]
1005e39a8:      cbz w9, 0x1005e39c8 <_js_array_grow+0x108>
1005e39ac:      ldr x9, [x8, #0x80]
1005e39b0:      cmp x9, x1
1005e39b4:      b.ne    0x1005e39c8 <_js_array_grow+0x108>
1005e39b8:      ldp x9, x10, [x8, #0x60]
1005e39bc:      cmp x9, x19
1005e39c0:      ccmp    x10, x19, #0x0, ls
1005e39c4:      b.hi    0x1005e39f8 <_js_array_grow+0x138>
1005e39c8:      ldrb    w9, [x8, #0xb8]
1005e39cc:      cbz w9, 0x1005e3a08 <_js_array_grow+0x148>
1005e39d0:      ldr x9, [x8, #0xb0]
1005e39d4:      cmp x9, x1
1005e39d8:      b.ne    0x1005e3a08 <_js_array_grow+0x148>
1005e39dc:      ldp x9, x10, [x8, #0x90]!
1005e39e0:      cmp x9, x19
1005e39e4:      ccmp    x10, x19, #0x0, ls
1005e39e8:      b.hi    0x1005e39fc <_js_array_grow+0x13c>
1005e39ec:      b   0x1005e3a08 <_js_array_grow+0x148>
1005e39f0:      add x8, x8, #0x30
1005e39f4:      b   0x1005e39fc <_js_array_grow+0x13c>
1005e39f8:      add x8, x8, #0x60
1005e39fc:      ldrb    w8, [x8, #0x19]
1005e3a00:      cmp w8, #0xff
1005e3a04:      b.ne    0x1005e3a14 <_js_array_grow+0x154>
1005e3a08:      mov x0, x19
1005e3a0c:      bl  0x1009960b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1005e3a10:      and w8, w0, #0xff
1005e3a14:      cbz w8, 0x1005e3a34 <_js_array_grow+0x174>
1005e3a18:      ldurb   w8, [x19, #-0x8]
1005e3a1c:      ldurb   w9, [x19, #-0x7]
1005e3a20:      mov w10, #0x82              ; =130
1005e3a24:      and w9, w9, w10
1005e3a28:      cmp w9, #0x2
1005e3a2c:      ccmp    w8, #0x1, #0x0, eq
1005e3a30:      b.eq    0x1005e3c58 <_js_array_grow+0x398>
1005e3a34:      mov x0, x19
1005e3a38:      bl  0x1005ac688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1005e3a3c:      mov x8, x0
1005e3a40:      cbz x0, 0x1005e3adc <_js_array_grow+0x21c>
1005e3a44:      ldrb    w9, [x8]
1005e3a48:      cmp w9, #0x1
1005e3a4c:      b.ne    0x1005e3b6c <_js_array_grow+0x2ac>
1005e3a50:      ldrsb   w9, [x8, #0x1]
1005e3a54:      mov x0, x8
1005e3a58:      tbz w9, #0x1f, 0x1005e3bb0 <_js_array_grow+0x2f0>
1005e3a5c:      mov x20, x8
1005e3a60:      ldr x19, [x8, #0x8]
1005e3a64:      mov x0, x19
1005e3a68:      bl  0x1005ac688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1005e3a6c:      mov x1, x22
1005e3a70:      cbz x0, 0x1005e3c34 <_js_array_grow+0x374>
1005e3a74:      ldrb    w8, [x0]
1005e3a78:      cmp w8, #0x1
1005e3a7c:      b.ne    0x1005e3c34 <_js_array_grow+0x374>
1005e3a80:      ldrsb   w8, [x0, #0x1]
1005e3a84:      tbz w8, #0x1f, 0x1005e3b64 <_js_array_grow+0x2a4>
1005e3a88:      mov w21, #0x1               ; =1
1005e3a8c:      ldr x19, [x0, #0x8]
1005e3a90:      mov x0, x19
1005e3a94:      bl  0x1005ac688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1005e3a98:      mov x1, x22
1005e3a9c:      cbz x0, 0x1005e3c34 <_js_array_grow+0x374>
1005e3aa0:      ldrb    w8, [x0]
1005e3aa4:      cmp w8, #0x1
1005e3aa8:      b.ne    0x1005e3c34 <_js_array_grow+0x374>
1005e3aac:      cmp w21, #0x3f
1005e3ab0:      b.hi    0x1005e3c34 <_js_array_grow+0x374>
1005e3ab4:      add w21, w21, #0x1
1005e3ab8:      ldrsb   w8, [x0, #0x1]
1005e3abc:      tbnz    w8, #0x1f, 0x1005e3a8c <_js_array_grow+0x1cc>
1005e3ac0:      mov x8, x20
1005e3ac4:      str x19, [x20, #0x8]
1005e3ac8:      ldrb    w9, [x20, #0x1]
1005e3acc:      orr w9, w9, #0x80
1005e3ad0:      strb    w9, [x20, #0x1]
1005e3ad4:      ldrb    w9, [x0]
1005e3ad8:      b   0x1005e3b74 <_js_array_grow+0x2b4>
1005e3adc:      mov x20, x8
1005e3ae0:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
1005e3ae4:      add x8, x8, #0x710
1005e3ae8:      ldaprb  w8, [x8]
1005e3aec:      cbz w8, 0x1005e3b1c <_js_array_grow+0x25c>
1005e3af0:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1005e3af4:      add x8, x8, #0x258
1005e3af8:      ldapr   x9, [x8]
1005e3afc:      cmp x9, x19
1005e3b00:      b.hi    0x1005e3b1c <_js_array_grow+0x25c>
1005e3b04:      ldapur  x8, [x8, #0x8]
1005e3b08:      cmp x8, x19
1005e3b0c:      b.lo    0x1005e3b1c <_js_array_grow+0x25c>
1005e3b10:      mov x0, x19
1005e3b14:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1005e3b18:      tbnz    w0, #0x0, 0x1005e3b60 <_js_array_grow+0x2a0>
1005e3b1c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
1005e3b20:      add x8, x8, #0xa61
1005e3b24:      ldaprb  w8, [x8]
1005e3b28:      mov x1, x22
1005e3b2c:      cbz w8, 0x1005e3c34 <_js_array_grow+0x374>
1005e3b30:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1005e3b34:      add x8, x8, #0xae8
1005e3b38:      ldapr   x9, [x8]
1005e3b3c:      cmp x9, x19
1005e3b40:      b.hi    0x1005e3c34 <_js_array_grow+0x374>
1005e3b44:      ldapur  x8, [x8, #0x8]
1005e3b48:      cmp x8, x19
1005e3b4c:      b.lo    0x1005e3c34 <_js_array_grow+0x374>
1005e3b50:      mov x0, x19
1005e3b54:      bl  0x100895b28 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1005e3b58:      mov x1, x22
1005e3b5c:      tbz w0, #0x0, 0x1005e3c34 <_js_array_grow+0x374>
1005e3b60:      mov x0, #0x0                ; =0
1005e3b64:      mov x8, x20
1005e3b68:      b   0x1005e3bb0 <_js_array_grow+0x2f0>
1005e3b6c:      mov x0, x8
1005e3b70:      mov x1, x22
1005e3b74:      cmp w9, #0x1
1005e3b78:      b.eq    0x1005e3bb0 <_js_array_grow+0x2f0>
1005e3b7c:      cmp w9, #0x9
1005e3b80:      b.ne    0x1005e3c34 <_js_array_grow+0x374>
1005e3b84:      ldr w8, [x19, #0x4]
1005e3b88:      mov w9, #0x5841             ; =22593
1005e3b8c:      movk    w9, #0x4c5a, lsl #16
1005e3b90:      cmp w8, w9
1005e3b94:      b.ne    0x1005e3c34 <_js_array_grow+0x374>
1005e3b98:      mov x0, x19
1005e3b9c:      bl  0x1003db41c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1005e3ba0:      mov x1, x22
1005e3ba4:      cbz x0, 0x1005e3c34 <_js_array_grow+0x374>
1005e3ba8:      mov x19, x0
1005e3bac:      b   0x1005e3c7c <_js_array_grow+0x3bc>
1005e3bb0:      ldp w10, w9, [x19]
1005e3bb4:      cmp w10, w9
1005e3bb8:      b.ls    0x1005e3bd8 <_js_array_grow+0x318>
1005e3bbc:      cbz x8, 0x1005e3be8 <_js_array_grow+0x328>
1005e3bc0:      ldr w8, [x0, #0x4]
1005e3bc4:      lsl x9, x9, #3
1005e3bc8:      add x9, x9, #0x10
1005e3bcc:      cmp x9, x8
1005e3bd0:      b.ne    0x1005e3be8 <_js_array_grow+0x328>
1005e3bd4:      b   0x1005e3c7c <_js_array_grow+0x3bc>
1005e3bd8:      mov w8, #0xe100             ; =57600
1005e3bdc:      movk    w8, #0x5f5, lsl #16
1005e3be0:      cmp w10, w8
1005e3be4:      b.ls    0x1005e3c7c <_js_array_grow+0x3bc>
1005e3be8:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
1005e3bec:      add x8, x8, #0x710
1005e3bf0:      ldaprb  w8, [x8]
1005e3bf4:      cbz w8, 0x1005e3c24 <_js_array_grow+0x364>
1005e3bf8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1005e3bfc:      add x8, x8, #0x258
1005e3c00:      ldapr   x9, [x8]
1005e3c04:      cmp x9, x19
1005e3c08:      b.hi    0x1005e3c24 <_js_array_grow+0x364>
1005e3c0c:      ldapur  x8, [x8, #0x8]
1005e3c10:      cmp x8, x19
1005e3c14:      b.lo    0x1005e3c24 <_js_array_grow+0x364>
1005e3c18:      mov x0, x19
1005e3c1c:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1005e3c20:      tbnz    w0, #0x0, 0x1005e3c7c <_js_array_grow+0x3bc>
1005e3c24:      mov x0, x19
1005e3c28:      bl  0x10055733c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
1005e3c2c:      mov x1, x22
1005e3c30:      tbnz    w0, #0x0, 0x1005e3c7c <_js_array_grow+0x3bc>
1005e3c34:      mov x0, x1
1005e3c38:      ldp x29, x30, [sp, #0x80]
1005e3c3c:      ldp x20, x19, [sp, #0x70]
1005e3c40:      ldp x22, x21, [sp, #0x60]
1005e3c44:      ldp x24, x23, [sp, #0x50]
1005e3c48:      ldp x26, x25, [sp, #0x40]
1005e3c4c:      ldp x28, x27, [sp, #0x30]
1005e3c50:      add sp, sp, #0x90
1005e3c54:      b   0x10042fcac <_js_array_alloc>
1005e3c58:      ldr w8, [x19]
1005e3c5c:      mov w9, #0xe100             ; =57600
1005e3c60:      movk    w9, #0x5f5, lsl #16
1005e3c64:      orr w9, w9, #0x1
1005e3c68:      cmp w8, w9
1005e3c6c:      b.hs    0x1005e3a34 <_js_array_grow+0x174>
1005e3c70:      ldr w9, [x19, #0x4]
1005e3c74:      cmp w8, w9
1005e3c78:      b.hi    0x1005e3a34 <_js_array_grow+0x174>
1005e3c7c:      mov x0, x19
1005e3c80:      bl  0x100593ec0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
1005e3c84:      tst w0, #0x6
1005e3c88:      b.ne    0x1005e4190 <_js_array_grow+0x8d0>
1005e3c8c:      mov x0, x19
1005e3c90:      bl  0x100593ec0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
1005e3c94:      tbnz    w0, #0x0, 0x1005e4190 <_js_array_grow+0x8d0>
1005e3c98:      adrp    x23, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1005e3c9c:      add x23, x23, #0xfe0
1005e3ca0:      ldr x8, [x23]
1005e3ca4:      cmn x8, #0x1
1005e3ca8:      b.eq    0x1005e3cdc <_js_array_grow+0x41c>
1005e3cac:      mrs x9, TPIDRRO_EL0
1005e3cb0:      and x9, x9, #0xfffffffffffffff8
1005e3cb4:      ldr x8, [x9, x8, lsl #3]
1005e3cb8:      cbz x8, 0x1005e3cdc <_js_array_grow+0x41c>
1005e3cbc:      ldr x8, [x8, #0x19e8]
1005e3cc0:      cbz x8, 0x1005e3cdc <_js_array_grow+0x41c>
1005e3cc4:      ldr x9, [x8]
1005e3cc8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1005e3ccc:      cmp x9, x10
1005e3cd0:      b.hs    0x1005e4218 <_js_array_grow+0x958>
1005e3cd4:      ldr x20, [x8, #0x18]
1005e3cd8:      b   0x1005e3cec <_js_array_grow+0x42c>
1005e3cdc:      adrp    x0, 0x1010a3000 <_anon.69648fd02457b37c9b869298da3a2160.1393+0xd00>
1005e3ce0:      add x0, x0, #0x778
1005e3ce4:      bl  0x10013582c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1005e3ce8:      mov x20, x0
1005e3cec:      str x20, [sp]
1005e3cf0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1005e3cf4:      stp x19, x8, [sp, #0x20]
1005e3cf8:      mov w8, #0x1                ; =1
1005e3cfc:      str x8, [sp, #0x18]
1005e3d00:      add x0, sp, #0x18
1005e3d04:      bl  0x100556be8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1005e3d08:      str x0, [sp, #0x8]
1005e3d0c:      ldr w24, [x19, #0x4]
1005e3d10:      cmp w22, w24
1005e3d14:      b.ls    0x1005e3dd4 <_js_array_grow+0x514>
1005e3d18:      mov x21, x0
1005e3d1c:      lsl w8, w24, #1
1005e3d20:      cmp w22, w8
1005e3d24:      csel    w25, w22, w8, hi
1005e3d28:      ubfiz   x22, x25, #3, #32
1005e3d2c:      ldurb   w8, [x19, #-0x7]
1005e3d30:      tbnz    w8, #0x5, 0x1005e3ff0 <_js_array_grow+0x730>
1005e3d34:      ldr x8, [x23]
1005e3d38:      cmn x8, #0x1
1005e3d3c:      b.eq    0x1005e3fc8 <_js_array_grow+0x708>
1005e3d40:      mrs x9, TPIDRRO_EL0
1005e3d44:      and x9, x9, #0xfffffffffffffff8
1005e3d48:      ldr x0, [x9, x8, lsl #3]
1005e3d4c:      cbz x0, 0x1005e3fc8 <_js_array_grow+0x708>
1005e3d50:      lsr x1, x19, #20
1005e3d54:      ldr x8, [x0, #0x10]
1005e3d58:      ldrb    w9, [x8, #0x28]
1005e3d5c:      tbz w9, #0x0, 0x1005e3d7c <_js_array_grow+0x4bc>
1005e3d60:      ldr x9, [x8, #0x20]
1005e3d64:      cmp x9, x1
1005e3d68:      b.ne    0x1005e3d7c <_js_array_grow+0x4bc>
1005e3d6c:      ldp x9, x10, [x8]
1005e3d70:      cmp x9, x19
1005e3d74:      ccmp    x10, x19, #0x0, ls
1005e3d78:      b.hi    0x1005e3e68 <_js_array_grow+0x5a8>
1005e3d7c:      ldrb    w9, [x8, #0x58]
1005e3d80:      cbz w9, 0x1005e3da0 <_js_array_grow+0x4e0>
1005e3d84:      ldr x9, [x8, #0x50]
1005e3d88:      cmp x9, x1
1005e3d8c:      b.ne    0x1005e3da0 <_js_array_grow+0x4e0>
1005e3d90:      ldp x9, x10, [x8, #0x30]
1005e3d94:      cmp x9, x19
1005e3d98:      ccmp    x10, x19, #0x0, ls
1005e3d9c:      b.hi    0x1005e3e64 <_js_array_grow+0x5a4>
1005e3da0:      ldrb    w9, [x8, #0x88]
1005e3da4:      cbz w9, 0x1005e3e30 <_js_array_grow+0x570>
1005e3da8:      ldr x9, [x8, #0x80]
1005e3dac:      cmp x9, x1
1005e3db0:      b.ne    0x1005e3e30 <_js_array_grow+0x570>
1005e3db4:      ldr x9, [x8, #0x60]
1005e3db8:      cmp x9, x19
1005e3dbc:      b.hi    0x1005e3e30 <_js_array_grow+0x570>
1005e3dc0:      ldr x9, [x8, #0x68]
1005e3dc4:      cmp x9, x19
1005e3dc8:      b.ls    0x1005e3e30 <_js_array_grow+0x570>
1005e3dcc:      add x8, x8, #0x60
1005e3dd0:      b   0x1005e3e68 <_js_array_grow+0x5a8>
1005e3dd4:      ldr x8, [x23]
1005e3dd8:      cmn x8, #0x1
1005e3ddc:      b.eq    0x1005e4180 <_js_array_grow+0x8c0>
1005e3de0:      mrs x9, TPIDRRO_EL0
1005e3de4:      and x9, x9, #0xfffffffffffffff8
1005e3de8:      ldr x8, [x9, x8, lsl #3]
1005e3dec:      cbz x8, 0x1005e4180 <_js_array_grow+0x8c0>
1005e3df0:      ldr x8, [x8, #0x19e8]
1005e3df4:      cbz x8, 0x1005e4180 <_js_array_grow+0x8c0>
1005e3df8:      ldr x9, [x8]
1005e3dfc:      cbnz    x9, 0x1005e4174 <_js_array_grow+0x8b4>
1005e3e00:      ldr x9, [x8, #0x18]
1005e3e04:      cmp x20, x9
1005e3e08:      b.hi    0x1005e3e10 <_js_array_grow+0x550>
1005e3e0c:      str x20, [x8, #0x18]
1005e3e10:      str xzr, [x8]
1005e3e14:      b   0x1005e4190 <_js_array_grow+0x8d0>
1005e3e18:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005e3e1c:      lsr x1, x19, #20
1005e3e20:      ldr x8, [x0, #0x10]
1005e3e24:      ldrb    w9, [x8, #0x28]
1005e3e28:      tbnz    w9, #0x0, 0x1005e3964 <_js_array_grow+0xa4>
1005e3e2c:      b   0x1005e3980 <_js_array_grow+0xc0>
1005e3e30:      ldrb    w9, [x8, #0xb8]
1005e3e34:      cbz w9, 0x1005e3e74 <_js_array_grow+0x5b4>
1005e3e38:      ldr x9, [x8, #0xb0]
1005e3e3c:      cmp x9, x1
1005e3e40:      b.ne    0x1005e3e74 <_js_array_grow+0x5b4>
1005e3e44:      ldr x9, [x8, #0x90]
1005e3e48:      cmp x9, x19
1005e3e4c:      b.hi    0x1005e3e74 <_js_array_grow+0x5b4>
1005e3e50:      ldr x9, [x8, #0x98]
1005e3e54:      cmp x9, x19
1005e3e58:      b.ls    0x1005e3e74 <_js_array_grow+0x5b4>
1005e3e5c:      add x8, x8, #0x90
1005e3e60:      b   0x1005e3e68 <_js_array_grow+0x5a8>
1005e3e64:      add x8, x8, #0x30
1005e3e68:      ldrb    w8, [x8, #0x19]
1005e3e6c:      cmp w8, #0xff
1005e3e70:      b.ne    0x1005e3e80 <_js_array_grow+0x5c0>
1005e3e74:      mov x0, x19
1005e3e78:      bl  0x1009960b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1005e3e7c:      and w8, w0, #0xff
1005e3e80:      cmp w8, #0x1
1005e3e84:      b.ne    0x1005e3ff0 <_js_array_grow+0x730>
1005e3e88:      mov w8, #0x3ffe             ; =16382
1005e3e8c:      cmp w25, w8
1005e3e90:      b.hi    0x1005e3ff0 <_js_array_grow+0x730>
1005e3e94:      ldr x8, [x23]
1005e3e98:      cmn x8, #0x1
1005e3e9c:      b.eq    0x1005e3fe0 <_js_array_grow+0x720>
1005e3ea0:      mrs x9, TPIDRRO_EL0
1005e3ea4:      and x9, x9, #0xfffffffffffffff8
1005e3ea8:      ldr x0, [x9, x8, lsl #3]
1005e3eac:      cbz x0, 0x1005e3fe0 <_js_array_grow+0x720>
1005e3eb0:      ldr x8, [x0, #0x28]
1005e3eb4:      ldrb    w8, [x8]
1005e3eb8:      tbnz    w8, #0x0, 0x1005e3ff0 <_js_array_grow+0x730>
1005e3ebc:      ldr x8, [x23]
1005e3ec0:      cmn x8, #0x1
1005e3ec4:      b.eq    0x1005e41b4 <_js_array_grow+0x8f4>
1005e3ec8:      mrs x9, TPIDRRO_EL0
1005e3ecc:      and x9, x9, #0xfffffffffffffff8
1005e3ed0:      ldr x0, [x9, x8, lsl #3]
1005e3ed4:      cbz x0, 0x1005e41b4 <_js_array_grow+0x8f4>
1005e3ed8:      ldr x26, [x0, #0x8]
1005e3edc:      ldr x8, [x23]
1005e3ee0:      cmn x8, #0x1
1005e3ee4:      b.eq    0x1005e41c8 <_js_array_grow+0x908>
1005e3ee8:      mrs x9, TPIDRRO_EL0
1005e3eec:      and x9, x9, #0xfffffffffffffff8
1005e3ef0:      ldr x0, [x9, x8, lsl #3]
1005e3ef4:      cbz x0, 0x1005e41c8 <_js_array_grow+0x908>
1005e3ef8:      ldr x19, [x0]
1005e3efc:      ldr x8, [x26]
1005e3f00:      cbz x8, 0x1005e3f24 <_js_array_grow+0x664>
1005e3f04:      ldp x1, x0, [x19, #0x10]
1005e3f08:      cmp x0, x1
1005e3f0c:      b.hs    0x1005e4244 <_js_array_grow+0x984>
1005e3f10:      ldr x8, [x19, #0x8]
1005e3f14:      ldr x9, [x26, #0x8]
1005e3f18:      mov w10, #0x30              ; =48
1005e3f1c:      madd    x8, x0, x10, x8
1005e3f20:      str x9, [x8, #0x20]
1005e3f24:      add x27, x22, #0x10
1005e3f28:      ldr x1, [x19, #0x18]
1005e3f2c:      add x2, x22, #0x10
1005e3f30:      mov x0, x19
1005e3f34:      bl  0x10055536c <__RNvMs1_NtNtCs5gMwpk3Cs4e_13perry_runtime5arena5blockNtB5_5Arena15try_block_alloc>
1005e3f38:      cmp x0, #0x1
1005e3f3c:      b.ne    0x1005e3ff0 <_js_array_grow+0x730>
1005e3f40:      ldr x8, [x26]
1005e3f44:      cbz x8, 0x1005e3f74 <_js_array_grow+0x6b4>
1005e3f48:      ldp x8, x0, [x19, #0x10]
1005e3f4c:      cmp x0, x8
1005e3f50:      b.hs    0x1005e4250 <_js_array_grow+0x990>
1005e3f54:      ldr x8, [x19, #0x8]
1005e3f58:      mov w9, #0x30               ; =48
1005e3f5c:      madd    x8, x0, x9, x8
1005e3f60:      ldr x9, [x8, #0x10]
1005e3f64:      ldur    q0, [x8, #0x18]
1005e3f68:      str x9, [x26]
1005e3f6c:      ext.16b v0, v0, v0, #0x8
1005e3f70:      stur    q0, [x26, #0x8]
1005e3f74:      cbz x1, 0x1005e3ff0 <_js_array_grow+0x730>
1005e3f78:      mov w8, #0x1                ; =1
1005e3f7c:      strb    w8, [x1]
1005e3f80:      ldr x8, [x23]
1005e3f84:      cmn x8, #0x1
1005e3f88:      b.eq    0x1005e4208 <_js_array_grow+0x948>
1005e3f8c:      mrs x9, TPIDRRO_EL0
1005e3f90:      and x9, x9, #0xfffffffffffffff8
1005e3f94:      ldr x0, [x9, x8, lsl #3]
1005e3f98:      cbz x0, 0x1005e4208 <_js_array_grow+0x948>
1005e3f9c:      ldr x8, [x0, #0x30]
1005e3fa0:      ldrb    w8, [x8]
1005e3fa4:      orr w8, w8, #0x2
1005e3fa8:      strb    w8, [x1, #0x1]
1005e3fac:      mov x0, x1
1005e3fb0:      mov x19, x1
1005e3fb4:      bl  0x10058c858 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier19gc_note_black_birth>
1005e3fb8:      strh    wzr, [x19, #0x2]
1005e3fbc:      str w27, [x19, #0x4]
1005e3fc0:      add x19, x19, #0x8
1005e3fc4:      b   0x1005e4010 <_js_array_grow+0x750>
1005e3fc8:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005e3fcc:      lsr x1, x19, #20
1005e3fd0:      ldr x8, [x0, #0x10]
1005e3fd4:      ldrb    w9, [x8, #0x28]
1005e3fd8:      tbnz    w9, #0x0, 0x1005e3d60 <_js_array_grow+0x4a0>
1005e3fdc:      b   0x1005e3d7c <_js_array_grow+0x4bc>
1005e3fe0:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005e3fe4:      ldr x8, [x0, #0x28]
1005e3fe8:      ldrb    w8, [x8]
1005e3fec:      tbz w8, #0x0, 0x1005e3ebc <_js_array_grow+0x5fc>
1005e3ff0:      add x0, x22, #0x8
1005e3ff4:      mov w1, #0x8                ; =8
1005e3ff8:      mov w2, #0x1                ; =1
1005e3ffc:      bl  0x1007e8eec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena10allocators18arena_alloc_gc_old>
1005e4000:      mov x19, x0
1005e4004:      ldurb   w8, [x0, #-0x7]
1005e4008:      orr w8, w8, #0x20
1005e400c:      sturb   w8, [x0, #-0x7]
1005e4010:      ldr x8, [x23]
1005e4014:      cmn x8, #0x1
1005e4018:      b.eq    0x1005e407c <_js_array_grow+0x7bc>
1005e401c:      mrs x9, TPIDRRO_EL0
1005e4020:      and x9, x9, #0xfffffffffffffff8
1005e4024:      ldr x8, [x9, x8, lsl #3]
1005e4028:      cbz x8, 0x1005e407c <_js_array_grow+0x7bc>
1005e402c:      ldr x8, [x8, #0x19e8]
1005e4030:      cbz x8, 0x1005e407c <_js_array_grow+0x7bc>
1005e4034:      ldr x9, [x8]
1005e4038:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1005e403c:      cmp x9, x10
1005e4040:      b.hs    0x1005e4224 <_js_array_grow+0x964>
1005e4044:      add x10, x9, #0x1
1005e4048:      str x10, [x8]
1005e404c:      ldr x10, [x8, #0x18]
1005e4050:      cmp x21, x10
1005e4054:      b.hs    0x1005e4230 <_js_array_grow+0x970>
1005e4058:      ldr x10, [x8, #0x10]
1005e405c:      mov w11, #0x18              ; =24
1005e4060:      madd    x10, x21, x11, x10
1005e4064:      ldr x11, [x10]
1005e4068:      cmp x11, #0x1
1005e406c:      b.ne    0x1005e4234 <_js_array_grow+0x974>
1005e4070:      ldr x21, [x10, #0x8]
1005e4074:      str x9, [x8]
1005e4078:      b   0x1005e4090 <_js_array_grow+0x7d0>
1005e407c:      adrp    x0, 0x1010a3000 <_anon.69648fd02457b37c9b869298da3a2160.1393+0xd00>
1005e4080:      add x0, x0, #0x778
1005e4084:      add x1, sp, #0x8
1005e4088:      bl  0x100135650 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1005e408c:      mov x21, x0
1005e4090:      lsl x8, x24, #3
1005e4094:      add x22, x8, #0x8
1005e4098:      mov x0, x19
1005e409c:      mov x1, x21
1005e40a0:      mov x2, x22
1005e40a4:      bl  0x100cd8dac <_writev+0x100cd8dac>
1005e40a8:      str w25, [x19, #0x4]
1005e40ac:      sub x8, x25, x24
1005e40b0:      lsl x2, x8, #3
1005e40b4:      adrp    x1, 0x100dd1000 <_anon.32ca3690520b3140c3df72b88a347d65.557+0x2f2>
1005e40b8:      add x1, x1, #0x1b0
1005e40bc:      add x0, x19, x22
1005e40c0:      bl  0x100cd8dd0 <_writev+0x100cd8dd0>
1005e40c4:      ldurh   w8, [x21, #-0x6]
1005e40c8:      sturh   w8, [x19, #-0x6]
1005e40cc:      mov x0, x21
1005e40d0:      mov x1, x19
1005e40d4:      bl  0x10058961c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout15layout_transfer>
1005e40d8:      mov x0, x21
1005e40dc:      mov x1, x19
1005e40e0:      bl  0x10040006c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35transfer_array_named_property_owner>
1005e40e4:      mov x0, x21
1005e40e8:      mov x1, x19
1005e40ec:      bl  0x10036bbb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object16descriptor_state25transfer_descriptor_owner>
1005e40f0:      mov x0, x19
1005e40f4:      mov x1, x21
1005e40f8:      mov x2, x19
1005e40fc:      mov x3, x22
1005e4100:      bl  0x1009907b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier38relocate_copied_old_object_dirty_pages>
1005e4104:      tbnz    w0, #0x0, 0x1005e4110 <_js_array_grow+0x850>
1005e4108:      mov x0, x19
1005e410c:      bl  0x100590fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array15header_gc_slots34replay_array_growth_write_barriers>
1005e4110:      mov x0, x21
1005e4114:      bl  0x1005ac688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1005e4118:      cbz x0, 0x1005e41dc <_js_array_grow+0x91c>
1005e411c:      ldrb    w8, [x0]
1005e4120:      cmp w8, #0x1
1005e4124:      b.ne    0x1005e41dc <_js_array_grow+0x91c>
1005e4128:      ldrb    w8, [x0, #0x1]
1005e412c:      tbz w8, #0x1, 0x1005e41dc <_js_array_grow+0x91c>
1005e4130:      str x19, [x0, #0x8]
1005e4134:      orr w8, w8, #0x80
1005e4138:      strb    w8, [x0, #0x1]
1005e413c:      ldrh    w8, [x0, #0x2]
1005e4140:      orr w8, w8, #0xc000
1005e4144:      strh    w8, [x0, #0x2]
1005e4148:      ldr x8, [x23]
1005e414c:      cmn x8, #0x1
1005e4150:      b.eq    0x1005e4180 <_js_array_grow+0x8c0>
1005e4154:      mrs x9, TPIDRRO_EL0
1005e4158:      and x9, x9, #0xfffffffffffffff8
1005e415c:      ldr x8, [x9, x8, lsl #3]
1005e4160:      cbz x8, 0x1005e4180 <_js_array_grow+0x8c0>
1005e4164:      ldr x8, [x8, #0x19e8]
1005e4168:      cbz x8, 0x1005e4180 <_js_array_grow+0x8c0>
1005e416c:      ldr x9, [x8]
1005e4170:      cbz x9, 0x1005e3e00 <_js_array_grow+0x540>
1005e4174:      adrp    x0, 0x1010a3000 <_anon.69648fd02457b37c9b869298da3a2160.1393+0xd00>
1005e4178:      add x0, x0, #0xac8
1005e417c:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1005e4180:      adrp    x0, 0x1010a3000 <_anon.69648fd02457b37c9b869298da3a2160.1393+0xd00>
1005e4184:      add x0, x0, #0x778
1005e4188:      mov x1, sp
1005e418c:      bl  0x100135c08 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1005e4190:      mov x0, x19
1005e4194:      ldp x29, x30, [sp, #0x80]
1005e4198:      ldp x20, x19, [sp, #0x70]
1005e419c:      ldp x22, x21, [sp, #0x60]
1005e41a0:      ldp x24, x23, [sp, #0x50]
1005e41a4:      ldp x26, x25, [sp, #0x40]
1005e41a8:      ldp x28, x27, [sp, #0x30]
1005e41ac:      add sp, sp, #0x90
1005e41b0:      ret
1005e41b4:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005e41b8:      ldr x26, [x0, #0x8]
1005e41bc:      ldr x8, [x23]
1005e41c0:      cmn x8, #0x1
1005e41c4:      b.ne    0x1005e3ee8 <_js_array_grow+0x628>
1005e41c8:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005e41cc:      ldr x19, [x0]
1005e41d0:      ldr x8, [x26]
1005e41d4:      cbnz    x8, 0x1005e3f04 <_js_array_grow+0x644>
1005e41d8:      b   0x1005e3f24 <_js_array_grow+0x664>
1005e41dc:      str x21, [sp, #0x10]
1005e41e0:      add x8, sp, #0x10
1005e41e4:      adrp    x9, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1005e41e8:      add x9, x9, #0x288
1005e41ec:      stp x8, x9, [sp, #0x18]
1005e41f0:      adrp    x0, 0x100dd8000 <_anon.fb372044f547dc630365d8bf95ffa96d.972+0x970>
1005e41f4:      add x0, x0, #0x610
1005e41f8:      adrp    x2, 0x1010a6000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7closure5alloc27SINGLETON_CAPTURED_CLOSURES+0x19a8>
1005e41fc:      add x2, x2, #0x7f8
1005e4200:      add x1, sp, #0x18
1005e4204:      bl  0x100c8d47c <__RNvNtCsjgY6bXVaRmE_4core9panicking9panic_fmt>
1005e4208:      mov x19, x1
1005e420c:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005e4210:      mov x1, x19
1005e4214:      b   0x1005e3f9c <_js_array_grow+0x6dc>
1005e4218:      adrp    x0, 0x1010a3000 <_anon.69648fd02457b37c9b869298da3a2160.1393+0xd00>
1005e421c:      add x0, x0, #0x840
1005e4220:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1005e4224:      adrp    x0, 0x1010a3000 <_anon.69648fd02457b37c9b869298da3a2160.1393+0xd00>
1005e4228:      add x0, x0, #0x7e0
1005e422c:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1005e4230:      bl  0x100cac31c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1005e4234:      adrp    x0, 0x100dd5000 <_anon.69648fd02457b37c9b869298da3a2160.1367+0xb71>
1005e4238:      add x0, x0, #0xc3d
1005e423c:      mov w1, #0xb                ; =11
1005e4240:      bl  0x100cac2e4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1005e4244:      adrp    x2, 0x1010a4000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8schedule17SAFEPOINT_COUNTER+0x8>
1005e4248:      add x2, x2, #0x88
1005e424c:      bl  0x100c8d30c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1005e4250:      adrp    x2, 0x1010a4000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8schedule17SAFEPOINT_COUNTER+0x8>
1005e4254:      add x2, x2, #0xa0
1005e4258:      mov x1, x8
1005e425c:      bl  0x100c8d30c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
        ...
