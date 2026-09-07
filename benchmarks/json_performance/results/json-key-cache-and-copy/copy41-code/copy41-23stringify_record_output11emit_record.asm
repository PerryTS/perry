/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/copy41-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c3980 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record>:
1004c3980:      stp x28, x27, [sp, #-0x60]!
1004c3984:      stp x26, x25, [sp, #0x10]
1004c3988:      stp x24, x23, [sp, #0x20]
1004c398c:      stp x22, x21, [sp, #0x30]
1004c3990:      stp x20, x19, [sp, #0x40]
1004c3994:      stp x29, x30, [sp, #0x50]
1004c3998:      add x29, sp, #0x50
1004c399c:      sub sp, sp, #0x5d0
1004c39a0:      ldr xzr, [sp]
1004c39a4:      str x1, [sp, #0x38]
1004c39a8:      mov x22, x0
1004c39ac:      movi.2d v0, #0000000000000000
1004c39b0:      str d0, [sp, #0x40]
1004c39b4:      str wzr, [sp, #0x48]
1004c39b8:      str d0, [sp, #0x68]
1004c39bc:      str wzr, [sp, #0x70]
1004c39c0:      str d0, [sp, #0x90]
1004c39c4:      str wzr, [sp, #0x98]
1004c39c8:      str d0, [sp, #0xb8]
1004c39cc:      str wzr, [sp, #0xc0]
1004c39d0:      str d0, [sp, #0xe0]
1004c39d4:      str wzr, [sp, #0xe8]
1004c39d8:      str d0, [sp, #0x108]
1004c39dc:      str wzr, [sp, #0x110]
1004c39e0:      str d0, [sp, #0x130]
1004c39e4:      str wzr, [sp, #0x138]
1004c39e8:      str d0, [sp, #0x158]
1004c39ec:      str wzr, [sp, #0x160]
1004c39f0:      str d0, [sp, #0x180]
1004c39f4:      str wzr, [sp, #0x188]
1004c39f8:      str d0, [sp, #0x1a8]
1004c39fc:      str wzr, [sp, #0x1b0]
1004c3a00:      str d0, [sp, #0x1d0]
1004c3a04:      str wzr, [sp, #0x1d8]
1004c3a08:      str d0, [sp, #0x1f8]
1004c3a0c:      str wzr, [sp, #0x200]
1004c3a10:      str d0, [sp, #0x220]
1004c3a14:      str wzr, [sp, #0x228]
1004c3a18:      str d0, [sp, #0x248]
1004c3a1c:      str wzr, [sp, #0x250]
1004c3a20:      str d0, [sp, #0x270]
1004c3a24:      str wzr, [sp, #0x278]
1004c3a28:      str d0, [sp, #0x298]
1004c3a2c:      str wzr, [sp, #0x2a0]
1004c3a30:      str d0, [sp, #0x2c0]
1004c3a34:      str wzr, [sp, #0x2c8]
1004c3a38:      str d0, [sp, #0x2e8]
1004c3a3c:      str wzr, [sp, #0x2f0]
1004c3a40:      str d0, [sp, #0x310]
1004c3a44:      str wzr, [sp, #0x318]
1004c3a48:      str d0, [sp, #0x338]
1004c3a4c:      str wzr, [sp, #0x340]
1004c3a50:      str d0, [sp, #0x360]
1004c3a54:      str wzr, [sp, #0x368]
1004c3a58:      str d0, [sp, #0x388]
1004c3a5c:      str wzr, [sp, #0x390]
1004c3a60:      str d0, [sp, #0x3b0]
1004c3a64:      str wzr, [sp, #0x3b8]
1004c3a68:      str d0, [sp, #0x3d8]
1004c3a6c:      str wzr, [sp, #0x3e0]
1004c3a70:      str d0, [sp, #0x400]
1004c3a74:      str wzr, [sp, #0x408]
1004c3a78:      str d0, [sp, #0x428]
1004c3a7c:      str wzr, [sp, #0x430]
1004c3a80:      str d0, [sp, #0x450]
1004c3a84:      str wzr, [sp, #0x458]
1004c3a88:      str d0, [sp, #0x478]
1004c3a8c:      str wzr, [sp, #0x480]
1004c3a90:      str d0, [sp, #0x4a0]
1004c3a94:      str wzr, [sp, #0x4a8]
1004c3a98:      str d0, [sp, #0x4c8]
1004c3a9c:      str wzr, [sp, #0x4d0]
1004c3aa0:      str d0, [sp, #0x4f0]
1004c3aa4:      str wzr, [sp, #0x4f8]
1004c3aa8:      str d0, [sp, #0x518]
1004c3aac:      mov w8, #-0x40000000        ; =-1073741824
1004c3ab0:      ldr w20, [x0, #0x4]
1004c3ab4:      cmp w20, w8
1004c3ab8:      csel    w19, w20, w8, lt
1004c3abc:      str wzr, [sp, #0x520]
1004c3ac0:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3ac4:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3ac8:      ldr w21, [x9, #0x634]
1004c3acc:      cmp w21, #0x300
1004c3ad0:      b.hs    0x1004c3b64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c3ad4:      ldr x8, [x8, #0x48]
1004c3ad8:      cmn x8, #0x1
1004c3adc:      b.eq    0x1004c3b54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c3ae0:      mrs x9, TPIDRRO_EL0
1004c3ae4:      and x9, x9, #0xfffffffffffffff8
1004c3ae8:      ldr x0, [x9, x8, lsl #3]
1004c3aec:      cbz x0, 0x1004c3b54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c3af0:      add x8, x0, x21, lsl #3
1004c3af4:      ldr x0, [x8, #0x1e8]
1004c3af8:      cbz x0, 0x1004c3b64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c3afc:      ldr x0, [x0]
1004c3b00:      cbz x0, 0x1004c3b78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1f8>
1004c3b04:      mov w8, #-0x40000001        ; =-1073741825
1004c3b08:      cmp w20, w8
1004c3b0c:      b.gt    0x1004c3b88 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3b10:      ldr x10, [x0, #0x5198]
1004c3b14:      and w8, w19, #0x3fffffff
1004c3b18:      lsr x9, x8, #15
1004c3b1c:      cmp x9, x10
1004c3b20:      b.hs    0x1004c3b88 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3b24:      ldr x10, [x0, #0x5190]
1004c3b28:      ldr x9, [x10, x9, lsl #3]
1004c3b2c:      cbz x9, 0x1004c3b88 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3b30:      ubfx    x10, x8, #5, #10
1004c3b34:      ldr x9, [x9, x10, lsl #3]
1004c3b38:      cbz x9, 0x1004c3b88 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3b3c:      and x8, x8, #0x1f
1004c3b40:      add x8, x9, x8, lsl #5
1004c3b44:      ldrb    w9, [x8, #0x1c]
1004c3b48:      tbz w9, #0x0, 0x1004c3b88 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3b4c:      ldr x8, [x8]
1004c3b50:      b   0x1004c3b8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x20c>
1004c3b54:      bl  0x100adc030 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3b58:      add x8, x0, x21, lsl #3
1004c3b5c:      ldr x0, [x8, #0x1e8]
1004c3b60:      cbnz    x0, 0x1004c3afc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x17c>
1004c3b64:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
1004c3b68:      add x0, x0, #0x360
1004c3b6c:      bl  0x100ad949c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3b70:      ldr x0, [x0]
1004c3b74:      cbnz    x0, 0x1004c3b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x184>
1004c3b78:      bl  0x100adb16c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c3b7c:      mov w8, #-0x40000001        ; =-1073741825
1004c3b80:      cmp w20, w8
1004c3b84:      b.le    0x1004c3b10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x190>
1004c3b88:      mov x8, #0x0                ; =0
1004c3b8c:      mov x23, #0x0               ; =0
1004c3b90:      mov x19, #0x0               ; =0
1004c3b94:      add x9, x8, #0x8
1004c3b98:      sub x28, x29, #0x80
1004c3b9c:      add x8, x22, #0x10
1004c3ba0:      stp x8, x9, [sp, #0x28]
1004c3ba4:      add x8, sp, #0x2c0
1004c3ba8:      add x8, x8, #0x8
1004c3bac:      stp x22, x8, [sp, #0x8]
1004c3bb0:      mov w22, #0x2               ; =2
1004c3bb4:      mov w25, #0x28              ; =40
1004c3bb8:      mov w26, #0x2               ; =2
1004c3bbc:      ldr x8, [sp, #0x30]
1004c3bc0:      ldr x1, [x8, x23, lsl #3]
1004c3bc4:      sub x0, x29, #0x80
1004c3bc8:      bl  0x1004be070 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece>
1004c3bcc:      ldur    w8, [x29, #-0x80]
1004c3bd0:      cmn w8, #0x1
1004c3bd4:      b.eq    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bd8:      ldp w10, w9, [x29, #-0x7c]
1004c3bdc:      ldur    w11, [x29, #-0x74]
1004c3be0:      ldur    q0, [x28, #0x10]
1004c3be4:      stur    q0, [x29, #-0xe0]
1004c3be8:      ldur    x12, [x28, #0x20]
1004c3bec:      stur    x12, [x29, #-0xd0]
1004c3bf0:      add x13, sp, #0x40
1004c3bf4:      madd    x13, x23, x25, x13
1004c3bf8:      stp w8, w10, [x13]
1004c3bfc:      stp w9, w11, [x13, #0x8]
1004c3c00:      str q0, [x13, #0x10]
1004c3c04:      str x12, [x13, #0x20]
1004c3c08:      cbz w8, 0x1004c3c1c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x29c>
1004c3c0c:      cmp w8, #0x1
1004c3c10:      csel    w24, w11, w10, eq
1004c3c14:      csel    w27, w9, w10, eq
1004c3c18:      b   0x1004c3c24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x2a4>
1004c3c1c:      add w27, w10, #0x2
1004c3c20:      add w24, w9, #0x2
1004c3c24:      ldr x8, [sp, #0x28]
1004c3c28:      ldr x21, [x8, x23, lsl #3]
1004c3c2c:      sub x0, x29, #0xc8
1004c3c30:      mov x1, x21
1004c3c34:      bl  0x1004bd730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c3c38:      ldur    w8, [x29, #-0xc8]
1004c3c3c:      cmn w8, #0x1
1004c3c40:      b.eq    0x1004c3c9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x31c>
1004c3c44:      ldp w20, w21, [x29, #-0xc4]
1004c3c48:      ldur    w9, [x29, #-0xbc]
1004c3c4c:      add x10, sp, #0x180
1004c3c50:      madd    x10, x23, x25, x10
1004c3c54:      stp w8, w20, [x10]
1004c3c58:      stp w21, w9, [x10, #0x8]
1004c3c5c:      sub x11, x29, #0xc8
1004c3c60:      ldur    q0, [x11, #0x10]
1004c3c64:      str q0, [x10, #0x10]
1004c3c68:      ldur    x11, [x11, #0x20]
1004c3c6c:      str x11, [x10, #0x20]
1004c3c70:      cmp w8, #0x2
1004c3c74:      b.eq    0x1004c3e30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4b0>
1004c3c78:      cmp w8, #0x1
1004c3c7c:      b.ne    0x1004c3e4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4cc>
1004c3c80:      mov x20, x9
1004c3c84:      cmp x23, #0x0
1004c3c88:      mov w8, #0x1                ; =1
1004c3c8c:      cinc    w8, w8, ne
1004c3c90:      adds    w9, w27, w22
1004c3c94:      b.lo    0x1004c3e94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3c98:      b   0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3c9c:      mov w8, #0x7ffd             ; =32765
1004c3ca0:      cmp x8, x21, lsr #48
1004c3ca4:      b.ne    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ca8:      and x21, x21, #0xffffffffffff
1004c3cac:      mov x0, x21
1004c3cb0:      bl  0x100535f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004c3cb4:      cbz x0, 0x1004c3fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3cb8:      ldrb    w8, [x0]
1004c3cbc:      cmp w8, #0x1
1004c3cc0:      b.ne    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3cc4:      ldrsb   w8, [x0, #0x1]
1004c3cc8:      tbnz    w8, #0x1f, 0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ccc:      ldrh    w8, [x0, #0x2]
1004c3cd0:      tbnz    w8, #0xa, 0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3cd4:      ldr w8, [x0, #0x4]
1004c3cd8:      cmp w8, #0x10
1004c3cdc:      b.lo    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ce0:      mov x20, x19
1004c3ce4:      ldr w19, [x21]
1004c3ce8:      cmp w19, #0x10
1004c3cec:      b.hi    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3cf0:      ldr w9, [x21, #0x4]
1004c3cf4:      cmp w19, w9
1004c3cf8:      b.hi    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3cfc:      str w27, [sp, #0x20]
1004c3d00:      lsl x27, x19, #3
1004c3d04:      add x9, x27, #0x10
1004c3d08:      cmp x9, x8
1004c3d0c:      b.hi    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d10:      mov x0, x21
1004c3d14:      bl  0x1004fb7ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array6header35array_has_named_properties_resolved>
1004c3d18:      tbnz    w0, #0x0, 0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d1c:      mov x0, x21
1004c3d20:      bl  0x100567648 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object15prototype_chain23object_static_prototype>
1004c3d24:      cmp x0, #0x1
1004c3d28:      b.eq    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d2c:      str w24, [sp, #0x1c]
1004c3d30:      add x24, x20, x19
1004c3d34:      cmp x24, #0x10
1004c3d38:      b.hi    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d3c:      add x8, sp, #0x180
1004c3d40:      madd    x8, x23, x25, x8
1004c3d44:      mov w9, #-0x1               ; =-1
1004c3d48:      str w9, [x8]
1004c3d4c:      stp x20, x19, [x8, #0x8]
1004c3d50:      cbz w19, 0x1004c3e70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4f0>
1004c3d54:      str w22, [sp, #0x4]
1004c3d58:      mov w9, #0x28               ; =40
1004c3d5c:      mov x25, #0x0               ; =0
1004c3d60:      add x19, x21, #0x8
1004c3d64:      ldr x8, [sp, #0x10]
1004c3d68:      madd    x22, x20, x9, x8
1004c3d6c:      mov w20, #0x2               ; =2
1004c3d70:      mov w21, #0x2               ; =2
1004c3d74:      ldr x1, [x19, x25]
1004c3d78:      sub x0, x29, #0x80
1004c3d7c:      bl  0x1004bd730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c3d80:      ldur    w8, [x29, #-0x80]
1004c3d84:      cmn w8, #0x1
1004c3d88:      b.eq    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d8c:      ldp w10, w9, [x29, #-0x7c]
1004c3d90:      ldur    w11, [x29, #-0x74]
1004c3d94:      ldur    q0, [x28, #0x10]
1004c3d98:      stur    q0, [x29, #-0xa0]
1004c3d9c:      ldur    x12, [x28, #0x20]
1004c3da0:      stur    x12, [x29, #-0x90]
1004c3da4:      stp w8, w10, [x22, #-0x8]
1004c3da8:      stp w9, w11, [x22]
1004c3dac:      stur    q0, [x22, #0x8]
1004c3db0:      str x12, [x22, #0x18]
1004c3db4:      cbz w8, 0x1004c3dd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x458>
1004c3db8:      cmp w8, #0x1
1004c3dbc:      csel    w8, w11, w10, eq
1004c3dc0:      csel    w10, w9, w10, eq
1004c3dc4:      cmp x25, #0x0
1004c3dc8:      cset    w9, ne
1004c3dcc:      adds    w10, w10, w21
1004c3dd0:      b.lo    0x1004c3df0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x470>
1004c3dd4:      b   0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3dd8:      add w10, w10, #0x2
1004c3ddc:      add w8, w9, #0x2
1004c3de0:      cmp x25, #0x0
1004c3de4:      cset    w9, ne
1004c3de8:      adds    w10, w10, w21
1004c3dec:      b.hs    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3df0:      adds    w21, w10, w9
1004c3df4:      b.hs    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3df8:      mov x0, #0x0                ; =0
1004c3dfc:      adds    w8, w8, w20
1004c3e00:      cset    w10, hs
1004c3e04:      adds    w20, w8, w9
1004c3e08:      cset    w8, hs
1004c3e0c:      tbnz    w10, #0x0, 0x1004c3fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3e10:      tbnz    w8, #0x0, 0x1004c3fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3e14:      add x25, x25, #0x8
1004c3e18:      add x22, x22, #0x28
1004c3e1c:      cmp x27, x25
1004c3e20:      b.ne    0x1004c3d74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x3f4>
1004c3e24:      mov x19, x24
1004c3e28:      ldr w22, [sp, #0x4]
1004c3e2c:      b   0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4fc>
1004c3e30:      mov x21, x20
1004c3e34:      cmp x23, #0x0
1004c3e38:      mov w8, #0x1                ; =1
1004c3e3c:      cinc    w8, w8, ne
1004c3e40:      adds    w9, w27, w22
1004c3e44:      b.lo    0x1004c3e94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3e48:      b   0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3e4c:      add w8, w20, #0x2
1004c3e50:      add w20, w21, #0x2
1004c3e54:      mov x21, x8
1004c3e58:      cmp x23, #0x0
1004c3e5c:      mov w8, #0x1                ; =1
1004c3e60:      cinc    w8, w8, ne
1004c3e64:      adds    w9, w27, w22
1004c3e68:      b.lo    0x1004c3e94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3e6c:      b   0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3e70:      mov w20, #0x2               ; =2
1004c3e74:      mov w21, #0x2               ; =2
1004c3e78:      mov x19, x24
1004c3e7c:      ldp w24, w27, [sp, #0x1c]
1004c3e80:      cmp x23, #0x0
1004c3e84:      mov w8, #0x1                ; =1
1004c3e88:      cinc    w8, w8, ne
1004c3e8c:      adds    w9, w27, w22
1004c3e90:      b.hs    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3e94:      adds    w9, w21, w9
1004c3e98:      b.hs    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3e9c:      adds    w11, w9, w8
1004c3ea0:      b.hs    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ea4:      adds    w9, w24, w26
1004c3ea8:      b.hs    0x1004c3fb8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3eac:      mov x0, #0x0                ; =0
1004c3eb0:      adds    w9, w20, w9
1004c3eb4:      cset    w10, hs
1004c3eb8:      adds    w26, w9, w8
1004c3ebc:      cset    w8, hs
1004c3ec0:      tbnz    w10, #0x0, 0x1004c3fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3ec4:      tbnz    w8, #0x0, 0x1004c3fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3ec8:      add x23, x23, #0x1
1004c3ecc:      ldr x8, [sp, #0x38]
1004c3ed0:      cmp x23, x8
1004c3ed4:      mov x22, x11
1004c3ed8:      mov w25, #0x28              ; =40
1004c3edc:      b.ne    0x1004c3bbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x23c>
1004c3ee0:      bl  0x10026a3d0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c3ee4:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
1004c3ee8:      mov w8, #0x1                ; =1
1004c3eec:      ldr x19, [sp, #0x8]
1004c3ef0:      stp x19, x9, [x29, #-0x78]
1004c3ef4:      stp x0, x8, [x29, #-0x88]
1004c3ef8:      sub x0, x29, #0x80
1004c3efc:      bl  0x10026a4d8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c3f00:      mov x23, x0
1004c3f04:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c3f08:      add x0, x0, #0xbf0
1004c3f0c:      ldr x8, [x0]
1004c3f10:      blr x8
1004c3f14:      strb    wzr, [x0]
1004c3f18:      mov x0, x19
1004c3f1c:      bl  0x1004c385c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys>
1004c3f20:      tbz w0, #0x0, 0x1004c3fb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x630>
1004c3f24:      mov x0, x22
1004c3f28:      bl  0x10032590c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004c3f2c:      mov x21, x1
1004c3f30:      stp w26, w22, [x0]
1004c3f34:      stp wzr, wzr, [x0, #0xc]
1004c3f38:      str w22, [x0, #0x8]
1004c3f3c:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3f40:      ldr x8, [x24, #0x48]
1004c3f44:      cmn x8, #0x1
1004c3f48:      str x0, [sp, #0x20]
1004c3f4c:      b.eq    0x1004c3fdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3f50:      mrs x9, TPIDRRO_EL0
1004c3f54:      and x9, x9, #0xfffffffffffffff8
1004c3f58:      ldr x8, [x9, x8, lsl #3]
1004c3f5c:      cbz x8, 0x1004c3fdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3f60:      ldr x8, [x8, #0x19e8]
1004c3f64:      cbz x8, 0x1004c3fdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3f68:      ldr x9, [x8]
1004c3f6c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c3f70:      cmp x9, x10
1004c3f74:      b.hs    0x1004c4290 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x910>
1004c3f78:      add x10, x9, #0x1
1004c3f7c:      str x10, [x8]
1004c3f80:      ldr x10, [x8, #0x18]
1004c3f84:      cmp x23, x10
1004c3f88:      b.hs    0x1004c429c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1004c3f8c:      ldr x10, [x8, #0x10]
1004c3f90:      mov w11, #0x18              ; =24
1004c3f94:      madd    x10, x23, x11, x10
1004c3f98:      ldr x11, [x10]
1004c3f9c:      cmp x11, #0x1
1004c3fa0:      b.ne    0x1004c42a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x920>
1004c3fa4:      ldr x0, [x10, #0x8]
1004c3fa8:      str x9, [x8]
1004c3fac:      b   0x1004c3fe4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x664>
1004c3fb0:      sub x0, x29, #0x88
1004c3fb4:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3fb8:      mov x0, #0x0                ; =0
1004c3fbc:      add sp, sp, #0x5d0
1004c3fc0:      ldp x29, x30, [sp, #0x50]
1004c3fc4:      ldp x20, x19, [sp, #0x40]
1004c3fc8:      ldp x22, x21, [sp, #0x30]
1004c3fcc:      ldp x24, x23, [sp, #0x20]
1004c3fd0:      ldp x26, x25, [sp, #0x10]
1004c3fd4:      ldp x28, x27, [sp], #0x60
1004c3fd8:      ret
1004c3fdc:      mov x0, x23
1004c3fe0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c3fe4:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3fe8:      ldr w20, [x0, #0x4]
1004c3fec:      mov w8, #-0x40000000        ; =-1073741824
1004c3ff0:      cmp w20, w8
1004c3ff4:      csel    w19, w20, w8, lt
1004c3ff8:      ldr w22, [x9, #0x634]
1004c3ffc:      cmp w22, #0x300
1004c4000:      b.hs    0x1004c40a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c4004:      ldr x8, [x24, #0x48]
1004c4008:      cmn x8, #0x1
1004c400c:      b.eq    0x1004c4084 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c4010:      mrs x9, TPIDRRO_EL0
1004c4014:      and x9, x9, #0xfffffffffffffff8
1004c4018:      ldr x8, [x9, x8, lsl #3]
1004c401c:      cbz x8, 0x1004c4084 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c4020:      add x8, x8, x22, lsl #3
1004c4024:      ldr x8, [x8, #0x1e8]
1004c4028:      cbz x8, 0x1004c40a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c402c:      ldr x8, [x8]
1004c4030:      cbz x8, 0x1004c40c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x744>
1004c4034:      mov w9, #-0x40000001        ; =-1073741825
1004c4038:      cmp w20, w9
1004c403c:      b.gt    0x1004c40e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c4040:      ldr x11, [x8, #0x5198]
1004c4044:      and w9, w19, #0x3fffffff
1004c4048:      lsr x10, x9, #15
1004c404c:      cmp x10, x11
1004c4050:      b.hs    0x1004c40e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c4054:      ldr x8, [x8, #0x5190]
1004c4058:      ldr x8, [x8, x10, lsl #3]
1004c405c:      cbz x8, 0x1004c40e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c4060:      ubfx    x10, x9, #5, #10
1004c4064:      ldr x8, [x8, x10, lsl #3]
1004c4068:      cbz x8, 0x1004c40e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c406c:      and x9, x9, #0x1f
1004c4070:      add x8, x8, x9, lsl #5
1004c4074:      ldrb    w9, [x8, #0x1c]
1004c4078:      tbz w9, #0x0, 0x1004c40e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c407c:      ldr x8, [x8]
1004c4080:      b   0x1004c40e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c4084:      mov x23, x0
1004c4088:      bl  0x100adc030 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c408c:      mov x8, x0
1004c4090:      mov x0, x23
1004c4094:      add x8, x8, x22, lsl #3
1004c4098:      ldr x8, [x8, #0x1e8]
1004c409c:      cbnz    x8, 0x1004c402c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6ac>
1004c40a0:      adrp    x8, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
1004c40a4:      add x8, x8, #0x360
1004c40a8:      mov x22, x0
1004c40ac:      mov x0, x8
1004c40b0:      bl  0x100ad949c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c40b4:      mov x8, x0
1004c40b8:      mov x0, x22
1004c40bc:      ldr x8, [x8]
1004c40c0:      cbnz    x8, 0x1004c4034 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6b4>
1004c40c4:      mov x22, x0
1004c40c8:      bl  0x100adb16c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c40cc:      mov x8, x0
1004c40d0:      mov x0, x22
1004c40d4:      mov w9, #-0x40000001        ; =-1073741825
1004c40d8:      cmp w20, w9
1004c40dc:      b.le    0x1004c4040 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6c0>
1004c40e0:      mov x8, #0x0                ; =0
1004c40e4:      mov x19, #0x0               ; =0
1004c40e8:      mov w9, #0x7b               ; =123
1004c40ec:      strb    w9, [x21]
1004c40f0:      add x25, x8, #0x8
1004c40f4:      add x24, x0, #0x10
1004c40f8:      add x8, sp, #0x2c0
1004c40fc:      add x8, x8, #0x28
1004c4100:      stp x8, x25, [sp, #0x28]
1004c4104:      mov w22, #0x1               ; =1
1004c4108:      add x27, sp, #0x40
1004c410c:      mov w28, #0x3a              ; =58
1004c4110:      mov w20, #0x2c              ; =44
1004c4114:      b   0x1004c413c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c4118:      mov w8, #0x5d               ; =93
1004c411c:      strb    w8, [x21, x25]
1004c4120:      add x22, x25, #0x1
1004c4124:      ldp x25, x8, [sp, #0x30]
1004c4128:      add x27, sp, #0x40
1004c412c:      mov w28, #0x3a              ; =58
1004c4130:      add x19, x19, #0x1
1004c4134:      cmp x19, x8
1004c4138:      b.eq    0x1004c4268 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c413c:      cbz x19, 0x1004c4148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7c8>
1004c4140:      strb    w20, [x21, x22]
1004c4144:      add x22, x22, #0x1
1004c4148:      add x8, x19, x19, lsl #2
1004c414c:      lsl x23, x8, #3
1004c4150:      ldr x1, [x25, x19, lsl #3]
1004c4154:      add x0, x27, x23
1004c4158:      add x2, x21, x22
1004c415c:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c4160:      add x8, x0, x22
1004c4164:      strb    w28, [x21, x8]
1004c4168:      add x26, x8, #0x1
1004c416c:      ldr x1, [x24, x19, lsl #3]
1004c4170:      add x9, sp, #0x180
1004c4174:      add x0, x9, x23
1004c4178:      ldr w9, [x0]
1004c417c:      cmn w9, #0x1
1004c4180:      b.eq    0x1004c41a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x824>
1004c4184:      add x2, x21, x26
1004c4188:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c418c:      add x22, x0, x26
1004c4190:      add x19, x19, #0x1
1004c4194:      ldr x8, [sp, #0x38]
1004c4198:      cmp x19, x8
1004c419c:      b.ne    0x1004c413c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c41a0:      b   0x1004c4268 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c41a4:      ldp x23, x22, [x0, #0x8]
1004c41a8:      add x25, x8, #0x2
1004c41ac:      mov w8, #0x5b               ; =91
1004c41b0:      strb    w8, [x21, x26]
1004c41b4:      cbz x22, 0x1004c4118 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c41b8:      cmp x23, #0xf
1004c41bc:      b.hi    0x1004c42b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x930>
1004c41c0:      and x26, x1, #0xffffffffffff
1004c41c4:      add x8, sp, #0x2c0
1004c41c8:      mov w9, #0x28               ; =40
1004c41cc:      madd    x8, x23, x9, x8
1004c41d0:      ldp q0, q1, [x8]
1004c41d4:      stp q0, q1, [x29, #-0x80]
1004c41d8:      ldr x8, [x8, #0x20]
1004c41dc:      stur    x8, [x29, #-0x60]
1004c41e0:      ldr x1, [x26, #0x8]
1004c41e4:      sub x0, x29, #0x80
1004c41e8:      add x2, x21, x25
1004c41ec:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c41f0:      add x25, x0, x25
1004c41f4:      subs    x27, x22, #0x1
1004c41f8:      b.eq    0x1004c4118 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c41fc:      mov w9, #0x28               ; =40
1004c4200:      add x28, x26, #0x10
1004c4204:      cmp x23, #0x10
1004c4208:      mov w8, #0x10               ; =16
1004c420c:      csel    x8, x23, x8, lo
1004c4210:      sub x26, x8, #0xf
1004c4214:      add x22, x23, #0x1
1004c4218:      ldr x8, [sp, #0x28]
1004c421c:      madd    x23, x23, x9, x8
1004c4220:      strb    w20, [x21, x25]
1004c4224:      cbz x26, 0x1004c42b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x934>
1004c4228:      add x25, x25, #0x1
1004c422c:      ldp q0, q1, [x23]
1004c4230:      stp q0, q1, [x29, #-0x80]
1004c4234:      ldr x8, [x23, #0x20]
1004c4238:      stur    x8, [x29, #-0x60]
1004c423c:      ldr x1, [x28], #0x8
1004c4240:      sub x0, x29, #0x80
1004c4244:      add x2, x21, x25
1004c4248:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c424c:      add x25, x0, x25
1004c4250:      add x26, x26, #0x1
1004c4254:      add x22, x22, #0x1
1004c4258:      add x23, x23, #0x28
1004c425c:      subs    x27, x27, #0x1
1004c4260:      b.ne    0x1004c4220 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8a0>
1004c4264:      b   0x1004c4118 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c4268:      mov w8, #0x7d               ; =125
1004c426c:      strb    w8, [x21, x22]
1004c4270:      mov x19, #0x7fff000000000000 ; =9223090561878065152
1004c4274:      ldr x8, [sp, #0x20]
1004c4278:      bfxil   x19, x8, #0, #48
1004c427c:      sub x0, x29, #0x88
1004c4280:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c4284:      mov x1, x19
1004c4288:      mov w0, #0x1                ; =1
1004c428c:      b   0x1004c3fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c4290:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c4294:      add x0, x0, #0x8a8
1004c4298:      bl  0x100ab165c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c429c:      bl  0x100ae2f08 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c42a0:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0x870>
1004c42a4:      add x0, x0, #0x3ec
1004c42a8:      mov w1, #0xb                ; =11
1004c42ac:      bl  0x100ae2ed0 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c42b0:      mov x22, x23
1004c42b4:      adrp    x2, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
1004c42b8:      add x2, x2, #0x30
1004c42bc:      mov x0, x22
1004c42c0:      mov w1, #0x10               ; =16
1004c42c4:      bl  0x100ab178c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
