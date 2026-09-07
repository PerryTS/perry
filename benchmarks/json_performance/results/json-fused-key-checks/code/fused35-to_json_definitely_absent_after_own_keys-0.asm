/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c351c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys>:
1004c351c:      stp x22, x21, [sp, #-0x30]!
1004c3520:      stp x20, x19, [sp, #0x10]
1004c3524:      stp x29, x30, [sp, #0x20]
1004c3528:      add x29, sp, #0x20
1004c352c:      mov x19, x0
1004c3530:      ldr w20, [x0]
1004c3534:      cbz w20, 0x1004c35b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0x98>
1004c3538:      adrp    x1, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbb0>
1004c353c:      add x1, x1, #0xd0f
1004c3540:      mov x0, x20
1004c3544:      mov w2, #0x6                ; =6
1004c3548:      bl  0x1005536b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object13native_module25class_instance_has_member>
1004c354c:      tbnz    w0, #0x0, 0x1004c35c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xa8>
1004c3550:      adrp    x1, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbb0>
1004c3554:      add x1, x1, #0xd0f
1004c3558:      mov x0, x20
1004c355c:      mov w2, #0x6                ; =6
1004c3560:      bl  0x1006ed3f8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry9construct23lookup_prototype_method>
1004c3564:      cmp x0, #0x1
1004c3568:      b.eq    0x1004c35c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xa8>
1004c356c:      mov w8, #0x1f               ; =31
1004c3570:      mov x21, x8
1004c3574:      mov x0, x20
1004c3578:      bl  0x1006d9ed4 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry17prototype_objects22class_prototype_object>
1004c357c:      cbnz    x0, 0x1004c35c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xa8>
1004c3580:      mov x0, x20
1004c3584:      bl  0x1006dec7c <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry5state27class_decl_prototype_object>
1004c3588:      cbnz    x0, 0x1004c35c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xa8>
1004c358c:      mov x0, x20
1004c3590:      bl  0x10057e060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object19class_meta_registry19get_parent_class_id>
1004c3594:      cmp w0, #0x1
1004c3598:      b.ne    0x1004c35b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0x98>
1004c359c:      cbz w1, 0x1004c35b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0x98>
1004c35a0:      cmp w1, w20
1004c35a4:      b.eq    0x1004c35b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0x98>
1004c35a8:      sub w8, w21, #0x1
1004c35ac:      mov x20, x1
1004c35b0:      cbnz    w21, 0x1004c3570 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0x54>
1004c35b4:      mov x0, x19
1004c35b8:      bl  0x100567308 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object15prototype_chain23object_static_prototype>
1004c35bc:      cmp x0, #0x1
1004c35c0:      b.ne    0x1004c35cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xb0>
1004c35c4:      mov w0, #0x0                ; =0
1004c35c8:      b   0x1004c35ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xd0>
1004c35cc:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c35d0:      add x0, x0, #0xbf0
1004c35d4:      ldr x8, [x0]
1004c35d8:      blr x8
1004c35dc:      ldrb    w8, [x0]
1004c35e0:      cbz w8, 0x1004c35fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xe0>
1004c35e4:      cmp w8, #0x2
1004c35e8:      cset    w0, ne
1004c35ec:      ldp x29, x30, [sp, #0x20]
1004c35f0:      ldp x20, x19, [sp, #0x10]
1004c35f4:      ldp x22, x21, [sp], #0x30
1004c35f8:      ret
1004c35fc:      mov x19, x0
1004c3600:      bl  0x100ade9b8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe33compute_object_proto_tojson_state>
1004c3604:      and w8, w0, #0xff
1004c3608:      strb    w8, [x19]
1004c360c:      b   0x1004c35e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys+0xc8>
1004c3610:      nop
1004c3614:      nop
1004c3618:      nop
1004c361c:      nop
1004c3620:      nop
1004c3624:      nop
1004c3628:      nop
1004c362c:      nop
1004c3630:      nop
1004c3634:      nop
1004c3638:      nop
1004c363c:      nop
