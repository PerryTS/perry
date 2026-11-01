/* Native SDK acknowledgement mocks exercise the generated forwarding function.
   This checks C storage/call ordering, not actual Wasm resource ownership. */
#include "perry_producer_forward.c"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
static unsigned consumed, freed_buffers, kicks, dropped_subtasks, borrowed_handles;
static uint8_t *text(const char *value) {
    uint8_t *result = malloc(4); assert(result); memcpy(result, value, 4); return result;
}
$SIGNATURE {
    assert(consumed == 0 && freed_buffers == 0 && kicks == 0);
    assert($VALUE->item.__handle == 42);
    assert(borrowed_handles == 0 && $VALUE->loan.__handle == 43);
    assert($VALUE->loans.len == 2 && $VALUE->loans.ptr[1].__handle == 45);
    assert($VALUE->text.len == 4 && memcmp($VALUE->text.ptr, "pear", 4) == 0);
    assert($VALUE->values.len == 2 && memcmp($VALUE->values.ptr[1].ptr, "plum", 4) == 0);
    consumed++;
    $RETURN->ptr = text("done"); $RETURN->len = 4;
}
$BORROW_SIGNATURE {
    assert(consumed == 1 && kicks == 0);
    assert($BORROW_PARAMETER.__handle == 43 + borrowed_handles);
    ++borrowed_handles;
}
void plugin_payload_free($NATIVE_TYPE *value) {
    assert(consumed == 1 && freed_buffers == 0 && kicks == 0 && value->item.__handle == 42);
    assert(borrowed_handles == 3);
    free(value->text.ptr); ++freed_buffers;
    for (size_t i = 0; i < value->values.len; ++i) { free(value->values.ptr[i].ptr); ++freed_buffers; }
    free(value->values.ptr); ++freed_buffers;
    free(value->loans.ptr); ++freed_buffers;
}
plugin_subtask_status_t $KICK(void) {
    assert(consumed == 1 && freed_buffers == 5 && borrowed_handles == 3 && ++kicks == 1);
    return (7u << 4) | PLUGIN_SUBTASK_RETURNED;
}
void plugin_subtask_drop(plugin_subtask_t task) {
    assert(task == 7 && kicks == 1 && ++dropped_subtasks == 1);
}
int main(void) {
    $INPUT input = {0};
    input.item.__handle = 42;
    input.loan.__handle = 43;
    input.loans.ptr = calloc(2, sizeof(*input.loans.ptr)); assert(input.loans.ptr);
    input.loans.len = 2;
    input.loans.ptr[0].__handle = 44;
    input.loans.ptr[1].__handle = 45;
    input.text.ptr = text("pear"); input.text.len = 4;
    input.values.ptr = calloc(2, sizeof(*input.values.ptr)); assert(input.values.ptr);
    input.values.len = 2;
    input.values.ptr[0].ptr = text("pear"); input.values.ptr[0].len = 4;
    input.values.ptr[1].ptr = text("plum"); input.values.ptr[1].len = 4;
    plugin_string_t result = {0};
    $FORWARD(&input, &result);
    assert(consumed == 1 && freed_buffers == 5 && borrowed_handles == 3 && kicks == 1 && dropped_subtasks == 1);
    assert(result.len == 4 && memcmp(result.ptr, "done", 4) == 0);
    free(result.ptr);
    puts("PASS: typed facade input retirement and result custody");
    return 0;
}
