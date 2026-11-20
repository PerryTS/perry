// Canonical ABI proof fixture, not Perry's production task scheduler.
#include "scheduler_wake.h"
#include <stdbool.h>

#define CHECK(condition) do { if (!(condition)) __builtin_trap(); } while (0)
#define TASKS 16

typedef struct {
    bool busy, armed, pending;
    uint32_t id;
    scheduler_wake_stream_void_t reader;
    scheduler_wake_stream_void_writer_t writer;
    scheduler_wake_waitable_set_t set;
} task;
static task tasks[TASKS];

scheduler_wake_stream_void_t exports_scheduler_wake_signal_type(void) {
    scheduler_wake_stream_void_writer_t writer;
    scheduler_wake_stream_void_t reader = scheduler_wake_stream_void_new(&writer);
    scheduler_wake_stream_void_drop_writable(writer);
    return reader;
}

scheduler_wake_callback_code_t exports_scheduler_wake_wait(uint32_t id) {
    CHECK(id < TASKS);
    task *t = &tasks[id];
    CHECK(!t->busy);
    if (t->pending) {
        t->pending = false;
        exports_scheduler_wake_wait_return(id + 100);
        return SCHEDULER_WAKE_CALLBACK_CODE_EXIT;
    }
    t->id = id;
    t->busy = true;
    t->set = scheduler_wake_waitable_set_new();
    t->reader = scheduler_wake_stream_void_new(&t->writer);
    scheduler_wake_waitable_join(t->reader, t->set);
    CHECK(scheduler_wake_stream_void_read(t->reader, 1) == SCHEDULER_WAKE_WAITABLE_STATUS_BLOCKED);
    t->armed = true;
    scheduler_wake_context_set_0(t);
    return SCHEDULER_WAKE_CALLBACK_CODE_WAIT(t->set);
}

void exports_scheduler_wake_wake(uint32_t id) {
    CHECK(id < TASKS);
    task *t = &tasks[id];
    if (!t->busy) { t->pending = true; return; }
    if (!t->armed) return; // Coalesce wakeups until the read event is consumed.
    t->armed = false;
    scheduler_wake_waitable_status_t status = scheduler_wake_stream_void_write(t->writer, 1);
    CHECK(status != SCHEDULER_WAKE_WAITABLE_STATUS_BLOCKED);
    CHECK(SCHEDULER_WAKE_WAITABLE_STATE(status) == SCHEDULER_WAKE_WAITABLE_COMPLETED);
    CHECK(SCHEDULER_WAKE_WAITABLE_COUNT(status) == 1);
}

void exports_scheduler_wake_wake_twice(uint32_t id) {
    exports_scheduler_wake_wake(id);
    exports_scheduler_wake_wake(id);
}

scheduler_wake_callback_code_t exports_scheduler_wake_wait_callback(scheduler_wake_event_t *event) {
    task *t = scheduler_wake_context_get_0();
    CHECK(t && t->busy);
    if (event->event == SCHEDULER_WAKE_EVENT_CANCEL) {
        scheduler_wake_waitable_status_t status = scheduler_wake_stream_void_cancel_read(t->reader);
        CHECK(status != SCHEDULER_WAKE_WAITABLE_STATUS_BLOCKED);
        scheduler_wake_task_cancel();
    } else {
        CHECK(event->event == SCHEDULER_WAKE_EVENT_STREAM_READ);
        CHECK(event->waitable == t->reader);
        CHECK(SCHEDULER_WAKE_WAITABLE_STATE(event->code) == SCHEDULER_WAKE_WAITABLE_COMPLETED);
        CHECK(SCHEDULER_WAKE_WAITABLE_COUNT(event->code) == 1);
        exports_scheduler_wake_wait_return(t->id + 100);
    }
    scheduler_wake_waitable_join(t->reader, 0);
    scheduler_wake_stream_void_drop_readable(t->reader);
    scheduler_wake_stream_void_drop_writable(t->writer);
    scheduler_wake_waitable_set_drop(t->set);
    t->busy = t->armed = false;
    scheduler_wake_context_set_0(NULL);
    return SCHEDULER_WAKE_CALLBACK_CODE_EXIT;
}
