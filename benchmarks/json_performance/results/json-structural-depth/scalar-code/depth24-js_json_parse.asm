/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/depth24-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001007ee144 <_js_json_parse>:
1007ee144:      stp x29, x30, [sp, #-0x10]!
1007ee148:      mov x29, sp
1007ee14c:      cbz x0, 0x1007ee178 <_js_json_parse+0x34>
1007ee150:      ldr w1, [x0, #0x4]
1007ee154:      cbz w1, 0x1007ee178 <_js_json_parse+0x34>
1007ee158:      ldrb    w8, [x0, #0x14]
1007ee15c:      orr w8, w8, #0x20
1007ee160:      cmp w8, #0x7b
1007ee164:      b.ne    0x1007ee170 <_js_json_parse+0x2c>
1007ee168:      ldp x29, x30, [sp], #0x10
1007ee16c:      b   0x1004cf9f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api10parse_slow>
1007ee170:      ldp x29, x30, [sp], #0x10
1007ee174:      b   0x1004d1f34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer>
1007ee178:      adrp    x0, 0x100c0b000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x6d20>
1007ee17c:      add x0, x0, #0x142
1007ee180:      mov w1, #0x1c               ; =28
1007ee184:      bl  0x1004d2618 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18throw_syntax_error>
        ...
