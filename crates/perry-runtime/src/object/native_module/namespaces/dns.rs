//! Item-level native namespace declarations for `dns` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_DNS_DEFAULT = "dns.default" {
        cell = "node:dns.default";
        members = {
            lookup = (ref("node:dns.lookup"));
            lookupService = (ref("node:dns.lookupService"));
            Resolver = (ref("node:dns.Resolver"));
            getDefaultResultOrder = (ref("node:dns.getDefaultResultOrder"));
            setDefaultResultOrder = (ref("node:dns.setDefaultResultOrder"));
            setServers = (ref("node:dns.setServers"));
            ADDRCONFIG = (32.0);
            ALL = (16.0);
            V4MAPPED = (8.0);
            NODATA = ("ENODATA");
            FORMERR = ("EFORMERR");
            SERVFAIL = ("ESERVFAIL");
            NOTFOUND = ("ENOTFOUND");
            NOTIMP = ("ENOTIMP");
            REFUSED = ("EREFUSED");
            BADQUERY = ("EBADQUERY");
            BADNAME = ("EBADNAME");
            BADFAMILY = ("EBADFAMILY");
            BADRESP = ("EBADRESP");
            CONNREFUSED = ("ECONNREFUSED");
            TIMEOUT = ("ETIMEOUT");
            EOF = ("EOF");
            FILE = ("EFILE");
            NOMEM = ("ENOMEM");
            DESTRUCTION = ("EDESTRUCTION");
            BADSTR = ("EBADSTR");
            BADFLAGS = ("EBADFLAGS");
            NONAME = ("ENONAME");
            BADHINTS = ("EBADHINTS");
            NOTINITIALIZED = ("ENOTINITIALIZED");
            LOADIPHLPAPI = ("ELOADIPHLPAPI");
            ADDRGETNETWORKPARAMS = ("EADDRGETNETWORKPARAMS");
            CANCELLED = ("ECANCELLED");
            getServers = (ref("node:dns.getServers"));
            resolve = (ref("node:dns.resolve"));
            resolve4 = (ref("node:dns.resolve4"));
            resolve6 = (ref("node:dns.resolve6"));
            resolveAny = (ref("node:dns.resolveAny"));
            resolveCaa = (ref("node:dns.resolveCaa"));
            resolveCname = (ref("node:dns.resolveCname"));
            resolveMx = (ref("node:dns.resolveMx"));
            resolveNaptr = (ref("node:dns.resolveNaptr"));
            resolveNs = (ref("node:dns.resolveNs"));
            resolvePtr = (ref("node:dns.resolvePtr"));
            resolveSoa = (ref("node:dns.resolveSoa"));
            resolveSrv = (ref("node:dns.resolveSrv"));
            resolveTlsa = (ref("node:dns.resolveTlsa"));
            resolveTxt = (ref("node:dns.resolveTxt"));
            reverse = (ref("node:dns.reverse"));
            promises = (ref("node:dns.promises"));
        }
    }
}

crate::realm_decl! {
    namespace NM_DNS_PROMISES_DEFAULT = "dns/promises.default" {
        cell = "node:dns.promises";
        members = {
            lookup = (ref("node:dns/promises.lookup"));
            lookupService = (ref("node:dns/promises.lookupService"));
            Resolver = (ref("node:dns/promises.Resolver"));
            getDefaultResultOrder = (ref("node:dns/promises.getDefaultResultOrder"));
            setDefaultResultOrder = (ref("node:dns/promises.setDefaultResultOrder"));
            setServers = (ref("node:dns/promises.setServers"));
            NODATA = ("ENODATA");
            FORMERR = ("EFORMERR");
            SERVFAIL = ("ESERVFAIL");
            NOTFOUND = ("ENOTFOUND");
            NOTIMP = ("ENOTIMP");
            REFUSED = ("EREFUSED");
            BADQUERY = ("EBADQUERY");
            BADNAME = ("EBADNAME");
            BADFAMILY = ("EBADFAMILY");
            BADRESP = ("EBADRESP");
            CONNREFUSED = ("ECONNREFUSED");
            TIMEOUT = ("ETIMEOUT");
            EOF = ("EOF");
            FILE = ("EFILE");
            NOMEM = ("ENOMEM");
            DESTRUCTION = ("EDESTRUCTION");
            BADSTR = ("EBADSTR");
            BADFLAGS = ("EBADFLAGS");
            NONAME = ("ENONAME");
            BADHINTS = ("EBADHINTS");
            NOTINITIALIZED = ("ENOTINITIALIZED");
            LOADIPHLPAPI = ("ELOADIPHLPAPI");
            ADDRGETNETWORKPARAMS = ("EADDRGETNETWORKPARAMS");
            CANCELLED = ("ECANCELLED");
            getServers = (ref("node:dns/promises.getServers"));
            resolve = (ref("node:dns/promises.resolve"));
            resolve4 = (ref("node:dns/promises.resolve4"));
            resolve6 = (ref("node:dns/promises.resolve6"));
            resolveAny = (ref("node:dns/promises.resolveAny"));
            resolveCaa = (ref("node:dns/promises.resolveCaa"));
            resolveCname = (ref("node:dns/promises.resolveCname"));
            resolveMx = (ref("node:dns/promises.resolveMx"));
            resolveNaptr = (ref("node:dns/promises.resolveNaptr"));
            resolveNs = (ref("node:dns/promises.resolveNs"));
            resolvePtr = (ref("node:dns/promises.resolvePtr"));
            resolveSoa = (ref("node:dns/promises.resolveSoa"));
            resolveSrv = (ref("node:dns/promises.resolveSrv"));
            resolveTlsa = (ref("node:dns/promises.resolveTlsa"));
            resolveTxt = (ref("node:dns/promises.resolveTxt"));
            reverse = (ref("node:dns/promises.reverse"));
        }
    }
}

crate::realm_decl! {
    namespace NM_DNS = "dns" {
        default = NM_DNS_DEFAULT;
        births = [Object];
        members = {
            ADDRCONFIG = (32.0);
            ADDRGETNETWORKPARAMS = ("EADDRGETNETWORKPARAMS");
            ALL = (16.0);
            BADFAMILY = ("EBADFAMILY");
            BADFLAGS = ("EBADFLAGS");
            BADHINTS = ("EBADHINTS");
            BADNAME = ("EBADNAME");
            BADQUERY = ("EBADQUERY");
            BADRESP = ("EBADRESP");
            BADSTR = ("EBADSTR");
            CANCELLED = ("ECANCELLED");
            CONNREFUSED = ("ECONNREFUSED");
            DESTRUCTION = ("EDESTRUCTION");
            EOF = ("EOF");
            FILE = ("EFILE");
            FORMERR = ("EFORMERR");
            LOADIPHLPAPI = ("ELOADIPHLPAPI");
            NODATA = ("ENODATA");
            NOMEM = ("ENOMEM");
            NONAME = ("ENONAME");
            NOTFOUND = ("ENOTFOUND");
            NOTIMP = ("ENOTIMP");
            NOTINITIALIZED = ("ENOTINITIALIZED");
            REFUSED = ("EREFUSED");
            Resolver = (ref("node:dns.Resolver"));
            SERVFAIL = ("ESERVFAIL");
            TIMEOUT = ("ETIMEOUT");
            V4MAPPED = (8.0);
            default = (ref("node:dns.default"));
            getDefaultResultOrder = (ref("node:dns.getDefaultResultOrder"));
            getServers = (ref("node:dns.getServers"));
            lookup = (ref("node:dns.lookup"));
            lookupService = (ref("node:dns.lookupService"));
            promises = (ref("node:dns.promises"));
            resolve = (ref("node:dns.resolve"));
            resolve4 = (ref("node:dns.resolve4"));
            resolve6 = (ref("node:dns.resolve6"));
            resolveAny = (ref("node:dns.resolveAny"));
            resolveCaa = (ref("node:dns.resolveCaa"));
            resolveCname = (ref("node:dns.resolveCname"));
            resolveMx = (ref("node:dns.resolveMx"));
            resolveNaptr = (ref("node:dns.resolveNaptr"));
            resolveNs = (ref("node:dns.resolveNs"));
            resolvePtr = (ref("node:dns.resolvePtr"));
            resolveSoa = (ref("node:dns.resolveSoa"));
            resolveSrv = (ref("node:dns.resolveSrv"));
            resolveTlsa = (ref("node:dns.resolveTlsa"));
            resolveTxt = (ref("node:dns.resolveTxt"));
            reverse = (ref("node:dns.reverse"));
            setDefaultResultOrder = (ref("node:dns.setDefaultResultOrder"));
            setServers = (ref("node:dns.setServers"));
        }
    }
}

crate::realm_decl! {
    namespace NM_DNS_PROMISES = "dns/promises" {
        default = NM_DNS_PROMISES_DEFAULT;
        births = [Object];
        members = {
            ADDRGETNETWORKPARAMS = ("EADDRGETNETWORKPARAMS");
            BADFAMILY = ("EBADFAMILY");
            BADFLAGS = ("EBADFLAGS");
            BADHINTS = ("EBADHINTS");
            BADNAME = ("EBADNAME");
            BADQUERY = ("EBADQUERY");
            BADRESP = ("EBADRESP");
            BADSTR = ("EBADSTR");
            CANCELLED = ("ECANCELLED");
            CONNREFUSED = ("ECONNREFUSED");
            DESTRUCTION = ("EDESTRUCTION");
            EOF = ("EOF");
            FILE = ("EFILE");
            FORMERR = ("EFORMERR");
            LOADIPHLPAPI = ("ELOADIPHLPAPI");
            NODATA = ("ENODATA");
            NOMEM = ("ENOMEM");
            NONAME = ("ENONAME");
            NOTFOUND = ("ENOTFOUND");
            NOTIMP = ("ENOTIMP");
            NOTINITIALIZED = ("ENOTINITIALIZED");
            REFUSED = ("EREFUSED");
            Resolver = (ref("node:dns/promises.Resolver"));
            SERVFAIL = ("ESERVFAIL");
            TIMEOUT = ("ETIMEOUT");
            default = (ref("node:dns.promises"));
            getDefaultResultOrder = (ref("node:dns/promises.getDefaultResultOrder"));
            getServers = (ref("node:dns/promises.getServers"));
            lookup = (ref("node:dns/promises.lookup"));
            lookupService = (ref("node:dns/promises.lookupService"));
            resolve = (ref("node:dns/promises.resolve"));
            resolve4 = (ref("node:dns/promises.resolve4"));
            resolve6 = (ref("node:dns/promises.resolve6"));
            resolveAny = (ref("node:dns/promises.resolveAny"));
            resolveCaa = (ref("node:dns/promises.resolveCaa"));
            resolveCname = (ref("node:dns/promises.resolveCname"));
            resolveMx = (ref("node:dns/promises.resolveMx"));
            resolveNaptr = (ref("node:dns/promises.resolveNaptr"));
            resolveNs = (ref("node:dns/promises.resolveNs"));
            resolvePtr = (ref("node:dns/promises.resolvePtr"));
            resolveSoa = (ref("node:dns/promises.resolveSoa"));
            resolveSrv = (ref("node:dns/promises.resolveSrv"));
            resolveTlsa = (ref("node:dns/promises.resolveTlsa"));
            resolveTxt = (ref("node:dns/promises.resolveTxt"));
            reverse = (ref("node:dns/promises.reverse"));
            setDefaultResultOrder = (ref("node:dns/promises.setDefaultResultOrder"));
            setServers = (ref("node:dns/promises.setServers"));
        }
    }
}
