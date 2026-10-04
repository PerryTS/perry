// Worker URLs are built here; the Workers are constructed in another module.
export const echoWorker = (): URL => new URL("./worker_entry_url_echo_worker.ts", import.meta.url);
// Not a worker: a data file next to it. It must not be compiled as one.
export const notes = (): URL => new URL("./worker_entry_url_notes.json", import.meta.url);
