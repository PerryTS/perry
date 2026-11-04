//! Item-level native namespace declarations for `v8` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_V8 = "v8" {
        names = ["node:v8.default"];
        members = {
            cachedDataVersionTag = (ref("node:v8.cachedDataVersionTag"));
            getHeapSnapshot = (ref("node:v8.getHeapSnapshot"));
            getHeapStatistics = (ref("node:v8.getHeapStatistics"));
            getHeapSpaceStatistics = (ref("node:v8.getHeapSpaceStatistics"));
            getHeapCodeStatistics = (ref("node:v8.getHeapCodeStatistics"));
            setFlagsFromString = (ref("node:v8.setFlagsFromString"));
            Serializer = (ref("node:v8.Serializer"));
            Deserializer = (ref("node:v8.Deserializer"));
            DefaultSerializer = (ref("node:v8.DefaultSerializer"));
            DefaultDeserializer = (ref("node:v8.DefaultDeserializer"));
            deserialize = (ref("node:v8.deserialize"));
            takeCoverage = (ref("node:v8.takeCoverage"));
            stopCoverage = (ref("node:v8.stopCoverage"));
            serialize = (ref("node:v8.serialize"));
            writeHeapSnapshot = (ref("node:v8.writeHeapSnapshot"));
            promiseHooks;
            startupSnapshot;
            setHeapSnapshotNearHeapLimit = (ref("node:v8.setHeapSnapshotNearHeapLimit"));
            GCProfiler = (ref("node:v8.GCProfiler"));
        }
    }
}

crate::realm_decl! {
    namespace NM_V8_PROMISEHOOKS = "v8.promiseHooks" {
        cell = "node:v8.promiseHooks";
        names = ["node:v8.default.promiseHooks"];
        members = {
            createHook;
            onInit;
            onBefore;
            onAfter;
            onSettled;
        }
    }
}

crate::realm_decl! {
    namespace NM_V8_STARTUPSNAPSHOT = "v8.startupSnapshot" {
        cell = "node:v8.startupSnapshot";
        names = ["node:v8.default.startupSnapshot"];
        members = {
            addDeserializeCallback;
            addSerializeCallback;
            setDeserializeMainFunction;
            isBuildingSnapshot;
        }
    }
}
