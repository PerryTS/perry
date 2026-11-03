//! Item-level native namespace declarations for `url` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_URL_DEFAULT = "url.default" {
        cell = "node:url.default";
        members = {
            Url = (ref("node:url.Url"));
            parse = (ref("node:url.parse"));
            resolve = (ref("node:url.resolve"));
            resolveObject = (ref("node:url.resolveObject"));
            format = (ref("node:url.format"));
            URL = (ref(URL));
            URLSearchParams = (ref(URLSearchParams));
            URLPattern = (ref(URLPattern));
            domainToASCII = (ref("node:url.domainToASCII"));
            domainToUnicode = (ref("node:url.domainToUnicode"));
            pathToFileURL = (ref("node:url.pathToFileURL"));
            fileURLToPath = (ref("node:url.fileURLToPath"));
            fileURLToPathBuffer = (ref("node:url.fileURLToPathBuffer"));
            urlToHttpOptions = (ref("node:url.urlToHttpOptions"));
        }
    }
}

crate::realm_decl! {
    namespace NM_URL = "url" {
        default = NM_URL_DEFAULT;
        births = [Object];
        members = {
            URL = (ref(URL));
            URLSearchParams = (ref(URLSearchParams));
            URLPattern = (ref(URLPattern));
            Url = (ref("node:url.Url"));
            default = (ref("node:url.default"));
            domainToASCII = (ref("node:url.domainToASCII"));
            domainToUnicode = (ref("node:url.domainToUnicode"));
            fileURLToPath = (ref("node:url.fileURLToPath"));
            fileURLToPathBuffer = (ref("node:url.fileURLToPathBuffer"));
            format = (ref("node:url.format"));
            parse = (ref("node:url.parse"));
            pathToFileURL = (ref("node:url.pathToFileURL"));
            resolve = (ref("node:url.resolve"));
            resolveObject = (ref("node:url.resolveObject"));
            urlToHttpOptions = (ref("node:url.urlToHttpOptions"));
        }
    }
}
