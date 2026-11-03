//! Item-level native namespace declarations for `process` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_PROCESS = "process" {
        cell = "process";
        births = [Array, Buffer, Promise];
        members = {
            _debugEnd;
            _debugProcess;
            _events;
            _eventsCount = (0.0);
            _exiting = (false);
            _fatalException;
            _getActiveHandles;
            _getActiveRequests;
            _kill;
            _linkedBinding;
            _maxListeners = (undefined);
            _preload_modules;
            _rawDebug;
            _startProfilerIdleNotifier;
            _stopProfilerIdleNotifier;
            _tickCallback;
            abort;
            addListener;
            addUncaughtExceptionCaptureCallback;
            allowedNodeEnvironmentFlags;
            argv = (runtime(process.argv));
            argv0 = (runtime(process.argv0));
            arch = (target(process.arch));
            binding;
            channel = (undefined);
            chdir;
            config;
            connected = (undefined);
            cpuUsage;
            cwd;
            debugPort = (9229.0);
            disconnect = (undefined);
            dlopen;
            domain = (null);
            env = (runtime(process.env));
            eventNames;
            execve;
            ref;
            unref;
            execArgv = (runtime(process.execArgv));
            execPath = (runtime(process.execPath));
            features;
            finalization;
            getActiveResourcesInfo;
            getBuiltinModule;
            getMaxListeners;
            hrtime;
            kill;
            listenerCount;
            listeners;
            memoryUsage;
            moduleLoadList;
            nextTick;
            off;
            on;
            once;
            openStdin;
            pid = (runtime(process.pid));
            platform = (target(process.platform));
            ppid = (runtime(process.ppid));
            prependListener;
            prependOnceListener;
            rawListeners;
            release;
            removeAllListeners;
            removeListener;
            report;
            resourceUsage;
            reallyExit;
            setMaxListeners;
            setSourceMapsEnabled;
            send = (undefined);
            sourceMapsEnabled = (false);
            stderr;
            stdin;
            stdout;
            title = (runtime(process.title));
            uptime;
            version = ("v22.0.0");
            versions;
        }
    }
}
