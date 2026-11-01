/* Instance-local canonical writers. This registry contains native identities
   and operation frames only; notify never runs JS or canonical built-ins. */
typedef struct perry_writer {
    struct perry_writer *next;
    uint32_t type, handle, reader, cancel;
    bool stream, release, abort, ended, written, reader_local, reader_closed;
    perry_io *pending;
    perry_task *parent;
    void (*drop)(uint32_t);
} perry_writer;
static perry_writer *perry_writers;
static perry_writer *perry_writer_find(uint32_t type, uint32_t handle) {
    for (perry_writer *w = perry_writers; w; w = w->next)
        if (w->type == type && w->handle == handle) return w;
    return NULL;
}
static void perry_producer_notify(uint32_t type, uint32_t handle, uint32_t operation, uint32_t action) {
    if (action == 3) {
        for (perry_writer *w = perry_writers; w; w = w->next)
            if (w->reader_local && w->type == type && w->reader == handle) w->reader_local = false;
        return;
    }
    perry_writer *w = perry_writer_find(type, handle);
    if (!w) return;
    if (action == 0) {
        if (operation && (!w->pending || w->pending->complete || w->pending->operation == operation)) w->cancel = operation;
    } else if (action == 1) w->release = true;
    else if (action == 2) w->abort = true;
    else __builtin_trap();
}
static void perry_producer_reader_dropped(uint32_t type, uint32_t reader) {
    for (perry_writer *w = perry_writers; w; w = w->next) {
        if (w->reader_local && w->type == type && w->reader == reader) {
            w->reader_local = false;
            w->reader_closed = true;
        }
    }
}
static void perry_producer_flush(void) {
    perry_writer **link = &perry_writers;
    while (*link) {
        perry_writer *w = *link;
        /* Retire an unfinished future rather than illegally dropping it. */
        perry_require(!w->abort);
        if (w->release && !w->stream && !w->written && !w->reader_closed) __builtin_trap();
        if (w->pending && !w->pending->complete && !w->pending->cancelled &&
            (w->release || w->cancel == w->pending->operation)) {
            perry_io_cancel(w->parent, w->pending);
            link = &perry_writers;
            continue;
        }
        if (w->pending || !(w->release || w->ended)) { link = &w->next; continue; }
        perry_require(w->stream || w->written || w->reader_closed);
        *link = w->next;
        /* Unlink before a canonical drop can enter a resource destructor. */
        w->drop(w->handle);
        free(w);
    }
}
