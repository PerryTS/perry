Fix the intermittent timer-heap parity failure (#11148). Register the cross-class
ordering probe in increasing delay order, cancel its interval on the first tick,
and report after all three callbacks. Scheduling pauses can no longer reverse
its intended deadlines or add extra interval ticks. The unref/refresh probe now
reports whether each callback ran instead of depending on their relative order.

Validated with the pinned Node oracle: 100 normal and 100 injected-registration-
pause runs produced identical output. The original fixture changes output with
the same pause, and deliberately dispatching the interval after both timeouts
still changes the fixed fixture's ordering verdict.
