typedef struct {
    perry_io base;
    perry_writer *writer;
    perry_allocations arena;
    uint32_t length, sent;
    $ITEMS
} $STEM_state;
static void $STEM_deliver(void *pointer) {
    $STEM_state *data = pointer;
    perry_io *io = &data->base;
    perry_writer *writer = data->writer;
    void *f = io->frame;
    perry_require(!io->complete && !io->joined);
    io->complete = true;
    $W_waitable_state_t state = $U_WAITABLE_STATE(io->status);
    perry_require($VALID);
    uint32_t count = $COUNT;
    perry_require(count <= data->length);
    data->sent = count;
    bool ended = state == $U_WAITABLE_DROPPED $ENDED;
    writer->ended |= ended;
    writer->written |= $WRITTEN;
    writer->reader_closed |= state == $U_WAITABLE_DROPPED;
    bool discard = writer->release;
    bool closed = ended || discard;
    bool cancelled = io->cancelled || state == $U_WAITABLE_CANCELLED;
    int32_t observed = perry_component_frame_endpoint_prepare(f);
    perry_require(observed >= 0);
    uint32_t result = 0;
    if (observed) {
        uint32_t remaining = discard ? 0 : data->length - count;
        /* Mark before a partial lift can fail. Store retirement then owns
           any already-adopted suffix handles. */
        io->claimed = !discard;
        $LIFT_REMAINDER
        result = perry_component_frame_object(f);
        uint32_t written = perry_component_frame_number(f, count);
        uint32_t cancelled_value = perry_component_frame_bool(f, cancelled);
        uint32_t closed_value = perry_component_frame_bool(f, closed);
        perry_component_frame_set(f, result, (const uint8_t *)"written", 7, written);
        perry_component_frame_set(f, result, (const uint8_t *)"remaining", 9, $REMAINDER_SLOT);
        perry_component_frame_set(f, result, (const uint8_t *)"cancelled", 9, cancelled_value);
        perry_component_frame_set(f, result, (const uint8_t *)"closed", 6, closed_value);
        perry_require(!perry_component_frame_failed(f));
    }
    perry_require(perry_component_frame_endpoint_finish(f, result, (closed ? 1u : 0u) | (observed ? 0u : 2u)) == 0);
}
static void $STEM_cancel(perry_io *io) {
    perry_require(!io->cancelled && !io->complete);
    io->cancelled = true;
    if (io->joined) { $W_waitable_join(io->reader, 0); io->joined = false; }
    io->status = $HELPER_cancel_write(io->reader);
    perry_require(io->status != $U_WAITABLE_STATUS_BLOCKED);
}
static void $STEM_destroy(perry_io *io) {
    $STEM_state *data = ($STEM_state *)io;
    perry_require(io->complete && !io->joined && data->sent <= data->length);
    if (!io->claimed) {
        $DISCARD_REMAINDER
    }
    if (data->writer->pending == io) { data->writer->pending = NULL; data->writer->parent = NULL; }
    if (data->writer->cancel == io->operation) data->writer->cancel = 0;
    perry_allocations_end(&data->arena, false);
    perry_component_frame_drop(io->frame);
    free(data);
}
static int32_t $STEM_start(void *f, uint32_t handle, uint32_t operation, uint32_t input) {
    perry_task *parent = perry_active_task;
    perry_writer *writer = perry_writer_find($TYPE, handle);
    if (!parent || !parent->asynchronous || parent->cancelled || !writer || writer->release ||
        writer->ended || (writer->pending && !writer->pending->complete)) {
        perry_bad(f, "Guest write requires an open writer and async task"); return -1;
    }
    $STEM_state *data = calloc(1, sizeof(*data));
    if (!data) { perry_bad(f, "Guest write allocation failed"); return -1; }
    perry_allocations arena = {0};
    (void)input;
    size_t length = $LENGTH;
    if (perry_component_frame_failed(f) || length == 0 || length > 65536) {
        if (!perry_component_frame_failed(f)) perry_bad(f, "Invalid guest write count");
        free(data); return -1;
    }
    data->length = (uint32_t)length;
    $LOWER_INPUT
    data->arena = arena;
    if (perry_component_frame_failed(f) || perry_component_frame_endpoint_write_commit(f) < 0) {
        perry_allocations_end(&data->arena, false); free(data); return -1;
    }
    perry_io *io = &data->base;
    io->frame = f; io->type = $TYPE; io->reader = handle; io->operation = operation;
    io->event = $U_EVENT_$EVENT_WRITE;
    io->deliver = $STEM_deliver; io->cancel = $STEM_cancel; io->destroy = $STEM_destroy;
    io->next = parent->io; parent->io = io;
    data->writer = writer; writer->pending = io; writer->parent = parent;
    if (writer->cancel == operation) {
        io->cancelled = true; io->status = $U_WAITABLE_CANCELLED;
    } else {
        io->status = $NATIVE_WRITE;
    }
    if (io->status == $U_WAITABLE_STATUS_BLOCKED) {
        perry_task_set(parent);
        $W_waitable_join(handle, parent->set); io->joined = true;
    } else {
        $STEM_deliver(io);
    }
    return 0;
}
static uint32_t perry_new_$TYPE(void *f) {
    perry_writer *entry = calloc(1, sizeof(*entry));
    if (!entry) { perry_bad(f, "Guest factory allocation failed"); return 0; }
    $HELPER_writer_t writer;
    $CTYPE reader = $HELPER_new(&writer);
    entry->type = $TYPE; entry->handle = writer; entry->reader = reader;
    entry->reader_local = true; entry->stream = $STREAM; entry->drop = $HELPER_drop_writable;
    entry->next = perry_writers; perry_writers = entry;
    uint32_t result = perry_component_frame_endpoint_pair(f, $TYPE, reader, writer);
    perry_require(result != 0 && !perry_component_frame_failed(f));
    return result;
}
