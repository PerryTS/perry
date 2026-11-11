/* A real component caller sends canonical CANCEL; the native Driver stays live. */
#include "supervisor.h"
#include <stdbool.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) __builtin_trap(); } while (0)
typedef struct {
    supervisor_subtask_t cancelled, sibling, gate;
    supervisor_waitable_set_t set;
    test_async_guest_imports_service_payload_t first, second;
    supervisor_cancel_probe_args_t cancel_args;
    supervisor_run_args_t run_args;
    uint32_t proceed;
    bool cancelling;
} state;
static state task;

static supervisor_subtask_t begin(supervisor_subtask_status_t packed) {
    supervisor_subtask_state_t status = SUPERVISOR_SUBTASK_STATE(packed);
    CHECK(status == SUPERVISOR_SUBTASK_STARTING || status == SUPERVISOR_SUBTASK_STARTED);
    supervisor_subtask_t handle = SUPERVISOR_SUBTASK_HANDLE(packed);
    CHECK(handle != 0);
    supervisor_waitable_join(handle, task.set);
    return handle;
}
static void retire(supervisor_subtask_t *handle) {
    supervisor_waitable_join(*handle, 0);
    supervisor_subtask_drop(*handle);
    *handle = 0;
}
supervisor_callback_code_t exports_supervisor_exercise(void) {
    CHECK(!task.cancelled && !task.sibling && !task.gate);
    memset(&task, 0, sizeof(task));
    task.set = supervisor_waitable_set_new();
    /* Borrowed parameters must remain valid through STARTING. */
    task.cancel_args.id = 1;
    task.run_args.id = 3;
    supervisor_string_set(&task.cancel_args.input.text, "cancel input 🌍");
    supervisor_string_set(&task.run_args.input.text, "sibling input 日本語");
    task.cancel_args.input.owner = 1;
    task.run_args.input.owner = 3;
    task.cancelled = begin(supervisor_cancel_probe(&task.cancel_args, &task.first));
    task.sibling = begin(supervisor_run(&task.run_args, &task.second));
    task.gate = begin(supervisor_proceed(&task.proceed));
    return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
}
supervisor_callback_code_t exports_supervisor_exercise_callback(supervisor_event_t *event) {
    CHECK(event->event == SUPERVISOR_EVENT_SUBTASK);
    if (event->code == SUPERVISOR_SUBTASK_STARTED) return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    CHECK(event->code == SUPERVISOR_SUBTASK_RETURNED);
    if (event->waitable == task.gate) {
        CHECK(!task.cancelling && task.proceed == 1);
        retire(&task.gate);
        supervisor_waitable_join(task.cancelled, 0);
        /* No Rust runtime scope or borrow is live during this suspension. */
        supervisor_subtask_status_t status = supervisor_subtask_cancel(task.cancelled);
        CHECK(SUPERVISOR_SUBTASK_STATE(status) == SUPERVISOR_SUBTASK_RETURNED_CANCELLED);
        supervisor_subtask_drop(task.cancelled);
        task.cancelled = 0;
        task.cancelling = true;
        CHECK(supervisor_cancel_poke() == 0);
        CHECK(supervisor_cancel_poke() == 0);
        return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    }
    CHECK(task.cancelling && event->waitable == task.sibling);
    retire(&task.sibling);
    CHECK(task.second.owner == 3);
    test_async_guest_imports_service_payload_free(&task.second);
    CHECK(supervisor_cancel_poke() == 0);
    exports_supervisor_exercise_return(77);
    supervisor_waitable_set_drop(task.set);
    return SUPERVISOR_CALLBACK_CODE_EXIT;
}
