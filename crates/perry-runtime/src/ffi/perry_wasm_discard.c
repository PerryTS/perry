/* `perry_host.discard_pages` without making it an import of every module
 * (`src/wasm_discard.rs` has the contract).
 *
 * `perry_wasm_discard_host` is a weak undefined reference, so it neither
 * pulls `perry_wasm_discard_import.c` out of the runtime archive nor fails
 * the link when the member is left out: its address is null and the fallback
 * answers 0 ("the host did nothing"). A link that passes
 * `-u perry_wasm_discard_host` (the compiler does when PERRY_WASM_DISCARD=1)
 * loads that member, and the module imports `perry_host.discard_pages`. */
#if defined(__wasip1__)

int perry_wasm_discard_host(void *ptr, __SIZE_TYPE__ len) __attribute__((weak));

int perry_wasm_discard(void *ptr, __SIZE_TYPE__ len) {
    if (!perry_wasm_discard_host) return 0;
    return perry_wasm_discard_host(ptr, len);
}

/* Whether the import member was linked. */
int perry_wasm_discard_linked(void) {
    return perry_wasm_discard_host != 0;
}
#endif
