//! Item-level native namespace declarations for `os` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_OS_DEFAULT = "os.default" {
        cell = "node:os.default";
        members = {
            arch = (ref("node:os.arch"));
            availableParallelism = (ref("node:os.availableParallelism"));
            cpus = (ref("node:os.cpus"));
            endianness = (ref("node:os.endianness"));
            freemem = (ref("node:os.freemem"));
            getPriority = (ref("node:os.getPriority"));
            homedir = (ref("node:os.homedir"));
            hostname = (ref("node:os.hostname"));
            loadavg = (ref("node:os.loadavg"));
            networkInterfaces = (ref("node:os.networkInterfaces"));
            platform = (ref("node:os.platform"));
            release = (ref("node:os.release"));
            setPriority = (ref("node:os.setPriority"));
            tmpdir = (ref("node:os.tmpdir"));
            totalmem = (ref("node:os.totalmem"));
            type = (ref("node:os.type"));
            userInfo = (ref("node:os.userInfo"));
            uptime = (ref("node:os.uptime"));
            version = (ref("node:os.version"));
            machine = (ref("node:os.machine"));
            constants = (ref("node:os.constants"));
            EOL = ("\n");
            devNull = ("/dev/null");
        }
    }
}
