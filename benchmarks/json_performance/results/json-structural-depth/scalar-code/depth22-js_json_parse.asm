/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/depth22-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001007eda84 <_js_json_parse>:
1007eda84:      stp x29, x30, [sp, #-0x10]!
1007eda88:      mov x29, sp
1007eda8c:      cbz x0, 0x1007edab8 <_js_json_parse+0x34>
1007eda90:      ldr w1, [x0, #0x4]
1007eda94:      cbz w1, 0x1007edab8 <_js_json_parse+0x34>
1007eda98:      ldrb    w8, [x0, #0x14]
1007eda9c:      orr w8, w8, #0x20
1007edaa0:      cmp w8, #0x7b
1007edaa4:      b.ne    0x1007edab0 <_js_json_parse+0x2c>
1007edaa8:      ldp x29, x30, [sp], #0x10
1007edaac:      b   0x1004d0338 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api10parse_slow>
1007edab0:      ldp x29, x30, [sp], #0x10
1007edab4:      b   0x1004d2874 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer>
1007edab8:      adrp    x0, 0x100c0a000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x63e0>
1007edabc:      add x0, x0, #0xa82
1007edac0:      mov w1, #0x1c               ; =28
1007edac4:      bl  0x1004d2f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18throw_syntax_error>
        ...
