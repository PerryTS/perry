/* A separate component owns the resource type and can backpressure an async
   callee before it consumes arguments. No Perry scheduler internals are mocked. */
#include "provider.h"
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) __builtin_trap(); } while (0)
#define F(name) exports_test_async_compound_resources_foreign_##name
typedef exports_test_async_compound_resources_foreign_own_item_t owner;
typedef exports_test_async_compound_resources_foreign_item_t item;
typedef exports_test_async_compound_resources_foreign_borrow_item_t loan;
typedef struct {
    exports_test_async_compound_resources_foreign_request_t input;
    exports_test_async_compound_resources_foreign_response_t result;
    bool input_live, result_ready;
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

static void release_input(exchange *state) {
    CHECK(state->input_live);
    for (size_t i = 0; i < state->input.owners.len; ++i) {
        F(item_drop_own)(state->input.owners.ptr[i].first);
        F(item_drop_own)(state->input.owners.ptr[i].second);
    }
    if (state->input.fallback.is_some) F(item_drop_own)(state->input.fallback.val);
    F(request_free)(&state->input);
    state->input_live = false;
}
static void prepare(exchange *state) {
    CHECK(state->input_live && state->input.owners.len == 2 && state->input.fallback.is_some);
    uint32_t values[5];
    for (size_t i = 0; i < 2; ++i) {
        const exports_test_async_compound_resources_foreign_leaf_t *leaf = &state->input.owners.ptr[i];
        CHECK(leaf->label.len == strlen("compound🍐"));
        CHECK(memcmp(leaf->label.ptr, "compound🍐", leaf->label.len) == 0);
        values[i * 2] = (uint32_t)(uintptr_t)F(item_rep)(leaf->first);
        values[i * 2 + 1] = (uint32_t)(uintptr_t)F(item_rep)(leaf->second);
    }
    values[4] = (uint32_t)(uintptr_t)F(item_rep)(state->input.fallback.val);
    uint32_t choice = state->input.choice;
    CHECK(choice < 5);
    release_input(state);
    if (choice == 2) {
        state->result.is_err = true;
        state->result.val.err = F(constructor_item)(values[0] + 1000000);
    } else if (choice <= 1) {
        state->result.val.ok.tag = EXPORTS_TEST_ASYNC_COMPOUND_RESOURCES_FOREIGN_REPLY_HELD;
        state->result.val.ok.val.held.f0 = F(constructor_item)(values[0] + 1000000);
        if (choice == 0) {
            state->result.val.ok.val.held.f1.is_some = true;
            exports_test_async_compound_resources_foreign_owners_t *list = &state->result.val.ok.val.held.f1.val;
            list->len = 4; list->ptr = calloc(list->len, sizeof(*list->ptr)); CHECK(list->ptr);
            for (size_t i = 0; i < list->len; ++i) list->ptr[i] = F(constructor_item)(values[i + 1] + 1000000);
        }
    } else if (choice == 3) {
        state->result.val.ok.tag = EXPORTS_TEST_ASYNC_COMPOUND_RESOURCES_FOREIGN_REPLY_TEXT;
        provider_string_dup(&state->result.val.ok.val.text, "result🍐");
    } else {
        state->result.val.ok.tag = EXPORTS_TEST_ASYNC_COMPOUND_RESOURCES_FOREIGN_REPLY_NOTHING;
    }
    state->result_ready = true;
}

static void close_exchange(exchange *state) {
    CHECK(state->gate == 0);
    provider_waitable_set_drop(state->set);
    provider_context_set_0(NULL);
    CHECK(!state->input_live);
    if (state->result_ready) F(response_free)(&state->result);
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
provider_callback_code_t F(exchange)(uint32_t id, exports_test_async_compound_resources_foreign_request_t *value) {
    exchange *state = calloc(1, sizeof(*state));
    CHECK(state != NULL);
    state->input = *value; state->input_live = true; state->id = id;
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
        release_input(state);
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
