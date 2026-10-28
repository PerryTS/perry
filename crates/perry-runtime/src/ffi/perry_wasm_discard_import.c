/* Linked only on request (`-u perry_wasm_discard_host`, see
 * `perry_wasm_discard.c`): the module then imports `perry_host.discard_pages`,
 * which returns 1 when the range now reads as zero and 0 when the host did
 * nothing. */
#if defined(__wasip1__)

__attribute__((__import_module__("perry_host"), __import_name__("discard_pages")))
extern int perry_host_discard_pages(void *ptr, __SIZE_TYPE__ len);

/* The marker the compiler asks for; it keeps this member, and so the import. */
int perry_wasm_discard_host(void *ptr, __SIZE_TYPE__ len) {
    return perry_host_discard_pages(ptr, len);
}
#endif
