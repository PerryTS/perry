//! Item-level native namespace declarations for `inspector` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_INSPECTOR_NETWORK = "inspector.Network" {
        cell = "node:inspector.Network";
        births = [Array, Error, Function, Object];
        members = {
            requestWillBeSent;
            responseReceived;
            loadingFinished;
            loadingFailed;
            dataSent;
            dataReceived;
            webSocketCreated;
            webSocketClosed;
            webSocketHandshakeResponseReceived;
        }
    }
}

crate::realm_decl! {
    namespace NM_INSPECTOR_DEFAULT = "inspector.default" {
        cell = "node:inspector.default";
        members = {
            open = (ref("node:inspector.open"));
            close = (ref("node:inspector.close"));
            url = (ref("node:inspector.url"));
            waitForDebugger = (ref("node:inspector.waitForDebugger"));
            console = (ref("node:inspector.console"));
            Session = (ref("node:inspector.Session"));
            Network = (ref("node:inspector.Network"));
            NetworkResources;
            DOMStorage;
        }
    }
}

crate::realm_decl! {
    namespace NM_INSPECTOR_DOMSTORAGE = "inspector.DOMStorage" {
        cell = "node:inspector.default.DOMStorage";
        members = {
            domStorageItemAdded;
            domStorageItemRemoved;
            domStorageItemUpdated;
            domStorageItemsCleared;
            registerStorage;
        }
    }
}

crate::realm_decl! {
    namespace NM_INSPECTOR_NETWORKRESOURCES = "inspector.NetworkResources" {
        cell = "node:inspector.default.NetworkResources";
        members = {
            put;
        }
    }
}

crate::realm_decl! {
    namespace NM_INSPECTOR_PROMISES_DEFAULT = "inspector/promises.default" {
        cell = "node:inspector/promises.default";
        members = {
            open = (ref("node:inspector.open"));
            close = (ref("node:inspector.close"));
            url = (ref("node:inspector.url"));
            waitForDebugger = (ref("node:inspector.waitForDebugger"));
            console = (ref("node:inspector.console"));
            Session = (ref("node:inspector/promises.Session"));
            Network = (ref("node:inspector.Network"));
            NetworkResources = (ref("node:inspector.default.NetworkResources"));
            DOMStorage = (ref("node:inspector.default.DOMStorage"));
        }
    }
}

crate::realm_decl! {
    namespace NM_INSPECTOR = "inspector" {
        births = [Object];
        members = {
            open = (ref("node:inspector.open"));
            close = (ref("node:inspector.close"));
            url = (ref("node:inspector.url"));
            waitForDebugger = (ref("node:inspector.waitForDebugger"));
            console = (ref("node:inspector.console"));
            Session = (ref("node:inspector.Session"));
            Network = (ref("node:inspector.Network"));
            NetworkResources = (ref("node:inspector.default.NetworkResources"));
            DOMStorage = (ref("node:inspector.default.DOMStorage"));
        }
    }
}

crate::realm_decl! {
    namespace NM_INSPECTOR_PROMISES = "inspector/promises" {
        births = [Object];
        members = {
            open = (ref("node:inspector.open"));
            close = (ref("node:inspector.close"));
            url = (ref("node:inspector.url"));
            waitForDebugger = (ref("node:inspector.waitForDebugger"));
            console = (ref("node:inspector.console"));
            Session = (ref("node:inspector/promises.Session"));
            Network = (ref("node:inspector.Network"));
            NetworkResources = (ref("node:inspector.default.NetworkResources"));
            DOMStorage = (ref("node:inspector.default.DOMStorage"));
        }
    }
}
