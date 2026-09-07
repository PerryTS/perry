/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c3840 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record>:
1004c3840:      stp x28, x27, [sp, #-0x60]!
1004c3844:      stp x26, x25, [sp, #0x10]
1004c3848:      stp x24, x23, [sp, #0x20]
1004c384c:      stp x22, x21, [sp, #0x30]
1004c3850:      stp x20, x19, [sp, #0x40]
1004c3854:      stp x29, x30, [sp, #0x50]
1004c3858:      add x29, sp, #0x50
1004c385c:      sub sp, sp, #0x5d0
1004c3860:      ldr xzr, [sp]
1004c3864:      str x1, [sp, #0x38]
1004c3868:      mov x22, x0
1004c386c:      movi.2d v0, #0000000000000000
1004c3870:      str d0, [sp, #0x40]
1004c3874:      str wzr, [sp, #0x48]
1004c3878:      str d0, [sp, #0x68]
1004c387c:      str wzr, [sp, #0x70]
1004c3880:      str d0, [sp, #0x90]
1004c3884:      str wzr, [sp, #0x98]
1004c3888:      str d0, [sp, #0xb8]
1004c388c:      str wzr, [sp, #0xc0]
1004c3890:      str d0, [sp, #0xe0]
1004c3894:      str wzr, [sp, #0xe8]
1004c3898:      str d0, [sp, #0x108]
1004c389c:      str wzr, [sp, #0x110]
1004c38a0:      str d0, [sp, #0x130]
1004c38a4:      str wzr, [sp, #0x138]
1004c38a8:      str d0, [sp, #0x158]
1004c38ac:      str wzr, [sp, #0x160]
1004c38b0:      str d0, [sp, #0x180]
1004c38b4:      str wzr, [sp, #0x188]
1004c38b8:      str d0, [sp, #0x1a8]
1004c38bc:      str wzr, [sp, #0x1b0]
1004c38c0:      str d0, [sp, #0x1d0]
1004c38c4:      str wzr, [sp, #0x1d8]
1004c38c8:      str d0, [sp, #0x1f8]
1004c38cc:      str wzr, [sp, #0x200]
1004c38d0:      str d0, [sp, #0x220]
1004c38d4:      str wzr, [sp, #0x228]
1004c38d8:      str d0, [sp, #0x248]
1004c38dc:      str wzr, [sp, #0x250]
1004c38e0:      str d0, [sp, #0x270]
1004c38e4:      str wzr, [sp, #0x278]
1004c38e8:      str d0, [sp, #0x298]
1004c38ec:      str wzr, [sp, #0x2a0]
1004c38f0:      str d0, [sp, #0x2c0]
1004c38f4:      str wzr, [sp, #0x2c8]
1004c38f8:      str d0, [sp, #0x2e8]
1004c38fc:      str wzr, [sp, #0x2f0]
1004c3900:      str d0, [sp, #0x310]
1004c3904:      str wzr, [sp, #0x318]
1004c3908:      str d0, [sp, #0x338]
1004c390c:      str wzr, [sp, #0x340]
1004c3910:      str d0, [sp, #0x360]
1004c3914:      str wzr, [sp, #0x368]
1004c3918:      str d0, [sp, #0x388]
1004c391c:      str wzr, [sp, #0x390]
1004c3920:      str d0, [sp, #0x3b0]
1004c3924:      str wzr, [sp, #0x3b8]
1004c3928:      str d0, [sp, #0x3d8]
1004c392c:      str wzr, [sp, #0x3e0]
1004c3930:      str d0, [sp, #0x400]
1004c3934:      str wzr, [sp, #0x408]
1004c3938:      str d0, [sp, #0x428]
1004c393c:      str wzr, [sp, #0x430]
1004c3940:      str d0, [sp, #0x450]
1004c3944:      str wzr, [sp, #0x458]
1004c3948:      str d0, [sp, #0x478]
1004c394c:      str wzr, [sp, #0x480]
1004c3950:      str d0, [sp, #0x4a0]
1004c3954:      str wzr, [sp, #0x4a8]
1004c3958:      str d0, [sp, #0x4c8]
1004c395c:      str wzr, [sp, #0x4d0]
1004c3960:      str d0, [sp, #0x4f0]
1004c3964:      str wzr, [sp, #0x4f8]
1004c3968:      str d0, [sp, #0x518]
1004c396c:      mov w8, #-0x40000000        ; =-1073741824
1004c3970:      ldr w20, [x0, #0x4]
1004c3974:      cmp w20, w8
1004c3978:      csel    w19, w20, w8, lt
1004c397c:      str wzr, [sp, #0x520]
1004c3980:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3984:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3988:      ldr w21, [x9, #0x634]
1004c398c:      cmp w21, #0x300
1004c3990:      b.hs    0x1004c3a24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c3994:      ldr x8, [x8, #0x48]
1004c3998:      cmn x8, #0x1
1004c399c:      b.eq    0x1004c3a14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c39a0:      mrs x9, TPIDRRO_EL0
1004c39a4:      and x9, x9, #0xfffffffffffffff8
1004c39a8:      ldr x0, [x9, x8, lsl #3]
1004c39ac:      cbz x0, 0x1004c3a14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c39b0:      add x8, x0, x21, lsl #3
1004c39b4:      ldr x0, [x8, #0x1e8]
1004c39b8:      cbz x0, 0x1004c3a24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c39bc:      ldr x0, [x0]
1004c39c0:      cbz x0, 0x1004c3a38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1f8>
1004c39c4:      mov w8, #-0x40000001        ; =-1073741825
1004c39c8:      cmp w20, w8
1004c39cc:      b.gt    0x1004c3a48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c39d0:      ldr x10, [x0, #0x5198]
1004c39d4:      and w8, w19, #0x3fffffff
1004c39d8:      lsr x9, x8, #15
1004c39dc:      cmp x9, x10
1004c39e0:      b.hs    0x1004c3a48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c39e4:      ldr x10, [x0, #0x5190]
1004c39e8:      ldr x9, [x10, x9, lsl #3]
1004c39ec:      cbz x9, 0x1004c3a48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c39f0:      ubfx    x10, x8, #5, #10
1004c39f4:      ldr x9, [x9, x10, lsl #3]
1004c39f8:      cbz x9, 0x1004c3a48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c39fc:      and x8, x8, #0x1f
1004c3a00:      add x8, x9, x8, lsl #5
1004c3a04:      ldrb    w9, [x8, #0x1c]
1004c3a08:      tbz w9, #0x0, 0x1004c3a48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c3a0c:      ldr x8, [x8]
1004c3a10:      b   0x1004c3a4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x20c>
1004c3a14:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3a18:      add x8, x0, x21, lsl #3
1004c3a1c:      ldr x0, [x8, #0x1e8]
1004c3a20:      cbnz    x0, 0x1004c39bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x17c>
1004c3a24:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
1004c3a28:      add x0, x0, #0x360
1004c3a2c:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3a30:      ldr x0, [x0]
1004c3a34:      cbnz    x0, 0x1004c39c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x184>
1004c3a38:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c3a3c:      mov w8, #-0x40000001        ; =-1073741825
1004c3a40:      cmp w20, w8
1004c3a44:      b.le    0x1004c39d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x190>
1004c3a48:      mov x8, #0x0                ; =0
1004c3a4c:      mov x23, #0x0               ; =0
1004c3a50:      mov x19, #0x0               ; =0
1004c3a54:      add x9, x8, #0x8
1004c3a58:      sub x28, x29, #0x80
1004c3a5c:      add x8, x22, #0x10
1004c3a60:      stp x8, x9, [sp, #0x28]
1004c3a64:      add x8, sp, #0x2c0
1004c3a68:      add x8, x8, #0x8
1004c3a6c:      stp x22, x8, [sp, #0x8]
1004c3a70:      mov w22, #0x2               ; =2
1004c3a74:      mov w25, #0x28              ; =40
1004c3a78:      mov w26, #0x2               ; =2
1004c3a7c:      ldr x8, [sp, #0x30]
1004c3a80:      ldr x1, [x8, x23, lsl #3]
1004c3a84:      sub x0, x29, #0x80
1004c3a88:      bl  0x1004bdf30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece>
1004c3a8c:      ldur    w8, [x29, #-0x80]
1004c3a90:      cmn w8, #0x1
1004c3a94:      b.eq    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3a98:      ldp w10, w9, [x29, #-0x7c]
1004c3a9c:      ldur    w11, [x29, #-0x74]
1004c3aa0:      ldur    q0, [x28, #0x10]
1004c3aa4:      stur    q0, [x29, #-0xe0]
1004c3aa8:      ldur    x12, [x28, #0x20]
1004c3aac:      stur    x12, [x29, #-0xd0]
1004c3ab0:      add x13, sp, #0x40
1004c3ab4:      madd    x13, x23, x25, x13
1004c3ab8:      stp w8, w10, [x13]
1004c3abc:      stp w9, w11, [x13, #0x8]
1004c3ac0:      str q0, [x13, #0x10]
1004c3ac4:      str x12, [x13, #0x20]
1004c3ac8:      cbz w8, 0x1004c3adc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x29c>
1004c3acc:      cmp w8, #0x1
1004c3ad0:      csel    w24, w11, w10, eq
1004c3ad4:      csel    w27, w9, w10, eq
1004c3ad8:      b   0x1004c3ae4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x2a4>
1004c3adc:      add w27, w10, #0x2
1004c3ae0:      add w24, w9, #0x2
1004c3ae4:      ldr x8, [sp, #0x28]
1004c3ae8:      ldr x21, [x8, x23, lsl #3]
1004c3aec:      sub x0, x29, #0xc8
1004c3af0:      mov x1, x21
1004c3af4:      bl  0x1004bd5f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c3af8:      ldur    w8, [x29, #-0xc8]
1004c3afc:      cmn w8, #0x1
1004c3b00:      b.eq    0x1004c3b5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x31c>
1004c3b04:      ldp w20, w21, [x29, #-0xc4]
1004c3b08:      ldur    w9, [x29, #-0xbc]
1004c3b0c:      add x10, sp, #0x180
1004c3b10:      madd    x10, x23, x25, x10
1004c3b14:      stp w8, w20, [x10]
1004c3b18:      stp w21, w9, [x10, #0x8]
1004c3b1c:      sub x11, x29, #0xc8
1004c3b20:      ldur    q0, [x11, #0x10]
1004c3b24:      str q0, [x10, #0x10]
1004c3b28:      ldur    x11, [x11, #0x20]
1004c3b2c:      str x11, [x10, #0x20]
1004c3b30:      cmp w8, #0x2
1004c3b34:      b.eq    0x1004c3cf0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4b0>
1004c3b38:      cmp w8, #0x1
1004c3b3c:      b.ne    0x1004c3d0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4cc>
1004c3b40:      mov x20, x9
1004c3b44:      cmp x23, #0x0
1004c3b48:      mov w8, #0x1                ; =1
1004c3b4c:      cinc    w8, w8, ne
1004c3b50:      adds    w9, w27, w22
1004c3b54:      b.lo    0x1004c3d54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3b58:      b   0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b5c:      mov w8, #0x7ffd             ; =32765
1004c3b60:      cmp x8, x21, lsr #48
1004c3b64:      b.ne    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b68:      and x21, x21, #0xffffffffffff
1004c3b6c:      mov x0, x21
1004c3b70:      bl  0x100535e04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004c3b74:      cbz x0, 0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3b78:      ldrb    w8, [x0]
1004c3b7c:      cmp w8, #0x1
1004c3b80:      b.ne    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b84:      ldrsb   w8, [x0, #0x1]
1004c3b88:      tbnz    w8, #0x1f, 0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b8c:      ldrh    w8, [x0, #0x2]
1004c3b90:      tbnz    w8, #0xa, 0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b94:      ldr w8, [x0, #0x4]
1004c3b98:      cmp w8, #0x10
1004c3b9c:      b.lo    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ba0:      mov x20, x19
1004c3ba4:      ldr w19, [x21]
1004c3ba8:      cmp w19, #0x10
1004c3bac:      b.hi    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bb0:      ldr w9, [x21, #0x4]
1004c3bb4:      cmp w19, w9
1004c3bb8:      b.hi    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bbc:      str w27, [sp, #0x20]
1004c3bc0:      lsl x27, x19, #3
1004c3bc4:      add x9, x27, #0x10
1004c3bc8:      cmp x9, x8
1004c3bcc:      b.hi    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bd0:      mov x0, x21
1004c3bd4:      bl  0x1004fb6ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array6header35array_has_named_properties_resolved>
1004c3bd8:      tbnz    w0, #0x0, 0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bdc:      mov x0, x21
1004c3be0:      bl  0x100567508 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object15prototype_chain23object_static_prototype>
1004c3be4:      cmp x0, #0x1
1004c3be8:      b.eq    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bec:      str w24, [sp, #0x1c]
1004c3bf0:      add x24, x20, x19
1004c3bf4:      cmp x24, #0x10
1004c3bf8:      b.hi    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3bfc:      add x8, sp, #0x180
1004c3c00:      madd    x8, x23, x25, x8
1004c3c04:      mov w9, #-0x1               ; =-1
1004c3c08:      str w9, [x8]
1004c3c0c:      stp x20, x19, [x8, #0x8]
1004c3c10:      cbz w19, 0x1004c3d30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4f0>
1004c3c14:      str w22, [sp, #0x4]
1004c3c18:      mov w9, #0x28               ; =40
1004c3c1c:      mov x25, #0x0               ; =0
1004c3c20:      add x19, x21, #0x8
1004c3c24:      ldr x8, [sp, #0x10]
1004c3c28:      madd    x22, x20, x9, x8
1004c3c2c:      mov w20, #0x2               ; =2
1004c3c30:      mov w21, #0x2               ; =2
1004c3c34:      ldr x1, [x19, x25]
1004c3c38:      sub x0, x29, #0x80
1004c3c3c:      bl  0x1004bd5f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c3c40:      ldur    w8, [x29, #-0x80]
1004c3c44:      cmn w8, #0x1
1004c3c48:      b.eq    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3c4c:      ldp w10, w9, [x29, #-0x7c]
1004c3c50:      ldur    w11, [x29, #-0x74]
1004c3c54:      ldur    q0, [x28, #0x10]
1004c3c58:      stur    q0, [x29, #-0xa0]
1004c3c5c:      ldur    x12, [x28, #0x20]
1004c3c60:      stur    x12, [x29, #-0x90]
1004c3c64:      stp w8, w10, [x22, #-0x8]
1004c3c68:      stp w9, w11, [x22]
1004c3c6c:      stur    q0, [x22, #0x8]
1004c3c70:      str x12, [x22, #0x18]
1004c3c74:      cbz w8, 0x1004c3c98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x458>
1004c3c78:      cmp w8, #0x1
1004c3c7c:      csel    w8, w11, w10, eq
1004c3c80:      csel    w10, w9, w10, eq
1004c3c84:      cmp x25, #0x0
1004c3c88:      cset    w9, ne
1004c3c8c:      adds    w10, w10, w21
1004c3c90:      b.lo    0x1004c3cb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x470>
1004c3c94:      b   0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3c98:      add w10, w10, #0x2
1004c3c9c:      add w8, w9, #0x2
1004c3ca0:      cmp x25, #0x0
1004c3ca4:      cset    w9, ne
1004c3ca8:      adds    w10, w10, w21
1004c3cac:      b.hs    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3cb0:      adds    w21, w10, w9
1004c3cb4:      b.hs    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3cb8:      mov x0, #0x0                ; =0
1004c3cbc:      adds    w8, w8, w20
1004c3cc0:      cset    w10, hs
1004c3cc4:      adds    w20, w8, w9
1004c3cc8:      cset    w8, hs
1004c3ccc:      tbnz    w10, #0x0, 0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3cd0:      tbnz    w8, #0x0, 0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3cd4:      add x25, x25, #0x8
1004c3cd8:      add x22, x22, #0x28
1004c3cdc:      cmp x27, x25
1004c3ce0:      b.ne    0x1004c3c34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x3f4>
1004c3ce4:      mov x19, x24
1004c3ce8:      ldr w22, [sp, #0x4]
1004c3cec:      b   0x1004c3d3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4fc>
1004c3cf0:      mov x21, x20
1004c3cf4:      cmp x23, #0x0
1004c3cf8:      mov w8, #0x1                ; =1
1004c3cfc:      cinc    w8, w8, ne
1004c3d00:      adds    w9, w27, w22
1004c3d04:      b.lo    0x1004c3d54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3d08:      b   0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d0c:      add w8, w20, #0x2
1004c3d10:      add w20, w21, #0x2
1004c3d14:      mov x21, x8
1004c3d18:      cmp x23, #0x0
1004c3d1c:      mov w8, #0x1                ; =1
1004c3d20:      cinc    w8, w8, ne
1004c3d24:      adds    w9, w27, w22
1004c3d28:      b.lo    0x1004c3d54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3d2c:      b   0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d30:      mov w20, #0x2               ; =2
1004c3d34:      mov w21, #0x2               ; =2
1004c3d38:      mov x19, x24
1004c3d3c:      ldp w24, w27, [sp, #0x1c]
1004c3d40:      cmp x23, #0x0
1004c3d44:      mov w8, #0x1                ; =1
1004c3d48:      cinc    w8, w8, ne
1004c3d4c:      adds    w9, w27, w22
1004c3d50:      b.hs    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d54:      adds    w9, w21, w9
1004c3d58:      b.hs    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d5c:      adds    w11, w9, w8
1004c3d60:      b.hs    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d64:      adds    w9, w24, w26
1004c3d68:      b.hs    0x1004c3e78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3d6c:      mov x0, #0x0                ; =0
1004c3d70:      adds    w9, w20, w9
1004c3d74:      cset    w10, hs
1004c3d78:      adds    w26, w9, w8
1004c3d7c:      cset    w8, hs
1004c3d80:      tbnz    w10, #0x0, 0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3d84:      tbnz    w8, #0x0, 0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3d88:      add x23, x23, #0x1
1004c3d8c:      ldr x8, [sp, #0x38]
1004c3d90:      cmp x23, x8
1004c3d94:      mov x22, x11
1004c3d98:      mov w25, #0x28              ; =40
1004c3d9c:      b.ne    0x1004c3a7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x23c>
1004c3da0:      bl  0x10026a3d0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c3da4:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
1004c3da8:      mov w8, #0x1                ; =1
1004c3dac:      ldr x19, [sp, #0x8]
1004c3db0:      stp x19, x9, [x29, #-0x78]
1004c3db4:      stp x0, x8, [x29, #-0x88]
1004c3db8:      sub x0, x29, #0x80
1004c3dbc:      bl  0x10026a4d8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c3dc0:      mov x23, x0
1004c3dc4:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c3dc8:      add x0, x0, #0xbf0
1004c3dcc:      ldr x8, [x0]
1004c3dd0:      blr x8
1004c3dd4:      strb    wzr, [x0]
1004c3dd8:      mov x0, x19
1004c3ddc:      bl  0x1004c371c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys>
1004c3de0:      tbz w0, #0x0, 0x1004c3e70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x630>
1004c3de4:      mov x0, x22
1004c3de8:      bl  0x10032590c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004c3dec:      mov x21, x1
1004c3df0:      stp w26, w22, [x0]
1004c3df4:      stp wzr, wzr, [x0, #0xc]
1004c3df8:      str w22, [x0, #0x8]
1004c3dfc:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3e00:      ldr x8, [x24, #0x48]
1004c3e04:      cmn x8, #0x1
1004c3e08:      str x0, [sp, #0x20]
1004c3e0c:      b.eq    0x1004c3e9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3e10:      mrs x9, TPIDRRO_EL0
1004c3e14:      and x9, x9, #0xfffffffffffffff8
1004c3e18:      ldr x8, [x9, x8, lsl #3]
1004c3e1c:      cbz x8, 0x1004c3e9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3e20:      ldr x8, [x8, #0x19e8]
1004c3e24:      cbz x8, 0x1004c3e9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3e28:      ldr x9, [x8]
1004c3e2c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c3e30:      cmp x9, x10
1004c3e34:      b.hs    0x1004c4150 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x910>
1004c3e38:      add x10, x9, #0x1
1004c3e3c:      str x10, [x8]
1004c3e40:      ldr x10, [x8, #0x18]
1004c3e44:      cmp x23, x10
1004c3e48:      b.hs    0x1004c415c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1004c3e4c:      ldr x10, [x8, #0x10]
1004c3e50:      mov w11, #0x18              ; =24
1004c3e54:      madd    x10, x23, x11, x10
1004c3e58:      ldr x11, [x10]
1004c3e5c:      cmp x11, #0x1
1004c3e60:      b.ne    0x1004c4160 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x920>
1004c3e64:      ldr x0, [x10, #0x8]
1004c3e68:      str x9, [x8]
1004c3e6c:      b   0x1004c3ea4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x664>
1004c3e70:      sub x0, x29, #0x88
1004c3e74:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3e78:      mov x0, #0x0                ; =0
1004c3e7c:      add sp, sp, #0x5d0
1004c3e80:      ldp x29, x30, [sp, #0x50]
1004c3e84:      ldp x20, x19, [sp, #0x40]
1004c3e88:      ldp x22, x21, [sp, #0x30]
1004c3e8c:      ldp x24, x23, [sp, #0x20]
1004c3e90:      ldp x26, x25, [sp, #0x10]
1004c3e94:      ldp x28, x27, [sp], #0x60
1004c3e98:      ret
1004c3e9c:      mov x0, x23
1004c3ea0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c3ea4:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3ea8:      ldr w20, [x0, #0x4]
1004c3eac:      mov w8, #-0x40000000        ; =-1073741824
1004c3eb0:      cmp w20, w8
1004c3eb4:      csel    w19, w20, w8, lt
1004c3eb8:      ldr w22, [x9, #0x634]
1004c3ebc:      cmp w22, #0x300
1004c3ec0:      b.hs    0x1004c3f60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c3ec4:      ldr x8, [x24, #0x48]
1004c3ec8:      cmn x8, #0x1
1004c3ecc:      b.eq    0x1004c3f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3ed0:      mrs x9, TPIDRRO_EL0
1004c3ed4:      and x9, x9, #0xfffffffffffffff8
1004c3ed8:      ldr x8, [x9, x8, lsl #3]
1004c3edc:      cbz x8, 0x1004c3f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3ee0:      add x8, x8, x22, lsl #3
1004c3ee4:      ldr x8, [x8, #0x1e8]
1004c3ee8:      cbz x8, 0x1004c3f60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c3eec:      ldr x8, [x8]
1004c3ef0:      cbz x8, 0x1004c3f84 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x744>
1004c3ef4:      mov w9, #-0x40000001        ; =-1073741825
1004c3ef8:      cmp w20, w9
1004c3efc:      b.gt    0x1004c3fa0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c3f00:      ldr x11, [x8, #0x5198]
1004c3f04:      and w9, w19, #0x3fffffff
1004c3f08:      lsr x10, x9, #15
1004c3f0c:      cmp x10, x11
1004c3f10:      b.hs    0x1004c3fa0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c3f14:      ldr x8, [x8, #0x5190]
1004c3f18:      ldr x8, [x8, x10, lsl #3]
1004c3f1c:      cbz x8, 0x1004c3fa4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3f20:      ubfx    x10, x9, #5, #10
1004c3f24:      ldr x8, [x8, x10, lsl #3]
1004c3f28:      cbz x8, 0x1004c3fa4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3f2c:      and x9, x9, #0x1f
1004c3f30:      add x8, x8, x9, lsl #5
1004c3f34:      ldrb    w9, [x8, #0x1c]
1004c3f38:      tbz w9, #0x0, 0x1004c3fa0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c3f3c:      ldr x8, [x8]
1004c3f40:      b   0x1004c3fa4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3f44:      mov x23, x0
1004c3f48:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3f4c:      mov x8, x0
1004c3f50:      mov x0, x23
1004c3f54:      add x8, x8, x22, lsl #3
1004c3f58:      ldr x8, [x8, #0x1e8]
1004c3f5c:      cbnz    x8, 0x1004c3eec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6ac>
1004c3f60:      adrp    x8, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
1004c3f64:      add x8, x8, #0x360
1004c3f68:      mov x22, x0
1004c3f6c:      mov x0, x8
1004c3f70:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3f74:      mov x8, x0
1004c3f78:      mov x0, x22
1004c3f7c:      ldr x8, [x8]
1004c3f80:      cbnz    x8, 0x1004c3ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6b4>
1004c3f84:      mov x22, x0
1004c3f88:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c3f8c:      mov x8, x0
1004c3f90:      mov x0, x22
1004c3f94:      mov w9, #-0x40000001        ; =-1073741825
1004c3f98:      cmp w20, w9
1004c3f9c:      b.le    0x1004c3f00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6c0>
1004c3fa0:      mov x8, #0x0                ; =0
1004c3fa4:      mov x19, #0x0               ; =0
1004c3fa8:      mov w9, #0x7b               ; =123
1004c3fac:      strb    w9, [x21]
1004c3fb0:      add x25, x8, #0x8
1004c3fb4:      add x24, x0, #0x10
1004c3fb8:      add x8, sp, #0x2c0
1004c3fbc:      add x8, x8, #0x28
1004c3fc0:      stp x8, x25, [sp, #0x28]
1004c3fc4:      mov w22, #0x1               ; =1
1004c3fc8:      add x27, sp, #0x40
1004c3fcc:      mov w28, #0x3a              ; =58
1004c3fd0:      mov w20, #0x2c              ; =44
1004c3fd4:      b   0x1004c3ffc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c3fd8:      mov w8, #0x5d               ; =93
1004c3fdc:      strb    w8, [x21, x25]
1004c3fe0:      add x22, x25, #0x1
1004c3fe4:      ldp x25, x8, [sp, #0x30]
1004c3fe8:      add x27, sp, #0x40
1004c3fec:      mov w28, #0x3a              ; =58
1004c3ff0:      add x19, x19, #0x1
1004c3ff4:      cmp x19, x8
1004c3ff8:      b.eq    0x1004c4128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c3ffc:      cbz x19, 0x1004c4008 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7c8>
1004c4000:      strb    w20, [x21, x22]
1004c4004:      add x22, x22, #0x1
1004c4008:      add x8, x19, x19, lsl #2
1004c400c:      lsl x23, x8, #3
1004c4010:      ldr x1, [x25, x19, lsl #3]
1004c4014:      add x0, x27, x23
1004c4018:      add x2, x21, x22
1004c401c:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c4020:      add x8, x0, x22
1004c4024:      strb    w28, [x21, x8]
1004c4028:      add x26, x8, #0x1
1004c402c:      ldr x1, [x24, x19, lsl #3]
1004c4030:      add x9, sp, #0x180
1004c4034:      add x0, x9, x23
1004c4038:      ldr w9, [x0]
1004c403c:      cmn w9, #0x1
1004c4040:      b.eq    0x1004c4064 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x824>
1004c4044:      add x2, x21, x26
1004c4048:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c404c:      add x22, x0, x26
1004c4050:      add x19, x19, #0x1
1004c4054:      ldr x8, [sp, #0x38]
1004c4058:      cmp x19, x8
1004c405c:      b.ne    0x1004c3ffc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c4060:      b   0x1004c4128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c4064:      ldp x23, x22, [x0, #0x8]
1004c4068:      add x25, x8, #0x2
1004c406c:      mov w8, #0x5b               ; =91
1004c4070:      strb    w8, [x21, x26]
1004c4074:      cbz x22, 0x1004c3fd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c4078:      cmp x23, #0xf
1004c407c:      b.hi    0x1004c4170 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x930>
1004c4080:      and x26, x1, #0xffffffffffff
1004c4084:      add x8, sp, #0x2c0
1004c4088:      mov w9, #0x28               ; =40
1004c408c:      madd    x8, x23, x9, x8
1004c4090:      ldp q0, q1, [x8]
1004c4094:      stp q0, q1, [x29, #-0x80]
1004c4098:      ldr x8, [x8, #0x20]
1004c409c:      stur    x8, [x29, #-0x60]
1004c40a0:      ldr x1, [x26, #0x8]
1004c40a4:      sub x0, x29, #0x80
1004c40a8:      add x2, x21, x25
1004c40ac:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c40b0:      add x25, x0, x25
1004c40b4:      subs    x27, x22, #0x1
1004c40b8:      b.eq    0x1004c3fd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c40bc:      mov w9, #0x28               ; =40
1004c40c0:      add x28, x26, #0x10
1004c40c4:      cmp x23, #0x10
1004c40c8:      mov w8, #0x10               ; =16
1004c40cc:      csel    x8, x23, x8, lo
1004c40d0:      sub x26, x8, #0xf
1004c40d4:      add x22, x23, #0x1
1004c40d8:      ldr x8, [sp, #0x28]
1004c40dc:      madd    x23, x23, x9, x8
1004c40e0:      strb    w20, [x21, x25]
1004c40e4:      cbz x26, 0x1004c4174 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x934>
1004c40e8:      add x25, x25, #0x1
1004c40ec:      ldp q0, q1, [x23]
1004c40f0:      stp q0, q1, [x29, #-0x80]
1004c40f4:      ldr x8, [x23, #0x20]
1004c40f8:      stur    x8, [x29, #-0x60]
1004c40fc:      ldr x1, [x28], #0x8
1004c4100:      sub x0, x29, #0x80
1004c4104:      add x2, x21, x25
1004c4108:      bl  0x1004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c410c:      add x25, x0, x25
1004c4110:      add x26, x26, #0x1
1004c4114:      add x22, x22, #0x1
1004c4118:      add x23, x23, #0x28
1004c411c:      subs    x27, x27, #0x1
1004c4120:      b.ne    0x1004c40e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8a0>
1004c4124:      b   0x1004c3fd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c4128:      mov w8, #0x7d               ; =125
1004c412c:      strb    w8, [x21, x22]
1004c4130:      mov x19, #0x7fff000000000000 ; =9223090561878065152
1004c4134:      ldr x8, [sp, #0x20]
1004c4138:      bfxil   x19, x8, #0, #48
1004c413c:      sub x0, x29, #0x88
1004c4140:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c4144:      mov x1, x19
1004c4148:      mov w0, #0x1                ; =1
1004c414c:      b   0x1004c3e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c4150:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c4154:      add x0, x0, #0x8a8
1004c4158:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c415c:      bl  0x100ae2dc8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c4160:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0x9b0>
1004c4164:      add x0, x0, #0x2ac
1004c4168:      mov w1, #0xb                ; =11
1004c416c:      bl  0x100ae2d90 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c4170:      mov x22, x23
1004c4174:      adrp    x2, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
1004c4178:      add x2, x2, #0x30
1004c417c:      mov x0, x22
1004c4180:      mov w1, #0x10               ; =16
1004c4184:      bl  0x100ab164c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
