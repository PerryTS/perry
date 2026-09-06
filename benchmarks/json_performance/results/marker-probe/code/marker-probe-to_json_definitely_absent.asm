/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003e4824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>:
1003e4824:      stp x22, x21, [sp, #-0x30]!
1003e4828:      stp x20, x19, [sp, #0x10]
1003e482c:      stp x29, x30, [sp, #0x20]
1003e4830:      add x29, sp, #0x20
1003e4834:      mov x19, x0
1003e4838:      ldr w20, [x0, #0x4]
1003e483c:      adrp    x8, 0x10112a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
1003e4840:      add x8, x8, #0x94
1003e4844:      ldr w21, [x8]
1003e4848:      cmp w21, #0x300
1003e484c:      b.hs    0x1003e4924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x100>
1003e4850:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
1003e4854:      add x8, x8, #0x768
1003e4858:      ldr x8, [x8]
1003e485c:      cmn x8, #0x1
1003e4860:      b.eq    0x1003e4914 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xf0>
1003e4864:      mrs x9, TPIDRRO_EL0
1003e4868:      and x9, x9, #0xfffffffffffffff8
1003e486c:      ldr x0, [x9, x8, lsl #3]
1003e4870:      cbz x0, 0x1003e4914 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0xf0>
1003e4874:      add x8, x0, x21, lsl #3
1003e4878:      ldr x0, [x8, #0x1e8]
1003e487c:      cbz x0, 0x1003e4924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x100>
1003e4880:      ldr x0, [x0]
1003e4884:      cbz x0, 0x1003e4938 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x114>
1003e4888:      mov w8, #-0x40000001        ; =-1073741825
1003e488c:      cmp w20, w8
1003e4890:      b.gt    0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e4894:      ldr x9, [x0, #0x5198]
1003e4898:      ubfx    x8, x20, #15, #15
1003e489c:      cmp x8, x9
1003e48a0:      b.hs    0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e48a4:      ldr x9, [x0, #0x5190]
1003e48a8:      ldr x8, [x9, x8, lsl #3]
1003e48ac:      cbz x8, 0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e48b0:      ubfx    x9, x20, #5, #10
1003e48b4:      ldr x8, [x8, x9, lsl #3]
1003e48b8:      cbz x8, 0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e48bc:      and x9, x20, #0x1f
1003e48c0:      add x8, x8, x9, lsl #5
1003e48c4:      ldrb    w9, [x8, #0x1c]
1003e48c8:      tbz w9, #0x0, 0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e48cc:      ldr x8, [x8]
1003e48d0:      cbz x8, 0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e48d4:      sub x9, x8, #0x100, lsl #12 ; =0x100000
1003e48d8:      tst x8, #0x7
1003e48dc:      mov x10, #0x7fffffffffff    ; =140737488355327
1003e48e0:      movk    x10, #0xffef, lsl #16
1003e48e4:      ccmp    x9, x10, #0x2, eq
1003e48e8:      b.hi    0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e48ec:      ldurb   w9, [x8, #-0x8]
1003e48f0:      cmp w9, #0x1
1003e48f4:      b.ne    0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e48f8:      ldp w20, w9, [x8], #0x8
1003e48fc:      cmp w20, #0x1, lsl #12      ; =0x1000
1003e4900:      ccmp    w20, w9, #0x2, ls
1003e4904:      b.hi    0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e4908:      cbz w20, 0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e490c:      mov w9, #0x7fff             ; =32767
1003e4910:      b   0x1003e4a48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x224>
1003e4914:      bl  0x100cc8104 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1003e4918:      add x8, x0, x21, lsl #3
1003e491c:      ldr x0, [x8, #0x1e8]
1003e4920:      cbnz    x0, 0x1003e4880 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x5c>
1003e4924:      adrp    x0, 0x1010d9000 <_anon.72cde5cdc14742b721629e115e16bf6f.1612+0x158>
1003e4928:      add x0, x0, #0x288
1003e492c:      bl  0x100cc7950 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1003e4930:      ldr x0, [x0]
1003e4934:      cbnz    x0, 0x1003e4888 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x64>
1003e4938:      bl  0x100cc7e88 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1003e493c:      mov w8, #-0x40000001        ; =-1073741825
1003e4940:      cmp w20, w8
1003e4944:      b.le    0x1003e4894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x70>
1003e4948:      ldr w20, [x19]
1003e494c:      cbz w20, 0x1003e49cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1a8>
1003e4950:      adrp    x1, 0x100dce000 <_anon.d2e5ad0658597b6ed712fc4e9a751921.2057+0x439>
1003e4954:      add x1, x1, #0xb34
1003e4958:      mov x0, x20
1003e495c:      mov w2, #0x6                ; =6
1003e4960:      bl  0x10020c960 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object13native_module25class_instance_has_member>
1003e4964:      tbnz    w0, #0x0, 0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e4968:      adrp    x1, 0x100dce000 <_anon.d2e5ad0658597b6ed712fc4e9a751921.2057+0x439>
1003e496c:      add x1, x1, #0xb34
1003e4970:      mov x0, x20
1003e4974:      mov w2, #0x6                ; =6
1003e4978:      bl  0x10088de74 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry9construct23lookup_prototype_method>
1003e497c:      cmp x0, #0x1
1003e4980:      b.eq    0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e4984:      mov w8, #0x1f               ; =31
1003e4988:      mov x21, x8
1003e498c:      mov x0, x20
1003e4990:      bl  0x1004247e8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_objects22class_prototype_object>
1003e4994:      cbnz    x0, 0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e4998:      mov x0, x20
1003e499c:      bl  0x100426c74 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state27class_decl_prototype_object>
1003e49a0:      cbnz    x0, 0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e49a4:      mov x0, x20
1003e49a8:      bl  0x1003fc4a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object19class_meta_registry19get_parent_class_id>
1003e49ac:      cmp w0, #0x1
1003e49b0:      b.ne    0x1003e49cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1a8>
1003e49b4:      cbz w1, 0x1003e49cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1a8>
1003e49b8:      cmp w1, w20
1003e49bc:      b.eq    0x1003e49cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1a8>
1003e49c0:      sub w8, w21, #0x1
1003e49c4:      mov x20, x1
1003e49c8:      cbnz    w21, 0x1003e4988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x164>
1003e49cc:      mov x0, x19
1003e49d0:      bl  0x100378564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
1003e49d4:      cmp x0, #0x1
1003e49d8:      b.ne    0x1003e49e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1c0>
1003e49dc:      mov w0, #0x0                ; =0
1003e49e0:      b   0x1003e4a04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1e0>
1003e49e4:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
1003e49e8:      add x0, x0, #0x258
1003e49ec:      ldr x8, [x0]
1003e49f0:      blr x8
1003e49f4:      ldrb    w8, [x0]
1003e49f8:      cbz w8, 0x1003e4a14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1f0>
1003e49fc:      cmp w8, #0x2
1003e4a00:      cset    w0, ne
1003e4a04:      ldp x29, x30, [sp, #0x20]
1003e4a08:      ldp x20, x19, [sp, #0x10]
1003e4a0c:      ldp x22, x21, [sp], #0x30
1003e4a10:      ret
1003e4a14:      mov x19, x0
1003e4a18:      bl  0x100cb4984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe33compute_object_proto_tojson_state>
1003e4a1c:      and w8, w0, #0xff
1003e4a20:      strb    w8, [x19]
1003e4a24:      b   0x1003e49fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1d8>
1003e4a28:      add x0, x10, #0x14
1003e4a2c:      mov x21, x8
1003e4a30:      bl  0x100cb4880 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json>
1003e4a34:      mov w9, #0x7fff             ; =32767
1003e4a38:      mov x8, x21
1003e4a3c:      tbnz    w0, #0x0, 0x1003e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x1b8>
1003e4a40:      subs    x20, x20, #0x1
1003e4a44:      b.eq    0x1003e4948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x124>
1003e4a48:      ldr x10, [x8], #0x8
1003e4a4c:      cmp x9, x10, lsr #48
1003e4a50:      and x10, x10, #0xffffffffffff
1003e4a54:      ccmp    x10, #0x0, #0x4, eq
1003e4a58:      b.eq    0x1003e4a40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x21c>
1003e4a5c:      ldr w1, [x10, #0x4]
1003e4a60:      cmp w1, #0x6
1003e4a64:      b.lo    0x1003e4a40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x21c>
1003e4a68:      ldrb    w11, [x10, #0x14]
1003e4a6c:      cmp w11, #0x74
1003e4a70:      b.eq    0x1003e4a28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x204>
1003e4a74:      cmp w11, #0x5f
1003e4a78:      b.ne    0x1003e4a40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x21c>
1003e4a7c:      b   0x1003e4a28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent+0x204>
1003e4a80:      udf #0x0
