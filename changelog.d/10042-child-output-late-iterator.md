- Retain real child stdout/stderr pipe EOF before dispatching end callbacks, so
  async iterators first pulled or created after EOF finish instead of waiting
  forever. Update `readable` and `readableEnded` consistently with that state.
- Add runtime regressions for late readers, delayed first pulls, pending empty
  pulls, and buffered chunks, plus a bounded real-child Node/native parity fixture
  at O0, Os, and Oz. The regression is independent of any application bundle.
