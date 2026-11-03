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

crate::realm_decl! {
    namespace NM_OS_CONSTANTS = "os.constants" {
        cell = "node:os.constants";
        members = {
            UV_UDP_REUSEADDR = (4.0);
            dlopen;
            errno;
            signals;
            priority;
        }
    }
}

crate::realm_decl! {
    namespace NM_OS_CONSTANTS_DLOPEN = "os.constants.dlopen" {
        cell = "node:os.constants.dlopen";
        members = {
            RTLD_LAZY = (1.0);
            RTLD_NOW = (2.0);
            RTLD_GLOBAL = (256.0);
            RTLD_LOCAL = (0.0);
            RTLD_DEEPBIND = (8.0) if cfg(all(target_os = "linux", target_env = "gnu"));
        }
    }
}

crate::realm_decl! {
    namespace NM_OS_CONSTANTS_ERRNO = "os.constants.errno" {
        cell = "node:os.constants.errno";
        members = {
            E2BIG = (7.0);
            EACCES = (13.0);
            EADDRINUSE = (98.0);
            EADDRNOTAVAIL = (99.0);
            EAFNOSUPPORT = (97.0);
            EAGAIN = (11.0);
            EALREADY = (114.0);
            EBADF = (9.0);
            EBADMSG = (74.0);
            EBUSY = (16.0);
            ECANCELED = (125.0);
            ECHILD = (10.0);
            ECONNABORTED = (103.0);
            ECONNREFUSED = (111.0);
            ECONNRESET = (104.0);
            EDEADLK = (35.0);
            EDESTADDRREQ = (89.0);
            EDOM = (33.0);
            EDQUOT = (122.0);
            EEXIST = (17.0);
            EFAULT = (14.0);
            EFBIG = (27.0);
            EHOSTUNREACH = (113.0);
            EIDRM = (43.0);
            EILSEQ = (84.0);
            EINPROGRESS = (115.0);
            EINTR = (4.0);
            EINVAL = (22.0);
            EIO = (5.0);
            EISCONN = (106.0);
            EISDIR = (21.0);
            ELOOP = (40.0);
            EMFILE = (24.0);
            EMLINK = (31.0);
            EMSGSIZE = (90.0);
            EMULTIHOP = (72.0);
            ENAMETOOLONG = (36.0);
            ENETDOWN = (100.0);
            ENETRESET = (102.0);
            ENETUNREACH = (101.0);
            ENFILE = (23.0);
            ENOBUFS = (105.0);
            ENODATA = (61.0);
            ENODEV = (19.0);
            ENOENT = (2.0);
            ENOEXEC = (8.0);
            ENOLCK = (37.0);
            ENOLINK = (67.0);
            ENOMEM = (12.0);
            ENOMSG = (42.0);
            ENOPROTOOPT = (92.0);
            ENOSPC = (28.0);
            ENOSR = (63.0);
            ENOSTR = (60.0);
            ENOSYS = (38.0);
            ENOTCONN = (107.0);
            ENOTDIR = (20.0);
            ENOTEMPTY = (39.0);
            ENOTSOCK = (88.0);
            ENOTSUP = (95.0);
            ENOTTY = (25.0);
            ENXIO = (6.0);
            EOPNOTSUPP = (95.0);
            EOVERFLOW = (75.0);
            EPERM = (1.0);
            EPIPE = (32.0);
            EPROTO = (71.0);
            EPROTONOSUPPORT = (93.0);
            EPROTOTYPE = (91.0);
            ERANGE = (34.0);
            EROFS = (30.0);
            ESPIPE = (29.0);
            ESRCH = (3.0);
            ESTALE = (116.0);
            ETIME = (62.0);
            ETIMEDOUT = (110.0);
            ETXTBSY = (26.0);
            EWOULDBLOCK = (11.0);
            EXDEV = (18.0);
        }
    }
}

crate::realm_decl! {
    namespace NM_OS_CONSTANTS_SIGNALS = "os.constants.signals" {
        cell = "node:os.constants.signals";
        members = {
            SIGHUP = (1.0);
            SIGINT = (2.0);
            SIGQUIT = (3.0);
            SIGILL = (4.0);
            SIGTRAP = (5.0);
            SIGABRT = (6.0);
            SIGIOT = (6.0);
            SIGBUS = (7.0);
            SIGFPE = (8.0);
            SIGKILL = (9.0);
            SIGUSR1 = (10.0);
            SIGSEGV = (11.0);
            SIGUSR2 = (12.0);
            SIGPIPE = (13.0);
            SIGALRM = (14.0);
            SIGTERM = (15.0);
            SIGCHLD = (17.0);
            SIGSTKFLT = (16.0) if cfg(target_os = "linux");
            SIGCONT = (18.0);
            SIGSTOP = (19.0);
            SIGTSTP = (20.0);
            SIGTTIN = (21.0);
            SIGTTOU = (22.0);
            SIGURG = (23.0);
            SIGXCPU = (24.0);
            SIGXFSZ = (25.0);
            SIGVTALRM = (26.0);
            SIGPROF = (27.0);
            SIGWINCH = (28.0);
            SIGIO = (29.0);
            SIGPOLL = (29.0) if cfg(target_os = "linux");
            SIGPWR = (30.0) if cfg(target_os = "linux");
            SIGSYS = (31.0);
            SIGINFO = (29.0) if cfg(target_os = "macos");
        }
    }
}

crate::realm_decl! {
    namespace NM_OS_CONSTANTS_PRIORITY = "os.constants.priority" {
        cell = "node:os.constants.priority";
        members = {
            PRIORITY_LOW = (19.0);
            PRIORITY_BELOW_NORMAL = (10.0);
            PRIORITY_NORMAL = (0.0);
            PRIORITY_ABOVE_NORMAL = (-7.0);
            PRIORITY_HIGH = (-14.0);
            PRIORITY_HIGHEST = (-20.0);
        }
    }
}
