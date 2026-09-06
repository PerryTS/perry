/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/plan-scan-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002f08c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>:
1002f08c0:      stp x22, x21, [sp, #-0x30]!
1002f08c4:      stp x20, x19, [sp, #0x10]
1002f08c8:      stp x29, x30, [sp, #0x20]
1002f08cc:      add x29, sp, #0x20
1002f08d0:      mov x19, x0
1002f08d4:      ldr w21, [x0, #0x4]
1002f08d8:      mov w8, #-0x40000000        ; =-1073741824
1002f08dc:      cmp w21, w8
1002f08e0:      csel    w20, w21, w8, lt
1002f08e4:      adrp    x8, 0x101119000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
1002f08e8:      add x8, x8, #0x94
1002f08ec:      ldr w22, [x8]
1002f08f0:      cmp w22, #0x300
1002f08f4:      b.hs    0x1002f0a64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1a4>
1002f08f8:      adrp    x8, 0x101118000 <_perry_global_baseline_worker_ts__1>
1002f08fc:      add x8, x8, #0xec8
1002f0900:      ldr x8, [x8]
1002f0904:      cmn x8, #0x1
1002f0908:      b.eq    0x1002f0a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x194>
1002f090c:      mrs x9, TPIDRRO_EL0
1002f0910:      and x9, x9, #0xfffffffffffffff8
1002f0914:      ldr x0, [x9, x8, lsl #3]
1002f0918:      cbz x0, 0x1002f0a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x194>
1002f091c:      add x8, x0, x22, lsl #3
1002f0920:      ldr x0, [x8, #0x1e8]
1002f0924:      cbz x0, 0x1002f0a64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1a4>
1002f0928:      ldr x0, [x0]
1002f092c:      cbz x0, 0x1002f0a78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1002f0930:      mov w8, #-0x40000001        ; =-1073741825
1002f0934:      cmp w21, w8
1002f0938:      b.gt    0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f093c:      ldr x10, [x0, #0x5198]
1002f0940:      and w8, w20, #0x3fffffff
1002f0944:      lsr x9, x8, #15
1002f0948:      cmp x9, x10
1002f094c:      b.hs    0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f0950:      ldr x10, [x0, #0x5190]
1002f0954:      ldr x9, [x10, x9, lsl #3]
1002f0958:      cbz x9, 0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f095c:      ubfx    x10, x8, #5, #10
1002f0960:      ldr x9, [x9, x10, lsl #3]
1002f0964:      cbz x9, 0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f0968:      and x8, x8, #0x1f
1002f096c:      add x8, x9, x8, lsl #5
1002f0970:      ldrb    w9, [x8, #0x1c]
1002f0974:      tbz w9, #0x0, 0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f0978:      ldr x0, [x8]
1002f097c:      cbz x0, 0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f0980:      bl  0x1002f0aa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json>
1002f0984:      tbnz    w0, #0x0, 0x1002f0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x15c>
1002f0988:      ldr w20, [x19]
1002f098c:      cbz w20, 0x1002f0a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x14c>
1002f0990:      adrp    x1, 0x100db4000 <_anon.cb60b81e29dbf4f2e7061ebb8588111d.321+0x4c>
1002f0994:      add x1, x1, #0xe25
1002f0998:      mov x0, x20
1002f099c:      mov w2, #0x6                ; =6
1002f09a0:      bl  0x100301720 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object13native_module25class_instance_has_member>
1002f09a4:      tbnz    w0, #0x0, 0x1002f0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x15c>
1002f09a8:      adrp    x1, 0x100db4000 <_anon.cb60b81e29dbf4f2e7061ebb8588111d.321+0x4c>
1002f09ac:      add x1, x1, #0xe25
1002f09b0:      mov x0, x20
1002f09b4:      mov w2, #0x6                ; =6
1002f09b8:      bl  0x100a7ab04 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry9construct23lookup_prototype_method>
1002f09bc:      cmp x0, #0x1
1002f09c0:      b.eq    0x1002f0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x15c>
1002f09c4:      mov w8, #0x1f               ; =31
1002f09c8:      mov x21, x8
1002f09cc:      mov x0, x20
1002f09d0:      bl  0x100325ad0 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_objects22class_prototype_object>
1002f09d4:      cbnz    x0, 0x1002f0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x15c>
1002f09d8:      mov x0, x20
1002f09dc:      bl  0x1002850e8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state27class_decl_prototype_object>
1002f09e0:      cbnz    x0, 0x1002f0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x15c>
1002f09e4:      mov x0, x20
1002f09e8:      bl  0x1003087b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object19class_meta_registry19get_parent_class_id>
1002f09ec:      cmp w0, #0x1
1002f09f0:      b.ne    0x1002f0a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x14c>
1002f09f4:      cbz w1, 0x1002f0a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x14c>
1002f09f8:      cmp w1, w20
1002f09fc:      b.eq    0x1002f0a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x14c>
1002f0a00:      sub w8, w21, #0x1
1002f0a04:      mov x20, x1
1002f0a08:      cbnz    w21, 0x1002f09c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x108>
1002f0a0c:      mov x0, x19
1002f0a10:      bl  0x1003b1a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
1002f0a14:      cmp x0, #0x1
1002f0a18:      b.ne    0x1002f0a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x164>
1002f0a1c:      mov w0, #0x0                ; =0
1002f0a20:      b   0x1002f0a44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x184>
1002f0a24:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
1002f0a28:      add x0, x0, #0x4c0
1002f0a2c:      ldr x8, [x0]
1002f0a30:      blr x8
1002f0a34:      ldrb    w8, [x0]
1002f0a38:      cbz w8, 0x1002f0a8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1cc>
1002f0a3c:      cmp w8, #0x2
1002f0a40:      cset    w0, ne
1002f0a44:      ldp x29, x30, [sp, #0x20]
1002f0a48:      ldp x20, x19, [sp, #0x10]
1002f0a4c:      ldp x22, x21, [sp], #0x30
1002f0a50:      ret
1002f0a54:      bl  0x100c8b6c8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1002f0a58:      add x8, x0, x22, lsl #3
1002f0a5c:      ldr x0, [x8, #0x1e8]
1002f0a60:      cbnz    x0, 0x1002f0928 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x68>
1002f0a64:      adrp    x0, 0x1010a3000 <_anon.7adc4553ee057240d1951d2053fb5027.1693+0x1c8>
1002f0a68:      add x0, x0, #0x218
1002f0a6c:      bl  0x100c8ae04 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1002f0a70:      ldr x0, [x0]
1002f0a74:      cbnz    x0, 0x1002f0930 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x70>
1002f0a78:      bl  0x100cb2b78 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1002f0a7c:      mov w8, #-0x40000001        ; =-1073741825
1002f0a80:      cmp w21, w8
1002f0a84:      b.le    0x1002f093c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x7c>
1002f0a88:      b   0x1002f0988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xc8>
1002f0a8c:      mov x19, x0
1002f0a90:      bl  0x100c88a80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe33compute_object_proto_tojson_state>
1002f0a94:      and w8, w0, #0xff
1002f0a98:      strb    w8, [x19]
1002f0a9c:      b   0x1002f0a3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x17c>
