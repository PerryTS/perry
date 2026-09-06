/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100cb4880 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json>:
100cb4880:      mov x8, x0
100cb4884:      mov w0, #0x0                ; =0
100cb4888:      cmp w1, #0x15
100cb488c:      b.gt    0x100cb48c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0x48>
100cb4890:      cmp w1, #0x6
100cb4894:      b.eq    0x100cb4918 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0x98>
100cb4898:      cmp w1, #0xa
100cb489c:      b.ne    0x100cb4980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0x100>
100cb48a0:      ldrh    w9, [x8, #0x8]
100cb48a4:      ldr x8, [x8]
100cb48a8:      mov x10, #0x5f5f            ; =24415
100cb48ac:      movk    x10, #0x6f6d, lsl #16
100cb48b0:      movk    x10, #0x7564, lsl #32
100cb48b4:      movk    x10, #0x656c, lsl #48
100cb48b8:      cmp x8, x10
100cb48bc:      mov w8, #0x5f5f             ; =24415
100cb48c0:      ccmp    x9, x8, #0x0, eq
100cb48c4:      b   0x100cb497c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0xfc>
100cb48c8:      cmp w1, #0x16
100cb48cc:      b.eq    0x100cb4938 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0xb8>
100cb48d0:      cmp w1, #0x17
100cb48d4:      b.ne    0x100cb4980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0x100>
100cb48d8:      ldp x9, x10, [x8]
100cb48dc:      ldur    x8, [x8, #0xf]
100cb48e0:      mov x11, #0x5f5f            ; =24415
100cb48e4:      movk    x11, #0x6570, lsl #16
100cb48e8:      movk    x11, #0x7272, lsl #32
100cb48ec:      movk    x11, #0x5f79, lsl #48
100cb48f0:      cmp x9, x11
100cb48f4:      mov x9, #0x6574             ; =25972
100cb48f8:      movk    x9, #0x706d, lsl #16
100cb48fc:      movk    x9, #0x726f, lsl #32
100cb4900:      movk    x9, #0x6c61, lsl #48
100cb4904:      ccmp    x10, x9, #0x0, eq
100cb4908:      mov x9, #0x5f6c             ; =24428
100cb490c:      movk    x9, #0x6563, lsl #16
100cb4910:      movk    x9, #0x6c6c, lsl #32
100cb4914:      b   0x100cb4974 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0xf4>
100cb4918:      ldrh    w9, [x8, #0x4]
100cb491c:      ldr w8, [x8]
100cb4920:      orr x8, x8, x9, lsl #32
100cb4924:      mov x9, #0x6f74             ; =28532
100cb4928:      movk    x9, #0x534a, lsl #16
100cb492c:      movk    x9, #0x4e4f, lsl #32
100cb4930:      cmp x8, x9
100cb4934:      b   0x100cb497c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe30marker_bytes_may_carry_to_json+0xfc>
100cb4938:      ldp x9, x10, [x8]
100cb493c:      ldur    x8, [x8, #0xe]
100cb4940:      mov x11, #0x5f5f            ; =24415
100cb4944:      movk    x11, #0x6570, lsl #16
100cb4948:      movk    x11, #0x7272, lsl #32
100cb494c:      movk    x11, #0x5f79, lsl #48
100cb4950:      cmp x9, x11
100cb4954:      mov x9, #0x6566             ; =25958
100cb4958:      movk    x9, #0x6374, lsl #16
100cb495c:      movk    x9, #0x5f68, lsl #32
100cb4960:      movk    x9, #0x6168, lsl #48
100cb4964:      ccmp    x10, x9, #0x0, eq
100cb4968:      mov x9, #0x6168             ; =24936
100cb496c:      movk    x9, #0x646e, lsl #16
100cb4970:      movk    x9, #0x656c, lsl #32
100cb4974:      movk    x9, #0x5f5f, lsl #48
100cb4978:      ccmp    x8, x9, #0x0, eq
100cb497c:      cset    w0, eq
100cb4980:      ret
