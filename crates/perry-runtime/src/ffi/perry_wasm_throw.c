/* Standard wasm EH ABI tag, internal to the core module. No host tag import. */
__asm__(".globl __cpp_exception\n.tagtype __cpp_exception i32\n__cpp_exception:\n");

_Noreturn void perry_wasm_throw_header(void *object) {
    __builtin_wasm_throw(0, object);
}
