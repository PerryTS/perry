#ifndef PERRY_COMPONENT_FRAME_H
#define PERRY_COMPONENT_FRAME_H
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>

typedef uint32_t (*perry_guest_import)(void *, uint32_t, uint32_t, uint32_t);
typedef struct {
    uint32_t abi_version;
    size_t size;
    const uint8_t *contract;
    size_t contract_len;
    perry_guest_import import;
    uint32_t (*resource)(void *, uint32_t, uint32_t, uint32_t);
} perry_guest_ops;
int32_t js_component_set_guest_ops(const perry_guest_ops *);
void *perry_component_frame_new(void);
/* Binds the module's layout image before allocating the returned owned frame.
   Initializer errors are retained in that frame; a null initializer returns NULL. */
void *perry_component_frame_bootstrap(const void *, void *);
void perry_component_frame_drop(void *);
int32_t perry_component_frame_failed(void *);
void perry_component_frame_fail(void *, const uint8_t *, size_t);
void perry_component_frame_initialize(void *, const void *);
uint32_t perry_component_frame_number(void *, double);
uint32_t perry_component_frame_bool(void *, int32_t);
uint32_t perry_component_frame_s64(void *, int64_t);
uint32_t perry_component_frame_u64(void *, uint64_t);
uint32_t perry_component_frame_string(void *, const uint8_t *, size_t);
uint32_t perry_component_frame_char(void *, uint32_t);
uint32_t perry_component_frame_bytes(void *, const uint8_t *, size_t);
uint32_t perry_component_frame_array(void *, uint32_t);
uint32_t perry_component_frame_object(void *);
void perry_component_frame_set(void *, uint32_t, const uint8_t *, size_t, uint32_t);
uint32_t perry_component_frame_get(void *, uint32_t, const uint8_t *, size_t);
void perry_component_frame_array_set(void *, uint32_t, uint32_t, uint32_t);
uint32_t perry_component_frame_array_get(void *, uint32_t, uint32_t);
uint32_t perry_component_frame_array_len(void *, uint32_t);
/* Copy into a distinct conversion frame before entering task_resume. The
   source and destination may also be equal. No JS or jobs are driven. */
uint32_t perry_component_frame_copy(void *destination, void *source, uint32_t);
double perry_component_frame_get_number(void *, uint32_t);
double perry_component_frame_get_integer(void *, uint32_t, double, double);
int32_t perry_component_frame_get_bool(void *, uint32_t);
uint64_t perry_component_frame_get_u64(void *, uint32_t);
int64_t perry_component_frame_get_s64(void *, uint32_t);
uint32_t perry_component_frame_get_char(void *, uint32_t);
size_t perry_component_frame_string_len(void *, uint32_t);
void perry_component_frame_string_copy(void *, uint32_t, uint8_t *, size_t);
size_t perry_component_frame_bytes_len(void *, uint32_t);
void perry_component_frame_bytes_copy(void *, uint32_t, uint8_t *, size_t);
uint32_t perry_component_frame_call(void *, uint32_t, uint32_t, uint32_t);
/* Observe an actual Promise: 0 pending, 1 fulfilled with a fresh rooted result
   slot, -1 frame error. Non-null output is zeroed first; null is a frame error.
   Marks rejection handled. No jobs, thenables or I/O are driven here.
   Rejection keeps the original JS reason; an earlier frame error is preserved. */
int32_t perry_component_frame_promise_poll(void *, uint32_t, uint32_t *);
/* Internal async-scheduler foundation; drop through the ordinary frame API.
   Calls/checkpoints restore ALS and task identity before returning to C.
   Checkpoint/ready return -1 on frame error. Ready consumes hints and checks
   runnable jobs; it is not a waitable. No timer or foreign I/O is driven. */
void *perry_component_frame_task_new(void);
uint32_t perry_component_frame_task_call(void *, uint32_t, uint32_t, uint32_t);
/* Opt in after the first successful call, before suspension/checkpointing.
   Clear temporary input slots below that call's result; never read them again.
   Preserve the result, task context and borrow lifetime. Returns 0/-1.
   Pending resource custody rejects the operation before clearing any slot. */
int32_t perry_component_frame_task_release_arguments(void *, uint32_t);
int32_t perry_component_frame_task_checkpoint(void *);
int32_t perry_component_frame_task_ready(void *);
int32_t perry_component_frame_task_cancel(void *);
/* Finite synchronous callback under the task's saved ALS/domain. No canonical
   suspension, or reentry/drop of that same task frame, is allowed in callback. */
int32_t perry_component_frame_task_resume(void *, void (*)(void *), void *);
/* Begin ONLY on a heap import frame whose C owner retains the same pointer.
   Requires a live current task; returns a rooted Promise slot (0 on error).
   Commits staged resource ownership/borrow leases after Promise hooks succeed,
   before publishing deferral. Failure leaves the frame owned by the runtime.
   Runtime transfers this frame to C after its import callback returns, even
   for immediate completion. C drops it only after all canonical/native use.
   Resolve/reject require its live originating task and return 0/-1. They run
   normal Promise settlement hooks/thenable assimilation, but no job drain.
   Use task_resume for a later callback; immediate completion is already scoped.
   Failed/duplicate completion is inert apart from the recorded frame error. */
uint32_t perry_component_frame_import_begin(void *);
int32_t perry_component_frame_import_resolve(void *, uint32_t);
int32_t perry_component_frame_import_reject(void *, uint32_t);

uint32_t perry_component_frame_resource_lift(void *, uint32_t, uint32_t, uint32_t, int32_t);
uint32_t perry_component_frame_resource_lower(void *, uint32_t, uint32_t, int32_t, int32_t);
void perry_component_frame_resource_commit(void *);
/* Experimental owned future/stream readers. Type IDs use tN = 2*N and
   eN = 2*N+1 from the immutable manifest. Lower only stages custody; the
   resource commit API validates and commits the combined batch atomically.
   No canonical operation runs in Rust or from the GC weak pass. */
uint32_t perry_component_frame_endpoint_lift(void *, uint32_t, uint32_t);
uint32_t perry_component_frame_endpoint_lower(void *, uint32_t, uint32_t);
/* 1 drop ready, 0 empty, -1 error. Return before the typed canonical drop.
   Outputs must be distinct. Handle 0 is not an empty-queue sentinel. */
int32_t perry_component_frame_endpoint_pop(void *, uint32_t *, uint32_t *);
/* Retire incoming borrows before task.return/cancel. The frame and its values
   remain alive for post-return work. Also releases borrows on a failed frame. */
int32_t perry_component_frame_resource_scope_end(void *);
void perry_component_frame_resource_destroy(void *, uint32_t, uint32_t);

/* Only native C allocations live here. Every JS value is a rooted frame slot. */
typedef struct perry_allocation {
    void *pointer;
    struct perry_allocation *next;
} perry_allocation;
typedef struct { perry_allocation *head; } perry_allocations;
static void perry_bad(void *f, const char *message) {
    perry_component_frame_fail(f, (const uint8_t *)message, strlen(message));
}
static void *perry_allocate(void *f, perry_allocations *a, size_t count, size_t size) {
    if (perry_component_frame_failed(f) || count == 0) return NULL;
    if (size && count > SIZE_MAX / size) { perry_bad(f, "Component allocation overflow"); return NULL; }
    perry_allocation *record = malloc(sizeof(*record));
    void *pointer = calloc(count, size);
    if (!record || !pointer) {
        free(record); free(pointer); perry_bad(f, "Component allocation failed"); return NULL;
    }
    record->pointer = pointer; record->next = a->head; a->head = record;
    return pointer;
}
static void perry_allocations_end(perry_allocations *a, bool transferred) {
    while (a->head) {
        perry_allocation *record = a->head; a->head = record->next;
        if (!transferred) free(record->pointer);
        free(record);
    }
}
static bool perry_tag(void *f, uint32_t slot, const char *expected, size_t len) {
    size_t actual = perry_component_frame_string_len(f, slot);
    if (perry_component_frame_failed(f) || actual != len) return false;
    uint8_t *bytes = len ? malloc(len) : NULL;
    if (len && !bytes) { perry_bad(f, "Component allocation failed"); return false; }
    perry_component_frame_string_copy(f, slot, bytes, len);
    bool matches = !perry_component_frame_failed(f) && (!len || memcmp(bytes, expected, len) == 0);
    free(bytes);
    return matches;
}
#endif
