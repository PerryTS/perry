//! Item-level native namespace declarations for `worker_threads` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_WORKER_THREADS = "worker_threads" {
        names = ["node:worker_threads.default"];
        members = {
            isInternalThread = (false);
            isMainThread = (true);
            MessagePort = (ref(MessagePort));
            MessageChannel = (ref(MessageChannel));
            markAsUncloneable = (ref("node:worker_threads.markAsUncloneable"));
            markAsUntransferable = (ref("node:worker_threads.markAsUntransferable"));
            isMarkedAsUntransferable = (ref("node:worker_threads.isMarkedAsUntransferable"));
            moveMessagePortToContext = (ref("node:worker_threads.moveMessagePortToContext"));
            receiveMessageOnPort = (ref("node:worker_threads.receiveMessageOnPort"));
            resourceLimits;
            postMessageToThread = (ref("node:worker_threads.postMessageToThread"));
            threadId = (0.0);
            threadName = ("");
            SHARE_ENV = (Symbol.for("nodejs.worker_threads.SHARE_ENV"));
            Worker = (ref("node:worker_threads.Worker"));
            parentPort = (null);
            workerData = (null);
            BroadcastChannel = (ref(BroadcastChannel));
            setEnvironmentData = (ref("node:worker_threads.setEnvironmentData"));
            getEnvironmentData = (ref("node:worker_threads.getEnvironmentData"));
            locks = (ref("node:worker_threads.locks"));
        }
    }
}
