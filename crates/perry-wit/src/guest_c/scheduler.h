/* Private generated async scheduler. No JS addresses live in C. */
typedef struct perry_task perry_task;
typedef struct perry_io {
    void *frame;
    struct perry_io *next;
    uint32_t type, reader, operation;
    $W_event_code_t event;
    $W_waitable_status_t status;
    bool complete, cancelled, claimed, joined;
    void (*deliver)(void *);
    void (*cancel)(struct perry_io *);
    void (*destroy)(struct perry_io *);
} perry_io;
/* SDK libcall task hooks own canonical context 0 (stack/TLS). Ordinary C TLS
   follows the SDK task across WAIT/resume; the frame bridge separately selects
   the persistent JS agent only during finite Rust calls. P2 keeps its ABI. */
#ifdef __wasm_libcall_thread_context__
static _Thread_local void *perry_canonical_task;
static void *perry_task_context_get(void) { return perry_canonical_task; }
static void perry_task_context_set(void *task) { perry_canonical_task = task; }
#else
static void *perry_task_context_get(void) { return $W_context_get_0(); }
static void perry_task_context_set(void *task) { $W_context_set_0(task); }
#endif
typedef struct perry_child {
    void *frame;
    struct perry_child *next;
    $W_subtask_t handle;
    /* result_claimed means lifting has begun. A partial lift failure follows
       the runtime error/Store-retirement path; C must not drop those owners
       again after some have already been adopted by JS. */
    bool complete, params_consumed, result_ready, result_claimed;
    void (*finish)(void *);
    void (*destroy)(struct perry_child *);
} perry_child;
struct perry_task {
    void *frame;
    uint32_t promise;
    perry_task *next;
    perry_child *children;
    perry_io *io;
    $W_waitable_set_t set;
    $S_t reader;
    $S_writer_t writer;
    bool asynchronous, returned, cancelled, has_set, waiting, armed, read_pending;
};
static perry_task *perry_tasks;
/* Installed only around finite runtime calls, restored before canonical WAIT.
   Imports may attach children without reborrowing the active Rust task frame. */
static perry_task *perry_active_task;
static void perry_require(bool);
static int32_t perry_task_resume(perry_task *, void (*)(void *), void *);
static void perry_io_sweep(perry_task *);
static void perry_io_close(uint32_t, uint32_t);
static void perry_io_reap(perry_task *t) {
    perry_io **link = &t->io;
    while (*link) {
        perry_io *io = *link;
        if (!io->complete) { link = &io->next; continue; }
        perry_require(!io->joined);
        *link = io->next;
        io->destroy(io);
    }
}

/* A closed, extra export makes the maintained backend emit unit-stream
   helpers. It is absent from Perry's public contract and registration indices. */
$S_t exports_$W_$ANCHOR(void) {
    $S_writer_t writer;
    $S_t reader = $S_new(&writer);
    $S_drop_writable(writer);
    return reader;
}
static void perry_require(bool ok) { if (!ok) __builtin_trap(); }
static void perry_task_set(perry_task *t) {
    if (t->has_set) return;
    t->set = $W_waitable_set_new();
    t->has_set = true;
}
static void perry_task_reap(perry_task *t) {
    perry_io_reap(t);
    /* Runtime import-frame handoff has finished when a finite runtime call
       returns. Even an inline completion must wait for this boundary. */
    perry_child **link = &t->children;
    while (*link) {
        perry_child *child = *link;
        if (!child->complete) { link = &child->next; continue; }
        perry_require(child->handle == 0);
        *link = child->next;
        child->destroy(child);
    }
}
static uint32_t perry_task_call(perry_task *t, uint32_t interface, uint32_t function, uint32_t args) {
    perry_task *previous = perry_active_task;
    perry_active_task = t;
    uint32_t result = perry_component_frame_task_call(t->frame, interface, function, args);
    perry_active_task = previous;
    perry_task_reap(t);
    perry_endpoint_flush();
    return result;
}
static int32_t perry_task_checkpoint(perry_task *t) {
    perry_task *previous = perry_active_task;
    perry_active_task = t;
    int32_t status = perry_component_frame_task_checkpoint(t->frame);
    perry_active_task = previous;
    perry_task_reap(t);
    perry_endpoint_flush();
    return status;
}
static int32_t perry_task_resume(perry_task *t, void (*callback)(void *), void *data) {
    perry_task *previous = perry_active_task;
    perry_active_task = t;
    int32_t status = perry_component_frame_task_resume(t->frame, callback, data);
    perry_active_task = previous;
    perry_task_reap(t);
    perry_endpoint_flush();
    return status;
}
static void perry_io_cancel(perry_task *t, perry_io *io) {
    perry_require(!io->complete && !io->cancelled);
    io->cancel(io);
    if (t->cancelled) {
        perry_task *previous = perry_active_task;
        perry_active_task = t;
        io->deliver(io);
        perry_active_task = previous;
        perry_task_reap(t);
    } else {
        perry_require(perry_task_resume(t, io->deliver, io) >= 0);
    }
}
static void perry_io_sweep(perry_task *t) {
    for (;;) {
        perry_io *abandoned = NULL;
        for (perry_io *io = t->io; io; io = io->next) {
            if (io->complete || io->cancelled) continue;
            int32_t status = perry_component_frame_endpoint_status(io->frame);
            perry_require(status >= 0);
            if (status == 0) { abandoned = io; break; }
        }
        if (!abandoned) return;
        /* Resume/reap can free the node or attach more I/O. Rescan after it. */
        perry_io_cancel(t, abandoned);
    }
}
static void perry_io_close(uint32_t type, uint32_t reader) {
    for (perry_task *t = perry_tasks; t; t = t->next) {
        for (perry_io *io = t->io; io; io = io->next) {
            if (io->complete || io->cancelled || io->type != type || io->reader != reader ||
                (io->event != $U_EVENT_STREAM_READ && io->event != $U_EVENT_FUTURE_READ)) continue;
            perry_io_cancel(t, io);
            return;
        }
    }
}
static perry_task *perry_task_new(bool asynchronous) {
    void *init = perry_initialize();
    bool failed = perry_component_frame_failed(init);
    perry_component_frame_drop(init);
    perry_require(!failed);
    perry_task *t = calloc(1, sizeof(*t));
    perry_require(t != NULL);
    t->frame = perry_component_frame_task_new();
    perry_require(t->frame != NULL);
    t->asynchronous = asynchronous;
    t->next = perry_tasks; perry_tasks = t;
    return t;
}
static void perry_wake_all(void) {
    for (perry_task *t = perry_tasks; t; t = t->next) {
        if (!t->armed || t->cancelled) continue;
        int32_t ready = perry_component_frame_task_ready(t->frame);
        perry_require(ready >= 0);
        if (!ready) continue;
        t->armed = false;
        $W_waitable_status_t status = $S_write(t->writer, 1);
        perry_require(status != $U_WAITABLE_STATUS_BLOCKED);
        perry_require($U_WAITABLE_STATE(status) == $U_WAITABLE_COMPLETED);
        perry_require($U_WAITABLE_COUNT(status) == 1);
    }
}
static void perry_task_drop(perry_task *t) {
    perry_require(t->children == NULL && t->io == NULL);
    if (t->waiting) {
        /* Synchronous cancellation requires an unjoined waitable. Native
           children can finish while this signal read is still blocked. */
        $W_waitable_join(t->reader, 0);
        if (t->armed) {
            $W_waitable_status_t status = $S_cancel_read(t->reader);
            perry_require(status != $U_WAITABLE_STATUS_BLOCKED);
        }
        $S_drop_readable(t->reader);
        $S_drop_writable(t->writer);
    }
    if (t->has_set) $W_waitable_set_drop(t->set);
    perry_task **link = &perry_tasks;
    while (*link != t) { perry_require(*link != NULL); link = &(*link)->next; }
    *link = t->next;
    perry_component_frame_drop(t->frame);
    free(t);
    perry_endpoint_flush();
    /* Jobs whose old lease ended are now adoptable by another live call. */
    perry_wake_all();
}
static void perry_child_start(perry_task *t, perry_child *child, $W_subtask_status_t status) {
    $W_subtask_state_t state = $U_SUBTASK_STATE(status);
    child->handle = $U_SUBTASK_HANDLE(status);
    perry_require(state == $U_SUBTASK_STARTING || state == $U_SUBTASK_STARTED || state == $U_SUBTASK_RETURNED);
    child->params_consumed = state != $U_SUBTASK_STARTING;
    if (state == $U_SUBTASK_RETURNED) {
        if (child->handle) $W_subtask_drop(child->handle);
        child->handle = 0;
        child->complete = child->result_ready = true;
        /* Already inside the original task. Reentering its Rust frame here
           would alias an outstanding borrow. The import frame is distinct. */
        child->result_claimed = true;
        child->finish(child);
    } else {
        perry_require(child->handle != 0);
        perry_task_set(t);
        $W_waitable_join(child->handle, t->set);
    }
}
static void perry_child_complete(void *pointer) {
    perry_child *child = pointer;
    child->result_claimed = true;
    child->finish(child);
    /* A later conversion/settlement failure cannot unwind through C or leave
       an unresolved Promise alive. Inline completion uses the normal caller's
       JS error path instead; only resumed completions use this thunk. */
    perry_require(!perry_component_frame_failed(child->frame));
}
static void perry_task_event(perry_task *t, $W_event_t *event) {
    perry_require(t && t->waiting);
    if (event->event == $U_EVENT_STREAM_READ || event->event == $U_EVENT_FUTURE_READ ||
        event->event == $U_EVENT_STREAM_WRITE || event->event == $U_EVENT_FUTURE_WRITE) {
        for (perry_io *io = t->io; io; io = io->next) {
            if (io->complete || !io->joined || io->reader != event->waitable || io->event != event->event) continue;
            $W_waitable_join(io->reader, 0);
            io->joined = false;
            io->status = event->code;
            perry_require(perry_task_resume(t, io->deliver, io) >= 0);
            return;
        }
    }
    if (event->event == $U_EVENT_SUBTASK) {
        perry_child *child = t->children;
        while (child && child->handle != event->waitable) child = child->next;
        perry_require(child && !child->complete);
        if (event->code == $U_SUBTASK_STARTED) {
            child->params_consumed = true;
            return;
        }
        perry_require(event->code == $U_SUBTASK_RETURNED);
        child->params_consumed = true;
        $W_waitable_join(child->handle, 0);
        $W_subtask_drop(child->handle);
        child->handle = 0;
        child->complete = child->result_ready = true;
        perry_require(perry_task_resume(t, perry_child_complete, child) >= 0);
        return;
    }
    perry_require(t->read_pending && !t->armed && event->event == $U_EVENT_STREAM_READ);
    perry_require(event->waitable == t->reader);
    perry_require($U_WAITABLE_STATE(event->code) == $U_WAITABLE_COMPLETED);
    perry_require($U_WAITABLE_COUNT(event->code) == 1);
    t->read_pending = false;
}
static $W_callback_code_t perry_task_cancel(perry_task *t) {
    perry_require(t && t->waiting && !t->returned && !t->cancelled);
    t->cancelled = true;
    /* End every runtime scope before a synchronous canonical cancel can
       suspend or let another guest task enter this instance. */
    perry_require(perry_component_frame_task_cancel(t->frame) == 0);
    while (t->io) {
        perry_io_cancel(t, t->io);
    }
    for (perry_child *child = t->children; child; child = child->next) {
        perry_require(!child->complete && child->handle != 0);
        $W_waitable_join(child->handle, 0);
        $W_subtask_state_t state = $U_SUBTASK_STATE($W_subtask_cancel(child->handle));
        perry_require(state == $U_SUBTASK_RETURNED || state == $U_SUBTASK_STARTED_CANCELLED || state == $U_SUBTASK_RETURNED_CANCELLED);
        /* STARTED_CANCELLED acknowledges cancellation before parameter
           consumption. Other terminal states transferred the arguments,
           even if their STARTED notification has not been observed yet. */
        perry_require(!child->params_consumed || state != $U_SUBTASK_STARTED_CANCELLED);
        child->params_consumed = state != $U_SUBTASK_STARTED_CANCELLED;
        /* Cancellation may lose to a successful return. Only that status
           initializes the typed result and requires result deallocation. */
        child->result_ready = state == $U_SUBTASK_RETURNED;
        $W_subtask_drop(child->handle);
        child->handle = 0;
        child->complete = true;
    }
    perry_task_reap(t);
    perry_require(perry_component_frame_resource_scope_end(t->frame) == 0);
    $W_task_cancel();
    perry_task_context_set(NULL);
    perry_task_drop(t);
    return $U_CALLBACK_CODE_EXIT;
}
static int32_t perry_task_poll(perry_task *t, uint32_t *result) {
    /* Observe/mark the exported Promise handled before running jobs. An
       immediate rejection belongs to this call, not an unhandled-job hook. */
    int32_t status = perry_component_frame_promise_poll(t->frame, t->promise, result);
    perry_require(status >= 0);
    perry_require(perry_component_frame_task_ready(t->frame) >= 0);
    perry_require(perry_task_checkpoint(t) >= 0);
    if (!status) status = perry_component_frame_promise_poll(t->frame, t->promise, result);
    perry_require(status >= 0);
    return status;
}
static $W_callback_code_t perry_task_wait(perry_task *t) {
    perry_task_set(t);
    if (!t->waiting) {
        t->reader = $S_new(&t->writer);
        $W_waitable_join(t->reader, t->set);
        t->waiting = true;
    }
    /* A subtask event can arrive while the signal read is still armed, or
       while its completed event is queued. Preserve either read until its
       own event is consumed; only that event permits a new read. */
    if (!t->read_pending) {
        perry_require($S_read(t->reader, 1) == $U_WAITABLE_STATUS_BLOCKED);
        t->read_pending = t->armed = true;
    }
    perry_task_context_set(t);
    perry_wake_all();
    return $U_CALLBACK_CODE_WAIT(t->set);
}
