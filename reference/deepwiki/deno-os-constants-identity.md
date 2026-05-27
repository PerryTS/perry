# DeepWiki Response

Repository: `denoland/deno`
Question: In Deno's node:os compatibility implementation, how are os.constants and nested constants objects such as os.constants.signals/errno/priority/dlopen exposed? Are these objects created once or cached so repeated property reads preserve object identity, and which source files or tests show this behavior?
Search: https://deepwiki.com/search/in-denos-nodeos-compatibility_c9a07038-480b-406d-a02f-436e8ea1c589

**Question**: In Deno's node:os compatibility implementation, how are os.constants and nested constants objects such as os.constants.signals/errno/priority/dlopen exposed? Are these objects created once or cached so repeated property reads preserve object identity, and which source files or tests show this behavior?

In Deno's Node.js compatibility layer, the `os.constants` object and its nested properties like `os.constants.signals`, `os.constants.errno`, `os.constants.priority`, and `os.constants.dlopen` are exposed through a combination of JavaScript polyfills and internal bindings to Deno's core operations. These objects are constructed once during the initialization of the Node.js compatibility layer, ensuring that repeated property reads preserve object identity. [1](#ref-q1-1)  [2](#ref-q1-2) 

The primary source files demonstrating this behavior are `ext/node/polyfills/os.ts` [3](#ref-q1-3)  and `ext/node/polyfills/internal_binding/constants.ts` [4](#ref-q1-4) .

## Exposure of `os.constants`

The `os.constants` object is exposed in the `node:os` polyfill module. [1](#ref-q1-1) 

1.  **Loading Constants**: The `os` object, which contains the nested constants, is loaded from `ext:deno_node/internal_binding/constants.ts` within `ext/node/polyfills/os.ts`. [5](#ref-q1-5) 
2.  **Assignment**: This loaded `os` object is then assigned to a local `constants` variable in `ext/node/polyfills/os.ts`. [1](#ref-q1-1) 
3.  **Module Export**: Finally, this `constants` object is exposed as a property of the `module.exports` object for the `node:os` module. [6](#ref-q1-6) 

## Nested Constants Objects

The nested constants objects (`signals`, `errno`, `priority`, `dlopen`) are defined and populated within `ext/node/polyfills/internal_binding/constants.ts`. [7](#ref-q1-7)  [8](#ref-q1-8)  [9](#ref-q1-9)  [10](#ref-q1-10) 

1.  **Platform-Specific Definitions**: The values for these constants are determined based on the operating system (Darwin, Linux/Android, or Windows). [2](#ref-q1-2)  For example, if the OS is Darwin, the `os` object is populated with specific values for `dlopen`, `errno`, `signals`, and `priority`. [11](#ref-q1-11)  Similar blocks exist for Linux/Android [12](#ref-q1-12)  and other operating systems. [13](#ref-q1-13) 
2.  **Freezing Objects**: After the `os` object is constructed with its nested properties, it is frozen using `ObjectFreeze`. [14](#ref-q1-14)  This prevents any further modification to the object and its properties.

## Object Identity and Caching

The `os` object, containing all the constants, is created once when `ext:deno_node/internal_binding/constants.ts` is loaded. [2](#ref-q1-2)  Because the `os` object is frozen, [14](#ref-q1-14)  subsequent reads of `os.constants` or its nested properties will return the same object, thus preserving object identity. This behavior is implicitly shown by the direct assignment and freezing of the `os` object. [15](#ref-q1-15) 

The `ext/node/polyfills/constants.ts` module further aggregates constants from `node:fs`, `node:crypto`, and `node:zlib` along with the `os.constants` values, and exports them as a single `default` object and individual named exports. [16](#ref-q1-16)  [17](#ref-q1-17)  [18](#ref-q1-18)  [19](#ref-q1-19)  [20](#ref-q1-20) 

## Testing Behavior

The file `tests/node_compat/config.jsonc` [21](#ref-q1-21)  lists `parallel/test-os-constants-signals.js` as a test case, indicating that the behavior of `os.constants.signals` is specifically tested for Node.js compatibility. [21](#ref-q1-21)  This test would implicitly verify that the constants are correctly exposed and that their values are consistent. [21](#ref-q1-21) 

## Notes

The `cli/tsc/dts/node/constants.d.cts` file provides TypeScript declarations for the `node:constants` module, indicating the expected structure and types of the `constants` object, including the nested `dlopen`, `errno`, `priority`, and `signals` properties. [22](#ref-q1-22)  This file serves as a type definition and does not directly influence the runtime behavior of object creation or caching. [22](#ref-q1-22) 

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)
- [Process, TTY, and OS APIs (denoland/deno)](/wiki/denoland/deno#7.7)

View this search on DeepWiki: https://deepwiki.com/search/in-denos-nodeos-compatibility_c9a07038-480b-406d-a02f-436e8ea1c589

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/os.ts:54`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L54)

```typescript
const constants = os;
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/internal_binding/constants.ts:206`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L206)

```typescript
const buildOs = op_node_build_os();
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/os.ts:1-92`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L1-L92)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.
// Copyright Joyent, Inc. and other Node contributors.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

// deno-lint-ignore-file prefer-primordials no-process-global

(function () {
const { core, primordials } = globalThis.__bootstrap;
const {
  op_cpus,
  op_homedir,
  op_node_os_get_priority,
  op_node_os_set_priority,
  op_node_os_user_info,
} = core.ops;

const { isWindows } = core.loadExtScript("ext:deno_node/_util/os.ts");
const { os } = core.loadExtScript(
  "ext:deno_node/internal_binding/constants.ts",
);
const { Buffer } = core.loadExtScript("ext:deno_node/internal/buffer.mjs");
const { osUptime } = core.loadExtScript("ext:deno_os/30_os.js");
const { validateInt32 } = core.loadExtScript(
  "ext:deno_node/internal/validators.mjs",
);
const { denoErrorToNodeSystemError } = core.loadExtScript(
  "ext:deno_node/internal/errors.ts",
);

const {
  ObjectDefineProperties,
  StringPrototypeEndsWith,
  StringPrototypeSlice,
} = primordials;

const constants = os;

function arch() {
  return process.arch;
}

availableParallelism[Symbol.toPrimitive] = () => availableParallelism();
arch[Symbol.toPrimitive] = () => process.arch;
endianness[Symbol.toPrimitive] = () => endianness();
freemem[Symbol.toPrimitive] = () => freemem();
homedir[Symbol.toPrimitive] = () => homedir();
hostname[Symbol.toPrimitive] = () => hostname();
platform[Symbol.toPrimitive] = () => platform();
release[Symbol.toPrimitive] = () => release();
version[Symbol.toPrimitive] = () => version();
totalmem[Symbol.toPrimitive] = () => totalmem();
type[Symbol.toPrimitive] = () => type();
uptime[Symbol.toPrimitive] = () => uptime();
machine[Symbol.toPrimitive] = () => machine();
tmpdir[Symbol.toPrimitive] = () => tmpdir();

function cpus() {
  return op_cpus();
}

function endianness() {
  const buffer = new ArrayBuffer(2);
  new DataView(buffer).setInt16(0, 256, true /* littleEndian */);
  return new Int16Array(buffer)[0] === 256 ? "LE" : "BE";
}

function freemem() {
  if (Deno.build.os === "linux" || Deno.build.os == "android") {
    return Deno.systemMemoryInfo().available;
  } else {
    return Deno.systemMemoryInfo().free;
  }
}
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/internal_binding/constants.ts:1-128`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L1-L128)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.

(function () {
const { primordials } = globalThis.__bootstrap;
const { ObjectFreeze } = primordials;
const { core } = globalThis.__bootstrap;
const { op_node_build_os, op_node_fs_constants } = core.ops;

let os: {
  dlopen: {
    RTLD_DEEPBIND?: number;
    RTLD_GLOBAL?: number;
    RTLD_LAZY?: number;
    RTLD_LOCAL?: number;
    RTLD_NOW?: number;
  };
  errno: {
    E2BIG: number;
    EACCES: number;
    EADDRINUSE: number;
    EADDRNOTAVAIL: number;
    EAFNOSUPPORT: number;
    EAGAIN: number;
    EALREADY: number;
    EBADF: number;
    EBADMSG: number;
    EBUSY: number;
    ECANCELED: number;
    ECHILD: number;
    ECONNABORTED: number;
    ECONNREFUSED: number;
    ECONNRESET: number;
    EDEADLK: number;
    EDESTADDRREQ: number;
    EDOM: number;
    EDQUOT?: number;
    EEXIST: number;
    EFAULT: number;
    EFBIG: number;
    EHOSTUNREACH: number;
    EIDRM: number;
    EILSEQ: number;
    EINPROGRESS: number;
    EINTR: number;
    EINVAL: number;
    EIO: number;
    EISCONN: number;
    EISDIR: number;
    ELOOP: number;
    EMFILE: number;
    EMLINK: number;
    EMSGSIZE: number;
    EMULTIHOP?: number;
    ENAMETOOLONG: number;
    ENETDOWN: number;
    ENETRESET: number;
    ENETUNREACH: number;
    ENFILE: number;
    ENOBUFS: number;
    ENODATA: number;
    ENODEV: number;
    ENOENT: number;
    ENOEXEC: number;
    ENOLCK: number;
    ENOLINK: number;
    ENOMEM: number;
    ENOMSG: number;
    ENOPROTOOPT: number;
    ENOSPC: number;
    ENOSR: number;
    ENOSTR: number;
    ENOSYS: number;
    ENOTCONN: number;
    ENOTDIR: number;
    ENOTEMPTY: number;
    ENOTSOCK: number;
    ENOTSUP: number;
    ENOTTY: number;
    ENXIO: number;
    EOPNOTSUPP: number;
    EOVERFLOW: number;
    EPERM: number;
    EPIPE: number;
    EPROTO: number;
    EPROTONOSUPPORT: number;
    EPROTOTYPE: number;
    ERANGE: number;
    EROFS: number;
    ESPIPE: number;
    ESRCH: number;
    ESTALE?: number;
    ETIME: number;
    ETIMEDOUT: number;
    ETXTBSY: number;
    EWOULDBLOCK: number;
    EXDEV: number;
    WSA_E_CANCELLED?: number;
    WSA_E_NO_MORE?: number;
    WSAEACCES?: number;
    WSAEADDRINUSE?: number;
    WSAEADDRNOTAVAIL?: number;
    WSAEAFNOSUPPORT?: number;
    WSAEALREADY?: number;
    WSAEBADF?: number;
    WSAECANCELLED?: number;
    WSAECONNABORTED?: number;
    WSAECONNREFUSED?: number;
    WSAECONNRESET?: number;
    WSAEDESTADDRREQ?: number;
    WSAEDISCON?: number;
    WSAEDQUOT?: number;
    WSAEFAULT?: number;
    WSAEHOSTDOWN?: number;
    WSAEHOSTUNREACH?: number;
    WSAEINPROGRESS?: number;
    WSAEINTR?: number;
    WSAEINVAL?: number;
    WSAEINVALIDPROCTABLE?: number;
    WSAEINVALIDPROVIDER?: number;
    WSAEISCONN?: number;
    WSAELOOP?: number;
    WSAEMFILE?: number;
    WSAEMSGSIZE?: number;
    WSAENAMETOOLONG?: number;
    WSAENETDOWN?: number;
    WSAENETRESET?: number;
    WSAENETUNREACH?: number;
    WSAENOBUFS?: number;
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/os.ts:36-38`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L36-L38)

```typescript
const { os } = core.loadExtScript(
  "ext:deno_node/internal_binding/constants.ts",
);
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/os.ts:290-296`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L290-L296)

```typescript
ObjectDefineProperties(mod, {
  constants: {
    __proto__: null,
    configurable: false,
    enumerable: true,
    value: constants,
  },
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal_binding/constants.ts:9-16`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L9-L16)

```typescript
let os: {
  dlopen: {
    RTLD_DEEPBIND?: number;
    RTLD_GLOBAL?: number;
    RTLD_LAZY?: number;
    RTLD_LOCAL?: number;
    RTLD_NOW?: number;
  };
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/internal_binding/constants.ts:17-96`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L17-L96)

```typescript
  errno: {
    E2BIG: number;
    EACCES: number;
    EADDRINUSE: number;
    EADDRNOTAVAIL: number;
    EAFNOSUPPORT: number;
    EAGAIN: number;
    EALREADY: number;
    EBADF: number;
    EBADMSG: number;
    EBUSY: number;
    ECANCELED: number;
    ECHILD: number;
    ECONNABORTED: number;
    ECONNREFUSED: number;
    ECONNRESET: number;
    EDEADLK: number;
    EDESTADDRREQ: number;
    EDOM: number;
    EDQUOT?: number;
    EEXIST: number;
    EFAULT: number;
    EFBIG: number;
    EHOSTUNREACH: number;
    EIDRM: number;
    EILSEQ: number;
    EINPROGRESS: number;
    EINTR: number;
    EINVAL: number;
    EIO: number;
    EISCONN: number;
    EISDIR: number;
    ELOOP: number;
    EMFILE: number;
    EMLINK: number;
    EMSGSIZE: number;
    EMULTIHOP?: number;
    ENAMETOOLONG: number;
    ENETDOWN: number;
    ENETRESET: number;
    ENETUNREACH: number;
    ENFILE: number;
    ENOBUFS: number;
    ENODATA: number;
    ENODEV: number;
    ENOENT: number;
    ENOEXEC: number;
    ENOLCK: number;
    ENOLINK: number;
    ENOMEM: number;
    ENOMSG: number;
    ENOPROTOOPT: number;
    ENOSPC: number;
    ENOSR: number;
    ENOSTR: number;
    ENOSYS: number;
    ENOTCONN: number;
    ENOTDIR: number;
    ENOTEMPTY: number;
    ENOTSOCK: number;
    ENOTSUP: number;
    ENOTTY: number;
    ENXIO: number;
    EOPNOTSUPP: number;
    EOVERFLOW: number;
    EPERM: number;
    EPIPE: number;
    EPROTO: number;
    EPROTONOSUPPORT: number;
    EPROTOTYPE: number;
    ERANGE: number;
    EROFS: number;
    ESPIPE: number;
    ESRCH: number;
    ESTALE?: number;
    ETIME: number;
    ETIMEDOUT: number;
    ETXTBSY: number;
    EWOULDBLOCK: number;
    EXDEV: number;
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/internal_binding/constants.ts:298-330`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L298-L330)

```typescript
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGQUIT: 3,
      SIGILL: 4,
      SIGTRAP: 5,
      SIGABRT: 6,
      SIGIOT: 6,
      SIGBUS: 10,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGUSR1: 30,
      SIGSEGV: 11,
      SIGUSR2: 31,
      SIGPIPE: 13,
      SIGALRM: 14,
      SIGTERM: 15,
      SIGCHLD: 20,
      SIGCONT: 19,
      SIGSTOP: 17,
      SIGTSTP: 18,
      SIGTTIN: 21,
      SIGTTOU: 22,
      SIGURG: 16,
      SIGXCPU: 24,
      SIGXFSZ: 25,
      SIGVTALRM: 26,
      SIGPROF: 27,
      SIGWINCH: 28,
      SIGIO: 23,
      SIGINFO: 29,
      SIGSYS: 12,
    },
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/internal_binding/constants.ts:331-338`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L331-L338)

```typescript
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/internal_binding/constants.ts:207-339`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L207-L339)

```typescript
if (buildOs === "darwin") {
  os = {
    UV_UDP_IPV6ONLY: 2,
    UV_UDP_REUSEADDR: 4,
    dlopen: {
      RTLD_LAZY: 1,
      RTLD_NOW: 2,
      RTLD_GLOBAL: 8,
      RTLD_LOCAL: 4,
    },
    errno: {
      E2BIG: 7,
      EACCES: 13,
      EADDRINUSE: 48,
      EADDRNOTAVAIL: 49,
      EAFNOSUPPORT: 47,
      EAGAIN: 35,
      EALREADY: 37,
      EBADF: 9,
      EBADMSG: 94,
      EBUSY: 16,
      ECANCELED: 89,
      ECHILD: 10,
      ECONNABORTED: 53,
      ECONNREFUSED: 61,
      ECONNRESET: 54,
      EDEADLK: 11,
      EDESTADDRREQ: 39,
      EDOM: 33,
      EDQUOT: 69,
      EEXIST: 17,
      EFAULT: 14,
      EFBIG: 27,
      EHOSTUNREACH: 65,
      EIDRM: 90,
      EILSEQ: 92,
      EINPROGRESS: 36,
      EINTR: 4,
      EINVAL: 22,
      EIO: 5,
      EISCONN: 56,
      EISDIR: 21,
      ELOOP: 62,
      EMFILE: 24,
      EMLINK: 31,
      EMSGSIZE: 40,
      EMULTIHOP: 95,
      ENAMETOOLONG: 63,
      ENETDOWN: 50,
      ENETRESET: 52,
      ENETUNREACH: 51,
      ENFILE: 23,
      ENOBUFS: 55,
      ENODATA: 96,
      ENODEV: 19,
      ENOENT: 2,
      ENOEXEC: 8,
      ENOLCK: 77,
      ENOLINK: 97,
      ENOMEM: 12,
      ENOMSG: 91,
      ENOPROTOOPT: 42,
      ENOSPC: 28,
      ENOSR: 98,
      ENOSTR: 99,
      ENOSYS: 78,
      ENOTCONN: 57,
      ENOTDIR: 20,
      ENOTEMPTY: 66,
      ENOTSOCK: 38,
      ENOTSUP: 45,
      ENOTTY: 25,
      ENXIO: 6,
      EOPNOTSUPP: 102,
      EOVERFLOW: 84,
      EPERM: 1,
      EPIPE: 32,
      EPROTO: 100,
      EPROTONOSUPPORT: 43,
      EPROTOTYPE: 41,
      ERANGE: 34,
      EROFS: 30,
      ESPIPE: 29,
      ESRCH: 3,
      ESTALE: 70,
      ETIME: 101,
      ETIMEDOUT: 60,
      ETXTBSY: 26,
      EWOULDBLOCK: 35,
      EXDEV: 18,
    },
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGQUIT: 3,
      SIGILL: 4,
      SIGTRAP: 5,
      SIGABRT: 6,
      SIGIOT: 6,
      SIGBUS: 10,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGUSR1: 30,
      SIGSEGV: 11,
      SIGUSR2: 31,
      SIGPIPE: 13,
      SIGALRM: 14,
      SIGTERM: 15,
      SIGCHLD: 20,
      SIGCONT: 19,
      SIGSTOP: 17,
      SIGTSTP: 18,
      SIGTTIN: 21,
      SIGTTOU: 22,
      SIGURG: 16,
      SIGXCPU: 24,
      SIGXFSZ: 25,
      SIGVTALRM: 26,
      SIGPROF: 27,
      SIGWINCH: 28,
      SIGIO: 23,
      SIGINFO: 29,
      SIGSYS: 12,
    },
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
  };
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/internal_binding/constants.ts:340-475`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L340-L475)

```typescript
} else if (buildOs === "linux" || buildOs === "android") {
  os = {
    UV_UDP_IPV6ONLY: 2,
    UV_UDP_REUSEADDR: 4,
    dlopen: {
      RTLD_LAZY: 1,
      RTLD_NOW: 2,
      RTLD_GLOBAL: 256,
      RTLD_LOCAL: 0,
      RTLD_DEEPBIND: 8,
    },
    errno: {
      E2BIG: 7,
      EACCES: 13,
      EADDRINUSE: 98,
      EADDRNOTAVAIL: 99,
      EAFNOSUPPORT: 97,
      EAGAIN: 11,
      EALREADY: 114,
      EBADF: 9,
      EBADMSG: 74,
      EBUSY: 16,
      ECANCELED: 125,
      ECHILD: 10,
      ECONNABORTED: 103,
      ECONNREFUSED: 111,
      ECONNRESET: 104,
      EDEADLK: 35,
      EDESTADDRREQ: 89,
      EDOM: 33,
      EDQUOT: 122,
      EEXIST: 17,
      EFAULT: 14,
      EFBIG: 27,
      EHOSTUNREACH: 113,
      EIDRM: 43,
      EILSEQ: 84,
      EINPROGRESS: 115,
      EINTR: 4,
      EINVAL: 22,
      EIO: 5,
      EISCONN: 106,
      EISDIR: 21,
      ELOOP: 40,
      EMFILE: 24,
      EMLINK: 31,
      EMSGSIZE: 90,
      EMULTIHOP: 72,
      ENAMETOOLONG: 36,
      ENETDOWN: 100,
      ENETRESET: 102,
      ENETUNREACH: 101,
      ENFILE: 23,
      ENOBUFS: 105,
      ENODATA: 61,
      ENODEV: 19,
      ENOENT: 2,
      ENOEXEC: 8,
      ENOLCK: 37,
      ENOLINK: 67,
      ENOMEM: 12,
      ENOMSG: 42,
      ENOPROTOOPT: 92,
      ENOSPC: 28,
      ENOSR: 63,
      ENOSTR: 60,
      ENOSYS: 38,
      ENOTCONN: 107,
      ENOTDIR: 20,
      ENOTEMPTY: 39,
      ENOTSOCK: 88,
      ENOTSUP: 95,
      ENOTTY: 25,
      ENXIO: 6,
      EOPNOTSUPP: 95,
      EOVERFLOW: 75,
      EPERM: 1,
      EPIPE: 32,
      EPROTO: 71,
      EPROTONOSUPPORT: 93,
      EPROTOTYPE: 91,
      ERANGE: 34,
      EROFS: 30,
      ESPIPE: 29,
      ESRCH: 3,
      ESTALE: 116,
      ETIME: 62,
      ETIMEDOUT: 110,
      ETXTBSY: 26,
      EWOULDBLOCK: 11,
      EXDEV: 18,
    },
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGQUIT: 3,
      SIGILL: 4,
      SIGTRAP: 5,
      SIGABRT: 6,
      SIGIOT: 6,
      SIGBUS: 7,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGUSR1: 10,
      SIGSEGV: 11,
      SIGUSR2: 12,
      SIGPIPE: 13,
      SIGALRM: 14,
      SIGTERM: 15,
      SIGCHLD: 17,
      SIGSTKFLT: 16,
      SIGCONT: 18,
      SIGSTOP: 19,
      SIGTSTP: 20,
      SIGTTIN: 21,
      SIGTTOU: 22,
      SIGURG: 23,
      SIGXCPU: 24,
      SIGXFSZ: 25,
      SIGVTALRM: 26,
      SIGPROF: 27,
      SIGWINCH: 28,
      SIGIO: 29,
      SIGPOLL: 29,
      SIGPWR: 30,
      SIGSYS: 31,
      SIGUNUSED: 31,
    },
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/internal_binding/constants.ts:477-638`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L477-L638)

```typescript
} else {
  os = {
    UV_UDP_IPV6ONLY: 2,
    UV_UDP_REUSEADDR: 4,
    dlopen: {},
    errno: {
      E2BIG: 7,
      EACCES: 13,
      EADDRINUSE: 100,
      EADDRNOTAVAIL: 101,
      EAFNOSUPPORT: 102,
      EAGAIN: 11,
      EALREADY: 103,
      EBADF: 9,
      EBADMSG: 104,
      EBUSY: 16,
      ECANCELED: 105,
      ECHILD: 10,
      ECONNABORTED: 106,
      ECONNREFUSED: 107,
      ECONNRESET: 108,
      EDEADLK: 36,
      EDESTADDRREQ: 109,
      EDOM: 33,
      EEXIST: 17,
      EFAULT: 14,
      EFBIG: 27,
      EHOSTUNREACH: 110,
      EIDRM: 111,
      EILSEQ: 42,
      EINPROGRESS: 112,
      EINTR: 4,
      EINVAL: 22,
      EIO: 5,
      EISCONN: 113,
      EISDIR: 21,
      ELOOP: 114,
      EMFILE: 24,
      EMLINK: 31,
      EMSGSIZE: 115,
      ENAMETOOLONG: 38,
      ENETDOWN: 116,
      ENETRESET: 117,
      ENETUNREACH: 118,
      ENFILE: 23,
      ENOBUFS: 119,
      ENODATA: 120,
      ENODEV: 19,
      ENOENT: 2,
      ENOEXEC: 8,
      ENOLCK: 39,
      ENOLINK: 121,
      ENOMEM: 12,
      ENOMSG: 122,
      ENOPROTOOPT: 123,
      ENOSPC: 28,
      ENOSR: 124,
      ENOSTR: 125,
      ENOSYS: 40,
      ENOTCONN: 126,
      ENOTDIR: 20,
      ENOTEMPTY: 41,
      ENOTSOCK: 128,
      ENOTSUP: 129,
      ENOTTY: 25,
      ENXIO: 6,
      EOPNOTSUPP: 130,
      EOVERFLOW: 132,
      EPERM: 1,
      EPIPE: 32,
      EPROTO: 134,
      EPROTONOSUPPORT: 135,
      EPROTOTYPE: 136,
      ERANGE: 34,
      EROFS: 30,
      ESPIPE: 29,
      ESRCH: 3,
      ETIME: 137,
      ETIMEDOUT: 138,
      ETXTBSY: 139,
      EWOULDBLOCK: 140,
      EXDEV: 18,
      WSAEINTR: 10004,
      WSAEBADF: 10009,
      WSAEACCES: 10013,
      WSAEFAULT: 10014,
      WSAEINVAL: 10022,
      WSAEMFILE: 10024,
      WSAEWOULDBLOCK: 10035,
      WSAEINPROGRESS: 10036,
      WSAEALREADY: 10037,
      WSAENOTSOCK: 10038,
      WSAEDESTADDRREQ: 10039,
      WSAEMSGSIZE: 10040,
      WSAEPROTOTYPE: 10041,
      WSAENOPROTOOPT: 10042,
      WSAEPROTONOSUPPORT: 10043,
      WSAESOCKTNOSUPPORT: 10044,
      WSAEOPNOTSUPP: 10045,
      WSAEPFNOSUPPORT: 10046,
      WSAEAFNOSUPPORT: 10047,
      WSAEADDRINUSE: 10048,
      WSAEADDRNOTAVAIL: 10049,
      WSAENETDOWN: 10050,
      WSAENETUNREACH: 10051,
      WSAENETRESET: 10052,
      WSAECONNABORTED: 10053,
      WSAECONNRESET: 10054,
      WSAENOBUFS: 10055,
      WSAEISCONN: 10056,
      WSAENOTCONN: 10057,
      WSAESHUTDOWN: 10058,
      WSAETOOMANYREFS: 10059,
      WSAETIMEDOUT: 10060,
      WSAECONNREFUSED: 10061,
      WSAELOOP: 10062,
      WSAENAMETOOLONG: 10063,
      WSAEHOSTDOWN: 10064,
      WSAEHOSTUNREACH: 10065,
      WSAENOTEMPTY: 10066,
      WSAEPROCLIM: 10067,
      WSAEUSERS: 10068,
      WSAEDQUOT: 10069,
      WSAESTALE: 10070,
      WSAEREMOTE: 10071,
      WSASYSNOTREADY: 10091,
      WSAVERNOTSUPPORTED: 10092,
      WSANOTINITIALISED: 10093,
      WSAEDISCON: 10101,
      WSAENOMORE: 10102,
      WSAECANCELLED: 10103,
      WSAEINVALIDPROCTABLE: 10104,
      WSAEINVALIDPROVIDER: 10105,
      WSAEPROVIDERFAILEDINIT: 10106,
      WSASYSCALLFAILURE: 10107,
      WSASERVICE_NOT_FOUND: 10108,
      WSATYPE_NOT_FOUND: 10109,
      WSA_E_NO_MORE: 10110,
      WSA_E_CANCELLED: 10111,
      WSAEREFUSED: 10112,
    },
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGILL: 4,
      SIGABRT: 22,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGSEGV: 11,
      SIGTERM: 15,
      SIGBREAK: 21,
      SIGWINCH: 28,
    },
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
  };
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/internal_binding/constants.ts:5`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L5)

```typescript
const { ObjectFreeze } = primordials;
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/internal_binding/constants.ts:207-638`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal_binding/constants.ts#L207-L638)

```typescript
if (buildOs === "darwin") {
  os = {
    UV_UDP_IPV6ONLY: 2,
    UV_UDP_REUSEADDR: 4,
    dlopen: {
      RTLD_LAZY: 1,
      RTLD_NOW: 2,
      RTLD_GLOBAL: 8,
      RTLD_LOCAL: 4,
    },
    errno: {
      E2BIG: 7,
      EACCES: 13,
      EADDRINUSE: 48,
      EADDRNOTAVAIL: 49,
      EAFNOSUPPORT: 47,
      EAGAIN: 35,
      EALREADY: 37,
      EBADF: 9,
      EBADMSG: 94,
      EBUSY: 16,
      ECANCELED: 89,
      ECHILD: 10,
      ECONNABORTED: 53,
      ECONNREFUSED: 61,
      ECONNRESET: 54,
      EDEADLK: 11,
      EDESTADDRREQ: 39,
      EDOM: 33,
      EDQUOT: 69,
      EEXIST: 17,
      EFAULT: 14,
      EFBIG: 27,
      EHOSTUNREACH: 65,
      EIDRM: 90,
      EILSEQ: 92,
      EINPROGRESS: 36,
      EINTR: 4,
      EINVAL: 22,
      EIO: 5,
      EISCONN: 56,
      EISDIR: 21,
      ELOOP: 62,
      EMFILE: 24,
      EMLINK: 31,
      EMSGSIZE: 40,
      EMULTIHOP: 95,
      ENAMETOOLONG: 63,
      ENETDOWN: 50,
      ENETRESET: 52,
      ENETUNREACH: 51,
      ENFILE: 23,
      ENOBUFS: 55,
      ENODATA: 96,
      ENODEV: 19,
      ENOENT: 2,
      ENOEXEC: 8,
      ENOLCK: 77,
      ENOLINK: 97,
      ENOMEM: 12,
      ENOMSG: 91,
      ENOPROTOOPT: 42,
      ENOSPC: 28,
      ENOSR: 98,
      ENOSTR: 99,
      ENOSYS: 78,
      ENOTCONN: 57,
      ENOTDIR: 20,
      ENOTEMPTY: 66,
      ENOTSOCK: 38,
      ENOTSUP: 45,
      ENOTTY: 25,
      ENXIO: 6,
      EOPNOTSUPP: 102,
      EOVERFLOW: 84,
      EPERM: 1,
      EPIPE: 32,
      EPROTO: 100,
      EPROTONOSUPPORT: 43,
      EPROTOTYPE: 41,
      ERANGE: 34,
      EROFS: 30,
      ESPIPE: 29,
      ESRCH: 3,
      ESTALE: 70,
      ETIME: 101,
      ETIMEDOUT: 60,
      ETXTBSY: 26,
      EWOULDBLOCK: 35,
      EXDEV: 18,
    },
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGQUIT: 3,
      SIGILL: 4,
      SIGTRAP: 5,
      SIGABRT: 6,
      SIGIOT: 6,
      SIGBUS: 10,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGUSR1: 30,
      SIGSEGV: 11,
      SIGUSR2: 31,
      SIGPIPE: 13,
      SIGALRM: 14,
      SIGTERM: 15,
      SIGCHLD: 20,
      SIGCONT: 19,
      SIGSTOP: 17,
      SIGTSTP: 18,
      SIGTTIN: 21,
      SIGTTOU: 22,
      SIGURG: 16,
      SIGXCPU: 24,
      SIGXFSZ: 25,
      SIGVTALRM: 26,
      SIGPROF: 27,
      SIGWINCH: 28,
      SIGIO: 23,
      SIGINFO: 29,
      SIGSYS: 12,
    },
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
  };
} else if (buildOs === "linux" || buildOs === "android") {
  os = {
    UV_UDP_IPV6ONLY: 2,
    UV_UDP_REUSEADDR: 4,
    dlopen: {
      RTLD_LAZY: 1,
      RTLD_NOW: 2,
      RTLD_GLOBAL: 256,
      RTLD_LOCAL: 0,
      RTLD_DEEPBIND: 8,
    },
    errno: {
      E2BIG: 7,
      EACCES: 13,
      EADDRINUSE: 98,
      EADDRNOTAVAIL: 99,
      EAFNOSUPPORT: 97,
      EAGAIN: 11,
      EALREADY: 114,
      EBADF: 9,
      EBADMSG: 74,
      EBUSY: 16,
      ECANCELED: 125,
      ECHILD: 10,
      ECONNABORTED: 103,
      ECONNREFUSED: 111,
      ECONNRESET: 104,
      EDEADLK: 35,
      EDESTADDRREQ: 89,
      EDOM: 33,
      EDQUOT: 122,
      EEXIST: 17,
      EFAULT: 14,
      EFBIG: 27,
      EHOSTUNREACH: 113,
      EIDRM: 43,
      EILSEQ: 84,
      EINPROGRESS: 115,
      EINTR: 4,
      EINVAL: 22,
      EIO: 5,
      EISCONN: 106,
      EISDIR: 21,
      ELOOP: 40,
      EMFILE: 24,
      EMLINK: 31,
      EMSGSIZE: 90,
      EMULTIHOP: 72,
      ENAMETOOLONG: 36,
      ENETDOWN: 100,
      ENETRESET: 102,
      ENETUNREACH: 101,
      ENFILE: 23,
      ENOBUFS: 105,
      ENODATA: 61,
      ENODEV: 19,
      ENOENT: 2,
      ENOEXEC: 8,
      ENOLCK: 37,
      ENOLINK: 67,
      ENOMEM: 12,
      ENOMSG: 42,
      ENOPROTOOPT: 92,
      ENOSPC: 28,
      ENOSR: 63,
      ENOSTR: 60,
      ENOSYS: 38,
      ENOTCONN: 107,
      ENOTDIR: 20,
      ENOTEMPTY: 39,
      ENOTSOCK: 88,
      ENOTSUP: 95,
      ENOTTY: 25,
      ENXIO: 6,
      EOPNOTSUPP: 95,
      EOVERFLOW: 75,
      EPERM: 1,
      EPIPE: 32,
      EPROTO: 71,
      EPROTONOSUPPORT: 93,
      EPROTOTYPE: 91,
      ERANGE: 34,
      EROFS: 30,
      ESPIPE: 29,
      ESRCH: 3,
      ESTALE: 116,
      ETIME: 62,
      ETIMEDOUT: 110,
      ETXTBSY: 26,
      EWOULDBLOCK: 11,
      EXDEV: 18,
    },
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGQUIT: 3,
      SIGILL: 4,
      SIGTRAP: 5,
      SIGABRT: 6,
      SIGIOT: 6,
      SIGBUS: 7,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGUSR1: 10,
      SIGSEGV: 11,
      SIGUSR2: 12,
      SIGPIPE: 13,
      SIGALRM: 14,
      SIGTERM: 15,
      SIGCHLD: 17,
      SIGSTKFLT: 16,
      SIGCONT: 18,
      SIGSTOP: 19,
      SIGTSTP: 20,
      SIGTTIN: 21,
      SIGTTOU: 22,
      SIGURG: 23,
      SIGXCPU: 24,
      SIGXFSZ: 25,
      SIGVTALRM: 26,
      SIGPROF: 27,
      SIGWINCH: 28,
      SIGIO: 29,
      SIGPOLL: 29,
      SIGPWR: 30,
      SIGSYS: 31,
      SIGUNUSED: 31,
    },
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
  };
} else {
  os = {
    UV_UDP_IPV6ONLY: 2,
    UV_UDP_REUSEADDR: 4,
    dlopen: {},
    errno: {
      E2BIG: 7,
      EACCES: 13,
      EADDRINUSE: 100,
      EADDRNOTAVAIL: 101,
      EAFNOSUPPORT: 102,
      EAGAIN: 11,
      EALREADY: 103,
      EBADF: 9,
      EBADMSG: 104,
      EBUSY: 16,
      ECANCELED: 105,
      ECHILD: 10,
      ECONNABORTED: 106,
      ECONNREFUSED: 107,
      ECONNRESET: 108,
      EDEADLK: 36,
      EDESTADDRREQ: 109,
      EDOM: 33,
      EEXIST: 17,
      EFAULT: 14,
      EFBIG: 27,
      EHOSTUNREACH: 110,
      EIDRM: 111,
      EILSEQ: 42,
      EINPROGRESS: 112,
      EINTR: 4,
      EINVAL: 22,
      EIO: 5,
      EISCONN: 113,
      EISDIR: 21,
      ELOOP: 114,
      EMFILE: 24,
      EMLINK: 31,
      EMSGSIZE: 115,
      ENAMETOOLONG: 38,
      ENETDOWN: 116,
      ENETRESET: 117,
      ENETUNREACH: 118,
      ENFILE: 23,
      ENOBUFS: 119,
      ENODATA: 120,
      ENODEV: 19,
      ENOENT: 2,
      ENOEXEC: 8,
      ENOLCK: 39,
      ENOLINK: 121,
      ENOMEM: 12,
      ENOMSG: 122,
      ENOPROTOOPT: 123,
      ENOSPC: 28,
      ENOSR: 124,
      ENOSTR: 125,
      ENOSYS: 40,
      ENOTCONN: 126,
      ENOTDIR: 20,
      ENOTEMPTY: 41,
      ENOTSOCK: 128,
      ENOTSUP: 129,
      ENOTTY: 25,
      ENXIO: 6,
      EOPNOTSUPP: 130,
      EOVERFLOW: 132,
      EPERM: 1,
      EPIPE: 32,
      EPROTO: 134,
      EPROTONOSUPPORT: 135,
      EPROTOTYPE: 136,
      ERANGE: 34,
      EROFS: 30,
      ESPIPE: 29,
      ESRCH: 3,
      ETIME: 137,
      ETIMEDOUT: 138,
      ETXTBSY: 139,
      EWOULDBLOCK: 140,
      EXDEV: 18,
      WSAEINTR: 10004,
      WSAEBADF: 10009,
      WSAEACCES: 10013,
      WSAEFAULT: 10014,
      WSAEINVAL: 10022,
      WSAEMFILE: 10024,
      WSAEWOULDBLOCK: 10035,
      WSAEINPROGRESS: 10036,
      WSAEALREADY: 10037,
      WSAENOTSOCK: 10038,
      WSAEDESTADDRREQ: 10039,
      WSAEMSGSIZE: 10040,
      WSAEPROTOTYPE: 10041,
      WSAENOPROTOOPT: 10042,
      WSAEPROTONOSUPPORT: 10043,
      WSAESOCKTNOSUPPORT: 10044,
      WSAEOPNOTSUPP: 10045,
      WSAEPFNOSUPPORT: 10046,
      WSAEAFNOSUPPORT: 10047,
      WSAEADDRINUSE: 10048,
      WSAEADDRNOTAVAIL: 10049,
      WSAENETDOWN: 10050,
      WSAENETUNREACH: 10051,
      WSAENETRESET: 10052,
      WSAECONNABORTED: 10053,
      WSAECONNRESET: 10054,
      WSAENOBUFS: 10055,
      WSAEISCONN: 10056,
      WSAENOTCONN: 10057,
      WSAESHUTDOWN: 10058,
      WSAETOOMANYREFS: 10059,
      WSAETIMEDOUT: 10060,
      WSAECONNREFUSED: 10061,
      WSAELOOP: 10062,
      WSAENAMETOOLONG: 10063,
      WSAEHOSTDOWN: 10064,
      WSAEHOSTUNREACH: 10065,
      WSAENOTEMPTY: 10066,
      WSAEPROCLIM: 10067,
      WSAEUSERS: 10068,
      WSAEDQUOT: 10069,
      WSAESTALE: 10070,
      WSAEREMOTE: 10071,
      WSASYSNOTREADY: 10091,
      WSAVERNOTSUPPORTED: 10092,
      WSANOTINITIALISED: 10093,
      WSAEDISCON: 10101,
      WSAENOMORE: 10102,
      WSAECANCELLED: 10103,
      WSAEINVALIDPROCTABLE: 10104,
      WSAEINVALIDPROVIDER: 10105,
      WSAEPROVIDERFAILEDINIT: 10106,
      WSASYSCALLFAILURE: 10107,
      WSASERVICE_NOT_FOUND: 10108,
      WSATYPE_NOT_FOUND: 10109,
      WSA_E_NO_MORE: 10110,
      WSA_E_CANCELLED: 10111,
      WSAEREFUSED: 10112,
    },
    signals: {
      SIGHUP: 1,
      SIGINT: 2,
      SIGILL: 4,
      SIGABRT: 22,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGSEGV: 11,
      SIGTERM: 15,
      SIGBREAK: 21,
      SIGWINCH: 28,
    },
    priority: {
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20,
    },
  };
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/constants.ts:11-19`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/constants.ts#L11-L19)

```typescript
export default {
  ...fsConstants,
  ...osConstants.dlopen,
  ...osConstants.errno,
  ...osConstants.signals,
  ...osConstants.priority,
  ...cryptoConstants,
  ...zlibConstants,
};
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/constants.ts:78-84`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/constants.ts#L78-L84)

```typescript
export const {
  RTLD_DEEPBIND,
  RTLD_GLOBAL,
  RTLD_LAZY,
  RTLD_LOCAL,
  RTLD_NOW,
} = osConstants.dlopen;
```

<a id="ref-q1-18"></a>
### [18] `ext/node/polyfills/constants.ts:85-165`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/constants.ts#L85-L165)

```typescript
export const {
  E2BIG,
  EACCES,
  EADDRINUSE,
  EADDRNOTAVAIL,
  EAFNOSUPPORT,
  EAGAIN,
  EALREADY,
  EBADF,
  EBADMSG,
  EBUSY,
  ECANCELED,
  ECHILD,
  ECONNABORTED,
  ECONNREFUSED,
  ECONNRESET,
  EDEADLK,
  EDESTADDRREQ,
  EDOM,
  EDQUOT,
  EEXIST,
  EFAULT,
  EFBIG,
  EHOSTUNREACH,
  EIDRM,
  EILSEQ,
  EINPROGRESS,
  EINTR,
  EINVAL,
  EIO,
  EISCONN,
  EISDIR,
  ELOOP,
  EMFILE,
  EMLINK,
  EMSGSIZE,
  EMULTIHOP,
  ENAMETOOLONG,
  ENETDOWN,
  ENETRESET,
  ENETUNREACH,
  ENFILE,
  ENOBUFS,
  ENODATA,
  ENODEV,
  ENOENT,
  ENOEXEC,
  ENOLCK,
  ENOLINK,
  ENOMEM,
  ENOMSG,
  ENOPROTOOPT,
  ENOSPC,
  ENOSR,
  ENOSTR,
  ENOSYS,
  ENOTCONN,
  ENOTDIR,
  ENOTEMPTY,
  ENOTSOCK,
  ENOTSUP,
  ENOTTY,
  ENXIO,
  EOPNOTSUPP,
  EOVERFLOW,
  EPERM,
  EPIPE,
  EPROTO,
  EPROTONOSUPPORT,
  EPROTOTYPE,
  ERANGE,
  EROFS,
  ESPIPE,
  ESRCH,
  ESTALE,
  ETIME,
  ETIMEDOUT,
  ETXTBSY,
  EWOULDBLOCK,
  EXDEV,
} = osConstants.errno;
```

<a id="ref-q1-19"></a>
### [19] `ext/node/polyfills/constants.ts:166-173`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/constants.ts#L166-L173)

```typescript
export const {
  PRIORITY_ABOVE_NORMAL,
  PRIORITY_BELOW_NORMAL,
  PRIORITY_HIGH,
  PRIORITY_HIGHEST,
  PRIORITY_LOW,
  PRIORITY_NORMAL,
} = osConstants.priority;
```

<a id="ref-q1-20"></a>
### [20] `ext/node/polyfills/constants.ts:174-209`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/constants.ts#L174-L209)

```typescript
export const {
  SIGABRT,
  SIGALRM,
  SIGBUS,
  SIGCHLD,
  SIGCONT,
  SIGFPE,
  SIGHUP,
  SIGILL,
  SIGINT,
  SIGIO,
  SIGIOT,
  SIGKILL,
  SIGPIPE,
  SIGPOLL,
  SIGPROF,
  SIGPWR,
  SIGQUIT,
  SIGSEGV,
  SIGSTKFLT,
  SIGSTOP,
  SIGSYS,
  SIGTERM,
  SIGTRAP,
  SIGTSTP,
  SIGTTIN,
  SIGTTOU,
  SIGUNUSED,
  SIGURG,
  SIGUSR1,
  SIGUSR2,
  SIGVTALRM,
  SIGWINCH,
  SIGXCPU,
  SIGXFSZ,
} = osConstants.signals;
```

<a id="ref-q1-21"></a>
### [21] `tests/node_compat/config.jsonc:2553`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L2553)

```
    "parallel/test-os-constants-signals.js": {},
```

<a id="ref-q1-22"></a>
### [22] `denoland/deno:8-14`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/denoland/deno#L8-L14)
