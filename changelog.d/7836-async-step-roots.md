**Async functions no longer retain pre-collection Promise addresses while a
suspended step resumes.** Async step calls keep activation and ambient trap
pointers in mutable roots, preventing a moving collection inside user code from
using stale pre-collection addresses after the step returns.
