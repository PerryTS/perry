/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001007eda44 <_js_json_parse>:
1007eda44:      stp x29, x30, [sp, #-0x10]!
1007eda48:      mov x29, sp
1007eda4c:      cbz x0, 0x1007eda78 <_js_json_parse+0x34>
1007eda50:      ldr w1, [x0, #0x4]
1007eda54:      cbz w1, 0x1007eda78 <_js_json_parse+0x34>
1007eda58:      ldrb    w8, [x0, #0x14]
1007eda5c:      orr w8, w8, #0x20
1007eda60:      cmp w8, #0x7b
1007eda64:      b.ne    0x1007eda70 <_js_json_parse+0x2c>
1007eda68:      ldp x29, x30, [sp], #0x10
1007eda6c:      b   0x1004cf778 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api10parse_slow>
1007eda70:      ldp x29, x30, [sp], #0x10
1007eda74:      b   0x1004d1cb4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer>
1007eda78:      adrp    x0, 0x100c0a000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x6420>
1007eda7c:      add x0, x0, #0xa42
1007eda80:      mov w1, #0x1c               ; =28
1007eda84:      bl  0x1004d2144 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18throw_syntax_error>
        ...
