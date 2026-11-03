//! Item-level native namespace declarations for `cluster` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_CLUSTER_DEFAULT = "cluster.default" {
        cell = "node:cluster.default";
        extends = ("node:events.EventEmitter.prototype");
        members = {
            _events;
            _eventsCount = (0.0);
            _maxListeners = (undefined);
            isWorker = (false);
            isMaster = (true);
            isPrimary = (true);
            Worker = (ref("node:cluster.Worker"));
            workers = (ref("node:cluster.workers"));
            settings = (ref("node:cluster.settings"));
            SCHED_NONE = (1.0);
            SCHED_RR = (2.0);
            schedulingPolicy = (2.0);
            setupPrimary = (ref("node:cluster.setupMaster"));
            setupMaster = (ref("node:cluster.setupMaster"));
            fork = (ref("node:cluster.fork"));
            disconnect = (ref("node:cluster.disconnect"));
        }
    }
}
