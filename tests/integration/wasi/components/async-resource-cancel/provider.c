/* A separate component owns the resource type and can backpressure an async
   callee before it consumes arguments. No Perry scheduler internals are mocked. */
#include "provider.h"
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define CHECK(x) do { if (!(x)) __builtin_trap(); } while (0)
#define F(name) exports_test_async_guest_resources_foreign_##name
typedef exports_test_async_guest_resources_foreign_own_item_t owner;
typedef exports_test_async_guest_resources_foreign_item_t item;
typedef exports_test_async_guest_resources_foreign_borrow_item_t loan;
typedef struct {
    owner input, result;
    bool complete_on_cancel;
    uint32_t id;
    provider_subtask_t gate;
    provider_waitable_set_t set;
} exchange;
static bool blocked, complete_on_cancel;

owner F(constructor_item)(uint32_t value) {
    CHECK(value != 0);
    return F(item_new)((item *)(uintptr_t)value);
}
uint32_t F(method_item_read)(loan value) { return (uint32_t)(uintptr_t)value; }
void F(item_destructor)(item *value) { provider_dropped((uint32_t)(uintptr_t)value); }
void F(block)(void) { CHECK(!blocked); blocked = true; provider_backpressure_inc(); }
void F(unblock)(void) { CHECK(blocked); blocked = false; provider_backpressure_dec(); }
void F(complete_on_cancel)(void) { CHECK(!complete_on_cancel); complete_on_cancel = true; }

static void prepare(exchange *state) {
    uint32_t value = (uint32_t)(uintptr_t)F(item_rep)(state->input);
    F(item_drop_own)(state->input);
    state->result = F(item_new)((item *)(uintptr_t)(value + 1000000));
}

static void close_exchange(exchange *state) {
    CHECK(state->gate == 0);
    provider_waitable_set_drop(state->set);
    provider_context_set_0(NULL);
    free(state);
}
static provider_callback_code_t finish(exchange *state) {
    CHECK(!state->complete_on_cancel);
    prepare(state);
    /* The embedding only releases the caller's cancellation gate here: the
       owned result exists, but the Perry import callback has not run yet. */
    provider_ready(state->id);
    F(exchange_return)(state->result);
    close_exchange(state);
    return PROVIDER_CALLBACK_CODE_EXIT;
}
provider_callback_code_t F(exchange)(uint32_t id, owner value) {
    exchange *state = calloc(1, sizeof(*state));
    CHECK(state != NULL);
    state->input = value; state->id = id;
    state->complete_on_cancel = complete_on_cancel; complete_on_cancel = false;
    state->set = provider_waitable_set_new();
    provider_context_set_0(state);
    provider_entered(id);
    if (state->complete_on_cancel) {
        prepare(state);
        provider_prepared(id);
    }
    provider_subtask_status_t status = provider_gate(id);
    state->gate = PROVIDER_SUBTASK_HANDLE(status);
    provider_subtask_state_t code = PROVIDER_SUBTASK_STATE(status);
    if (code == PROVIDER_SUBTASK_RETURNED) {
        if (state->gate) provider_subtask_drop(state->gate);
        state->gate = 0;
        return finish(state);
    }
    CHECK((code == PROVIDER_SUBTASK_STARTING || code == PROVIDER_SUBTASK_STARTED) && state->gate);
    provider_waitable_join(state->gate, state->set);
    return PROVIDER_CALLBACK_CODE_WAIT(state->set);
}
provider_callback_code_t F(exchange_callback)(provider_event_t *event) {
    exchange *state = provider_context_get_0();
    CHECK(state && state->gate);
    if (event->event == PROVIDER_EVENT_CANCEL) {
        provider_waitable_join(state->gate, 0);
        provider_subtask_state_t code = PROVIDER_SUBTASK_STATE(provider_subtask_cancel(state->gate));
        CHECK(code == PROVIDER_SUBTASK_RETURNED || code == PROVIDER_SUBTASK_STARTED_CANCELLED || code == PROVIDER_SUBTASK_RETURNED_CANCELLED);
        provider_subtask_drop(state->gate); state->gate = 0;
        if (state->complete_on_cancel) {
            /* Returning is legal after CANCEL. Perry has already stopped its
               JS task, so this owner must be discarded without being lifted
               or published to JS. The provider created it before cancellation. */
            F(exchange_return)(state->result);
            close_exchange(state);
            return PROVIDER_CALLBACK_CODE_EXIT;
        }
        F(item_drop_own)(state->input);
        provider_task_cancel();
        close_exchange(state);
        return PROVIDER_CALLBACK_CODE_EXIT;
    }
    CHECK(event->event == PROVIDER_EVENT_SUBTASK && event->waitable == state->gate);
    if (event->code == PROVIDER_SUBTASK_STARTED) return PROVIDER_CALLBACK_CODE_WAIT(state->set);
    CHECK(event->code == PROVIDER_SUBTASK_RETURNED);
    provider_waitable_join(state->gate, 0);
    provider_subtask_drop(state->gate); state->gate = 0;
    return finish(state);
}
provider_callback_code_t F(borrow_wait)(uint32_t id, loan value) {
    (void)id;
    F(borrow_wait_return)((uint32_t)(uintptr_t)value);
    return PROVIDER_CALLBACK_CODE_EXIT;
}
provider_callback_code_t F(borrow_wait_callback)(provider_event_t *event) {
    (void)event; __builtin_trap();
}
