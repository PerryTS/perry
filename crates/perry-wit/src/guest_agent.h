/* Perry owns one JS agent per component instance. The WASI SDK gives each
   canonical task fresh TLS and frees it at task exit. Keep finite Rust frame
   calls on the implicit main agent's TLS; restore the SDK task's TLS before
   returning to C, task.return, WAIT, or EXIT. No Rust scope spans suspension.
   These are the linker-synthesized TLS helpers used by WASI SDK 34's libcall
   thread-context ABI (wasi-libc, private/wasi/wasip3_tls.h). The constructor
   captures this module's implicit-main TLS during reactor initialization.
   TLS bases are native storage, never managed JS heap addresses. */
#if defined(__wasm_libcall_thread_context__) && !defined(PERRY_COMPONENT_AGENT_H)
#define PERRY_COMPONENT_AGENT_H
extern void *__wasm_get_tls_base(void);
extern void __wasm_set_tls_base(void *);
static void *perry_agent_tls;
static bool perry_agent_tls_captured;
__attribute__((constructor)) static void perry_capture_agent_tls(void) {
    perry_agent_tls = __wasm_get_tls_base();
    perry_agent_tls_captured = true;
}
static inline void *perry_agent_enter(void) {
    if (!perry_agent_tls_captured) __builtin_trap();
    void *previous = __wasm_get_tls_base();
    __wasm_set_tls_base(perry_agent_tls);
    return previous;
}
#define PERRY_AGENT_RETURN(type, name, parameters, arguments) \
    static inline type perry_agent_##name parameters { \
        void *previous = perry_agent_enter(); \
        type result = name arguments; \
        __wasm_set_tls_base(previous); \
        return result; \
    }
#define PERRY_AGENT_VOID(name, parameters, arguments) \
    static inline void perry_agent_##name parameters { \
        void *previous = perry_agent_enter(); \
        name arguments; \
        __wasm_set_tls_base(previous); \
    }

PERRY_AGENT_RETURN(int32_t, js_component_set_guest_ops, (const perry_guest_ops * a0), (a0))
PERRY_AGENT_VOID(perry_component_frame_drop, (void * a0), (a0))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_failed, (void * a0), (a0))
PERRY_AGENT_VOID(perry_component_frame_fail, (void * a0, const uint8_t * a1, size_t a2), (a0, a1, a2))
PERRY_AGENT_VOID(perry_component_frame_initialize, (void * a0, const void * a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_number, (void * a0, double a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_bool, (void * a0, int32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_s64, (void * a0, int64_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_u64, (void * a0, uint64_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_string, (void * a0, const uint8_t * a1, size_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_char, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_bytes, (void * a0, const uint8_t * a1, size_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_array, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_object, (void * a0), (a0))
PERRY_AGENT_VOID(perry_component_frame_set, (void * a0, uint32_t a1, const uint8_t * a2, size_t a3, uint32_t a4), (a0, a1, a2, a3, a4))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_get, (void * a0, uint32_t a1, const uint8_t * a2, size_t a3), (a0, a1, a2, a3))
PERRY_AGENT_VOID(perry_component_frame_array_set, (void * a0, uint32_t a1, uint32_t a2, uint32_t a3), (a0, a1, a2, a3))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_array_get, (void * a0, uint32_t a1, uint32_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_array_len, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_copy, (void * a0, void * a1, uint32_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(double, perry_component_frame_get_number, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(double, perry_component_frame_get_integer, (void * a0, uint32_t a1, double a2, double a3), (a0, a1, a2, a3))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_get_bool, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint64_t, perry_component_frame_get_u64, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(int64_t, perry_component_frame_get_s64, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_get_char, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(size_t, perry_component_frame_string_len, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_VOID(perry_component_frame_string_copy, (void * a0, uint32_t a1, uint8_t * a2, size_t a3), (a0, a1, a2, a3))
PERRY_AGENT_RETURN(size_t, perry_component_frame_bytes_len, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_VOID(perry_component_frame_bytes_copy, (void * a0, uint32_t a1, uint8_t * a2, size_t a3), (a0, a1, a2, a3))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_call, (void * a0, uint32_t a1, uint32_t a2, uint32_t a3), (a0, a1, a2, a3))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_promise_poll, (void * a0, uint32_t a1, uint32_t * a2), (a0, a1, a2))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_task_call, (void * a0, uint32_t a1, uint32_t a2, uint32_t a3), (a0, a1, a2, a3))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_task_release_arguments, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_task_checkpoint, (void * a0), (a0))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_task_ready, (void * a0), (a0))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_task_cancel, (void * a0), (a0))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_task_resume, (void * a0, void (*a1)(void *), void * a2), (a0, a1, a2))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_import_begin, (void * a0), (a0))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_import_resolve, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_import_reject, (void * a0, uint32_t a1), (a0, a1))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_resource_lift, (void * a0, uint32_t a1, uint32_t a2, uint32_t a3, int32_t a4), (a0, a1, a2, a3, a4))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_resource_lower, (void * a0, uint32_t a1, uint32_t a2, int32_t a3, int32_t a4), (a0, a1, a2, a3, a4))
PERRY_AGENT_VOID(perry_component_frame_resource_commit, (void * a0), (a0))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_endpoint_lift, (void * a0, uint32_t a1, uint32_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(uint32_t, perry_component_frame_endpoint_lower, (void * a0, uint32_t a1, uint32_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_endpoint_pop, (void * a0, uint32_t * a1, uint32_t * a2), (a0, a1, a2))
PERRY_AGENT_RETURN(int32_t, perry_component_frame_resource_scope_end, (void * a0), (a0))
PERRY_AGENT_VOID(perry_component_frame_resource_destroy, (void * a0, uint32_t a1, uint32_t a2), (a0, a1, a2))
PERRY_AGENT_RETURN(void *, perry_component_frame_new, (void), ())
PERRY_AGENT_RETURN(void *, perry_component_frame_bootstrap, (const void * a0, void * a1), (a0, a1))
PERRY_AGENT_RETURN(void *, perry_component_frame_task_new, (void), ())

#undef PERRY_AGENT_RETURN
#undef PERRY_AGENT_VOID

#define js_component_set_guest_ops perry_agent_js_component_set_guest_ops
#define perry_component_frame_drop perry_agent_perry_component_frame_drop
#define perry_component_frame_failed perry_agent_perry_component_frame_failed
#define perry_component_frame_fail perry_agent_perry_component_frame_fail
#define perry_component_frame_initialize perry_agent_perry_component_frame_initialize
#define perry_component_frame_number perry_agent_perry_component_frame_number
#define perry_component_frame_bool perry_agent_perry_component_frame_bool
#define perry_component_frame_s64 perry_agent_perry_component_frame_s64
#define perry_component_frame_u64 perry_agent_perry_component_frame_u64
#define perry_component_frame_string perry_agent_perry_component_frame_string
#define perry_component_frame_char perry_agent_perry_component_frame_char
#define perry_component_frame_bytes perry_agent_perry_component_frame_bytes
#define perry_component_frame_array perry_agent_perry_component_frame_array
#define perry_component_frame_object perry_agent_perry_component_frame_object
#define perry_component_frame_set perry_agent_perry_component_frame_set
#define perry_component_frame_get perry_agent_perry_component_frame_get
#define perry_component_frame_array_set perry_agent_perry_component_frame_array_set
#define perry_component_frame_array_get perry_agent_perry_component_frame_array_get
#define perry_component_frame_array_len perry_agent_perry_component_frame_array_len
#define perry_component_frame_copy perry_agent_perry_component_frame_copy
#define perry_component_frame_get_number perry_agent_perry_component_frame_get_number
#define perry_component_frame_get_integer perry_agent_perry_component_frame_get_integer
#define perry_component_frame_get_bool perry_agent_perry_component_frame_get_bool
#define perry_component_frame_get_u64 perry_agent_perry_component_frame_get_u64
#define perry_component_frame_get_s64 perry_agent_perry_component_frame_get_s64
#define perry_component_frame_get_char perry_agent_perry_component_frame_get_char
#define perry_component_frame_string_len perry_agent_perry_component_frame_string_len
#define perry_component_frame_string_copy perry_agent_perry_component_frame_string_copy
#define perry_component_frame_bytes_len perry_agent_perry_component_frame_bytes_len
#define perry_component_frame_bytes_copy perry_agent_perry_component_frame_bytes_copy
#define perry_component_frame_call perry_agent_perry_component_frame_call
#define perry_component_frame_promise_poll perry_agent_perry_component_frame_promise_poll
#define perry_component_frame_task_call perry_agent_perry_component_frame_task_call
#define perry_component_frame_task_release_arguments perry_agent_perry_component_frame_task_release_arguments
#define perry_component_frame_task_checkpoint perry_agent_perry_component_frame_task_checkpoint
#define perry_component_frame_task_ready perry_agent_perry_component_frame_task_ready
#define perry_component_frame_task_cancel perry_agent_perry_component_frame_task_cancel
#define perry_component_frame_task_resume perry_agent_perry_component_frame_task_resume
#define perry_component_frame_import_begin perry_agent_perry_component_frame_import_begin
#define perry_component_frame_import_resolve perry_agent_perry_component_frame_import_resolve
#define perry_component_frame_import_reject perry_agent_perry_component_frame_import_reject
#define perry_component_frame_resource_lift perry_agent_perry_component_frame_resource_lift
#define perry_component_frame_resource_lower perry_agent_perry_component_frame_resource_lower
#define perry_component_frame_resource_commit perry_agent_perry_component_frame_resource_commit
#define perry_component_frame_endpoint_lift perry_agent_perry_component_frame_endpoint_lift
#define perry_component_frame_endpoint_lower perry_agent_perry_component_frame_endpoint_lower
#define perry_component_frame_endpoint_pop perry_agent_perry_component_frame_endpoint_pop
#define perry_component_frame_resource_scope_end perry_agent_perry_component_frame_resource_scope_end
#define perry_component_frame_resource_destroy perry_agent_perry_component_frame_resource_destroy
#define perry_component_frame_new perry_agent_perry_component_frame_new
#define perry_component_frame_bootstrap perry_agent_perry_component_frame_bootstrap
#define perry_component_frame_task_new perry_agent_perry_component_frame_task_new
#endif
