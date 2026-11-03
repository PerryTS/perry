/* Canonical CANCEL crosses from this C component into an actual Perry guest. */
#include "supervisor.h"
#include <stdbool.h>
#include <string.h>
#include <stdlib.h>

#define CHECK(x) do { if (!(x)) __builtin_trap(); } while (0)
#define F(name) test_async_compound_resources_foreign_##name
typedef test_async_compound_resources_foreign_own_item_t owner;
typedef struct {
    bool active, acknowledged, sibling_done, returned;
    uint32_t mode, base, proceed, sibling_value, ignored, published_before;
    supervisor_subtask_t call, sibling, gate;
    supervisor_waitable_set_t set;
    owner input, sibling_owner;
    supervisor_cancel_exchange_args_t exchange;
    supervisor_response_t result;
} state;
static state task;

static supervisor_subtask_t begin(supervisor_subtask_status_t status) {
    supervisor_subtask_state_t code = SUPERVISOR_SUBTASK_STATE(status);
    CHECK(code == SUPERVISOR_SUBTASK_STARTING || code == SUPERVISOR_SUBTASK_STARTED);
    supervisor_subtask_t handle = SUPERVISOR_SUBTASK_HANDLE(status);
    CHECK(handle);
    supervisor_waitable_join(handle, task.set);
    return handle;
}
static void retire(supervisor_subtask_t *handle) {
    CHECK(*handle);
    supervisor_waitable_join(*handle, 0);
    supervisor_subtask_drop(*handle); *handle = 0;
}
static supervisor_callback_code_t finish_if_ready(void) {
    if (!task.acknowledged || !task.sibling_done) return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    CHECK(!task.call && !task.sibling && !task.gate);
    F(item_drop_own)(task.sibling_owner);
    supervisor_waitable_set_drop(task.set);
    task.active = false;
    exports_supervisor_exercise_return(77);
    return SUPERVISOR_CALLBACK_CODE_EXIT;
}
static uint32_t choice(void) { return ((task.base / 100) % 10) % 5; }
static void request(void) {
    supervisor_request_t *value = &task.exchange.value;
    task.exchange.id = task.base + 10;
    value->choice = choice();
    value->owners.len = 2;
    value->owners.ptr = calloc(2, sizeof(*value->owners.ptr)); CHECK(value->owners.ptr);
    const uint32_t offsets[4] = {1, 3, 4, 5};
    for (size_t i = 0; i < 2; ++i) {
        value->owners.ptr[i].first = F(constructor_item)(task.base + offsets[i * 2]);
        value->owners.ptr[i].second = F(constructor_item)(task.base + offsets[i * 2 + 1]);
        supervisor_string_dup(&value->owners.ptr[i].label, "compound🍐");
    }
    value->fallback.is_some = true;
    value->fallback.val = F(constructor_item)(task.base + 6);
}
static void drop_result_owner(owner value, uint32_t offset) {
    CHECK(F(method_item_read)(F(borrow_item)(value)) == task.base + 1000000 + offset);
    F(item_drop_own)(value);
}
static void drop_response(void) {
    supervisor_response_t *value = &task.result;
    if (choice() == 2) {
        CHECK(value->is_err);
        drop_result_owner(value->val.err, 1);
    } else {
        CHECK(!value->is_err);
        if (choice() <= 1) {
            CHECK(value->val.ok.tag == TEST_ASYNC_COMPOUND_RESOURCES_FOREIGN_REPLY_HELD);
            drop_result_owner(value->val.ok.val.held.f0, 1);
            CHECK(value->val.ok.val.held.f1.is_some == (choice() == 0));
            if (choice() == 0) {
                test_async_compound_resources_foreign_owners_t *list = &value->val.ok.val.held.f1.val;
                CHECK(list->len == 4);
                for (size_t i = 0; i < list->len; ++i) drop_result_owner(list->ptr[i], (uint32_t)i + 3);
            }
        } else if (choice() == 3) {
            CHECK(value->val.ok.tag == TEST_ASYNC_COMPOUND_RESOURCES_FOREIGN_REPLY_TEXT);
            CHECK(value->val.ok.val.text.len == strlen("result🍐"));
            CHECK(memcmp(value->val.ok.val.text.ptr, "result🍐", value->val.ok.val.text.len) == 0);
        } else {
            CHECK(value->val.ok.tag == TEST_ASYNC_COMPOUND_RESOURCES_FOREIGN_REPLY_NOTHING);
        }
    }
    supervisor_response_free(value);
}
supervisor_callback_code_t exports_supervisor_exercise(uint32_t mode, uint32_t base) {
    CHECK(!task.active && mode <= 3);
    memset(&task, 0, sizeof(task));
    task.active = true; task.mode = mode; task.base = base;
    task.set = supervisor_waitable_set_new();
    task.published_before = supervisor_published();
    if (mode == 1) task.input = F(constructor_item)(base + 1);
    else request();
    task.sibling_owner = F(constructor_item)(base + 2);
    if (mode == 0) F(block)();
    if (mode == 3) F(complete_on_cancel)();
    if (mode == 1) {
        task.call = begin(supervisor_cancel_loan(base + 10, F(borrow_item)(task.input), &task.ignored));
    } else {
        task.call = begin(supervisor_cancel_exchange(&task.exchange, &task.result));
    }
    task.sibling = begin(supervisor_loan(base + 11, F(borrow_item)(task.sibling_owner), &task.sibling_value));
    task.gate = begin(supervisor_proceed(mode, base, &task.proceed));
    return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
}
supervisor_callback_code_t exports_supervisor_exercise_callback(supervisor_event_t *event) {
    CHECK(task.active && event->event == SUPERVISOR_EVENT_SUBTASK);
    if (event->code == SUPERVISOR_SUBTASK_STARTED) return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    CHECK(event->code == SUPERVISOR_SUBTASK_RETURNED);
    if (event->waitable == task.gate) {
        CHECK(!task.acknowledged && task.proceed == 1);
        retire(&task.gate);
        supervisor_subtask_state_t code = SUPERVISOR_SUBTASK_RETURNED;
        if (task.call) {
            supervisor_waitable_join(task.call, 0);
            code = SUPERVISOR_SUBTASK_STATE(supervisor_subtask_cancel(task.call));
            CHECK(code == SUPERVISOR_SUBTASK_RETURNED || code == SUPERVISOR_SUBTASK_RETURNED_CANCELLED);
            supervisor_subtask_drop(task.call); task.call = 0;
        } else {
            CHECK(task.returned && task.mode == 2);
        }
        if (task.mode != 2) CHECK(code == SUPERVISOR_SUBTASK_RETURNED_CANCELLED);
        if (task.mode == 0) F(unblock)();
        if (task.mode == 1) {
            CHECK(supervisor_cancelled_expired());
            F(item_drop_own)(task.input);
        } else if (code == SUPERVISOR_SUBTASK_RETURNED) {
            /* Cancellation lost to a result whose ownership belongs here. */
            drop_response();
        }
        if (task.mode != 1) supervisor_request_free(&task.exchange.value);
        uint32_t published = supervisor_published() - task.published_before;
        CHECK(published <= 1 && (task.mode == 2 || published == 0));
        task.acknowledged = true;
        supervisor_acknowledged(task.mode, task.base, (uint32_t)code);
        return finish_if_ready();
    }
    if (event->waitable == task.call) {
        CHECK(task.mode == 2 && !task.returned);
        retire(&task.call); task.returned = true;
        return SUPERVISOR_CALLBACK_CODE_WAIT(task.set);
    }
    CHECK(event->waitable == task.sibling && !task.sibling_done);
    retire(&task.sibling);
    CHECK(task.sibling_value == task.base + 2);
    task.sibling_done = true;
    return finish_if_ready();
}
