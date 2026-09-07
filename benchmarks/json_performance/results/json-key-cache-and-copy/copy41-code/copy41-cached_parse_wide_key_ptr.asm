/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/copy41-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100305d50 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr>:
100305d50:      sub sp, sp, #0x60
100305d54:      stp x24, x23, [sp, #0x20]
100305d58:      stp x22, x21, [sp, #0x30]
100305d5c:      stp x20, x19, [sp, #0x40]
100305d60:      stp x29, x30, [sp, #0x50]
100305d64:      add x29, sp, #0x50
100305d68:      mov x20, x1
100305d6c:      mov x21, x0
100305d70:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
100305d74:      add x0, x0, #0xb60
100305d78:      ldr x8, [x0]
100305d7c:      blr x8
100305d80:      mov x19, x0
100305d84:      ldrb    w8, [x0, #0x38]
100305d88:      mov x23, x0
100305d8c:      cmp w8, #0x1
100305d90:      b.ne    0x100305e6c <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x11c>
100305d94:      ldr x24, [x23]
100305d98:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
100305d9c:      cmp x24, x8
100305da0:      b.hs    0x100305e80 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x130>
100305da4:      add x8, x24, #0x1
100305da8:      mov x0, x23
100305dac:      str x8, [x0], #0x8
100305db0:      mov x1, x21
100305db4:      mov x2, x20
100305db8:      bl  0x10013996c <__RINvMs1_NtCshsF3QZNC8Sh_9hashbrown3mapINtB6_7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderNtNtNtCs8BpVhDwHqJW_3std4hash6random11RandomStateE3getShEB1s_>
100305dbc:      cbz x0, 0x100305dcc <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x7c>
100305dc0:      ldr x22, [x0]
100305dc4:      str x24, [x23]
100305dc8:      b   0x100305e50 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x100>
100305dcc:      str x24, [x23]
100305dd0:      mov x0, x21
100305dd4:      mov x1, x20
100305dd8:      bl  0x1008852d4 <_js_string_from_bytes_longlived>
100305ddc:      mov x22, x0
100305de0:      ldrb    w8, [x19, #0x38]
100305de4:      cmp w8, #0x1
100305de8:      b.ne    0x100305e8c <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x13c>
100305dec:      ldr x8, [x19]
100305df0:      cbnz    x8, 0x100305ea8 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x158>
100305df4:      mov x8, #-0x1               ; =-1
100305df8:      str x8, [x19]
100305dfc:      cbz x20, 0x100305e28 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0xd8>
100305e00:      mov x0, x20
100305e04:      mov w1, #0x1                ; =1
100305e08:      bl  0x100af6688 <_mi_malloc_aligned>
100305e0c:      cbz x0, 0x100305eb4 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x164>
100305e10:      stp x20, x0, [sp, #0x8]
100305e14:      mov x1, x21
100305e18:      mov x2, x20
100305e1c:      bl  0x100af92ac <_writev+0x100af92ac>
100305e20:      str x20, [sp, #0x18]
100305e24:      b   0x100305e34 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0xe4>
100305e28:      mov w8, #0x1                ; =1
100305e2c:      stp xzr, x8, [sp, #0x8]
100305e30:      str xzr, [sp, #0x18]
100305e34:      add x0, x19, #0x8
100305e38:      add x1, sp, #0x8
100305e3c:      mov x2, x22
100305e40:      bl  0x1002310ec <__RNvMs1_NtCshsF3QZNC8Sh_9hashbrown3mapINtB5_7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderNtNtNtCs8BpVhDwHqJW_3std4hash6random11RandomStateE6insertB1r_>
100305e44:      ldr x8, [x19]
100305e48:      add x8, x8, #0x1
100305e4c:      str x8, [x19]
100305e50:      mov x0, x22
100305e54:      ldp x29, x30, [sp, #0x50]
100305e58:      ldp x20, x19, [sp, #0x40]
100305e5c:      ldp x22, x21, [sp, #0x30]
100305e60:      ldp x24, x23, [sp, #0x20]
100305e64:      add sp, sp, #0x60
100305e68:      ret
100305e6c:      mov x0, x19
100305e70:      bl  0x100abcf30 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
100305e74:      mov x23, x0
100305e78:      cbnz    x0, 0x100305d94 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x44>
100305e7c:      b   0x100305e9c <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x14c>
100305e80:      adrp    x0, 0x100e79000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x3fc8>
100305e84:      add x0, x0, #0xe50
100305e88:      bl  0x100ab165c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100305e8c:      mov x0, x19
100305e90:      bl  0x100abcf30 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
100305e94:      mov x19, x0
100305e98:      cbnz    x0, 0x100305dec <__RNvNtCs7wa3bwJW6LN_13perry_runtime4json25cached_parse_wide_key_ptr+0x9c>
100305e9c:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
100305ea0:      add x0, x0, #0x850
100305ea4:      bl  0x100aefa5c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100305ea8:      adrp    x0, 0x100e79000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x3fc8>
100305eac:      add x0, x0, #0xe68
100305eb0:      bl  0x100ab162c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100305eb4:      mov w0, #0x1                ; =1
100305eb8:      mov x1, x20
100305ebc:      bl  0x100ab12a4 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
