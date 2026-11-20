Fixed native handle ids aliasing between perry-stdlib and the `perry-ext-*`
wrappers (#11196). perry-stdlib's common handle registry (StringDecoder, crypto
hashes, AsyncLocalStorage, the bundled `EventEmitter`, and others) had its own
id counter over the same `[1, 0x40000)` band that perry-ffi uses. Every ext
wrapper, net sockets and servers included, allocates from perry-ffi. So the
first stdlib handle and the first ext handle were both `1`, and any "is this
id mine?" check answered for the wrong object.

Two symptoms:

- Prebuilt archives (`PERRY_NO_AUTO_OPTIMIZE=1`, where the stdlib keeps
  `bundled-events`): `events.once(socket, 'connect')` found the bundled
  EventEmitter that shared the socket's id and queued its promise there. It
  never listened on the socket, so the promise never settled and
  redis@6.1.0's `client.connect()` hung.
- Auto-optimized builds: `new StringDecoder().write(buf)` ran
  `net.Socket#write` on the socket that shared its id and returned `false`.

Common ids now come from perry-ffi's shared numeric pool
(`perry_ffi::shared_handle_id_pool()`), under the common registry's own
domain. ext-net's reserved socket ids already worked this way. Retiring a
common id now tombstones it (`finish_retirement_permanently`). This keeps the
old never-reused behaviour: the old private quarantine was never drained, but
perry-ffi drains the shared pool on every tick. `perry-native-registration`
gains `begin_registration_with_id_in_domain` and
`finish_retirement_permanently`.

Gap test: `test-files/test_gap_common_ffi_handle_ids_distinct.ts`.
