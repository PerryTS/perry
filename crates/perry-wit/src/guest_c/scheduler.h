/* Private generated async-export scheduler. No JS addresses live in C. */
typedef struct perry_task {
    void *frame;
    uint32_t promise;
    struct perry_task *next;
    $W_waitable_set_t set;
    $W_stream_void_t reader;
    $W_stream_void_writer_t writer;
    bool waiting, armed;
} perry_task;
static perry_task *perry_tasks;

/* A closed, extra export makes the maintained backend emit unit-stream
   helpers. It is absent from Perry's public contract and registration indices. */
$W_stream_void_t exports_$W_$ANCHOR(void) {
    $W_stream_void_writer_t writer;
    $W_stream_void_t reader = $W_stream_void_new(&writer);
    $W_stream_void_drop_writable(writer);
    return reader;
}
static void perry_require(bool ok) { if (!ok) __builtin_trap(); }
static perry_task *perry_task_new(void) {
    void *init = perry_component_frame_new();
    perry_initialize(init);
    bool failed = perry_component_frame_failed(init);
    perry_component_frame_drop(init);
    perry_require(!failed);
    perry_task *t = calloc(1, sizeof(*t));
    perry_require(t != NULL);
    t->frame = perry_component_frame_task_new();
    perry_require(t->frame != NULL);
    t->next = perry_tasks; perry_tasks = t;
    return t;
}
static void perry_wake_all(void) {
    for (perry_task *t = perry_tasks; t; t = t->next) {
        if (!t->armed) continue;
        int32_t ready = perry_component_frame_task_ready(t->frame);
        perry_require(ready >= 0);
        if (!ready) continue;
        t->armed = false;
        $W_waitable_status_t status = $W_stream_void_write(t->writer, 1);
        perry_require(status != $U_WAITABLE_STATUS_BLOCKED);
        perry_require($U_WAITABLE_STATE(status) == $U_WAITABLE_COMPLETED);
        perry_require($U_WAITABLE_COUNT(status) == 1);
    }
}
static void perry_task_drop(perry_task *t) {
    if (t->waiting) {
        if (t->armed) {
            $W_waitable_status_t status = $W_stream_void_cancel_read(t->reader);
            perry_require(status != $U_WAITABLE_STATUS_BLOCKED);
        }
        $W_waitable_join(t->reader, 0);
        $W_stream_void_drop_readable(t->reader);
        $W_stream_void_drop_writable(t->writer);
        $W_waitable_set_drop(t->set);
    }
    perry_task **link = &perry_tasks;
    while (*link != t) { perry_require(*link != NULL); link = &(*link)->next; }
    *link = t->next;
    perry_component_frame_drop(t->frame);
    free(t);
    /* Jobs whose old lease ended are now adoptable by another live call. */
    perry_wake_all();
}
static void perry_task_event(perry_task *t, $W_event_t *event) {
    perry_require(t && t->waiting && !t->armed);
    /* Cooperative cancellation is deliberately unqualified. A trap retires
       the host store; silently adopting cancelled task jobs would be wrong. */
    perry_require(event->event == $U_EVENT_STREAM_READ);
    perry_require(event->waitable == t->reader);
    perry_require($U_WAITABLE_STATE(event->code) == $U_WAITABLE_COMPLETED);
    perry_require($U_WAITABLE_COUNT(event->code) == 1);
}
static int32_t perry_task_poll(perry_task *t, uint32_t *result) {
    /* Observe/mark the exported Promise handled before running jobs. An
       immediate rejection belongs to this call, not an unhandled-job hook. */
    int32_t status = perry_component_frame_promise_poll(t->frame, t->promise, result);
    perry_require(status >= 0);
    perry_require(perry_component_frame_task_ready(t->frame) >= 0);
    perry_require(perry_component_frame_task_checkpoint(t->frame) >= 0);
    if (!status) status = perry_component_frame_promise_poll(t->frame, t->promise, result);
    perry_require(status >= 0);
    return status;
}
static $W_callback_code_t perry_task_wait(perry_task *t) {
    if (!t->waiting) {
        t->set = $W_waitable_set_new();
        t->reader = $W_stream_void_new(&t->writer);
        $W_waitable_join(t->reader, t->set);
        t->waiting = true;
    }
    perry_require(!t->armed);
    perry_require($W_stream_void_read(t->reader, 1) == $U_WAITABLE_STATUS_BLOCKED);
    t->armed = true;
    $W_context_set_0(t);
    perry_wake_all();
    return $U_CALLBACK_CODE_WAIT(t->set);
}
