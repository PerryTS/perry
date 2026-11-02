`PerformanceObserver.observe()` no longer recurses through native method
dispatch until the process exhausts its stack. Perry now records the built-in
`perf_hooks` class hierarchy as class-default prototype wiring instead of
misclassifying it as a user `Object.setPrototypeOf` override. This also keeps
`PerformanceMark`, `PerformanceMeasure`, `PerformanceResourceTiming`, and
observer entry-list instances on the ordinary built-in dispatch path.
