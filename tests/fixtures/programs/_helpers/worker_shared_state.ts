// Imported by the main thread and by every worker. Each thread must get its
// own copy: its own initialization and its own state.
export const state = { count: 0, inits: 0 };
state.inits++;
