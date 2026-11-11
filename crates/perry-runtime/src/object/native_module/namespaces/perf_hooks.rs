//! Item-level native namespace declarations for `perf_hooks` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_PERF_HOOKS_CONSTANTS = "perf_hooks.constants" {
        cell = "node:perf_hooks.constants";
        names = ["performance.constants", "performance.default.constants"];
        members = {
            NODE_PERFORMANCE_GC_MAJOR = (4.0);
            NODE_PERFORMANCE_GC_MINOR = (1.0);
            NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP = (2.0);
            NODE_PERFORMANCE_GC_INCREMENTAL = (8.0);
            NODE_PERFORMANCE_GC_WEAKCB = (16.0);
            NODE_PERFORMANCE_GC_FLAGS_NO = (0.0);
            NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED = (2.0);
            NODE_PERFORMANCE_GC_FLAGS_FORCED = (4.0);
            NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING = (8.0);
            NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE = (16.0);
            NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY = (32.0);
            NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE = (64.0);
        }
    }
}

crate::realm_decl! {
    namespace NM_PERF_HOOKS = "perf_hooks" {
        default = NM_PERF_HOOKS_DEFAULT;
        members = {
            Performance = (ref(Performance));
            PerformanceEntry = (ref(PerformanceEntry));
            PerformanceMark = (ref(PerformanceMark));
            PerformanceMeasure = (ref(PerformanceMeasure));
            PerformanceObserver = (ref(PerformanceObserver));
            PerformanceObserverEntryList = (ref(PerformanceObserverEntryList));
            PerformanceResourceTiming = (ref(PerformanceResourceTiming));
            monitorEventLoopDelay = (ref(performance.monitorEventLoopDelay));
            eventLoopUtilization = (ref(performance.eventLoopUtilization));
            timerify = (ref(performance.timerify));
            createHistogram = (ref(performance.createHistogram));
            performance = (ref(performance));
            constants = (ref("node:perf_hooks.constants"));
            default = (ref(performance.default));
        }
    }
}

crate::realm_decl! {
    namespace NM_PERF_HOOKS_DEFAULT = "perf_hooks.default" {
        cell = "performance.default";
        members = {
            Performance = (ref(Performance));
            PerformanceEntry = (ref(PerformanceEntry));
            PerformanceMark = (ref(PerformanceMark));
            PerformanceMeasure = (ref(PerformanceMeasure));
            PerformanceObserver = (ref(PerformanceObserver));
            PerformanceObserverEntryList = (ref(PerformanceObserverEntryList));
            PerformanceResourceTiming = (ref(PerformanceResourceTiming));
            monitorEventLoopDelay = (ref(performance.monitorEventLoopDelay));
            eventLoopUtilization = (ref(performance.eventLoopUtilization));
            timerify = (ref(performance.timerify));
            createHistogram = (ref(performance.createHistogram));
            performance = (ref(performance));
            constants;
        }
    }
}

crate::realm_decl! {
    namespace NM_PERF_OBSERVER_LIST = "perf_observer_list" {
        members = {}
    }
}
