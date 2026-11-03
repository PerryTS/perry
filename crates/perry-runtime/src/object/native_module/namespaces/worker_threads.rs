//! Item-level native namespace declarations for `worker_threads` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_WORKER_THREADS = "worker_threads" {
        names = ["node:worker_threads.default"];
        members = {
            BroadcastChannel = (ref(BroadcastChannel));
            MessageChannel = (ref(MessageChannel));
            MessagePort = (ref(MessagePort));
            SHARE_ENV = (Symbol.for("nodejs.worker_threads.SHARE_ENV"));
            Worker = (ref("node:worker_threads.Worker"));
            getEnvironmentData = (ref("node:worker_threads.getEnvironmentData"));
            isInternalThread = (false);
            isMainThread = (true);
            isMarkedAsUntransferable = (ref("node:worker_threads.isMarkedAsUntransferable"));
            locks = (ref("node:worker_threads.locks"));
            markAsUncloneable = (ref("node:worker_threads.markAsUncloneable"));
            markAsUntransferable = (ref("node:worker_threads.markAsUntransferable"));
            moveMessagePortToContext = (ref("node:worker_threads.moveMessagePortToContext"));
            parentPort = (null);
            postMessageToThread = (ref("node:worker_threads.postMessageToThread"));
            receiveMessageOnPort = (ref("node:worker_threads.receiveMessageOnPort"));
            resourceLimits;
            setEnvironmentData = (ref("node:worker_threads.setEnvironmentData"));
            threadId = (0.0);
            threadName = ("");
            workerData = (null);
        }
    }
}
