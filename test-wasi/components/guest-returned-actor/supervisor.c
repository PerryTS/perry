/* Keep a real RETURNED subtask handle while its native child remains pending.
   The cancellation arm must trap in the engine before CANCEL reaches Perry. */
#include "supervisor.h"
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) __builtin_trap(); } while (0)
typedef struct {
    supervisor_subtask_t actor, gate;
    supervisor_waitable_set_t set;
    uint32_t acknowledgement;
} state;
static state task;

static supervisor_callback_code_t after_gate(void) {
    CHECK(task.actor && !task.gate && task.acknowledgement == 1);
    supervisor_mark(3);
    if (supervisor_cancel_mode()) {
        /* The native embedding releases the gate only after the actor's
           empty() child is actually polled and pending. */
        (void)supervisor_subtask_cancel(task.actor);
        __builtin_trap(); /* Returning from cancellation is a test failure. */
    }
    supervisor_subtask_drop(task.actor);
    task.actor = 0;
    supervisor_waitable_set_drop(task.set);
    supervisor_mark(4);
    exports_supervisor_exercise_return();
    return SUPERVISOR_CALLBACK_CODE_EXIT;
}
static supervisor_callback_code_t after_actor_return(void) {
    CHECK(task.actor && !task.gate);
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
    CHECK(!task.actor && !task.gate);
    memset(&task, 0, sizeof(task));
    task.set = supervisor_waitable_set_new();
    CHECK(supervisor_start() == 77);
    supervisor_mark(1);
    supervisor_subtask_status_t status = supervisor_kick();
    task.actor = SUPERVISOR_SUBTASK_HANDLE(status);
    CHECK(task.actor);
    supervisor_subtask_state_t code = SUPERVISOR_SUBTASK_STATE(status);
    if (code == SUPERVISOR_SUBTASK_RETURNED) return after_actor_return();
    CHECK(code == SUPERVISOR_SUBTASK_STARTING || code == SUPERVISOR_SUBTASK_STARTED);
    supervisor_waitable_join(task.actor, task.set);
    return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
}
supervisor_callback_code_t exports_supervisor_exercise_callback(supervisor_event_t *event) {
    CHECK(event->event == SUPERVISOR_EVENT_SUBTASK);
    if (event->code == SUPERVISOR_SUBTASK_STARTED) return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    CHECK(event->code == SUPERVISOR_SUBTASK_RETURNED);
    if (event->waitable == task.actor) {
        supervisor_waitable_join(task.actor, 0);
        return after_actor_return();
    }
    CHECK(event->waitable == task.gate);
    supervisor_waitable_join(task.gate, 0);
    supervisor_subtask_drop(task.gate);
    task.gate = 0;
    return after_gate();
}
