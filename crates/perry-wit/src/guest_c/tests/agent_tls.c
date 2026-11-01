#include "perry_component_frame.h"
#include <assert.h>
#include <stdio.h>

static int implicit_main, task_a, task_b;
static void *current_tls = &implicit_main;
static unsigned drops, callbacks;
void *__wasm_get_tls_base(void) { return current_tls; }
void __wasm_set_tls_base(void *base) { current_tls = base; }

/* Fake Rust entries run before the bridge's macros are installed. They reject
   task TLS, while the caller checks that every normal/error return restores it. */
void *perry_component_frame_new(void) {
    assert(current_tls == &implicit_main);
    return &implicit_main;
}
void *perry_component_frame_task_new(void) { return perry_component_frame_new(); }
void perry_component_frame_drop(void *frame) {
    assert(current_tls == &implicit_main && frame == &implicit_main);
    drops++;
}
int32_t perry_component_frame_task_ready(void *frame) {
    assert(current_tls == &implicit_main && frame == &implicit_main);
    return -1;
}
uint64_t perry_component_frame_get_u64(void *frame, uint32_t slot) {
    assert(current_tls == &implicit_main && frame == &implicit_main && slot == 7);
    return UINT64_MAX - 7;
}
int32_t perry_component_frame_task_resume(void *frame, void (*callback)(void *), void *data) {
    assert(current_tls == &implicit_main && frame == &implicit_main);
    callback(data);
    assert(current_tls == &implicit_main);
    return 17;
}

#define __wasm_libcall_thread_context__ 1
#include "perry_component_agent.h"

static void nested(void *frame) {
    assert(current_tls == &implicit_main);
    assert(perry_component_frame_get_u64(frame, 7) == UINT64_MAX - 7);
    assert(current_tls == &implicit_main);
    current_tls = &task_b;
    assert(perry_component_frame_new() == &implicit_main);
    assert(current_tls == &task_b);
    current_tls = &implicit_main;
    callbacks++;
}
int main(void) {
    void *tasks[] = {&task_a, &task_b};
    for (size_t i = 0; i < 2; ++i) {
        current_tls = tasks[i];
        void *frame = perry_component_frame_task_new();
        assert(current_tls == tasks[i]);
        assert(perry_component_frame_task_ready(frame) == -1);
        assert(current_tls == tasks[i]);
        assert(perry_component_frame_task_resume(frame, nested, frame) == 17);
        assert(current_tls == tasks[i]);
        perry_component_frame_drop(frame);
        assert(current_tls == tasks[i]);
    }
    assert(callbacks == 2 && drops == 2);
    puts("PASS: agent TLS, nested calls, errors, void and wide results");
}
