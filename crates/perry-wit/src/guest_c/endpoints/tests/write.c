/* Execute the actual generated writer, scheduler and cleanup callbacks against
   deterministic canonical acknowledgements. ASan/UBSan check caller buffers;
   resource counters independently distinguish consumed and recovered owners.
   This native test does not qualify actual Wasm canonical writes. */
#include "perry_adapter.c"
#include <assert.h>
#include <stdio.h>

static unsigned adopted[4], dropped[4], sent_handles[4];
static unsigned staged, commits, writes, cancels, finishes, io_frames, parent_frames;
static unsigned writer_drops, reader_drops, cases, length, sent, expected_flags;
static unsigned expected_remaining, factory_type, action;
static uint32_t joined;
static bool is_stream, observed, immediate, acknowledged, committed, failed, reject_commit;
static plugin_waitable_state_t terminal;
static const test_writer_custody_api_payload_t *storage;
static void *const io_frame = (void *)(uintptr_t)1;
static void *const parent_frame = (void *)(uintptr_t)2;
static void *const scratch_frame = (void *)(uintptr_t)3;

static plugin_waitable_status_t acknowledge(void) {
    assert(committed && !acknowledged && io_frames == 0);
    for (unsigned i = 0; i < length; ++i) {
        assert(storage[i].item.__handle == i + 1);
        assert(storage[i].text.len == 4 && memcmp(storage[i].text.ptr, "pear", 4) == 0);
        assert(dropped[i + 1] == 0 && adopted[i + 1] == 0);
        if (i < sent) ++sent_handles[i + 1];
    }
    acknowledged = true;
    return (is_stream ? sent << 4 : 0) | terminal;
}
static uint32_t create(uint32_t *writer) { *writer = 42; return 43; }
$FUTURE_HELPER_t $FUTURE_HELPER_new($FUTURE_HELPER_writer_t *writer) { return create(writer); }
$STREAM_HELPER_t $STREAM_HELPER_new($STREAM_HELPER_writer_t *writer) { return create(writer); }
static plugin_waitable_status_t write_items(uint32_t writer, const test_writer_custody_api_payload_t *items, size_t count) {
    assert(writer == 42 && count == length && committed && !acknowledged);
    assert(++writes == 1);
    storage = items;
    return immediate ? acknowledge() : PLUGIN_WAITABLE_STATUS_BLOCKED;
}
plugin_waitable_status_t $FUTURE_HELPER_write($FUTURE_HELPER_writer_t writer, const test_writer_custody_api_payload_t *items) { return write_items(writer, items, 1); }
plugin_waitable_status_t $STREAM_HELPER_write($STREAM_HELPER_writer_t writer, const test_writer_custody_api_payload_t *items, size_t count) { return write_items(writer, items, count); }
static plugin_waitable_status_t cancel(uint32_t writer) {
    assert(writer == 42 && joined == 0 && !acknowledged);
    assert(perry_tasks->io->cancelled && !perry_tasks->io->complete);
    assert(++cancels == 1);
    return acknowledge();
}
plugin_waitable_status_t $FUTURE_HELPER_cancel_write($FUTURE_HELPER_writer_t writer) { return cancel(writer); }
plugin_waitable_status_t $STREAM_HELPER_cancel_write($STREAM_HELPER_writer_t writer) { return cancel(writer); }
static void drop_writer(uint32_t writer) {
    assert(writer == 42 && perry_writers == NULL);
    assert(++writer_drops == 1);
}
void $FUTURE_HELPER_drop_writable($FUTURE_HELPER_writer_t writer) { drop_writer(writer); }
void $STREAM_HELPER_drop_writable($STREAM_HELPER_writer_t writer) { drop_writer(writer); }
static void drop_reader(uint32_t reader) { assert(reader == 43); assert(++reader_drops == 1); }
void $FUTURE_HELPER_drop_readable($FUTURE_HELPER_t reader) { drop_reader(reader); }
void $STREAM_HELPER_drop_readable($STREAM_HELPER_t reader) { drop_reader(reader); }
void test_writer_custody_api_item_drop_own(test_writer_custody_api_own_item_t item) {
    assert(acknowledged && committed && io_frames == 0);
    assert(item.__handle > 0 && item.__handle <= length);
    assert(sent_handles[item.__handle] == 0 && adopted[item.__handle] == 0);
    assert(++dropped[item.__handle] == 1);
}
void *perry_component_frame_new(void) { return scratch_frame; }
void perry_component_frame_drop(void *frame) {
    if (frame == scratch_frame) return;
    assert(acknowledged || reject_commit);
    if (frame == io_frame) assert(++io_frames == 1);
    else { assert(frame == parent_frame); assert(++parent_frames == 1); }
}
int32_t perry_component_frame_failed(void *frame) { (void)frame; return failed; }
void perry_component_frame_fail(void *frame, const uint8_t *text, size_t len) {
    (void)frame; (void)text; (void)len; failed = true;
}
uint32_t perry_component_frame_endpoint_pair(void *frame, uint32_t type, uint32_t reader, uint32_t writer) {
    assert(frame == scratch_frame && type == factory_type && reader == 43 && writer == 42); return 1;
}
int32_t perry_component_frame_endpoint_write_commit(void *frame) {
    assert(frame == io_frame && staged == length && !committed && ++commits == 1);
    if (reject_commit) { failed = true; return -1; }
    committed = true;
    if (action == 5) acknowledged = true; /* No canonical I/O started. */
    return 0;
}
int32_t perry_component_frame_endpoint_status(void *frame) { assert(frame == io_frame); return action != 6; }
int32_t perry_component_frame_endpoint_prepare(void *frame) {
    assert(frame == io_frame && acknowledged && committed);
    return observed && !perry_tasks->cancelled && action != 6;
}
int32_t perry_component_frame_endpoint_finish(void *frame, uint32_t value, uint32_t flags) {
    assert(frame == io_frame && acknowledged && io_frames == 0 && flags == expected_flags);
    if (!(flags & 2)) assert(value == 4000);
    assert(++finishes == 1); return 0;
}
uint32_t perry_component_frame_array_len(void *frame, uint32_t slot) {
    assert(frame == io_frame && slot == 11 && !committed); return length;
}
uint32_t perry_component_frame_array_get(void *frame, uint32_t slot, uint32_t index) {
    assert(frame == io_frame && slot == 11 && index < length && !committed); return 100 + index;
}
uint32_t perry_component_frame_get(void *frame, uint32_t slot, const uint8_t *key, size_t len) {
    assert(frame == io_frame && !committed);
    unsigned index = is_stream ? slot - 100 : 0;
    assert(index < length && (is_stream || slot == 11));
    if (len == 4 && memcmp(key, "item", 4) == 0) return 200 + index;
    assert(len == 4 && memcmp(key, "text", 4) == 0); return 300 + index;
}
uint32_t perry_component_frame_resource_lower(void *frame, uint32_t slot, uint32_t type, int32_t own, int32_t local) {
    (void)type;
    assert(frame == io_frame && !committed && own && !local && slot >= 200 && slot < 200 + length);
    ++staged; return slot - 200 + 1;
}
size_t perry_component_frame_string_len(void *frame, uint32_t slot) {
    assert(frame == io_frame && !committed && slot >= 300 && slot < 300 + length); return 4;
}
void perry_component_frame_string_copy(void *frame, uint32_t slot, uint8_t *bytes, size_t len) {
    assert(perry_component_frame_string_len(frame, slot) == len); memcpy(bytes, "pear", len);
}
uint32_t perry_component_frame_resource_lift(void *frame, uint32_t type, uint32_t handle, uint32_t rep, int32_t own) {
    (void)type; (void)rep;
    assert(frame == io_frame && acknowledged && observed && own && handle > sent && handle <= length);
    assert(sent_handles[handle] == 0 && dropped[handle] == 0 && ++adopted[handle] == 1);
    return 5000 + handle;
}
uint32_t perry_component_frame_string(void *frame, const uint8_t *text, size_t len) {
    assert(frame == io_frame && observed && acknowledged && len == 4 && memcmp(text, "pear", 4) == 0); return 6000;
}
uint32_t perry_component_frame_object(void *frame) { assert(frame == io_frame); return 4000; }
uint32_t perry_component_frame_number(void *frame, double value) { assert(frame == io_frame && value == sent); return 1000 + sent; }
uint32_t perry_component_frame_bool(void *frame, int32_t value) { assert(frame == io_frame); return 2000 + value; }
uint32_t perry_component_frame_array(void *frame, uint32_t len) {
    assert(frame == io_frame && len == expected_remaining); return 3000 + len;
}
void perry_component_frame_array_set(void *frame, uint32_t array, uint32_t index, uint32_t value) {
    assert(frame == io_frame && array == 3000 + expected_remaining && index < expected_remaining && value == 4000);
}
void perry_component_frame_set(void *frame, uint32_t object, const uint8_t *key, size_t len, uint32_t value) {
    assert(frame == io_frame && object == 4000);
    if (len == 7 && memcmp(key, "written", 7) == 0) assert(value == 1000 + sent);
    else if (len == 9 && memcmp(key, "remaining", 9) == 0) assert(value == 3000 + expected_remaining);
}
int32_t perry_component_frame_endpoint_pop(void *frame, uint32_t *type, uint32_t *handle) {
    (void)type; (void)handle; assert(frame == scratch_frame); return 0;
}
int32_t perry_component_frame_task_resume(void *frame, void (*callback)(void *), void *data) {
    assert(frame == parent_frame && acknowledged); callback(data); return 0;
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

static void cleanup(void) {
    failed = false;
    if (is_stream) $STREAM_HELPER_drop_readable(43); else $FUTURE_HELPER_drop_readable(43);
    perry_producer_reader_dropped(factory_type, 43);
    perry_producer_notify(factory_type, 42, 0, 1);
    perry_producer_flush();
    assert(perry_writers == NULL && writer_drops == 1 && reader_drops == 1);
}
static void reset(bool stream) {
    assert(perry_tasks == NULL && perry_writers == NULL);
    memset(adopted, 0, sizeof(adopted)); memset(dropped, 0, sizeof(dropped)); memset(sent_handles, 0, sizeof(sent_handles));
    staged = commits = writes = cancels = finishes = io_frames = parent_frames = writer_drops = reader_drops = joined = 0;
    acknowledged = committed = failed = reject_commit = false;
    storage = NULL; is_stream = stream; length = stream ? 3 : 1;
    factory_type = stream ? $STREAM_TYPE : $FUTURE_TYPE;
}
/* control: 0 completion, 1 cancellation, 2 close, 4 task cancellation,
   5 cancellation before canonical start, 6 abandoned observer sweep. */
static void check(bool stream, bool inline_write, plugin_waitable_state_t state, unsigned count, bool observer, unsigned control) {
    reset(stream); action = control; sent = count; terminal = state; immediate = inline_write; observed = observer;
    bool ended = state == PLUGIN_WAITABLE_DROPPED || (!stream && count == 1);
    bool live_observer = observer && control != 4 && control != 6;
    expected_flags = ((ended || control == 2) ? 1 : 0) | (live_observer ? 0 : 2);
    expected_remaining = control == 2 ? 0 : length - sent;
    assert(perry_producer_create(scratch_frame, factory_type) == 1);
    perry_task *task = calloc(1, sizeof(*task)); assert(task);
    task->frame = parent_frame; task->asynchronous = task->waiting = true;
    perry_tasks = perry_active_task = task;
    if (control == 5) perry_producer_notify(factory_type, 42, 7, 0);
    assert(perry_producer_write(io_frame, factory_type, 42, 7, 11) == 0);
    perry_active_task = NULL;
    assert(committed && io_frames == 0);
    if (inline_write || control == 5) {
        assert(finishes == 1 && task->io->complete && joined == 0);
        perry_task_reap(task);
    } else {
        assert(finishes == 0 && joined == 71 && task->io->joined);
        if (control == 0) {
            plugin_event_t event = { .event = stream ? PLUGIN_EVENT_STREAM_WRITE : PLUGIN_EVENT_FUTURE_WRITE, .waitable = 42, .code = acknowledge() };
            perry_task_event(task, &event);
        } else if (control == 1 || control == 2) {
            perry_producer_notify(factory_type, 42, control == 1 ? 7 : 0, control == 1 ? 0 : 1);
            perry_producer_flush();
        } else if (control == 4) assert(perry_task_cancel(task) == PLUGIN_CALLBACK_CODE_EXIT);
        else { assert(control == 6); perry_io_sweep(task); }
    }
    assert(finishes == 1 && io_frames == 1 && joined == 0);
    assert(writes == (control != 5) && cancels == (control != 0 && control != 5));
    for (unsigned i = 1; i <= length; ++i) {
        bool recovered = live_observer && control != 2 && i > sent;
        assert(sent_handles[i] == (i <= sent));
        assert(adopted[i] == recovered);
        assert(dropped[i] == (i > sent && !recovered));
    }
    if (control != 4) {
        assert(task->io == NULL && parent_frames == 0);
        if (task->has_set) plugin_waitable_set_drop(task->set);
        perry_tasks = NULL; perry_component_frame_drop(task->frame); free(task);
    }
    assert(perry_tasks == NULL && parent_frames == 1);
    cleanup(); ++cases;
}
static void refused_commit(void) {
    reset(true); action = 0; reject_commit = true;
    assert(perry_producer_create(scratch_frame, factory_type) == 1);
    perry_task task = { .asynchronous = true };
    perry_tasks = perry_active_task = &task;
    assert(perry_producer_write(io_frame, factory_type, 42, 7, 11) == -1);
    assert(commits == 1 && staged == 3 && !committed && writes == 0 && io_frames == 0 && task.io == NULL);
    /* Rejected handoff leaves the frame and all staged owners with Rust. */
    perry_component_frame_drop(io_frame);
    perry_tasks = perry_active_task = NULL;
    cleanup(); ++cases;
}
int main(int argc, char **argv) {
    if (argc == 2) {
        assert(strcmp(argv[1], "--unfinished-future") == 0);
        reset(false);
        assert(perry_producer_create(scratch_frame, factory_type) == 1);
        perry_producer_notify(factory_type, 42, 0, 1);
        perry_producer_flush(); /* Must trap before illegal drop-writable. */
        assert(0);
    }
    for (unsigned stream = 0; stream < 2; ++stream) {
        for (unsigned observer = 0; observer < 2; ++observer) {
            check(stream, true, PLUGIN_WAITABLE_COMPLETED, 1, observer, 0);
            check(stream, false, PLUGIN_WAITABLE_COMPLETED, 1, observer, 0);
            check(stream, false, PLUGIN_WAITABLE_DROPPED, 0, observer, 0);
            check(stream, false, PLUGIN_WAITABLE_CANCELLED, 0, observer, 1);
            check(stream, false, PLUGIN_WAITABLE_COMPLETED, 1, observer, 1);
            check(stream, false, PLUGIN_WAITABLE_CANCELLED, stream ? 1 : 0, observer, 4);
        }
    }
    check(true, false, PLUGIN_WAITABLE_CANCELLED, 1, true, 2);
    check(true, false, PLUGIN_WAITABLE_CANCELLED, 1, false, 2);
    check(true, true, PLUGIN_WAITABLE_CANCELLED, 0, true, 5);
    check(true, false, PLUGIN_WAITABLE_CANCELLED, 2, false, 6);
    refused_commit();
    assert(cases == 29);
    puts("PASS: 29 guest write custody cases");
    return 0;
}
