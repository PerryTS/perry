/* Deterministic canonical acknowledgements exercise generated read storage,
   scheduler callbacks and destructors under ASan/UBSan. This native test does
   not substitute for actual Wasm canonical I/O qualification. */
#include "perry_adapter.c"
#include <assert.h>
#include <stdio.h>

static unsigned adopted[4], dropped[4], freed_buffers, io_frames, parent_frames;
static unsigned finishes, resumes, prepares, native_cancels, reader_drops;
static unsigned provided, expected_flags, cases;
static uint32_t joined;
static bool observed, acknowledged, immediate, live;
static plugin_waitable_state_t terminal;
static test_endpoint_custody_api_payload_t *storage;
static void *const io_frame = (void *)(uintptr_t)1;
static void *const parent_frame = (void *)(uintptr_t)2;
static void *const scratch_frame = (void *)(uintptr_t)3;

static plugin_waitable_status_t acknowledge(void) {
    assert(!acknowledged && io_frames == 0 && freed_buffers == 0);
    for (unsigned i = 0; i < provided; ++i) {
        storage[i].item.__handle = i + 1;
        storage[i].text.ptr = malloc(4);
        assert(storage[i].text.ptr);
        memcpy(storage[i].text.ptr, "pear", 4);
        storage[i].text.len = 4;
    }
    acknowledged = true;
    return (provided << 4) | terminal;
}
void test_endpoint_custody_api_item_drop_own(test_endpoint_custody_api_own_item_t item) {
    assert(acknowledged && io_frames == 0);
    assert(item.__handle > 0 && item.__handle < 4);
    assert(adopted[item.__handle] == 0);
    assert(++dropped[item.__handle] == 1);
}
void test_endpoint_custody_api_payload_free(test_endpoint_custody_api_payload_t *item) {
    assert(acknowledged && io_frames == 0 && item->text.len == 4);
    assert(adopted[item->item.__handle] + dropped[item->item.__handle] == 1);
    free(item->text.ptr);
    ++freed_buffers;
}
plugin_waitable_status_t $FUTURE_HELPER_read($FUTURE_HELPER_t reader, test_endpoint_custody_api_payload_t *items) {
    assert(reader == 42 && !acknowledged);
    storage = items;
    /* Poison unused capacity; only the acknowledged prefix is initialized. */
    memset(storage, 0xa5, sizeof(*storage));
    return immediate ? acknowledge() : PLUGIN_WAITABLE_STATUS_BLOCKED;
}
plugin_waitable_status_t $STREAM_HELPER_read($STREAM_HELPER_t reader, test_endpoint_custody_api_payload_t *items, size_t count) {
    assert(reader == 42 && count == 3 && !acknowledged);
    storage = items;
    memset(storage, 0xa5, count * sizeof(*storage));
    return immediate ? acknowledge() : PLUGIN_WAITABLE_STATUS_BLOCKED;
}
static plugin_waitable_status_t cancel(uint32_t reader) {
    assert(reader == 42 && joined == 0 && !acknowledged);
    assert(perry_tasks->io->cancelled && !perry_tasks->io->complete);
    ++native_cancels;
    return acknowledge();
}
plugin_waitable_status_t $FUTURE_HELPER_cancel_read($FUTURE_HELPER_t reader) { return cancel(reader); }
plugin_waitable_status_t $STREAM_HELPER_cancel_read($STREAM_HELPER_t reader) { return cancel(reader); }
static void drop_reader(uint32_t reader) {
    assert(reader == 42 && acknowledged && io_frames == 1);
    assert(++reader_drops == 1);
}
void $FUTURE_HELPER_drop_readable($FUTURE_HELPER_t reader) { drop_reader(reader); }
void $STREAM_HELPER_drop_readable($STREAM_HELPER_t reader) { drop_reader(reader); }
void *perry_component_frame_new(void) { return scratch_frame; }
void perry_component_frame_drop(void *frame) {
    if (frame == scratch_frame) return;
    assert(acknowledged);
    if (frame == io_frame) { assert(freed_buffers == provided); assert(++io_frames == 1); }
    else { assert(frame == parent_frame); assert(++parent_frames == 1); }
}
int32_t perry_component_frame_failed(void *frame) { (void)frame; return 0; }
void perry_component_frame_fail(void *frame, const uint8_t *text, size_t len) {
    (void)frame; (void)text; (void)len; assert(0);
}
int32_t perry_component_frame_endpoint_status(void *frame) { assert(frame == io_frame); return live; }
int32_t perry_component_frame_endpoint_prepare(void *frame) {
    assert(frame == io_frame && acknowledged); ++prepares; return observed;
}
int32_t perry_component_frame_endpoint_finish(void *frame, uint32_t value, uint32_t flags) {
    assert(frame == io_frame && acknowledged && io_frames == 0);
    assert(flags == expected_flags);
    if (!(flags & 2)) assert(observed && value != 0);
    assert(++finishes == 1); return 0;
}
uint32_t perry_component_frame_resource_lift(void *frame, uint32_t type, uint32_t handle, uint32_t rep, int32_t own) {
    (void)type; (void)rep;
    assert(frame == io_frame && acknowledged && observed && own);
    assert(handle > 0 && handle < 4 && dropped[handle] == 0);
    assert(++adopted[handle] == 1); return handle;
}
uint32_t perry_component_frame_string(void *frame, const uint8_t *text, size_t len) {
    assert(frame == io_frame && observed && acknowledged && len == 4);
    assert(memcmp(text, "pear", 4) == 0); return 11;
}
uint32_t perry_component_frame_object(void *frame) { assert(frame == io_frame); return 12; }
void perry_component_frame_set(void *frame, uint32_t object, const uint8_t *key, size_t len, uint32_t value) {
    (void)object; (void)key; (void)len; (void)value; assert(frame == io_frame);
}
uint32_t perry_component_frame_bool(void *frame, int32_t value) { (void)value; assert(frame == io_frame); return 13; }
uint32_t perry_component_frame_array(void *frame, uint32_t len) { (void)len; assert(frame == io_frame); return 14; }
void perry_component_frame_array_set(void *frame, uint32_t array, uint32_t index, uint32_t value) {
    (void)array; (void)index; (void)value; assert(frame == io_frame);
}
int32_t perry_component_frame_endpoint_pop(void *frame, uint32_t *type, uint32_t *handle) {
    (void)type; (void)handle; assert(frame == scratch_frame); return 0;
}
int32_t perry_component_frame_task_resume(void *frame, void (*callback)(void *), void *data) {
    assert(frame == parent_frame && acknowledged); ++resumes; callback(data); return 0;
}
int32_t perry_component_frame_task_ready(void *frame) { (void)frame; return 0; }
int32_t perry_component_frame_task_cancel(void *frame) { assert(frame == parent_frame && !acknowledged); return 0; }
int32_t perry_component_frame_resource_scope_end(void *frame) { assert(frame == parent_frame && io_frames == 1); return 0; }
void plugin_waitable_join(uint32_t handle, plugin_waitable_set_t set) { assert(handle == 42 || handle == 0); joined = set; }
plugin_waitable_set_t plugin_waitable_set_new(void) { return 71; }
void plugin_waitable_set_drop(plugin_waitable_set_t set) { assert(set == 71 && joined == 0); }
plugin_subtask_status_t plugin_subtask_cancel(plugin_subtask_t child) { (void)child; assert(0); return 0; }
void plugin_subtask_drop(plugin_subtask_t child) { (void)child; assert(0); }
void plugin_task_cancel(void) { assert(acknowledged && io_frames == 1); }
void plugin_context_set_0(void *pointer) { assert(pointer == NULL); }
plugin_waitable_status_t plugin_stream_void_cancel_read(plugin_stream_void_t handle) { (void)handle; assert(0); return 0; }
void plugin_stream_void_drop_readable(plugin_stream_void_t handle) { assert(handle == 0); }
void plugin_stream_void_drop_writable(plugin_stream_void_writer_t handle) { assert(handle == 0); }
plugin_waitable_status_t plugin_stream_void_write(plugin_stream_void_writer_t handle, size_t count) { (void)handle; (void)count; assert(0); return 0; }

/* action: 0 completion, 1 explicit cancellation, 2 abandoned read sweep,
   3 disposal, 4 parent task cancellation. */
static void check(bool stream, bool inline_read, plugin_waitable_state_t state, unsigned count, bool observer, unsigned action) {
    assert(perry_tasks == NULL);
    memset(adopted, 0, sizeof(adopted)); memset(dropped, 0, sizeof(dropped));
    freed_buffers = io_frames = parent_frames = finishes = resumes = prepares = 0;
    native_cancels = reader_drops = joined = 0;
    provided = count; terminal = state; immediate = inline_read;
    observed = observer; live = action != 2; acknowledged = false;
    bool aborted = action != 0 || state == PLUGIN_WAITABLE_CANCELLED || (!stream && count == 0) || !observer;
    bool ended = state == PLUGIN_WAITABLE_DROPPED || (!stream && count == 1);
    expected_flags = (ended ? 1 : 0) | (aborted ? 2 : 0);
    perry_task *task = calloc(1, sizeof(*task)); assert(task);
    task->frame = parent_frame; task->asynchronous = task->waiting = true;
    perry_tasks = perry_active_task = task;
    int32_t status = stream ? $STREAM_STEM_start(io_frame, 42, 3) : $FUTURE_STEM_start(io_frame, 42, 1);
    perry_active_task = NULL;
    assert(status == 0 && io_frames == 0 && freed_buffers == 0);
    if (inline_read) {
        assert(finishes == 1 && task->io->complete && joined == 0);
        perry_task_reap(task); /* The runtime frame handoff has now returned. */
    } else {
        assert(finishes == 0 && joined == 71 && task->io->joined);
        if (action == 0) {
            plugin_event_t event = { .event = stream ? PLUGIN_EVENT_STREAM_READ : PLUGIN_EVENT_FUTURE_READ, .waitable = 42, .code = acknowledge() };
            perry_task_event(task, &event);
        } else if (action == 1) perry_io_cancel(task, task->io);
        else if (action == 2) perry_io_sweep(task);
        else if (action == 3) {
            perry_io_close(task->io->type, 42);
            if (stream) $STREAM_HELPER_drop_readable(42); else $FUTURE_HELPER_drop_readable(42);
        } else {
            assert(action == 4 && perry_task_cancel(task) == PLUGIN_CALLBACK_CODE_EXIT);
        }
    }
    assert(finishes == 1 && io_frames == 1 && freed_buffers == count && joined == 0);
    assert(native_cancels == (action != 0));
    assert(resumes == (!inline_read && action != 4));
    for (unsigned i = 1; i <= count; ++i) {
        assert(adopted[i] == !aborted);
        assert(dropped[i] == aborted);
    }
    if (action != 4) {
        assert(task->io == NULL && parent_frames == 0);
        if (task->has_set) plugin_waitable_set_drop(task->set);
        perry_tasks = NULL; perry_component_frame_drop(task->frame); free(task);
    }
    assert(perry_tasks == NULL && parent_frames == 1);
    ++cases;
}
int main(void) {
    for (unsigned stream = 0; stream < 2; ++stream) {
        for (unsigned observer = 0; observer < 2; ++observer) {
            check(stream, true, PLUGIN_WAITABLE_COMPLETED, 1, observer, 0);
            check(stream, false, PLUGIN_WAITABLE_COMPLETED, 1, observer, 0);
            check(stream, false, PLUGIN_WAITABLE_DROPPED, 0, observer, 0);
            check(stream, false, PLUGIN_WAITABLE_CANCELLED, 0, observer, 1);
            check(stream, false, PLUGIN_WAITABLE_COMPLETED, 1, observer, 1);
            check(stream, false, PLUGIN_WAITABLE_CANCELLED, stream ? 2 : 0, observer, 4);
        }
    }
    check(true, false, PLUGIN_WAITABLE_CANCELLED, 2, false, 2);
    check(true, false, PLUGIN_WAITABLE_CANCELLED, 1, true, 3);
    assert(cases == 26);
    puts("PASS: 26 guest read ownership cases");
    return 0;
}
