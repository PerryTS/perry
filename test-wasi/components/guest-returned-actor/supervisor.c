/* The private actor returns inline without a handle. Its native child stays
   pending until completion or cancellation of this caller retires the Store. */
#include "supervisor.h"
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) __builtin_trap(); } while (0)
typedef struct {
    supervisor_subtask_t gate;
    supervisor_waitable_set_t set;
    uint32_t acknowledgement;
    bool retiring;
} state;
static state task;

static supervisor_callback_code_t after_gate(void) {
    CHECK(!task.gate && task.acknowledgement == 1);
    supervisor_mark(3);
    if (supervisor_cancel_mode()) {
        /* Keep the caller cancellable at a second real native gate. There is
           no actor handle on which to send canonical subtask.cancel. */
        task.retiring = true;
        supervisor_subtask_status_t status = supervisor_proceed(&task.acknowledgement);
        task.gate = SUPERVISOR_SUBTASK_HANDLE(status);
        supervisor_subtask_state_t code = SUPERVISOR_SUBTASK_STATE(status);
        CHECK(task.gate && (code == SUPERVISOR_SUBTASK_STARTING || code == SUPERVISOR_SUBTASK_STARTED));
        supervisor_waitable_join(task.gate, task.set);
        return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    }
    supervisor_waitable_set_drop(task.set);
    supervisor_mark(4);
    exports_supervisor_exercise_return();
    return SUPERVISOR_CALLBACK_CODE_EXIT;
}
static supervisor_callback_code_t after_actor_return(void) {
    CHECK(!task.gate);
    supervisor_mark(2);
    supervisor_subtask_status_t status = supervisor_proceed(&task.acknowledgement);
    task.gate = SUPERVISOR_SUBTASK_HANDLE(status);
    supervisor_subtask_state_t code = SUPERVISOR_SUBTASK_STATE(status);
    if (code == SUPERVISOR_SUBTASK_RETURNED) {
        if (task.gate) supervisor_subtask_drop(task.gate);
        task.gate = 0;
        return after_gate();
    }
    CHECK(task.gate && (code == SUPERVISOR_SUBTASK_STARTING || code == SUPERVISOR_SUBTASK_STARTED));
    supervisor_waitable_join(task.gate, task.set);
    return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
}
supervisor_callback_code_t exports_supervisor_exercise(void) {
    CHECK(!task.gate);
    memset(&task, 0, sizeof(task));
    task.set = supervisor_waitable_set_new();
    CHECK(supervisor_start() == 77);
    supervisor_mark(1);
    supervisor_subtask_status_t status = supervisor_kick();
    CHECK(SUPERVISOR_SUBTASK_STATE(status) == SUPERVISOR_SUBTASK_RETURNED);
    CHECK(SUPERVISOR_SUBTASK_HANDLE(status) == 0);
    return after_actor_return();
}
supervisor_callback_code_t exports_supervisor_exercise_callback(supervisor_event_t *event) {
    CHECK(!task.retiring && event->event == SUPERVISOR_EVENT_SUBTASK);
    if (event->code == SUPERVISOR_SUBTASK_STARTED) return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    CHECK(event->code == SUPERVISOR_SUBTASK_RETURNED);
    CHECK(event->waitable == task.gate);
    supervisor_waitable_join(task.gate, 0);
    supervisor_subtask_drop(task.gate);
    task.gate = 0;
    return after_gate();
}
