- Retain real child stdout/stderr pipe EOF before dispatching end callbacks, so
  async iterators first pulled or created after EOF finish instead of waiting
  forever. Update `readable` and `readableEnded` consistently with that state.
- Finish empty output pipes after a failed spawn as well: no live reactor entry
  exists to deliver their EOF. This prevents ENOENT/EACCES cleanup from hanging
  while awaiting stdout/stderr or extra-pipe collectors.
- Schedule failed-spawn close after its error callback, preventing overdue
  close timers from reversing error/end ordering during slow startup.
- Root and reload child-event receivers and arguments across listener callbacks,
  including forwarding to the shared stream listener registry after EOF.
- Add runtime regressions for late readers, delayed first pulls, pending empty
  pulls, and buffered chunks, plus a bounded real-child Node/native parity fixture
  at O0, Os, and Oz. The regression is independent of any application bundle.
