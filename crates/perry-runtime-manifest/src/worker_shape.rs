//! The shape of the runtime Worker (`worker_threads.Worker`, the web
//! `Worker` global), in one place. perry-runtime's build script writes
//! both lists into the runtime (as it does the family bits), so neither
//! the runtime nor perry-stdlib links this crate: perry-stdlib's
//! `worker_object` sets the own keys from [`WORKER_OWN_KEYS`],
//! perry-runtime's `install_worker_metadata_getters` installs
//! [`WORKER_PROTOTYPE_ACCESSORS`] on `Worker.prototype`, and perry-wpfacts
//! reads both
//! (`members::worker_inherited`), so the compiler's view of a Worker's keys
//! is the runtime's.

/// The own keys of every Worker the runtime makes, in the order it sets
/// them (`Object.keys(worker)`).
pub const WORKER_OWN_KEYS: &[&str] = &[
    "stdin",
    "stdout",
    "stderr",
    "performance",
    "getHeapStatistics",
    "cpuUsage",
    "getHeapSnapshot",
    "startCpuProfile",
    "startHeapProfile",
    "postMessage",
    "terminate",
    "reload",
    "ref",
    "unref",
    "on",
    "addListener",
    "once",
    "off",
    "removeListener",
    "addEventListener",
    "removeEventListener",
    "onmessage",
    "onmessageerror",
    "onerror",
];

/// The metadata getters on `Worker.prototype`, beside its realm-declared
/// `constructor`; the index of each is its `WorkerMetadataField`.
pub const WORKER_PROTOTYPE_ACCESSORS: &[&str] = &["threadId", "threadName", "resourceLimits"];
