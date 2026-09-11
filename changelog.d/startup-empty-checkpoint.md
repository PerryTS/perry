### Runtime

- Avoid callback-phase and exception-trap setup at empty entry event-loop
  checkpoints. Preserve GC progress and event-loop timing, and retain the full
  pump for queued work, stdin, timers, rejections, and native completions.
