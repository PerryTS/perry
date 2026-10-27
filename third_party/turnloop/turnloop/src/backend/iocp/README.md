# Windows IOCP backend

One completion port belongs to each loop. Normal turns have no driver thread.
Requests use core generational IDs, per-direction FIFO queues, fixed operation
storage and a separate pinned kernel slab. Cancellation retains buffers, accepted
sockets and OVERLAPPED storage until acknowledgement; destruction drains pending
I/O before releasing caller memory.

- TCP uses provider-specific AcceptEx/ConnectEx and overlapped receive/send.
  Pooled idle reads arm zero-byte receives, then acquire a lease at readiness.
  Supported IFS handles skip success packets; other providers keep normal IOCP
  notifications. UDP preserves datagram boundaries and source addresses.
- Timed waits use high-resolution waitable timers and dynamically resolved NT
  completion packets. Missing NT exports return an error. GQCSEx is nonalertable
  and has no competing coarse timeout when an exact timer is armed. No global
  timer-period change is made. See the measured decision in
  [WINDOWS_RESULTS.md](../../../../../spikes/iocp/WINDOWS_RESULTS.md).
- Named pipes reserve `max(backlog, 1)` overlapped, byte-mode, local-only server
  instances at listen, and replenish their fixed slots on accept. Kernel storage
  stays separate from mutable metadata. Listener teardown cancels and joins private
  connects; never-reused port keys discard any packets arriving after release.
  Busy client opens submit NPFS `FSCTL_PIPE_WAIT` through `NtFsControlFile`, the overlapped
  availability operation underlying `WaitNamedPipeW`; no worker or retry tick is
  used. `pipe_connect_until` sets an absolute connection deadline. Expiry closes
  out the pending wait through normal cancellation acknowledgement before TimedOut;
  ordinary `pipe_connect` remains unbounded and cancellable.
- Socket passing is
  a private cooperative control protocol: WSADuplicateSocket targets the actual
  peer PID queried from the OS, and send acceptance retains independent socket
  ownership. Do not mix payload reads with receive-handle operations on a control
  stream. This is not a general-purpose untrusted wire protocol.
- IOCP association survives duplication. Known same-port accepted/reattached pipes
  use native IOCP directly; unassociated imports join the receiving port. Only a
  foreign association needs the event bridge. Quiescent transferred sockets and pipe
  endpoints suppress their old port's notifications with the OVERLAPPED event's
  low bit; one-shot registered event waits forward completion to the new port.
  Callbacks are joined before slot reuse. Named-pipe listeners and pending busy
  connections cannot be detached;
  socket listeners and connected pipe endpoints can. Reuse-port is unsupported.
- Child creation uses an explicit inherited handle list, correctly quoted argv,
  Unicode environment and suspended creation. `windows_hide` defaults false;
  true selects SW_HIDE and, without inherited stdio, CREATE_NO_WINDOW, as in libuv.
  Non-detached children join a process-lifetime, non-inheritable kill-on-close job
  with silent breakaway. Optional tree-control jobs (`new_process_group` or
  `detached`) have no kill-on-close limit: normal leader exit/release preserves
  grandchildren. `detached` also selects DETACHED_PROCESS/CREATE_NEW_PROCESS_GROUP
  and excludes the child from the lifetime job. Host jobs can restrict breakaway,
  as with libuv; access denied on lifetime assignment is tolerated. Parent stdio
  ends are overlapped; child ends are synchronous.
  Direct `.bat`/`.cmd` programs are rejected with InvalidInput, including PATH
  resolution; callers must explicitly select a shell for batch scripts.
  One-shot process waits publish to the owning notifier. Close terminates live
  owned children, including detached ones, then acknowledges exit before Closed.
  Loop Drop retains the same explicit ownership rule. uid/gid and non-Kill
  process signals return Unsupported; console signal subscriptions are separate.
- Standard streams are duplicated, preserving the host originals. Synchronous
  handles reserve two workers at adoption, one per direction. Reads and writes
  have independent FIFOs and cancellation state. Workers reuse the console
  classification captured at quiescent adoption; no worker allocation or
  classification syscall is needed per operation.
  Windows serializes all I/O on one synchronous file object: WriteFile on the
  same pipe endpoint waits, with no kernel request of its own and so beyond
  CancelSynchronousIo, until an idle ReadFile returns. For synchronous pipes the
  write worker therefore preempts an idle read with CancelSynchronousIo, and the
  read worker reissues the same request after the write. A cancelled pipe read
  has consumed no bytes, so read FIFO order, exactly-once completion and data are
  preserved, and duplex writes progress before the peer replies. A write waiting
  for the peer to drain its buffer still owns the object: reads on that endpoint
  complete only after it finishes or is cancelled. Writes are never preempted,
  because a cancelled partially consumed pipe write reports zero bytes.
  ReOpenFile cannot obtain an independent overlapped object for pipe ends
  (ERROR_PIPE_BUSY), and FSCTL_PIPE_ASSIGN_EVENT is not supported. libuv's
  non-overlapped pipes are subject to the same kernel constraint.
  Anonymous pipe ends are one-way; console input and screen output use separate
  native objects and are not preempted. Synchronous regular files preserve their
  shared file position, with no cross-direction ordering promise; reads at EOF
  complete rather than waiting for appended data. Other synchronous character
  devices use ReadFile/WriteFile with their native driver's serialization and
  cancellation semantics, without preemption. NUL exercises the ordinary
  character-device path; serial-port and third-party hardware-driver runtime
  behavior is not covered by that test.
  See [sem-fix2 evidence](../../../../../docs/lanes/iocp-semantics.md#sem-fix2).
  Imported handles are classified by native file mode before submission;
  overlapped pipes join this IOCP or route an existing foreign association, and
  other overlapped files are unsupported.
  One process-wide cancellation helper handles the race before a worker enters
  ReadFile, without blocking turns or polling idle handles. A cancellation that
  reports success just after kernel entry can be lost; the helper and the write
  preemption re-issue it after at most 10 ms while the worker is still inside I/O. Console input workers
  translate key records to UTF-8 and dispatch resize records to WinCh subscribers.
  Resize delivery requires the console input reader to be active. Captured console
  modes are restored on close/drop; input and output VT modes are distinguished.
- Console handlers fan out Int/Break/Hup to independent loop subscriptions.
  Unsupported Unix signals fail explicitly. The console handler is installed
  with the first subscription and never removed, as in libuv:
  SetConsoleCtrlHandler blocks while any control handler runs, so removing it
  beside a held close handler would deadlock the host's cleanup. Without a
  matching subscription it returns FALSE, as if absent. Unsubscribed CTRL_CLOSE
  therefore preserves older host handlers. With Hup subscribed, dispatch completes
  before Sleep(INFINITE), matching libuv's cleanup window (Windows normally
  terminates after about five seconds). Sleeping retains no subscription access.
  Windows invokes handlers newest first; this subscribed case prevents older host
  handlers from running. Windows provides no supported way to both continue that
  chain and retain the handler thread. Hup remains best-effort; the host must turn
  and exit within the OS budget. See [the decision and real-close tests](../../../../../docs/lanes/iocp-semantics.md).
- Integration::Event opts into a sole-consumer helper with a bounded queue and
  an auto-reset event. Timer resets carry generations so old forwarded packets
  cannot complete a later deadline. Hosts call turn(Now) after their GUI wait.
  A pump error remains observable on every subsequent turn/integration call,
  including turns with core completions already queued. Drop joins the helper,
  recovers retained packets and drains cancellation using blocking IOCP waits.
  An unrecoverable port/teardown error aborts, preserving buffer safety without spin.
- Core provides timer semantics, liveness, pooled buffers, the parking handshake,
  shared blocking jobs, external waits and executor behavior.

The tests in `turnloop-contract/tests/windows*.rs` instantiate the same shared
scenarios as Unix, plus native console, GUI, cancellation and handle-count checks.
The shared allocation and executor suites also execute on Windows. Run contracts
serially because console state, process resources and allocation counters are
part of the subjects being measured.
