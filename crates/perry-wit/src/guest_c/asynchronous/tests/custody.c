/* Execute the generated scheduler and resource destructors with deterministic
   native acknowledgements. Canonical I/O and JS lifting are explicit mocks;
   this is not a Wasmtime/component-model qualification. */
#include "perry_adapter.c"
#include <assert.h>
#include <stdio.h>

static unsigned dropped[128], adopted[128], child_frames, parent_frames;
static unsigned finished, cancelled, freed_results, begin_calls, native_calls;
static uint32_t joined;
static plugin_subtask_state_t cancel_state;
static bool acknowledged, lifting_failed;
static void *const child_frame = (void *)(uintptr_t)1;
static void *const parent_frame = (void *)(uintptr_t)2;

void test_custody_api_item_drop_own(test_custody_api_own_item_t item) {
    assert(acknowledged);
    assert(child_frames == 0); /* Handle cleanup precedes frame/loan retirement. */
    assert(item.__handle > 0 && item.__handle < 128);
    assert(item.__handle != 99); /* The borrowed handle is never an owner. */
    assert(++dropped[item.__handle] == 1 && adopted[item.__handle] == 0);
}
/* The maintained backend's buffer helpers intentionally do not drop handles.
   This mock records their call and frees exactly the same active allocations. */
void $RESULT_FREE(__typeof__(((perry_import_0_0_state *)0)->result) *result) {
    assert(acknowledged && child_frames == 0);
    ++freed_results;
    if (!result->is_err) {
        if (result->val.ok.tag == 1 && result->val.ok.val.held.f1.is_some)
            free(result->val.ok.val.held.f1.val.ptr);
        if (result->val.ok.tag == 2) free(result->val.ok.val.text.ptr);
    }
}
int32_t perry_component_frame_failed(void *f) { (void)f; return lifting_failed; }
void perry_component_frame_fail(void *f, const uint8_t *s, size_t n) {
    (void)f; (void)s; (void)n; assert(0);
}
void perry_component_frame_drop(void *f) {
    assert(acknowledged);
    if (f == child_frame) ++child_frames;
    else { assert(f == parent_frame); ++parent_frames; }
}
int32_t perry_component_frame_task_cancel(void *f) {
    assert(f == parent_frame && !acknowledged); ++cancelled; return 0;
}
int32_t perry_component_frame_task_ready(void *f) { (void)f; return 0; }
int32_t perry_component_frame_resource_scope_end(void *f) {
    assert(f == parent_frame && acknowledged && child_frames == 1); return 0;
}
int32_t perry_component_frame_task_resume(void *f, void (*cb)(void *), void *data) {
    assert(f == parent_frame && acknowledged); cb(data); return 0;
}
void plugin_waitable_join(uint32_t handle, plugin_waitable_set_t set) {
    assert(handle == 42 || handle == 0); joined = set;
}
plugin_subtask_status_t plugin_subtask_cancel(plugin_subtask_t handle) {
    assert(handle == 42 && joined == 0 && cancelled == 1);
    assert(!acknowledged && child_frames == 0 && freed_results == 0);
    for (unsigned i = 0; i < 128; ++i) assert(dropped[i] == 0);
    acknowledged = true; return cancel_state;
}
void plugin_subtask_drop(plugin_subtask_t h) { assert(h == 42 && acknowledged); }
plugin_waitable_set_t plugin_waitable_set_new(void) { return 71; }
void plugin_waitable_set_drop(plugin_waitable_set_t set) { assert(set == 71); }
plugin_waitable_status_t plugin_stream_void_cancel_read(plugin_stream_void_t h) {
    (void)h; assert(0); return 0;
}
void plugin_stream_void_drop_readable(plugin_stream_void_t h) { assert(h == 0); }
void plugin_stream_void_drop_writable(plugin_stream_void_writer_t h) { assert(h == 0); }
plugin_waitable_status_t plugin_stream_void_write(plugin_stream_void_writer_t h, size_t n) {
    (void)h; (void)n; assert(0); return 0;
}
void plugin_task_cancel(void) { assert(acknowledged && child_frames == 1); }
void plugin_context_set_0(void *p) { assert(p == NULL); }

/* The rejected-before-begin case executes the real simple import adapter.
   Its staged handle still belongs to JS, so neither native I/O nor drop runs. */
uint32_t perry_component_frame_array_get(void *f, uint32_t a, uint32_t n) {
    (void)f; (void)a; assert(n == 0); return 11;
}
uint32_t perry_component_frame_resource_lower(void *f, uint32_t s, uint32_t t, int32_t own, int32_t local) {
    (void)f; (void)t; assert(s == 11 && own && !local); return 11;
}
uint32_t perry_component_frame_import_begin(void *f) {
    assert(f == child_frame); ++begin_calls; return 0;
}
plugin_subtask_status_t test_custody_api_one(test_custody_api_own_item_t v, test_custody_api_own_item_t *r) {
    (void)v; (void)r; ++native_calls; assert(0); return 0;
}
uint32_t perry_component_frame_resource_lift(void *f, uint32_t t, uint32_t h, uint32_t r, int32_t own) {
    (void)t; (void)r; assert(f == child_frame && h == 12 && own && lifting_failed); return 0;
}
int32_t perry_component_frame_import_resolve(void *f, uint32_t s) {
    (void)f; (void)s; assert(0); return -1;
}

static void claim_result(void *p) {
    perry_import_0_0_state *data = p;
    assert(acknowledged && data->base.result_ready && data->base.result_claimed);
    assert(data->base.params_consumed && child_frames == 0);
    ++finished;
    if (data->result.is_err) ++adopted[10];
    else if (data->result.val.ok.tag == 1) {
        ++adopted[7];
        if (data->result.val.ok.val.held.f1.is_some) {
            ++adopted[8]; ++adopted[9];
        }
    }
}
static perry_task *new_parent(void) {
    assert(perry_tasks == NULL);
    memset(dropped, 0, sizeof(dropped)); memset(adopted, 0, sizeof(adopted));
    child_frames = parent_frames = finished = cancelled = freed_results = 0;
    begin_calls = native_calls = joined = 0;
    acknowledged = lifting_failed = false;
    perry_task *t = calloc(1, sizeof(*t)); assert(t);
    t->frame = parent_frame; t->asynchronous = true; t->waiting = true;
    perry_tasks = t; return t;
}
static perry_import_0_0_state *new_child(perry_task *t) {
    perry_import_0_0_state *data = calloc(1, sizeof(*data)); assert(data);
    data->base.frame = child_frame; data->base.destroy = perry_import_0_0_destroy;
    data->base.finish = claim_result; t->children = &data->base;
    data->args.value.owners.len = 2;
    data->args.value.owners.ptr = perry_allocate(child_frame, &data->arena, 2, sizeof(*data->args.value.owners.ptr));
    for (unsigned i = 0; i < 2; ++i) {
        data->args.value.owners.ptr[i].first.__handle = 1 + 2*i;
        data->args.value.owners.ptr[i].second.__handle = 2 + 2*i;
        data->args.value.owners.ptr[i].label.len = 3;
        data->args.value.owners.ptr[i].label.ptr = perry_allocate(child_frame, &data->arena, 3, 1);
    }
    data->args.value.borrowed.__handle = 99;
    data->args.value.fallback.is_some = true;
    data->args.value.fallback.val.__handle = 5;
    data->args.other.__handle = 6;
    /* Deliberately invalid until successful return. Cancellation must never
       traverse the uninitialized result, even though it contains owners. */
    memset(&data->result, 0xa5, sizeof(data->result));
    return data;
}
static unsigned prepare_result(perry_import_0_0_state *data, unsigned which) {
    memset(&data->result, 0, sizeof(data->result));
    if (which == 3) {
        data->result.is_err = true; data->result.val.err.__handle = 10; return 1;
    }
    data->result.val.ok.tag = which == 4 ? 1 : which;
    if (which == 1 || which == 4) {
        data->result.val.ok.val.held.f0.__handle = 7;
        data->result.val.ok.val.held.f1.is_some = which == 1;
        if (which == 1) {
            data->result.val.ok.val.held.f1.val.len = 2;
            data->result.val.ok.val.held.f1.val.ptr = calloc(2, sizeof(test_custody_api_own_item_t));
            data->result.val.ok.val.held.f1.val.ptr[0].__handle = 8;
            data->result.val.ok.val.held.f1.val.ptr[1].__handle = 9;
        } else {
            /* An inactive option payload must never be read. */
            data->result.val.ok.val.held.f1.val.ptr = (void *)(uintptr_t)1;
            data->result.val.ok.val.held.f1.val.len = SIZE_MAX;
        }
        return which == 1 ? 3 : 1;
    }
    if (which == 2) {
        data->result.val.ok.val.text.ptr = malloc(4);
        data->result.val.ok.val.text.len = 4;
    }
    return 0;
}
static void check_counts(unsigned args, unsigned results, unsigned claims, unsigned buffers) {
    unsigned a = 0, r = 0, c = 0;
    for (unsigned i = 1; i <= 6; ++i) a += dropped[i];
    for (unsigned i = 7; i <= 10; ++i) { r += dropped[i]; c += adopted[i]; }
    assert(a == args && r == results && c == claims && dropped[99] == 0);
    assert(child_frames == 1 && freed_results == buffers);
}
static void cancellation_case(plugin_subtask_state_t initial, plugin_subtask_state_t terminal, bool started_event, unsigned result) {
    perry_task *t = new_parent();
    perry_import_0_0_state *data = new_child(t);
    perry_child_start(t, &data->base, (42 << 4) | initial);
    if (started_event) {
        plugin_event_t event = { .event = PLUGIN_EVENT_SUBTASK, .waitable = 42, .code = PLUGIN_SUBTASK_STARTED };
        perry_task_event(t, &event);
    }
    unsigned owners = terminal == PLUGIN_SUBTASK_RETURNED ? prepare_result(data, result) : 0;
    cancel_state = terminal;
    assert(perry_task_cancel(t) == PLUGIN_CALLBACK_CODE_EXIT);
    check_counts(terminal == PLUGIN_SUBTASK_STARTED_CANCELLED ? 6 : 0, owners, 0, terminal == PLUGIN_SUBTASK_RETURNED);
    assert(finished == 0 && parent_frames == 1 && perry_tasks == NULL);
}
static void success_case(bool immediate, unsigned result) {
    perry_task *t = new_parent();
    perry_import_0_0_state *data = new_child(t);
    unsigned owners = prepare_result(data, result);
    acknowledged = immediate;
    perry_child_start(t, &data->base, (42 << 4) | (immediate ? PLUGIN_SUBTASK_RETURNED : PLUGIN_SUBTASK_STARTING));
    if (immediate) {
        assert(child_frames == 0 && finished == 1 && t->children != NULL);
        perry_task_reap(t); /* Only after the original runtime call returns. */
    } else {
        acknowledged = true;
        plugin_event_t event = { .event = PLUGIN_EVENT_SUBTASK, .waitable = 42, .code = PLUGIN_SUBTASK_RETURNED };
        perry_task_event(t, &event);
    }
    check_counts(0, 0, owners, 1);
    assert(finished == 1 && t->children == NULL);
    perry_task_drop(t); assert(parent_frames == 1 && perry_tasks == NULL);
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--failed-owned-lift")) {
        lifting_failed = true;
        perry_import_0_1_state data = {0};
        data.base.frame = child_frame; data.result.__handle = 12;
        perry_import_0_1_finish(&data);
        return 3; /* A failed lift must trap before Promise settlement. */
    }
    cancellation_case(PLUGIN_SUBTASK_STARTING, PLUGIN_SUBTASK_STARTED_CANCELLED, false, 0);
    cancellation_case(PLUGIN_SUBTASK_STARTED, PLUGIN_SUBTASK_RETURNED_CANCELLED, false, 0);
    cancellation_case(PLUGIN_SUBTASK_STARTING, PLUGIN_SUBTASK_RETURNED_CANCELLED, true, 0);
    cancellation_case(PLUGIN_SUBTASK_STARTING, PLUGIN_SUBTASK_RETURNED_CANCELLED, false, 0);
    for (unsigned result = 0; result <= 4; ++result)
        cancellation_case(PLUGIN_SUBTASK_STARTING, PLUGIN_SUBTASK_RETURNED, false, result);
    success_case(true, 1); success_case(false, 3); success_case(false, 4);
    for (unsigned failed = 0; failed < 2; ++failed) {
        perry_task *t = new_parent(); perry_active_task = t; lifting_failed = failed;
        assert(perry_import_0_1(child_frame, 0) == 0);
        perry_active_task = NULL; lifting_failed = false;
        assert(begin_calls == 1 && native_calls == 0 && t->children == NULL && child_frames == 0);
        for (unsigned i = 0; i < 128; ++i) assert(dropped[i] == 0);
        acknowledged = true; perry_task_drop(t);
    }
    puts("PASS: 14 canonical child ownership cases");
}
