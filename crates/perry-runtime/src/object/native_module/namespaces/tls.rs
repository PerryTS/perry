//! Item-level native namespace declarations for `tls` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_TLS = "tls" {
        names = ["node:tls.default"];
        members = {
            CLIENT_RENEG_LIMIT = (3.0);
            CLIENT_RENEG_WINDOW = (600.0);
            DEFAULT_CIPHERS = ("TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA");
            DEFAULT_ECDH_CURVE = ("auto");
            DEFAULT_MIN_VERSION = ("TLSv1.2");
            DEFAULT_MAX_VERSION = ("TLSv1.3");
            getCiphers = (ref("node:tls.getCiphers"));
            getCertificateCompressionAlgorithms = (ref("node:tls.getCertificateCompressionAlgorithms"));
            rootCertificates = (ref("node:tls.rootCertificates"));
            getCACertificates = (ref("node:tls.getCACertificates"));
            setDefaultCACertificates = (ref("node:tls.setDefaultCACertificates"));
            convertALPNProtocols = (ref("node:tls.convertALPNProtocols"));
            checkServerIdentity = (ref("node:tls.checkServerIdentity"));
            createSecureContext = (ref("node:tls.createSecureContext"));
            SecureContext = (ref("node:tls.SecureContext"));
            TLSSocket = (ref("node:tls.TLSSocket"));
            Server = (ref("node:tls.Server"));
            createServer = (ref("node:tls.createServer"));
            connect = (ref("node:tls.connect"));
        }
    }
}
