//! Item-level native namespace declarations for `http` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_HTTP = "http" {
        names = ["node:http.default"];
        members = {
            METHODS = (ref("node:http.METHODS"));
            STATUS_CODES = (ref("node:http.STATUS_CODES"));
            createServer = (ref("node:http.createServer"));
            Server = (ref("node:http.Server"));
            IncomingMessage = (ref("node:http.IncomingMessage"));
            OutgoingMessage = (ref("node:http.OutgoingMessage"));
            ServerResponse = (ref("node:http.ServerResponse"));
            ClientRequest = (ref("node:http.ClientRequest"));
            Agent = (ref("node:http.Agent"));
            WebSocket = (ref(WebSocket));
            _connectionListener = (ref("node:http._connectionListener"));
            get = (ref("node:http.get"));
            request = (ref("node:http.request"));
            maxHeaderSize = (16384.0);
            globalAgent = (ref("node:http.globalAgent"));
            validateHeaderName = (ref("node:http.validateHeaderName"));
            validateHeaderValue = (ref("node:http.validateHeaderValue"));
            setMaxIdleHTTPParsers = (ref("node:http.setMaxIdleHTTPParsers"));
            setGlobalProxyFromEnv = (ref("node:http.setGlobalProxyFromEnv"));
        }
    }
}
