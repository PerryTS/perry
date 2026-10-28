/* Run the generated export result path. This checks adapter ownership order;
   independent Wasm/component execution remains a separate qualification. */
#include "perry_adapter.c"
#include <assert.h>
#include <stdio.h>

static void *const task_frame = (void *)(uintptr_t)1;
static void *const result_frame = (void *)(uintptr_t)2;
static bool staged, committed, borrows_closed, returned;
static unsigned drops;

void *perry_component_frame_new(void) { return result_frame; }
int32_t perry_component_frame_failed(void *f) { (void)f; return 0; }
uint32_t perry_component_frame_copy(void *to, void *from, uint32_t value) {
    assert(to == result_frame && from == task_frame && value == 42); return 43;
}
int32_t perry_component_frame_task_resume(void *f, void (*cb)(void *), void *data) {
    assert(f == task_frame); cb(data); return 0;
}
int32_t perry_component_frame_task_checkpoint(void *f) {
    assert(f == task_frame && !returned); return 0;
}
int32_t perry_component_frame_task_ready(void *f) { assert(f == task_frame); return 0; }
int32_t perry_component_frame_promise_poll(void *f, uint32_t promise, uint32_t *value) {
    assert(f == task_frame && promise == 77); *value = 42; return 1;
}
uint32_t perry_component_frame_resource_lower(void *f, uint32_t slot, uint32_t ty,
                                             int32_t own, int32_t local) {
    (void)ty;
    assert(f == result_frame && slot == 43 && own && !local && !staged);
    staged = true; return 7;
}
void perry_component_frame_resource_commit(void *f) {
    assert(f == result_frame && staged && !committed && !returned); committed = true;
}
int32_t perry_component_frame_resource_scope_end(void *f) {
    assert(f == task_frame && committed && !returned); borrows_closed = true; return 0;
}
void $RETURN_FUNCTION(__typeof__(((perry_result_0_0 *)0)->result) result) {
    assert(committed && borrows_closed && !returned && result.__handle == 7);
    returned = true;
}
void perry_component_frame_drop(void *f) {
    assert(returned && (f == task_frame || f == result_frame)); ++drops;
}
plugin_waitable_status_t plugin_stream_void_write(plugin_stream_void_writer_t w, size_t n) {
    (void)w; (void)n; assert(0); return 0;
}
void plugin_waitable_join(uint32_t h, plugin_waitable_set_t s) { (void)h; (void)s; assert(0); }
plugin_waitable_status_t plugin_stream_void_cancel_read(plugin_stream_void_t h) {
    (void)h; assert(0); return 0;
}
void plugin_stream_void_drop_readable(plugin_stream_void_t h) { (void)h; assert(0); }
void plugin_stream_void_drop_writable(plugin_stream_void_writer_t h) { (void)h; assert(0); }
void plugin_waitable_set_drop(plugin_waitable_set_t s) { (void)s; assert(0); }
plugin_waitable_set_t plugin_waitable_set_new(void) { assert(0); return 0; }
plugin_stream_void_t plugin_stream_void_new(plugin_stream_void_writer_t *writer) {
    (void)writer; assert(0); return 0;
}
plugin_waitable_status_t plugin_stream_void_read(plugin_stream_void_t reader, size_t count) {
    (void)reader; (void)count; assert(0); return 0;
}
void plugin_context_set_0(void *p) { assert(p == NULL); }

int main(void) {
    perry_task *task = calloc(1, sizeof(*task));
    assert(task);
    task->frame = task_frame; task->promise = 77; task->asynchronous = true;
    perry_tasks = task;
    assert(perry_result_0_0_step(task) == PLUGIN_CALLBACK_CODE_EXIT);
    assert(returned && drops == 2 && perry_tasks == NULL);
    puts("PASS: async export commits its owner and ends borrows before return");
    return 0;
}
