//! Item-level native namespace declarations for `net` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_NET = "net" {
        names = ["node:net.default"];
        members = {
            BlockList = (ref("node:net.BlockList"));
            _createServerHandle = (ref("node:net._createServerHandle"));
            _normalizeArgs = (ref("node:net._normalizeArgs"));
            connect = (ref("node:net.connect"));
            createConnection = (ref("node:net.createConnection"));
            createServer = (ref("node:net.createServer"));
            isIP = (ref("node:net.isIP"));
            isIPv4 = (ref("node:net.isIPv4"));
            isIPv6 = (ref("node:net.isIPv6"));
            Server = (ref("node:net.Server"));
            Socket = (ref("node:net.Socket"));
            SocketAddress = (ref("node:net.SocketAddress"));
            Stream = (ref("node:net.Socket"));
            getDefaultAutoSelectFamily = (ref("node:net.getDefaultAutoSelectFamily"));
            setDefaultAutoSelectFamily = (ref("node:net.setDefaultAutoSelectFamily"));
            getDefaultAutoSelectFamilyAttemptTimeout = (ref("node:net.getDefaultAutoSelectFamilyAttemptTimeout"));
            setDefaultAutoSelectFamilyAttemptTimeout = (ref("node:net.setDefaultAutoSelectFamilyAttemptTimeout"));
        }
    }
}
