/* Independent maintained-C control: force a canonical WAIT/resume and use
   the generated scheduler's SDK task context helpers. The ordinary fixture
   separately executes Perry's JS async body; this control proves C TLS survives
   suspension without using canonical context 0, which SDK34 owns. */
typedef struct {
    plugin_values_t input;
    plugin_stream_void_t reader;
    plugin_stream_void_writer_t writer;
    plugin_waitable_set_t set;
} perry_tls_control;
plugin_callback_code_t exports_plugin_pass_stream_callback(plugin_event_t *event) {
    perry_tls_control *t = perry_task_context_get();
    perry_require(t != NULL);
    perry_require(event->event == PLUGIN_EVENT_STREAM_READ);
    perry_require(event->waitable == t->reader);
    perry_require(PLUGIN_WAITABLE_STATE(event->code) == PLUGIN_WAITABLE_COMPLETED);
    perry_require(PLUGIN_WAITABLE_COUNT(event->code) == 1);
    exports_plugin_pass_stream_return(t->input);
    plugin_waitable_join(t->reader, 0);
    plugin_stream_void_drop_readable(t->reader);
    plugin_stream_void_drop_writable(t->writer);
    plugin_waitable_set_drop(t->set);
    perry_task_context_set(NULL);
    free(t);
    return PLUGIN_CALLBACK_CODE_EXIT;
}
plugin_callback_code_t exports_plugin_pass_stream(plugin_values_t input) {
    perry_require(perry_task_context_get() == NULL);
    perry_tls_control *t = calloc(1, sizeof(*t));
    perry_require(t != NULL);
    perry_task_context_set(t);
    t->input = input;
    t->reader = plugin_stream_void_new(&t->writer);
    t->set = plugin_waitable_set_new();
    plugin_waitable_join(t->reader, t->set);
    perry_require(plugin_stream_void_read(t->reader, 1) == PLUGIN_WAITABLE_STATUS_BLOCKED);
    plugin_waitable_status_t written = plugin_stream_void_write(t->writer, 1);
    perry_require(PLUGIN_WAITABLE_STATE(written) == PLUGIN_WAITABLE_COMPLETED);
    perry_require(PLUGIN_WAITABLE_COUNT(written) == 1);
    return PLUGIN_CALLBACK_CODE_WAIT(t->set);
}
