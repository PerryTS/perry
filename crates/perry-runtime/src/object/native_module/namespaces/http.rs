//! Item-level native namespace declarations for `http` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_HTTP = "http" {
        names = ["node:http.default"];
        members = {
            _connectionListener = (ref("node:http._connectionListener"));
            METHODS = (ref("node:http.METHODS"));
            STATUS_CODES = (ref("node:http.STATUS_CODES"));
            Agent = (ref("node:http.Agent"));
            ClientRequest = (ref("node:http.ClientRequest"));
            IncomingMessage = (ref("node:http.IncomingMessage"));
            OutgoingMessage = (ref("node:http.OutgoingMessage"));
            Server = (ref("node:http.Server"));
            ServerResponse = (ref("node:http.ServerResponse"));
            createServer = (ref("node:http.createServer"));
            validateHeaderName = (ref("node:http.validateHeaderName"));
            validateHeaderValue = (ref("node:http.validateHeaderValue"));
            get = (ref("node:http.get"));
            request = (ref("node:http.request"));
            setMaxIdleHTTPParsers = (ref("node:http.setMaxIdleHTTPParsers"));
            setGlobalProxyFromEnv = (ref("node:http.setGlobalProxyFromEnv"));
            maxHeaderSize = (16384.0);
            globalAgent = (ref("node:http.globalAgent"));
            WebSocket = (ref(WebSocket));
        }
    }
}
