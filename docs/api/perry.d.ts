// Auto-generated from Perry's API manifest (#465). Do not edit by hand.
// Source: perry-api-manifest::API_MANIFEST
// Coverage: 1981 entries across 119 modules

type PerryU32 = number & { readonly __perryU32?: never };
type PerryU64 = number & { readonly __perryU64?: never };
type PerryUSize = number & { readonly __perryUSize?: never };
type PerryI32 = number & { readonly __perryI32?: never };
type PerryI64 = number & { readonly __perryI64?: never };
type PerryF32 = number & { readonly __perryF32?: never };
type PerryF64 = number & { readonly __perryF64?: never };
type PerryBufferLen = number & { readonly __perryBufferLen?: never };
type PerryHandleId = number & { readonly __perryHandleId?: never };
type PerryPod<T> = T & { readonly __perryPod?: never };
type NativeMemoryTypedView = Int8Array | Uint8Array | Uint8ClampedArray | Int16Array | Uint16Array | Int32Array | Uint32Array | Float32Array | Float64Array;
declare function sizeof<T extends PerryPod<any>>(): number;
declare function alignof<T extends PerryPod<any>>(): number;
declare function offsetof<T extends PerryPod<any>>(field: string): number;
interface PerryPodView<T> {
  readonly length: number;
  readonly [index: number]: T;
  readonly __perryPodView?: never;
}
interface NativeArena {
  view(kind: typeof Int8Array, byteOffset: number, length: number): Int8Array;
  view(kind: typeof Uint8Array, byteOffset: number, length: number): Uint8Array;
  view(kind: typeof Uint8ClampedArray, byteOffset: number, length: number): Uint8ClampedArray;
  view(kind: typeof Int16Array, byteOffset: number, length: number): Int16Array;
  view(kind: typeof Uint16Array, byteOffset: number, length: number): Uint16Array;
  view(kind: typeof Int32Array, byteOffset: number, length: number): Int32Array;
  view(kind: typeof Uint32Array, byteOffset: number, length: number): Uint32Array;
  view(kind: typeof Float32Array, byteOffset: number, length: number): Float32Array;
  view(kind: typeof Float64Array, byteOffset: number, length: number): Float64Array;
  view(kind: "Int8Array", byteOffset: number, length: number): Int8Array;
  view(kind: "Uint8Array", byteOffset: number, length: number): Uint8Array;
  view(kind: "Uint8ClampedArray", byteOffset: number, length: number): Uint8ClampedArray;
  view(kind: "Int16Array", byteOffset: number, length: number): Int16Array;
  view(kind: "Uint16Array", byteOffset: number, length: number): Uint16Array;
  view(kind: "Int32Array", byteOffset: number, length: number): Int32Array;
  view(kind: "Uint32Array", byteOffset: number, length: number): Uint32Array;
  view(kind: "Float32Array", byteOffset: number, length: number): Float32Array;
  view(kind: "Float64Array", byteOffset: number, length: number): Float64Array;
  podView<T extends PerryPod<any>>(byteOffset: number, count: number): PerryPodView<T>;
  dispose(): void;
}
interface NativeArenaConstructor {
  alloc(byteLength: number): NativeArena;
}
declare const NativeArena: NativeArenaConstructor;
interface NativeMemoryConstructor {
  fillU32(view: Uint32Array, value: number): void;
  copy(dst: NativeMemoryTypedView, src: NativeMemoryTypedView): void;
}
declare const NativeMemory: NativeMemoryConstructor;

declare module "@lydell/node-pty" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function spawn(...args: any[]): any;
}

declare module "@perryts/pdf" {
  /** stdlib */
  export function createPdf(opts: any): number;
  /** stdlib */
  export function pdfAddLine(...args: any[]): any;
  /** stdlib */
  export function pdfAddText(...args: any[]): any;
  /** stdlib */
  export function pdfNewPage(...args: any[]): any;
  /** stdlib */
  export function pdfSave(...args: any[]): any;
}

declare module "argon2" {
  /** stdlib */
  export function hash(password: string): any;
  /** stdlib */
  export function verify(hash: string, password: string): any;
}

declare module "assert" {
  /** stdlib */
  export class Assert { [key: string]: any; }
  /** stdlib */
  export class AssertionError { [key: string]: any; }
  /** stdlib */
  export const strict: any;
  /** stdlib */
  export function deepEqual(...args: any[]): any;
  /** stdlib */
  export function deepStrictEqual(...args: any[]): any;
  /** stdlib */
  export default function (...args: any[]): any;
  /** stdlib */
  export function doesNotMatch(...args: any[]): any;
  /** stdlib */
  export function doesNotReject(...args: any[]): any;
  /** stdlib */
  export function doesNotThrow(...args: any[]): any;
  /** stdlib */
  export function equal(...args: any[]): any;
  /** stdlib */
  export function fail(...args: any[]): any;
  /** stdlib */
  export function ifError(...args: any[]): any;
  /** stdlib */
  export function match(...args: any[]): any;
  /** stdlib */
  export function notDeepEqual(...args: any[]): any;
  /** stdlib */
  export function notDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function notEqual(...args: any[]): any;
  /** stdlib */
  export function notStrictEqual(...args: any[]): any;
  /** stdlib */
  export function ok(...args: any[]): any;
  /** stdlib */
  export function partialDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function rejects(...args: any[]): any;
  /** stdlib */
  export function strict(...args: any[]): any;
  /** stdlib */
  export function strictEqual(...args: any[]): any;
  /** stdlib */
  export function throws(...args: any[]): any;
}

declare module "assert/strict" {
  /** stdlib */
  export class Assert { [key: string]: any; }
  /** stdlib */
  export class AssertionError { [key: string]: any; }
  /** stdlib */
  export const strict: any;
  /** stdlib */
  export function deepEqual(...args: any[]): any;
  /** stdlib */
  export function deepStrictEqual(...args: any[]): any;
  /** stdlib */
  export default function (...args: any[]): any;
  /** stdlib */
  export function doesNotMatch(...args: any[]): any;
  /** stdlib */
  export function doesNotReject(...args: any[]): any;
  /** stdlib */
  export function doesNotThrow(...args: any[]): any;
  /** stdlib */
  export function equal(...args: any[]): any;
  /** stdlib */
  export function fail(...args: any[]): any;
  /** stdlib */
  export function ifError(...args: any[]): any;
  /** stdlib */
  export function match(...args: any[]): any;
  /** stdlib */
  export function notDeepEqual(...args: any[]): any;
  /** stdlib */
  export function notDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function notEqual(...args: any[]): any;
  /** stdlib */
  export function notStrictEqual(...args: any[]): any;
  /** stdlib */
  export function ok(...args: any[]): any;
  /** stdlib */
  export function partialDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function rejects(...args: any[]): any;
  /** stdlib */
  export function strict(...args: any[]): any;
  /** stdlib */
  export function strictEqual(...args: any[]): any;
  /** stdlib */
  export function throws(...args: any[]): any;
}

declare module "async_hooks" {
  /** stdlib */
  export class AsyncLocalStorage { [key: string]: any; }
  /** stdlib */
  export class AsyncResource { [key: string]: any; }
  /** stdlib */
  export const asyncWrapProviders: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function createHook(...args: any[]): any;
  /** stdlib */
  export function executionAsyncId(...args: any[]): any;
  /** stdlib */
  export function executionAsyncResource(...args: any[]): any;
  /** stdlib */
  export function triggerAsyncId(...args: any[]): any;
}

declare module "axios" {
  /** stdlib */
  export function all(...args: any[]): any;
  /** stdlib */
  export function create(...args: any[]): any;
  /** stdlib */
  export default function (...args: any[]): any;
  /** stdlib */
  function _delete(...args: any[]): any;
  export { _delete as delete };
  /** stdlib */
  export function get(...args: any[]): any;
  /** stdlib */
  export function head(...args: any[]): any;
  /** stdlib */
  export function options(...args: any[]): any;
  /** stdlib */
  export function patch(...args: any[]): any;
  /** stdlib */
  export function post(...args: any[]): any;
  /** stdlib */
  export function put(...args: any[]): any;
  /** stdlib */
  export function request(...args: any[]): any;
}

declare module "bcrypt" {
  /** stdlib */
  export function compare(plaintext: string, hash: string): any;
  /** stdlib */
  export function hash(password: string, saltOrRounds: any): any;
}

declare module "better-sqlite3" {
  /** stdlib */
  export default function (p0: string): any;
}

declare module "bignumber.js" {
  /** stdlib */
  export class BigNumber { [key: string]: any; }
}

declare module "buffer" {
  /** stdlib */
  export class Blob { [key: string]: any; }
  /** stdlib */
  export class Buffer { [key: string]: any; }
  /** stdlib */
  export class File { [key: string]: any; }
  /** stdlib */
  export const INSPECT_MAX_BYTES: any;
  /** stdlib */
  export const constants: any;
  /** stdlib */
  export const kMaxLength: any;
  /** stdlib */
  export const kStringMaxLength: any;
  /** stdlib */
  export function atob(...args: any[]): any;
  /** stdlib */
  export function btoa(...args: any[]): any;
  /** stdlib */
  export function isAscii(...args: any[]): any;
  /** stdlib */
  export function isUtf8(...args: any[]): any;
  /** stdlib */
  export function resolveObjectURL(...args: any[]): any;
  /** stdlib */
  export function transcode(...args: any[]): any;
}

declare module "bun" {
  /** stdlib */
  export const stderr: any;
  /** stdlib */
  export const stdin: any;
  /** stdlib */
  export const stdout: any;
  /** stdlib */
  export function file(...args: any[]): any;
  /** stdlib */
  export function fileURLToPath(...args: any[]): any;
  /** stdlib */
  export function hash(...args: any[]): any;
  /** stdlib */
  export function pathToFileURL(...args: any[]): any;
  /** stdlib */
  export function stringWidth(...args: any[]): any;
  /** stdlib */
  export function write(...args: any[]): any;
}

declare module "bun:ffi" {
  /** stdlib */
  export const FFIType: any;
  /** stdlib */
  export const suffix: any;
  /** stdlib @perryStub stage 3 — not yet implemented, throws at runtime (#6562) */
  export function CFunction(...args: any[]): any;
  /** stdlib */
  export function CString(...args: any[]): any;
  /** stdlib @perryStub stage 3 — not yet implemented, throws at runtime (#6562) */
  export function JSCallback(...args: any[]): any;
  /** stdlib */
  export function dlopen(...args: any[]): any;
  /** stdlib @perryStub stage ≥2 — not yet implemented, throws at runtime (#6562) */
  export function linkSymbols(...args: any[]): any;
  /** stdlib */
  export function ptr(...args: any[]): any;
  /** stdlib @perryStub stage ≥2 — not yet implemented, throws at runtime (#6562) */
  export function read(...args: any[]): any;
  /** stdlib @perryStub stage 2 — not yet implemented, throws at runtime (#6562) */
  export function toArrayBuffer(...args: any[]): any;
  /** stdlib @perryStub stage 2 — not yet implemented, throws at runtime (#6562) */
  export function toBuffer(...args: any[]): any;
  /** stdlib @perryStub stage ≥2 — not yet implemented, throws at runtime (#6562) */
  export function viewSource(...args: any[]): any;
}

declare module "cheerio" {
  /** stdlib */
  export function load(p0: string): any;
}

declare module "child_process" {
  /** stdlib */
  export class ChildProcess { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function _forkChild(...args: any[]): any;
  /** stdlib */
  export function exec(...args: any[]): any;
  /** stdlib */
  export function execFile(...args: any[]): any;
  /** stdlib */
  export function execFileSync(...args: any[]): any;
  /** stdlib */
  export function execSync(...args: any[]): any;
  /** stdlib */
  export function fork(...args: any[]): any;
  /** stdlib */
  export function spawn(...args: any[]): any;
  /** stdlib */
  export function spawnSync(...args: any[]): any;
}

declare module "cluster" {
  /** stdlib */
  export class Worker { [key: string]: any; }
  /** stdlib */
  export const SCHED_NONE: any;
  /** stdlib */
  export const SCHED_RR: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const isMaster: any;
  /** stdlib */
  export const isPrimary: any;
  /** stdlib */
  export const isWorker: any;
  /** stdlib */
  export const schedulingPolicy: any;
  /** stdlib */
  export const settings: any;
  /** stdlib */
  export const workers: any;
  /** stdlib */
  export function disconnect(...args: any[]): any;
  /** stdlib */
  export function fork(...args: any[]): any;
  /** stdlib */
  export function setupMaster(...args: any[]): any;
  /** stdlib */
  export function setupPrimary(...args: any[]): any;
}

declare module "commander" {
  /** stdlib */
  export const args: any;
}

declare module "console" {
  /** stdlib */
  export class Console { [key: string]: any; }
  /** stdlib */
  export function assert(...args: any[]): any;
  /** stdlib */
  export function clear(...args: any[]): any;
  /** stdlib */
  export function context(...args: any[]): any;
  /** stdlib */
  export function count(...args: any[]): any;
  /** stdlib */
  export function countReset(...args: any[]): any;
  /** stdlib */
  export function createTask(...args: any[]): any;
  /** stdlib */
  export function debug(...args: any[]): any;
  /** stdlib */
  export function dir(...args: any[]): any;
  /** stdlib */
  export function dirxml(...args: any[]): any;
  /** stdlib */
  export function error(...args: any[]): any;
  /** stdlib */
  export function group(...args: any[]): any;
  /** stdlib */
  export function groupCollapsed(...args: any[]): any;
  /** stdlib */
  export function groupEnd(...args: any[]): any;
  /** stdlib */
  export function info(...args: any[]): any;
  /** stdlib */
  export function log(...args: any[]): any;
  /** stdlib */
  export function profile(...args: any[]): any;
  /** stdlib */
  export function profileEnd(...args: any[]): any;
  /** stdlib */
  export function table(...args: any[]): any;
  /** stdlib */
  export function time(...args: any[]): any;
  /** stdlib */
  export function timeEnd(...args: any[]): any;
  /** stdlib */
  export function timeLog(...args: any[]): any;
  /** stdlib */
  export function timeStamp(...args: any[]): any;
  /** stdlib */
  export function trace(...args: any[]): any;
  /** stdlib */
  export function warn(...args: any[]): any;
}

declare module "constants" {
  /** stdlib */
  export const COPYFILE_EXCL: any;
  /** stdlib */
  export const COPYFILE_FICLONE: any;
  /** stdlib */
  export const COPYFILE_FICLONE_FORCE: any;
  /** stdlib */
  export const DH_CHECK_P_NOT_PRIME: any;
  /** stdlib */
  export const DH_CHECK_P_NOT_SAFE_PRIME: any;
  /** stdlib */
  export const DH_NOT_SUITABLE_GENERATOR: any;
  /** stdlib */
  export const DH_UNABLE_TO_CHECK_GENERATOR: any;
  /** stdlib */
  export const E2BIG: any;
  /** stdlib */
  export const EACCES: any;
  /** stdlib */
  export const EADDRINUSE: any;
  /** stdlib */
  export const EADDRNOTAVAIL: any;
  /** stdlib */
  export const EAFNOSUPPORT: any;
  /** stdlib */
  export const EAGAIN: any;
  /** stdlib */
  export const EALREADY: any;
  /** stdlib */
  export const EBADF: any;
  /** stdlib */
  export const EBADMSG: any;
  /** stdlib */
  export const EBUSY: any;
  /** stdlib */
  export const ECANCELED: any;
  /** stdlib */
  export const ECHILD: any;
  /** stdlib */
  export const ECONNABORTED: any;
  /** stdlib */
  export const ECONNREFUSED: any;
  /** stdlib */
  export const ECONNRESET: any;
  /** stdlib */
  export const EDEADLK: any;
  /** stdlib */
  export const EDESTADDRREQ: any;
  /** stdlib */
  export const EDOM: any;
  /** stdlib */
  export const EDQUOT: any;
  /** stdlib */
  export const EEXIST: any;
  /** stdlib */
  export const EFAULT: any;
  /** stdlib */
  export const EFBIG: any;
  /** stdlib */
  export const EHOSTUNREACH: any;
  /** stdlib */
  export const EIDRM: any;
  /** stdlib */
  export const EILSEQ: any;
  /** stdlib */
  export const EINPROGRESS: any;
  /** stdlib */
  export const EINTR: any;
  /** stdlib */
  export const EINVAL: any;
  /** stdlib */
  export const EIO: any;
  /** stdlib */
  export const EISCONN: any;
  /** stdlib */
  export const EISDIR: any;
  /** stdlib */
  export const ELOOP: any;
  /** stdlib */
  export const EMFILE: any;
  /** stdlib */
  export const EMLINK: any;
  /** stdlib */
  export const EMSGSIZE: any;
  /** stdlib */
  export const EMULTIHOP: any;
  /** stdlib */
  export const ENAMETOOLONG: any;
  /** stdlib */
  export const ENETDOWN: any;
  /** stdlib */
  export const ENETRESET: any;
  /** stdlib */
  export const ENETUNREACH: any;
  /** stdlib */
  export const ENFILE: any;
  /** stdlib */
  export const ENGINE_METHOD_ALL: any;
  /** stdlib */
  export const ENGINE_METHOD_CIPHERS: any;
  /** stdlib */
  export const ENGINE_METHOD_DH: any;
  /** stdlib */
  export const ENGINE_METHOD_DIGESTS: any;
  /** stdlib */
  export const ENGINE_METHOD_DSA: any;
  /** stdlib */
  export const ENGINE_METHOD_EC: any;
  /** stdlib */
  export const ENGINE_METHOD_NONE: any;
  /** stdlib */
  export const ENGINE_METHOD_PKEY_ASN1_METHS: any;
  /** stdlib */
  export const ENGINE_METHOD_PKEY_METHS: any;
  /** stdlib */
  export const ENGINE_METHOD_RAND: any;
  /** stdlib */
  export const ENGINE_METHOD_RSA: any;
  /** stdlib */
  export const ENOBUFS: any;
  /** stdlib */
  export const ENODATA: any;
  /** stdlib */
  export const ENODEV: any;
  /** stdlib */
  export const ENOENT: any;
  /** stdlib */
  export const ENOEXEC: any;
  /** stdlib */
  export const ENOLCK: any;
  /** stdlib */
  export const ENOLINK: any;
  /** stdlib */
  export const ENOMEM: any;
  /** stdlib */
  export const ENOMSG: any;
  /** stdlib */
  export const ENOPROTOOPT: any;
  /** stdlib */
  export const ENOSPC: any;
  /** stdlib */
  export const ENOSR: any;
  /** stdlib */
  export const ENOSTR: any;
  /** stdlib */
  export const ENOSYS: any;
  /** stdlib */
  export const ENOTCONN: any;
  /** stdlib */
  export const ENOTDIR: any;
  /** stdlib */
  export const ENOTEMPTY: any;
  /** stdlib */
  export const ENOTSOCK: any;
  /** stdlib */
  export const ENOTSUP: any;
  /** stdlib */
  export const ENOTTY: any;
  /** stdlib */
  export const ENXIO: any;
  /** stdlib */
  export const EOPNOTSUPP: any;
  /** stdlib */
  export const EOVERFLOW: any;
  /** stdlib */
  export const EPERM: any;
  /** stdlib */
  export const EPIPE: any;
  /** stdlib */
  export const EPROTO: any;
  /** stdlib */
  export const EPROTONOSUPPORT: any;
  /** stdlib */
  export const EPROTOTYPE: any;
  /** stdlib */
  export const ERANGE: any;
  /** stdlib */
  export const EROFS: any;
  /** stdlib */
  export const ESPIPE: any;
  /** stdlib */
  export const ESRCH: any;
  /** stdlib */
  export const ESTALE: any;
  /** stdlib */
  export const ETIME: any;
  /** stdlib */
  export const ETIMEDOUT: any;
  /** stdlib */
  export const ETXTBSY: any;
  /** stdlib */
  export const EWOULDBLOCK: any;
  /** stdlib */
  export const EXDEV: any;
  /** stdlib */
  export const F_OK: any;
  /** stdlib */
  export const OPENSSL_VERSION_NUMBER: any;
  /** stdlib */
  export const O_APPEND: any;
  /** stdlib */
  export const O_CREAT: any;
  /** stdlib */
  export const O_DIRECT: any;
  /** stdlib */
  export const O_DIRECTORY: any;
  /** stdlib */
  export const O_DSYNC: any;
  /** stdlib */
  export const O_EXCL: any;
  /** stdlib */
  export const O_NOATIME: any;
  /** stdlib */
  export const O_NOCTTY: any;
  /** stdlib */
  export const O_NOFOLLOW: any;
  /** stdlib */
  export const O_NONBLOCK: any;
  /** stdlib */
  export const O_RDONLY: any;
  /** stdlib */
  export const O_RDWR: any;
  /** stdlib */
  export const O_SYMLINK: any;
  /** stdlib */
  export const O_SYNC: any;
  /** stdlib */
  export const O_TRUNC: any;
  /** stdlib */
  export const O_WRONLY: any;
  /** stdlib */
  export const POINT_CONVERSION_COMPRESSED: any;
  /** stdlib */
  export const POINT_CONVERSION_HYBRID: any;
  /** stdlib */
  export const POINT_CONVERSION_UNCOMPRESSED: any;
  /** stdlib */
  export const PRIORITY_ABOVE_NORMAL: any;
  /** stdlib */
  export const PRIORITY_BELOW_NORMAL: any;
  /** stdlib */
  export const PRIORITY_HIGH: any;
  /** stdlib */
  export const PRIORITY_HIGHEST: any;
  /** stdlib */
  export const PRIORITY_LOW: any;
  /** stdlib */
  export const PRIORITY_NORMAL: any;
  /** stdlib */
  export const RSA_NO_PADDING: any;
  /** stdlib */
  export const RSA_PKCS1_OAEP_PADDING: any;
  /** stdlib */
  export const RSA_PKCS1_PADDING: any;
  /** stdlib */
  export const RSA_PKCS1_PSS_PADDING: any;
  /** stdlib */
  export const RSA_PSS_SALTLEN_AUTO: any;
  /** stdlib */
  export const RSA_PSS_SALTLEN_DIGEST: any;
  /** stdlib */
  export const RSA_PSS_SALTLEN_MAX_SIGN: any;
  /** stdlib */
  export const RSA_X931_PADDING: any;
  /** stdlib */
  export const RTLD_DEEPBIND: any;
  /** stdlib */
  export const RTLD_GLOBAL: any;
  /** stdlib */
  export const RTLD_LAZY: any;
  /** stdlib */
  export const RTLD_LOCAL: any;
  /** stdlib */
  export const RTLD_NOW: any;
  /** stdlib */
  export const R_OK: any;
  /** stdlib */
  export const SIGABRT: any;
  /** stdlib */
  export const SIGALRM: any;
  /** stdlib */
  export const SIGBUS: any;
  /** stdlib */
  export const SIGCHLD: any;
  /** stdlib */
  export const SIGCONT: any;
  /** stdlib */
  export const SIGFPE: any;
  /** stdlib */
  export const SIGHUP: any;
  /** stdlib */
  export const SIGILL: any;
  /** stdlib */
  export const SIGINFO: any;
  /** stdlib */
  export const SIGINT: any;
  /** stdlib */
  export const SIGIO: any;
  /** stdlib */
  export const SIGIOT: any;
  /** stdlib */
  export const SIGKILL: any;
  /** stdlib */
  export const SIGPIPE: any;
  /** stdlib */
  export const SIGPOLL: any;
  /** stdlib */
  export const SIGPROF: any;
  /** stdlib */
  export const SIGPWR: any;
  /** stdlib */
  export const SIGQUIT: any;
  /** stdlib */
  export const SIGSEGV: any;
  /** stdlib */
  export const SIGSTKFLT: any;
  /** stdlib */
  export const SIGSTOP: any;
  /** stdlib */
  export const SIGSYS: any;
  /** stdlib */
  export const SIGTERM: any;
  /** stdlib */
  export const SIGTRAP: any;
  /** stdlib */
  export const SIGTSTP: any;
  /** stdlib */
  export const SIGTTIN: any;
  /** stdlib */
  export const SIGTTOU: any;
  /** stdlib */
  export const SIGURG: any;
  /** stdlib */
  export const SIGUSR1: any;
  /** stdlib */
  export const SIGUSR2: any;
  /** stdlib */
  export const SIGVTALRM: any;
  /** stdlib */
  export const SIGWINCH: any;
  /** stdlib */
  export const SIGXCPU: any;
  /** stdlib */
  export const SIGXFSZ: any;
  /** stdlib */
  export const SSL_OP_ALL: any;
  /** stdlib */
  export const SSL_OP_ALLOW_NO_DHE_KEX: any;
  /** stdlib */
  export const SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION: any;
  /** stdlib */
  export const SSL_OP_CIPHER_SERVER_PREFERENCE: any;
  /** stdlib */
  export const SSL_OP_CISCO_ANYCONNECT: any;
  /** stdlib */
  export const SSL_OP_COOKIE_EXCHANGE: any;
  /** stdlib */
  export const SSL_OP_CRYPTOPRO_TLSEXT_BUG: any;
  /** stdlib */
  export const SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS: any;
  /** stdlib */
  export const SSL_OP_LEGACY_SERVER_CONNECT: any;
  /** stdlib */
  export const SSL_OP_NO_COMPRESSION: any;
  /** stdlib */
  export const SSL_OP_NO_ENCRYPT_THEN_MAC: any;
  /** stdlib */
  export const SSL_OP_NO_QUERY_MTU: any;
  /** stdlib */
  export const SSL_OP_NO_RENEGOTIATION: any;
  /** stdlib */
  export const SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION: any;
  /** stdlib */
  export const SSL_OP_NO_SSLv2: any;
  /** stdlib */
  export const SSL_OP_NO_SSLv3: any;
  /** stdlib */
  export const SSL_OP_NO_TICKET: any;
  /** stdlib */
  export const SSL_OP_NO_TLSv1: any;
  /** stdlib */
  export const SSL_OP_NO_TLSv1_1: any;
  /** stdlib */
  export const SSL_OP_NO_TLSv1_2: any;
  /** stdlib */
  export const SSL_OP_NO_TLSv1_3: any;
  /** stdlib */
  export const SSL_OP_PRIORITIZE_CHACHA: any;
  /** stdlib */
  export const SSL_OP_TLS_ROLLBACK_BUG: any;
  /** stdlib */
  export const S_IFBLK: any;
  /** stdlib */
  export const S_IFCHR: any;
  /** stdlib */
  export const S_IFDIR: any;
  /** stdlib */
  export const S_IFIFO: any;
  /** stdlib */
  export const S_IFLNK: any;
  /** stdlib */
  export const S_IFMT: any;
  /** stdlib */
  export const S_IFREG: any;
  /** stdlib */
  export const S_IFSOCK: any;
  /** stdlib */
  export const S_IRGRP: any;
  /** stdlib */
  export const S_IROTH: any;
  /** stdlib */
  export const S_IRUSR: any;
  /** stdlib */
  export const S_IRWXG: any;
  /** stdlib */
  export const S_IRWXO: any;
  /** stdlib */
  export const S_IRWXU: any;
  /** stdlib */
  export const S_IWGRP: any;
  /** stdlib */
  export const S_IWOTH: any;
  /** stdlib */
  export const S_IWUSR: any;
  /** stdlib */
  export const S_IXGRP: any;
  /** stdlib */
  export const S_IXOTH: any;
  /** stdlib */
  export const S_IXUSR: any;
  /** stdlib */
  export const TLS1_1_VERSION: any;
  /** stdlib */
  export const TLS1_2_VERSION: any;
  /** stdlib */
  export const TLS1_3_VERSION: any;
  /** stdlib */
  export const TLS1_VERSION: any;
  /** stdlib */
  export const UV_DIRENT_BLOCK: any;
  /** stdlib */
  export const UV_DIRENT_CHAR: any;
  /** stdlib */
  export const UV_DIRENT_DIR: any;
  /** stdlib */
  export const UV_DIRENT_FIFO: any;
  /** stdlib */
  export const UV_DIRENT_FILE: any;
  /** stdlib */
  export const UV_DIRENT_LINK: any;
  /** stdlib */
  export const UV_DIRENT_SOCKET: any;
  /** stdlib */
  export const UV_DIRENT_UNKNOWN: any;
  /** stdlib */
  export const UV_FS_COPYFILE_EXCL: any;
  /** stdlib */
  export const UV_FS_COPYFILE_FICLONE: any;
  /** stdlib */
  export const UV_FS_COPYFILE_FICLONE_FORCE: any;
  /** stdlib */
  export const UV_FS_O_FILEMAP: any;
  /** stdlib */
  export const UV_FS_SYMLINK_DIR: any;
  /** stdlib */
  export const UV_FS_SYMLINK_JUNCTION: any;
  /** stdlib */
  export const W_OK: any;
  /** stdlib */
  export const X_OK: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const defaultCoreCipherList: any;
}

declare module "cron" {
  /** stdlib */
  export class CronJob { [key: string]: any; }
  /** stdlib */
  export function describe(expr: string): string;
  /** stdlib */
  export function schedule(expr: string, handler: any): any;
  /** stdlib */
  export function validate(expr: string): boolean;
}

declare module "crypto" {
  /** stdlib */
  export class Cipheriv { [key: string]: any; }
  /** stdlib */
  export class Decipheriv { [key: string]: any; }
  /** stdlib */
  export class DiffieHellman { [key: string]: any; }
  /** stdlib */
  export class DiffieHellmanGroup { [key: string]: any; }
  /** stdlib */
  export class ECDH { [key: string]: any; }
  /** stdlib */
  export class KeyObject { [key: string]: any; }
  /** stdlib */
  export class X509Certificate { [key: string]: any; }
  /** stdlib */
  export const Certificate: any;
  /** stdlib */
  export const constants: any;
  /** stdlib */
  export const subtle: any;
  /** stdlib */
  export const webcrypto: any;
  /** stdlib */
  export function Hash(...args: any[]): any;
  /** stdlib */
  export function Hmac(...args: any[]): any;
  /** stdlib */
  export function Sign(...args: any[]): any;
  /** stdlib */
  export function Verify(...args: any[]): any;
  /** stdlib */
  export function argon2(...args: any[]): any;
  /** stdlib */
  export function argon2Sync(...args: any[]): any;
  /** stdlib */
  export function checkPrime(...args: any[]): any;
  /** stdlib */
  export function checkPrimeSync(...args: any[]): any;
  /** stdlib */
  export function createCipheriv(...args: any[]): any;
  /** stdlib */
  export function createDecipheriv(...args: any[]): any;
  /** stdlib */
  export function createDiffieHellman(...args: any[]): any;
  /** stdlib */
  export function createDiffieHellmanGroup(...args: any[]): any;
  /** stdlib */
  export function createECDH(...args: any[]): any;
  /** stdlib */
  export function createHash(...args: any[]): any;
  /** stdlib */
  export function createHmac(...args: any[]): any;
  /** stdlib */
  export function createPrivateKey(...args: any[]): any;
  /** stdlib */
  export function createPublicKey(...args: any[]): any;
  /** stdlib */
  export function createSecretKey(...args: any[]): any;
  /** stdlib */
  export function createSign(...args: any[]): any;
  /** stdlib */
  export function createVerify(...args: any[]): any;
  /** stdlib */
  export function decapsulate(...args: any[]): any;
  /** stdlib */
  export function diffieHellman(...args: any[]): any;
  /** stdlib */
  export function encapsulate(...args: any[]): any;
  /** stdlib */
  export function generateKey(...args: any[]): any;
  /** stdlib */
  export function generateKeyPair(...args: any[]): any;
  /** stdlib */
  export function generateKeyPairSync(...args: any[]): any;
  /** stdlib */
  export function generateKeySync(...args: any[]): any;
  /** stdlib */
  export function generatePrime(...args: any[]): any;
  /** stdlib */
  export function generatePrimeSync(...args: any[]): any;
  /** stdlib */
  export function getCipherInfo(...args: any[]): any;
  /** stdlib */
  export function getCiphers(...args: any[]): any;
  /** stdlib */
  export function getCurves(...args: any[]): any;
  /** stdlib */
  export function getDiffieHellman(...args: any[]): any;
  /** stdlib */
  export function getFips(...args: any[]): any;
  /** stdlib */
  export function getHashes(...args: any[]): any;
  /** stdlib */
  export function getRandomValues(...args: any[]): any;
  /** stdlib */
  export function hash(...args: any[]): any;
  /** stdlib */
  export function hkdf(...args: any[]): any;
  /** stdlib */
  export function hkdfSync(...args: any[]): any;
  /** stdlib */
  export function pbkdf2(...args: any[]): any;
  /** stdlib */
  export function pbkdf2Sync(...args: any[]): any;
  /** stdlib */
  export function privateDecrypt(...args: any[]): any;
  /** stdlib */
  export function privateEncrypt(...args: any[]): any;
  /** stdlib */
  export function publicDecrypt(...args: any[]): any;
  /** stdlib */
  export function publicEncrypt(...args: any[]): any;
  /** stdlib */
  export function randomBytes(...args: any[]): any;
  /** stdlib */
  export function randomFill(...args: any[]): any;
  /** stdlib */
  export function randomFillSync(...args: any[]): any;
  /** stdlib */
  export function randomInt(...args: any[]): any;
  /** stdlib */
  export function randomUUID(...args: any[]): any;
  /** stdlib */
  export function scrypt(...args: any[]): any;
  /** stdlib */
  export function scryptSync(...args: any[]): any;
  /** stdlib */
  export function secureHeapUsed(...args: any[]): any;
  /** stdlib */
  export function setFips(...args: any[]): any;
  /** stdlib */
  export function sign(...args: any[]): any;
  /** stdlib */
  export function timingSafeEqual(...args: any[]): any;
  /** stdlib */
  export function verify(...args: any[]): any;
}

declare module "date-fns" {
  /** stdlib */
  export function addDays(...args: any[]): any;
  /** stdlib */
  export function addMonths(...args: any[]): any;
  /** stdlib */
  export function addYears(...args: any[]): any;
  /** stdlib */
  export function differenceInDays(...args: any[]): any;
  /** stdlib */
  export function differenceInHours(...args: any[]): any;
  /** stdlib */
  export function differenceInMinutes(...args: any[]): any;
  /** stdlib */
  export function endOfDay(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function isAfter(...args: any[]): any;
  /** stdlib */
  export function isBefore(...args: any[]): any;
  /** stdlib */
  export function parseISO(...args: any[]): any;
  /** stdlib */
  export function startOfDay(...args: any[]): any;
}

declare module "dayjs" {
  /** stdlib */
  export function dayjs(input?: any): any;
  /** stdlib */
  export default function (input?: any): any;
}

declare module "dgram" {
  /** stdlib */
  export class Socket { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function Socket(...args: any[]): any;
  /** stdlib */
  export function createSocket(...args: any[]): any;
}

declare module "diagnostics_channel" {
  /** stdlib */
  export class BoundedChannel { [key: string]: any; }
  /** stdlib */
  export class Channel { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function boundedChannel(...args: any[]): any;
  /** stdlib */
  export function channel(...args: any[]): any;
  /** stdlib */
  export function hasSubscribers(...args: any[]): any;
  /** stdlib */
  export function subscribe(...args: any[]): any;
  /** stdlib */
  export function tracingChannel(...args: any[]): any;
  /** stdlib */
  export function unsubscribe(...args: any[]): any;
}

declare module "dns" {
  /** stdlib */
  export class Resolver { [key: string]: any; }
  /** stdlib */
  export const ADDRCONFIG: any;
  /** stdlib */
  export const ADDRCONFIG: any;
  /** stdlib */
  export const ADDRGETNETWORKPARAMS: any;
  /** stdlib */
  export const ADDRGETNETWORKPARAMS: any;
  /** stdlib */
  export const ALL: any;
  /** stdlib */
  export const ALL: any;
  /** stdlib */
  export const BADFAMILY: any;
  /** stdlib */
  export const BADFAMILY: any;
  /** stdlib */
  export const BADFLAGS: any;
  /** stdlib */
  export const BADFLAGS: any;
  /** stdlib */
  export const BADHINTS: any;
  /** stdlib */
  export const BADHINTS: any;
  /** stdlib */
  export const BADNAME: any;
  /** stdlib */
  export const BADNAME: any;
  /** stdlib */
  export const BADQUERY: any;
  /** stdlib */
  export const BADQUERY: any;
  /** stdlib */
  export const BADRESP: any;
  /** stdlib */
  export const BADRESP: any;
  /** stdlib */
  export const BADSTR: any;
  /** stdlib */
  export const BADSTR: any;
  /** stdlib */
  export const CANCELLED: any;
  /** stdlib */
  export const CANCELLED: any;
  /** stdlib */
  export const CONNREFUSED: any;
  /** stdlib */
  export const CONNREFUSED: any;
  /** stdlib */
  export const DESTRUCTION: any;
  /** stdlib */
  export const DESTRUCTION: any;
  /** stdlib */
  export const EOF: any;
  /** stdlib */
  export const EOF: any;
  /** stdlib */
  export const FILE: any;
  /** stdlib */
  export const FILE: any;
  /** stdlib */
  export const FORMERR: any;
  /** stdlib */
  export const FORMERR: any;
  /** stdlib */
  export const LOADIPHLPAPI: any;
  /** stdlib */
  export const LOADIPHLPAPI: any;
  /** stdlib */
  export const NODATA: any;
  /** stdlib */
  export const NODATA: any;
  /** stdlib */
  export const NOMEM: any;
  /** stdlib */
  export const NOMEM: any;
  /** stdlib */
  export const NONAME: any;
  /** stdlib */
  export const NONAME: any;
  /** stdlib */
  export const NOTFOUND: any;
  /** stdlib */
  export const NOTFOUND: any;
  /** stdlib */
  export const NOTIMP: any;
  /** stdlib */
  export const NOTIMP: any;
  /** stdlib */
  export const NOTINITIALIZED: any;
  /** stdlib */
  export const NOTINITIALIZED: any;
  /** stdlib */
  export const REFUSED: any;
  /** stdlib */
  export const REFUSED: any;
  /** stdlib */
  export const SERVFAIL: any;
  /** stdlib */
  export const SERVFAIL: any;
  /** stdlib */
  export const TIMEOUT: any;
  /** stdlib */
  export const TIMEOUT: any;
  /** stdlib */
  export const V4MAPPED: any;
  /** stdlib */
  export const V4MAPPED: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const promises: any;
  /** stdlib */
  export function Resolver(...args: any[]): any;
  /** stdlib */
  export function getDefaultResultOrder(...args: any[]): any;
  /** stdlib */
  export function getServers(...args: any[]): any;
  /** stdlib */
  export function lookup(...args: any[]): any;
  /** stdlib */
  export function lookupService(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
  /** stdlib */
  export function resolve4(...args: any[]): any;
  /** stdlib */
  export function resolve6(...args: any[]): any;
  /** stdlib */
  export function resolveAny(...args: any[]): any;
  /** stdlib */
  export function resolveCaa(...args: any[]): any;
  /** stdlib */
  export function resolveCname(...args: any[]): any;
  /** stdlib */
  export function resolveMx(...args: any[]): any;
  /** stdlib */
  export function resolveNaptr(...args: any[]): any;
  /** stdlib */
  export function resolveNs(...args: any[]): any;
  /** stdlib */
  export function resolvePtr(...args: any[]): any;
  /** stdlib */
  export function resolveSoa(...args: any[]): any;
  /** stdlib */
  export function resolveSrv(...args: any[]): any;
  /** stdlib */
  export function resolveTlsa(...args: any[]): any;
  /** stdlib */
  export function resolveTxt(...args: any[]): any;
  /** stdlib */
  export function reverse(...args: any[]): any;
  /** stdlib */
  export function setDefaultResultOrder(...args: any[]): any;
  /** stdlib */
  export function setServers(...args: any[]): any;
}

declare module "dns/promises" {
  /** stdlib */
  export class Resolver { [key: string]: any; }
  /** stdlib */
  export const ADDRGETNETWORKPARAMS: any;
  /** stdlib */
  export const BADFAMILY: any;
  /** stdlib */
  export const BADFLAGS: any;
  /** stdlib */
  export const BADHINTS: any;
  /** stdlib */
  export const BADNAME: any;
  /** stdlib */
  export const BADQUERY: any;
  /** stdlib */
  export const BADRESP: any;
  /** stdlib */
  export const BADSTR: any;
  /** stdlib */
  export const CANCELLED: any;
  /** stdlib */
  export const CONNREFUSED: any;
  /** stdlib */
  export const DESTRUCTION: any;
  /** stdlib */
  export const EOF: any;
  /** stdlib */
  export const FILE: any;
  /** stdlib */
  export const FORMERR: any;
  /** stdlib */
  export const LOADIPHLPAPI: any;
  /** stdlib */
  export const NODATA: any;
  /** stdlib */
  export const NOMEM: any;
  /** stdlib */
  export const NONAME: any;
  /** stdlib */
  export const NOTFOUND: any;
  /** stdlib */
  export const NOTIMP: any;
  /** stdlib */
  export const NOTINITIALIZED: any;
  /** stdlib */
  export const REFUSED: any;
  /** stdlib */
  export const SERVFAIL: any;
  /** stdlib */
  export const TIMEOUT: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function Resolver(...args: any[]): any;
  /** stdlib */
  export function getDefaultResultOrder(...args: any[]): any;
  /** stdlib */
  export function getServers(...args: any[]): any;
  /** stdlib */
  export function lookup(...args: any[]): any;
  /** stdlib */
  export function lookupService(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
  /** stdlib */
  export function resolve4(...args: any[]): any;
  /** stdlib */
  export function resolve6(...args: any[]): any;
  /** stdlib */
  export function resolveAny(...args: any[]): any;
  /** stdlib */
  export function resolveCaa(...args: any[]): any;
  /** stdlib */
  export function resolveCname(...args: any[]): any;
  /** stdlib */
  export function resolveMx(...args: any[]): any;
  /** stdlib */
  export function resolveNaptr(...args: any[]): any;
  /** stdlib */
  export function resolveNs(...args: any[]): any;
  /** stdlib */
  export function resolvePtr(...args: any[]): any;
  /** stdlib */
  export function resolveSoa(...args: any[]): any;
  /** stdlib */
  export function resolveSrv(...args: any[]): any;
  /** stdlib */
  export function resolveTlsa(...args: any[]): any;
  /** stdlib */
  export function resolveTxt(...args: any[]): any;
  /** stdlib */
  export function reverse(...args: any[]): any;
  /** stdlib */
  export function setDefaultResultOrder(...args: any[]): any;
  /** stdlib */
  export function setServers(...args: any[]): any;
}

declare module "domain" {
  /** stdlib */
  export class Domain { [key: string]: any; }
  /** stdlib */
  export const _stack: any;
  /** stdlib */
  export const active: any;
  /** stdlib */
  export const members: any;
  /** stdlib */
  export function Domain(...args: any[]): any;
  /** stdlib */
  export function create(...args: any[]): any;
  /** stdlib */
  export function createDomain(...args: any[]): any;
}

declare module "dotenv" {
  /** stdlib */
  export function config(...args: any[]): any;
}

declare module "ethers" {
  /** stdlib */
  export function formatEther(p0: any): string;
  /** stdlib */
  export function formatUnits(p0: any, p1: any): string;
  /** stdlib */
  export function getAddress(p0: string): string;
  /** stdlib */
  export function parseEther(p0: string): bigint;
  /** stdlib */
  export function parseUnits(p0: string, p1: any): bigint;
}

declare module "events" {
  /** stdlib */
  export class EventEmitter { [key: string]: any; }
  /** stdlib */
  export class EventEmitterAsyncResource { [key: string]: any; }
  /** stdlib */
  export const captureRejectionSymbol: any;
  /** stdlib */
  export const captureRejections: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const defaultMaxListeners: any;
  /** stdlib */
  export const errorMonitor: any;
  /** stdlib */
  export const usingDomains: any;
  /** stdlib */
  export function EventEmitter(...args: any[]): any;
  /** stdlib */
  export function EventEmitterAsyncResource(options: any): any;
  /** stdlib */
  export function addAbortListener(...args: any[]): any;
  /** stdlib */
  export function getEventListeners(...args: any[]): any;
  /** stdlib */
  export function getMaxListeners(...args: any[]): any;
  /** stdlib */
  export function init(...args: any[]): any;
  /** stdlib */
  export function listenerCount(...args: any[]): any;
  /** stdlib */
  export function on(...args: any[]): any;
  /** stdlib */
  export function once(...args: any[]): any;
  /** stdlib */
  export function setMaxListeners(...args: any[]): any;
}

declare module "exponential-backoff" {
  /** stdlib */
  export function backOff(p0: any, p1: any): any;
}

declare module "fastify" {
  /** stdlib */
  export default function (p0: any): any;
}

declare module "fetch" {
  /** stdlib */
  export class Blob { [key: string]: any; }
  /** stdlib */
  export class FormData { [key: string]: any; }
  /** stdlib */
  export class Headers { [key: string]: any; }
  /** stdlib */
  export class Request { [key: string]: any; }
  /** stdlib */
  export class Response { [key: string]: any; }
  /** stdlib */
  export default function (...args: any[]): any;
}

declare module "fs" {
  /** stdlib */
  export class Dir { [key: string]: any; }
  /** stdlib */
  export class Dirent { [key: string]: any; }
  /** stdlib */
  export class FileReadStream { [key: string]: any; }
  /** stdlib */
  export class FileWriteStream { [key: string]: any; }
  /** stdlib */
  export class ReadStream { [key: string]: any; }
  /** stdlib */
  export class Stats { [key: string]: any; }
  /** stdlib */
  export class Utf8Stream { [key: string]: any; }
  /** stdlib */
  export class WriteStream { [key: string]: any; }
  /** stdlib */
  export const constants: any;
  /** stdlib */
  export const promises: any;
  /** stdlib */
  export function _toUnixTimestamp(...args: any[]): any;
  /** stdlib */
  export function access(...args: any[]): any;
  /** stdlib */
  export function accessSync(...args: any[]): any;
  /** stdlib */
  export function appendFile(...args: any[]): any;
  /** stdlib */
  export function appendFileSync(...args: any[]): any;
  /** stdlib */
  export function chmod(...args: any[]): any;
  /** stdlib */
  export function chmodSync(...args: any[]): any;
  /** stdlib */
  export function chown(...args: any[]): any;
  /** stdlib */
  export function chownSync(...args: any[]): any;
  /** stdlib */
  export function close(...args: any[]): any;
  /** stdlib */
  export function closeSync(...args: any[]): any;
  /** stdlib */
  export function copyFile(...args: any[]): any;
  /** stdlib */
  export function copyFileSync(...args: any[]): any;
  /** stdlib */
  export function cp(...args: any[]): any;
  /** stdlib */
  export function cpSync(...args: any[]): any;
  /** stdlib */
  export function createReadStream(...args: any[]): any;
  /** stdlib */
  export function createWriteStream(...args: any[]): any;
  /** stdlib */
  export function exists(...args: any[]): any;
  /** stdlib */
  export function existsSync(...args: any[]): any;
  /** stdlib */
  export function fchmod(...args: any[]): any;
  /** stdlib */
  export function fchmodSync(...args: any[]): any;
  /** stdlib */
  export function fchown(...args: any[]): any;
  /** stdlib */
  export function fchownSync(...args: any[]): any;
  /** stdlib */
  export function fdatasync(...args: any[]): any;
  /** stdlib */
  export function fdatasyncSync(...args: any[]): any;
  /** stdlib */
  export function fstat(...args: any[]): any;
  /** stdlib */
  export function fstatSync(...args: any[]): any;
  /** stdlib */
  export function fsync(...args: any[]): any;
  /** stdlib */
  export function fsyncSync(...args: any[]): any;
  /** stdlib */
  export function ftruncate(...args: any[]): any;
  /** stdlib */
  export function ftruncateSync(...args: any[]): any;
  /** stdlib */
  export function futimes(...args: any[]): any;
  /** stdlib */
  export function futimesSync(...args: any[]): any;
  /** stdlib */
  export function glob(...args: any[]): any;
  /** stdlib */
  export function globSync(...args: any[]): any;
  /** stdlib */
  export function lchmod(...args: any[]): any;
  /** stdlib */
  export function lchmodSync(...args: any[]): any;
  /** stdlib */
  export function lchown(...args: any[]): any;
  /** stdlib */
  export function lchownSync(...args: any[]): any;
  /** stdlib */
  export function link(...args: any[]): any;
  /** stdlib */
  export function linkSync(...args: any[]): any;
  /** stdlib */
  export function lstat(...args: any[]): any;
  /** stdlib */
  export function lstatSync(...args: any[]): any;
  /** stdlib */
  export function lutimes(...args: any[]): any;
  /** stdlib */
  export function lutimesSync(...args: any[]): any;
  /** stdlib */
  export function mkdir(...args: any[]): any;
  /** stdlib */
  export function mkdirSync(...args: any[]): any;
  /** stdlib */
  export function mkdtemp(...args: any[]): any;
  /** stdlib */
  export function mkdtempDisposableSync(...args: any[]): any;
  /** stdlib */
  export function mkdtempSync(...args: any[]): any;
  /** stdlib */
  export function open(...args: any[]): any;
  /** stdlib */
  export function openAsBlob(...args: any[]): any;
  /** stdlib */
  export function openSync(...args: any[]): any;
  /** stdlib */
  export function opendir(...args: any[]): any;
  /** stdlib */
  export function opendirSync(...args: any[]): any;
  /** stdlib */
  export function read(...args: any[]): any;
  /** stdlib */
  export function readFile(...args: any[]): any;
  /** stdlib */
  export function readFileSync(...args: any[]): any;
  /** stdlib */
  export function readSync(...args: any[]): any;
  /** stdlib */
  export function readdir(...args: any[]): any;
  /** stdlib */
  export function readdirSync(...args: any[]): any;
  /** stdlib */
  export function readlink(...args: any[]): any;
  /** stdlib */
  export function readlinkSync(...args: any[]): any;
  /** stdlib */
  export function readv(...args: any[]): any;
  /** stdlib */
  export function readvSync(...args: any[]): any;
  /** stdlib */
  export function realpath(...args: any[]): any;
  /** stdlib */
  export function realpathSync(...args: any[]): any;
  /** stdlib */
  export function rename(...args: any[]): any;
  /** stdlib */
  export function renameSync(...args: any[]): any;
  /** stdlib */
  export function rm(...args: any[]): any;
  /** stdlib */
  export function rmSync(...args: any[]): any;
  /** stdlib */
  export function rmdir(...args: any[]): any;
  /** stdlib */
  export function rmdirSync(...args: any[]): any;
  /** stdlib */
  export function stat(...args: any[]): any;
  /** stdlib */
  export function statSync(...args: any[]): any;
  /** stdlib */
  export function statfs(...args: any[]): any;
  /** stdlib */
  export function statfsSync(...args: any[]): any;
  /** stdlib */
  export function symlink(...args: any[]): any;
  /** stdlib */
  export function symlinkSync(...args: any[]): any;
  /** stdlib */
  export function truncate(...args: any[]): any;
  /** stdlib */
  export function truncateSync(...args: any[]): any;
  /** stdlib */
  export function unlink(...args: any[]): any;
  /** stdlib */
  export function unlinkSync(...args: any[]): any;
  /** stdlib */
  export function unwatchFile(...args: any[]): any;
  /** stdlib */
  export function utimes(...args: any[]): any;
  /** stdlib */
  export function utimesSync(...args: any[]): any;
  /** stdlib */
  export function watch(...args: any[]): any;
  /** stdlib */
  export function watchFile(...args: any[]): any;
  /** stdlib */
  export function write(...args: any[]): any;
  /** stdlib */
  export function writeFile(...args: any[]): any;
  /** stdlib */
  export function writeFileSync(...args: any[]): any;
  /** stdlib */
  export function writeSync(...args: any[]): any;
  /** stdlib */
  export function writev(...args: any[]): any;
  /** stdlib */
  export function writevSync(...args: any[]): any;
}

declare module "fs/promises" {
  /** stdlib */
  export const constants: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function access(...args: any[]): any;
  /** stdlib */
  export function appendFile(...args: any[]): any;
  /** stdlib */
  export function chmod(...args: any[]): any;
  /** stdlib */
  export function chown(...args: any[]): any;
  /** stdlib */
  export function copyFile(...args: any[]): any;
  /** stdlib */
  export function cp(...args: any[]): any;
  /** stdlib */
  export function glob(...args: any[]): any;
  /** stdlib */
  export function lchmod(...args: any[]): any;
  /** stdlib */
  export function lchown(...args: any[]): any;
  /** stdlib */
  export function link(...args: any[]): any;
  /** stdlib */
  export function lstat(...args: any[]): any;
  /** stdlib */
  export function lutimes(...args: any[]): any;
  /** stdlib */
  export function mkdir(...args: any[]): any;
  /** stdlib */
  export function mkdtemp(...args: any[]): any;
  /** stdlib */
  export function mkdtempDisposable(...args: any[]): any;
  /** stdlib */
  export function open(...args: any[]): any;
  /** stdlib */
  export function opendir(...args: any[]): any;
  /** stdlib */
  export function readFile(...args: any[]): any;
  /** stdlib */
  export function readdir(...args: any[]): any;
  /** stdlib */
  export function readlink(...args: any[]): any;
  /** stdlib */
  export function realpath(...args: any[]): any;
  /** stdlib */
  export function rename(...args: any[]): any;
  /** stdlib */
  export function rm(...args: any[]): any;
  /** stdlib */
  export function rmdir(...args: any[]): any;
  /** stdlib */
  export function stat(...args: any[]): any;
  /** stdlib */
  export function statfs(...args: any[]): any;
  /** stdlib */
  export function symlink(...args: any[]): any;
  /** stdlib */
  export function truncate(...args: any[]): any;
  /** stdlib */
  export function unlink(...args: any[]): any;
  /** stdlib */
  export function utimes(...args: any[]): any;
  /** stdlib */
  export function watch(...args: any[]): any;
  /** stdlib */
  export function writeFile(...args: any[]): any;
}

declare module "http" {
  /** stdlib */
  export class Agent { [key: string]: any; }
  /** stdlib */
  export class ClientRequest { [key: string]: any; }
  /** stdlib */
  export class IncomingMessage { [key: string]: any; }
  /** stdlib */
  export class IncomingMessage { [key: string]: any; }
  /** stdlib */
  export class OutgoingMessage { [key: string]: any; }
  /** stdlib */
  export class OutgoingMessage { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class ServerResponse { [key: string]: any; }
  /** stdlib */
  export class ServerResponse { [key: string]: any; }
  /** stdlib */
  export class WebSocket { [key: string]: any; }
  /** stdlib */
  export const METHODS: any;
  /** stdlib */
  export const STATUS_CODES: any;
  /** stdlib */
  export const globalAgent: any;
  /** stdlib */
  export const kConnectionsCheckingInterval: any;
  /** stdlib */
  export const maxHeaderSize: any;
  /** stdlib */
  export function Agent(...args: any[]): any;
  /** stdlib */
  export function Server(...args: any[]): any;
  /** stdlib */
  export function _connectionListener(...args: any[]): any;
  /** stdlib */
  export function createServer(...args: any[]): any;
  /** stdlib */
  export function get(...args: any[]): any;
  /** stdlib */
  export function request(...args: any[]): any;
  /** stdlib */
  export function setGlobalProxyFromEnv(...args: any[]): any;
  /** stdlib */
  export function setMaxIdleHTTPParsers(...args: any[]): any;
  /** stdlib */
  export function validateHeaderName(...args: any[]): any;
  /** stdlib */
  export function validateHeaderValue(...args: any[]): any;
}

declare module "http2" {
  /** stdlib */
  export class Http2ServerRequest { [key: string]: any; }
  /** stdlib */
  export class Http2ServerResponse { [key: string]: any; }
  /** stdlib */
  export const constants: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const sensitiveHeaders: any;
  /** stdlib */
  export function connect(...args: any[]): any;
  /** stdlib */
  export function createSecureServer(...args: any[]): any;
  /** stdlib */
  export function createServer(...args: any[]): any;
  /** stdlib */
  export function getDefaultSettings(...args: any[]): any;
  /** stdlib */
  export function getPackedSettings(...args: any[]): any;
  /** stdlib */
  export function getUnpackedSettings(...args: any[]): any;
  /** stdlib */
  export function performServerHandshake(...args: any[]): any;
}

declare module "https" {
  /** stdlib */
  export class Agent { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export const globalAgent: any;
  /** stdlib */
  export function Agent(...args: any[]): any;
  /** stdlib */
  export function Server(...args: any[]): any;
  /** stdlib */
  export function createServer(...args: any[]): any;
  /** stdlib */
  export function get(...args: any[]): any;
  /** stdlib */
  export function request(...args: any[]): any;
}

declare module "inspector" {
  /** stdlib */
  export class Session { [key: string]: any; }
  /** stdlib */
  export const Network: any;
  /** stdlib */
  export const console: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function Session(...args: any[]): any;
  /** stdlib */
  export function close(...args: any[]): any;
  /** stdlib @perryStub accepts port/host but binds no real WebSocket inspector endpoint; sessions are in-process fakes (#4916) */
  export function open(...args: any[]): any;
  /** stdlib @perryStub always undefined: Perry never exposes a real inspector endpoint (#4916) */
  export function url(...args: any[]): any;
  /** stdlib @perryStub returns immediately after open(); there is no debugger to wait for (#4916) */
  export function waitForDebugger(...args: any[]): any;
}

declare module "inspector/promises" {
  /** stdlib */
  export class Session { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function Session(...args: any[]): any;
}

declare module "ioredis" {
  /** stdlib */
  export class Redis { [key: string]: any; }
  /** stdlib */
  export function createClient(p0: any): any;
}

declare module "iroh" {
  /** stdlib */
  export function bind(...args: any[]): any;
}

declare module "jsonwebtoken" {
  /** stdlib */
  export function decode(token: string): any;
  /** stdlib */
  export function sign(payload: any, secret: string, options?: any, kid?: string): string;
  /** stdlib */
  export function verify(token: string, secret: string): any;
}

declare module "lodash" {
  /** stdlib */
  export function camelCase(p0: string): string;
  /** stdlib */
  export function chunk(p0: any, p1: any): any;
  /** stdlib */
  export function clamp(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function compact(p0: any): any;
  /** stdlib */
  export function drop(p0: any, p1: any): any;
  /** stdlib */
  export function first(p0: any): any;
  /** stdlib */
  export function flatten(p0: any): any;
  /** stdlib */
  export function head(p0: any): any;
  /** stdlib */
  export function inRange(p0: any, p1: any, p2: any): boolean;
  /** stdlib */
  export function kebabCase(p0: string): string;
  /** stdlib */
  export function last(p0: any): any;
  /** stdlib */
  export function max(p0: any): any;
  /** stdlib */
  export function maxBy(p0: any, p1: any): any;
  /** stdlib */
  export function mean(p0: any): number;
  /** stdlib */
  export function meanBy(p0: any, p1: any): number;
  /** stdlib */
  export function min(p0: any): any;
  /** stdlib */
  export function minBy(p0: any, p1: any): any;
  /** stdlib */
  export function random(p0: any, p1: any): number;
  /** stdlib */
  export function range(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function reverse(p0: any): any;
  /** stdlib */
  export function size(p0: any): any;
  /** stdlib */
  export function snakeCase(p0: string): string;
  /** stdlib */
  export function sum(p0: any): number;
  /** stdlib */
  export function sumBy(p0: any, p1: any): number;
  /** stdlib */
  export function tail(p0: any): any;
  /** stdlib */
  export function take(p0: any, p1: any): any;
  /** stdlib */
  export function times(p0: any): any;
  /** stdlib */
  export function uniq(p0: any): any;
}

declare module "lru-cache" {
  /** stdlib */
  export default function (p0: any): any;
}

declare module "module" {
  /** stdlib */
  export class Module { [key: string]: any; }
  /** stdlib */
  export class SourceMap { [key: string]: any; }
  /** stdlib */
  export const Module: any;
  /** stdlib */
  export const _cache: any;
  /** stdlib */
  export const _extensions: any;
  /** stdlib */
  export const _pathCache: any;
  /** stdlib */
  export const builtinModules: any;
  /** stdlib */
  export const constants: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const globalPaths: any;
  /** stdlib */
  export function Module(...args: any[]): any;
  /** stdlib */
  export function SourceMap(...args: any[]): any;
  /** stdlib */
  export function _findPath(...args: any[]): any;
  /** stdlib */
  export function _initPaths(...args: any[]): any;
  /** stdlib */
  export function _load(...args: any[]): any;
  /** stdlib */
  export function _nodeModulePaths(...args: any[]): any;
  /** stdlib */
  export function _preloadModules(...args: any[]): any;
  /** stdlib */
  export function _resolveFilename(...args: any[]): any;
  /** stdlib */
  export function _resolveLookupPaths(...args: any[]): any;
  /** stdlib */
  export function createRequire(...args: any[]): any;
  /** stdlib */
  export function enableCompileCache(...args: any[]): any;
  /** stdlib */
  export function findPackageJSON(...args: any[]): any;
  /** stdlib */
  export function findSourceMap(...args: any[]): any;
  /** stdlib */
  export function flushCompileCache(...args: any[]): any;
  /** stdlib */
  export function getCompileCacheDir(...args: any[]): any;
  /** stdlib */
  export function getSourceMapsSupport(...args: any[]): any;
  /** stdlib */
  export function isBuiltin(...args: any[]): any;
  /** stdlib */
  export function register(...args: any[]): any;
  /** stdlib */
  export function registerHooks(...args: any[]): any;
  /** stdlib */
  export function runMain(...args: any[]): any;
  /** stdlib */
  export function setSourceMapsSupport(...args: any[]): any;
  /** stdlib */
  export function stripTypeScriptTypes(...args: any[]): any;
  /** stdlib */
  export function syncBuiltinESMExports(...args: any[]): any;
}

declare module "moment" {
  /** stdlib */
  export default function (input?: any): any;
  /** stdlib */
  export function moment(input?: any): any;
}

declare module "mongodb" {
  /** stdlib */
  export function connect(p0: any): any;
}

declare module "mysql2" {
  /** stdlib */
  export class Pool { [key: string]: any; }
  /** stdlib */
  export function createConnection(p0: any): any;
  /** stdlib */
  export function createPool(p0: any): any;
}

declare module "mysql2/promise" {
  /** stdlib */
  export class Pool { [key: string]: any; }
  /** stdlib */
  export function createConnection(p0: any): any;
  /** stdlib */
  export function createPool(p0: any): any;
}

declare module "nanoid" {
  /** stdlib */
  export function nanoid(size: number): string;
}

declare module "net" {
  /** stdlib */
  export class BlockList { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class Socket { [key: string]: any; }
  /** stdlib */
  export class SocketAddress { [key: string]: any; }
  /** stdlib */
  export class Stream { [key: string]: any; }
  /** stdlib */
  export function BlockList(...args: any[]): any;
  /** stdlib */
  export function Server(p0: any, p1: any): any;
  /** stdlib */
  export function Socket(...args: any[]): any;
  /** stdlib */
  export function SocketAddress(options: any): any;
  /** stdlib */
  export function Stream(...args: any[]): any;
  /** stdlib */
  export function _createServerHandle(p0: any, p1: any, p2: any, p3: any, p4: any): any;
  /** stdlib */
  export function _normalizeArgs(p0: any): any;
  /** stdlib */
  export function connect(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function createConnection(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function createServer(p0: any, p1: any): any;
  /** stdlib */
  export function getDefaultAutoSelectFamily(...args: any[]): any;
  /** stdlib */
  export function getDefaultAutoSelectFamilyAttemptTimeout(...args: any[]): any;
  /** stdlib */
  export function isIP(...args: any[]): any;
  /** stdlib */
  export function isIPv4(...args: any[]): any;
  /** stdlib */
  export function isIPv6(...args: any[]): any;
  /** stdlib */
  export function setDefaultAutoSelectFamily(...args: any[]): any;
  /** stdlib */
  export function setDefaultAutoSelectFamilyAttemptTimeout(...args: any[]): any;
}

declare module "node-cron" {
  /** stdlib */
  export function schedule(...args: any[]): any;
  /** stdlib */
  export function validate(...args: any[]): any;
}

declare module "node-fetch" {
  /** stdlib */
  export class Blob { [key: string]: any; }
  /** stdlib */
  export class FormData { [key: string]: any; }
  /** stdlib */
  export class Headers { [key: string]: any; }
  /** stdlib */
  export class Request { [key: string]: any; }
  /** stdlib */
  export class Response { [key: string]: any; }
  /** stdlib */
  export default function (...args: any[]): any;
}

declare module "node-pty" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function spawn(...args: any[]): any;
}

declare module "nodemailer" {
  /** stdlib */
  export function createTransport(p0: any): any;
}

declare module "os" {
  /** stdlib */
  export const EOL: any;
  /** stdlib */
  export const constants: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const devNull: any;
  /** stdlib */
  export function arch(...args: any[]): any;
  /** stdlib */
  export function availableParallelism(...args: any[]): any;
  /** stdlib */
  export function cpus(...args: any[]): any;
  /** stdlib */
  export function endianness(...args: any[]): any;
  /** stdlib */
  export function freemem(...args: any[]): any;
  /** stdlib */
  export function getPriority(pid?: number): number;
  /** stdlib */
  export function homedir(...args: any[]): any;
  /** stdlib */
  export function hostname(...args: any[]): any;
  /** stdlib */
  export function loadavg(...args: any[]): any;
  /** stdlib */
  export function machine(...args: any[]): any;
  /** stdlib */
  export function networkInterfaces(...args: any[]): any;
  /** stdlib */
  export function platform(...args: any[]): any;
  /** stdlib */
  export function release(...args: any[]): any;
  /** stdlib */
  export function setPriority(pidOrPriority: number, priority?: number): void;
  /** stdlib */
  export function tmpdir(...args: any[]): any;
  /** stdlib */
  export function totalmem(...args: any[]): any;
  /** stdlib */
  export function type(...args: any[]): any;
  /** stdlib */
  export function uptime(...args: any[]): any;
  /** stdlib */
  export function userInfo(...args: any[]): any;
  /** stdlib */
  export function version(...args: any[]): any;
}

declare module "path" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const delimiter: any;
  /** stdlib */
  export const posix: any;
  /** stdlib */
  export const sep: any;
  /** stdlib */
  export const win32: any;
  /** stdlib */
  export function _makeLong(...args: any[]): any;
  /** stdlib */
  export function basename(...args: any[]): any;
  /** stdlib */
  export function dirname(...args: any[]): any;
  /** stdlib */
  export function extname(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function isAbsolute(...args: any[]): any;
  /** stdlib */
  export function join(...args: any[]): any;
  /** stdlib */
  export function matchesGlob(...args: any[]): any;
  /** stdlib */
  export function normalize(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function relative(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
  /** stdlib */
  export function toNamespacedPath(...args: any[]): any;
}

declare module "path/posix" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const delimiter: any;
  /** stdlib */
  export const posix: any;
  /** stdlib */
  export const sep: any;
  /** stdlib */
  export const win32: any;
  /** stdlib */
  export function _makeLong(...args: any[]): any;
  /** stdlib */
  export function basename(...args: any[]): any;
  /** stdlib */
  export function dirname(...args: any[]): any;
  /** stdlib */
  export function extname(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function isAbsolute(...args: any[]): any;
  /** stdlib */
  export function join(...args: any[]): any;
  /** stdlib */
  export function matchesGlob(...args: any[]): any;
  /** stdlib */
  export function normalize(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function relative(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
  /** stdlib */
  export function toNamespacedPath(...args: any[]): any;
}

declare module "path/win32" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const delimiter: any;
  /** stdlib */
  export const posix: any;
  /** stdlib */
  export const sep: any;
  /** stdlib */
  export const win32: any;
  /** stdlib */
  export function _makeLong(...args: any[]): any;
  /** stdlib */
  export function basename(...args: any[]): any;
  /** stdlib */
  export function dirname(...args: any[]): any;
  /** stdlib */
  export function extname(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function isAbsolute(...args: any[]): any;
  /** stdlib */
  export function join(...args: any[]): any;
  /** stdlib */
  export function matchesGlob(...args: any[]): any;
  /** stdlib */
  export function normalize(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function relative(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
  /** stdlib */
  export function toNamespacedPath(...args: any[]): any;
}

declare module "perf_hooks" {
  /** stdlib */
  export class Performance { [key: string]: any; }
  /** stdlib */
  export class PerformanceEntry { [key: string]: any; }
  /** stdlib */
  export class PerformanceMark { [key: string]: any; }
  /** stdlib */
  export class PerformanceMeasure { [key: string]: any; }
  /** stdlib */
  export class PerformanceObserver { [key: string]: any; }
  /** stdlib */
  export class PerformanceObserverEntryList { [key: string]: any; }
  /** stdlib */
  export class PerformanceResourceTiming { [key: string]: any; }
  /** stdlib */
  export const constants: any;
  /** stdlib */
  export const performance: any;
  /** stdlib */
  export function createHistogram(...args: any[]): any;
  /** stdlib */
  export function monitorEventLoopDelay(...args: any[]): any;
  /** stdlib */
  export function timerify(...args: any[]): any;
}

declare module "perry" {
  /** stdlib */
  export const isStandaloneExecutable: any;
  /** stdlib */
  export function embeddedFiles(...args: any[]): any;
  /** stdlib */
  export function readEmbedded(path: string): Buffer;
}

declare module "perry/ads" {
  /** stdlib */
  export function js_ads_banner_create(...args: any[]): any;
  /** stdlib */
  export function js_ads_banner_destroy(...args: any[]): any;
  /** stdlib */
  export function js_ads_interstitial_load(...args: any[]): any;
  /** stdlib */
  export function js_ads_interstitial_show(...args: any[]): any;
  /** stdlib */
  export function js_ads_request_consent(...args: any[]): any;
  /** stdlib */
  export function js_ads_rewarded_load(...args: any[]): any;
  /** stdlib */
  export function js_ads_rewarded_show(...args: any[]): any;
}

declare module "perry/audio" {
  /** stdlib */
  export function createBus(...args: any[]): any;
  /** stdlib */
  export function crossfade(...args: any[]): any;
  /** stdlib */
  export function destroyBus(...args: any[]): any;
  /** stdlib */
  export function fadeIn(...args: any[]): any;
  /** stdlib */
  export function fadeOut(...args: any[]): any;
  /** stdlib */
  export function getDuration(...args: any[]): any;
  /** stdlib */
  export function getPosition(...args: any[]): any;
  /** stdlib */
  export function isPlaying(...args: any[]): any;
  /** stdlib */
  export function loadSound(...args: any[]): any;
  /** stdlib */
  export function muteBus(...args: any[]): any;
  /** stdlib */
  export function onEnded(...args: any[]): any;
  /** stdlib */
  export function onLoaded(...args: any[]): any;
  /** stdlib */
  export function pause(...args: any[]): any;
  /** stdlib */
  export function play(...args: any[]): any;
  /** stdlib */
  export function resume(...args: any[]): any;
  /** stdlib */
  export function resumeAll(...args: any[]): any;
  /** stdlib */
  export function setMasterVolume(...args: any[]): any;
  /** stdlib */
  export function setPan(...args: any[]): any;
  /** stdlib */
  export function setRate(...args: any[]): any;
  /** stdlib */
  export function setVolume(...args: any[]): any;
  /** stdlib */
  export function soloBus(...args: any[]): any;
  /** stdlib */
  export function stop(...args: any[]): any;
  /** stdlib */
  export function suspend(...args: any[]): any;
  /** stdlib */
  export function unload(...args: any[]): any;
}

declare module "perry/background" {
  /** stdlib */
  export function cancel(...args: any[]): any;
  /** stdlib */
  export function registerTask(...args: any[]): any;
  /** stdlib */
  export function schedule(...args: any[]): any;
}

declare module "perry/compose" {
  /** stdlib */
  export function config(...args: any[]): any;
  /** stdlib */
  export function down(...args: any[]): any;
  /** stdlib */
  export function exec(...args: any[]): any;
  /** stdlib */
  export function logs(...args: any[]): any;
  /** stdlib */
  export function ps(...args: any[]): any;
  /** stdlib */
  export function restart(...args: any[]): any;
  /** stdlib */
  export function start(...args: any[]): any;
  /** stdlib */
  export function stop(...args: any[]): any;
  /** stdlib */
  export function up(...args: any[]): any;
}

declare module "perry/container" {
  /** stdlib */
  export function composeUp(...args: any[]): any;
  /** stdlib */
  export function create(...args: any[]): any;
  /** stdlib */
  export function detectBackend(...args: any[]): any;
  /** stdlib */
  export function downAll(...args: any[]): any;
  /** stdlib */
  export function downByProject(...args: any[]): any;
  /** stdlib */
  export function exec(...args: any[]): any;
  /** stdlib */
  export function getAvailableBackends(...args: any[]): any;
  /** stdlib */
  export function getBackend(...args: any[]): any;
  /** stdlib */
  export function getBackendPriority(...args: any[]): any;
  /** stdlib */
  export function inspect(...args: any[]): any;
  /** stdlib */
  export function list(...args: any[]): any;
  /** stdlib */
  export function listImages(...args: any[]): any;
  /** stdlib */
  export function logs(...args: any[]): any;
  /** stdlib */
  export function pullImage(...args: any[]): any;
  /** stdlib */
  export function remove(...args: any[]): any;
  /** stdlib */
  export function removeIfExists(...args: any[]): any;
  /** stdlib */
  export function removeImage(...args: any[]): any;
  /** stdlib */
  export function run(...args: any[]): any;
  /** stdlib */
  export function selectBackendFor(...args: any[]): any;
  /** stdlib */
  export function setBackend(...args: any[]): any;
  /** stdlib */
  export function setBackends(...args: any[]): any;
  /** stdlib */
  export function start(...args: any[]): any;
  /** stdlib */
  export function stop(...args: any[]): any;
}

declare module "perry/container-compose" {
  /** stdlib */
  export function config(...args: any[]): any;
  /** stdlib */
  export function down(...args: any[]): any;
  /** stdlib */
  export function exec(...args: any[]): any;
  /** stdlib */
  export function logs(...args: any[]): any;
  /** stdlib */
  export function ps(...args: any[]): any;
  /** stdlib */
  export function restart(...args: any[]): any;
  /** stdlib */
  export function start(...args: any[]): any;
  /** stdlib */
  export function stop(...args: any[]): any;
  /** stdlib */
  export function up(...args: any[]): any;
}

declare module "perry/gc" {
  /** stdlib */
  export function collect(): void;
  /** stdlib */
  export function idleHint(): boolean;
  /** stdlib */
  export function minor(): number;
}

declare module "perry/i18n" {
  /** stdlib */
  export function Currency(...args: any[]): any;
  /** stdlib */
  export function FormatNumber(...args: any[]): any;
  /** stdlib */
  export function FormatTime(...args: any[]): any;
  /** stdlib */
  export function LongDate(...args: any[]): any;
  /** stdlib */
  export function Percent(...args: any[]): any;
  /** stdlib */
  export function Raw(...args: any[]): any;
  /** stdlib */
  export function ShortDate(...args: any[]): any;
  /** stdlib */
  export function t(...args: any[]): any;
}

declare module "perry/media" {
  /** stdlib */
  export function createPlayer(...args: any[]): any;
  /** stdlib */
  export function destroy(...args: any[]): any;
  /** stdlib */
  export function getCurrentTime(...args: any[]): any;
  /** stdlib */
  export function getDuration(...args: any[]): any;
  /** stdlib */
  export function getState(...args: any[]): any;
  /** stdlib */
  export function isPlaying(...args: any[]): any;
  /** stdlib */
  export function onStateChange(...args: any[]): any;
  /** stdlib */
  export function onTimeUpdate(...args: any[]): any;
  /** stdlib */
  export function pause(...args: any[]): any;
  /** stdlib */
  export function play(...args: any[]): any;
  /** stdlib */
  export function seek(...args: any[]): any;
  /** stdlib */
  export function setNowPlaying(...args: any[]): any;
  /** stdlib */
  export function setRate(...args: any[]): any;
  /** stdlib */
  export function setVolume(...args: any[]): any;
  /** stdlib */
  export function stop(...args: any[]): any;
}

declare module "perry/plugin" {
  /** stdlib */
  export class PluginApi { [key: string]: any; }
  /** stdlib */
  export function discoverPlugins(...args: any[]): any;
  /** stdlib */
  export function emitEvent(...args: any[]): any;
  /** stdlib */
  export function emitHook(...args: any[]): any;
  /** stdlib */
  export function initPlugins(...args: any[]): any;
  /** stdlib */
  export function invokeTool(...args: any[]): any;
  /** stdlib */
  export function listHooks(...args: any[]): any;
  /** stdlib */
  export function listPlugins(...args: any[]): any;
  /** stdlib */
  export function listTools(...args: any[]): any;
  /** stdlib */
  export function loadPlugin(...args: any[]): any;
  /** stdlib */
  export function pluginCount(...args: any[]): any;
  /** stdlib */
  export function setPluginConfig(...args: any[]): any;
  /** stdlib */
  export function unloadPlugin(...args: any[]): any;
}

declare module "perry/system" {
  /** stdlib */
  export function appGetLaunchUrl(...args: any[]): any;
  /** stdlib */
  export function appGroupDelete(...args: any[]): any;
  /** stdlib */
  export function appGroupGet(...args: any[]): any;
  /** stdlib */
  export function appGroupSet(...args: any[]): any;
  /** stdlib */
  export function appOnOpenUrl(...args: any[]): any;
  /** stdlib */
  export function audioGetLevel(...args: any[]): any;
  /** stdlib */
  export function audioGetPeak(...args: any[]): any;
  /** stdlib */
  export function audioGetWaveform(...args: any[]): any;
  /** stdlib */
  export function audioRegisterCallback(...args: any[]): any;
  /** stdlib */
  export function audioSetOutputFilename(...args: any[]): any;
  /** stdlib */
  export function audioStart(...args: any[]): any;
  /** stdlib */
  export function audioStartRecording(...args: any[]): any;
  /** stdlib */
  export function audioStop(...args: any[]): any;
  /** stdlib */
  export function audioStopRecording(...args: any[]): any;
  /** stdlib */
  export function audioUnregisterCallback(...args: any[]): any;
  /** stdlib */
  export function geolocationGetCurrent(...args: any[]): any;
  /** stdlib */
  export function geolocationRequestPermission(...args: any[]): any;
  /** stdlib */
  export function geolocationStopWatch(...args: any[]): any;
  /** stdlib */
  export function geolocationWatch(...args: any[]): any;
  /** stdlib */
  export function getAppBuildNumber(...args: any[]): any;
  /** stdlib */
  export function getAppIcon(...args: any[]): any;
  /** stdlib */
  export function getAppVersion(...args: any[]): any;
  /** stdlib */
  export function getBundleId(...args: any[]): any;
  /** stdlib */
  export function getDeviceIdiom(...args: any[]): any;
  /** stdlib */
  export function getDeviceModel(...args: any[]): any;
  /** stdlib */
  export function getLocale(...args: any[]): any;
  /** stdlib */
  export function getOSVersion(...args: any[]): any;
  /** stdlib */
  export function getSafeAreaInsets(...args: any[]): any;
  /** stdlib */
  export function hapticPlay(type: string): void;
  /** stdlib */
  export function imagePickerPick(...args: any[]): any;
  /** stdlib */
  export function isDarkMode(...args: any[]): any;
  /** stdlib */
  export function keychainDelete(...args: any[]): any;
  /** stdlib */
  export function keychainGet(...args: any[]): any;
  /** stdlib */
  export function keychainSave(...args: any[]): any;
  /** stdlib */
  export function networkGetStatus(...args: any[]): any;
  /** stdlib */
  export function networkOnChange(...args: any[]): any;
  /** stdlib */
  export function networkStopOnChange(...args: any[]): any;
  /** stdlib */
  export function notificationCancel(...args: any[]): any;
  /** stdlib */
  export function notificationOnBackgroundReceive(...args: any[]): any;
  /** stdlib */
  export function notificationOnReceive(...args: any[]): any;
  /** stdlib */
  export function notificationOnTap(...args: any[]): any;
  /** stdlib */
  export function notificationRegisterRemote(...args: any[]): any;
  /** stdlib */
  export function notificationSend(...args: any[]): any;
  /** stdlib */
  export function openURL(...args: any[]): any;
  /** stdlib */
  export function preferencesGet(...args: any[]): any;
  /** stdlib */
  export function preferencesSet(...args: any[]): any;
  /** stdlib */
  export function shareText(...args: any[]): any;
  /** stdlib */
  export function shareUrl(...args: any[]): any;
  /** stdlib */
  export function takeScreenshot(...args: any[]): any;
}

declare module "perry/thread" {
  /** stdlib */
  export function parallelFilter(p0: any, p1: any): any;
  /** stdlib */
  export function parallelMap(p0: any, p1: any): any;
  /** stdlib */
  export function spawn(p0: any): Promise<any>;
}

declare module "perry/tui" {
  /** stdlib */
  export function AnimatedSpinner(p0: any, p1: any): any;
  /** stdlib */
  export function Box(...args: any[]): any;
  /** stdlib */
  export function Input(p0: string): any;
  /** stdlib */
  export function InputAt(p0: string, p1: any): any;
  /** stdlib */
  export function List(p0: any, p1: any): any;
  /** stdlib */
  export function ProgressBar(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function Select(p0: any, p1: any): any;
  /** stdlib */
  export function Spacer(...args: any[]): any;
  /** stdlib */
  export function Spinner(p0: any): any;
  /** stdlib */
  export function Table(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function Tabs(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function Text(p0: string): any;
  /** stdlib */
  export function TextArea(p0: string): any;
  /** stdlib */
  export function TextStyled(p0: string, p1: string, p2: string, p3: any): any;
  /** stdlib */
  export function boxSetAlignItems(p0: any, p1: string): void;
  /** stdlib */
  export function boxSetFlexBasis(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetFlexBasisPct(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetFlexDirection(p0: any, p1: string): void;
  /** stdlib */
  export function boxSetFlexGrow(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetFlexShrink(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetGap(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetHeight(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetHeightPct(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetJustifyContent(p0: any, p1: string): void;
  /** stdlib */
  export function boxSetPadding(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetPaddingEach(p0: any, p1: any, p2: any, p3: any, p4: any): void;
  /** stdlib */
  export function boxSetWidth(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetWidthPct(p0: any, p1: any): void;
  /** stdlib */
  export function enter(): void;
  /** stdlib */
  export function exit(): void;
  /** stdlib */
  export function focus(p0: any): void;
  /** stdlib */
  export function focusNext(): void;
  /** stdlib */
  export function focusPrevious(): void;
  /** stdlib */
  export function render(p0: any): void;
  /** stdlib */
  export function run(p0: any): void;
  /** stdlib */
  export function state(p0: any): any;
  /** stdlib */
  export function useApp(...args: any[]): any;
  /** stdlib */
  export function useEffect(p0: any, p1: any): void;
  /** stdlib */
  export function useFocus(p0: any, p1: any): any;
  /** stdlib */
  export function useFocusManager(...args: any[]): any;
  /** stdlib */
  export function useInput(p0: any): void;
  /** stdlib */
  export function useMemo(p0: any, p1: any): any;
  /** stdlib */
  export function useRef(p0: any): any;
  /** stdlib */
  export function useState(p0: any): any;
  /** stdlib */
  export function useStateSet(p0: any, p1: any): void;
  /** stdlib */
  export function useStateTuple(p0: any): any;
  /** stdlib */
  export function useStdout(...args: any[]): any;
  /** stdlib */
  export function waitUntilExit(): void;
}

declare module "perry/ui" {
  /** stdlib */
  export function App(...args: any[]): any;
  /** stdlib */
  export function AttributedText(...args: any[]): any;
  /** stdlib */
  export function BloomView(...args: any[]): any;
  /** stdlib */
  export function BottomNavigation(...args: any[]): any;
  /** stdlib */
  export function Button(...args: any[]): any;
  /** stdlib */
  export function CameraView(...args: any[]): any;
  /** stdlib */
  export function Canvas(...args: any[]): any;
  /** stdlib */
  export function Divider(...args: any[]): any;
  /** stdlib */
  export function ForEach(...args: any[]): any;
  /** stdlib */
  export function HStack(...args: any[]): any;
  /** stdlib */
  export function HStackWithInsets(...args: any[]): any;
  /** stdlib */
  export function Image(...args: any[]): any;
  /** stdlib */
  export function ImageFile(...args: any[]): any;
  /** stdlib */
  export function ImageGallery(...args: any[]): any;
  /** stdlib */
  export function ImageSymbol(...args: any[]): any;
  /** stdlib */
  export function LazyVStack(...args: any[]): any;
  /** stdlib */
  export function NavStack(...args: any[]): any;
  /** stdlib */
  export function Picker(...args: any[]): any;
  /** stdlib */
  export function ProgressView(...args: any[]): any;
  /** stdlib */
  export function ScrollView(...args: any[]): any;
  /** stdlib */
  export function Section(...args: any[]): any;
  /** stdlib */
  export function SecureField(...args: any[]): any;
  /** stdlib */
  export function Slider(...args: any[]): any;
  /** stdlib */
  export function Spacer(...args: any[]): any;
  /** stdlib */
  export function SplitView(...args: any[]): any;
  /** stdlib */
  export function State(...args: any[]): any;
  /** stdlib */
  export function TabBar(...args: any[]): any;
  /** stdlib */
  export function Table(...args: any[]): any;
  /** stdlib */
  export function Text(...args: any[]): any;
  /** stdlib */
  export function TextArea(...args: any[]): any;
  /** stdlib */
  export function TextField(...args: any[]): any;
  /** stdlib */
  export function Toggle(...args: any[]): any;
  /** stdlib */
  export function VStack(...args: any[]): any;
  /** stdlib */
  export function VStackWithInsets(...args: any[]): any;
  /** stdlib */
  export function WebView(...args: any[]): any;
  /** stdlib */
  export function Window(...args: any[]): any;
  /** stdlib */
  export function ZStack(...args: any[]): any;
  /** stdlib */
  export function addKeyboardShortcut(...args: any[]): any;
  /** stdlib */
  export function alert(...args: any[]): any;
  /** stdlib */
  export function alertWithButtons(...args: any[]): any;
  /** stdlib */
  export function appSetActivationPolicy(...args: any[]): any;
  /** stdlib */
  export function appSetMaxSize(...args: any[]): any;
  /** stdlib */
  export function appSetMinSize(...args: any[]): any;
  /** stdlib */
  export function appSetTimer(...args: any[]): any;
  /** stdlib */
  export function attributedTextAppend(...args: any[]): any;
  /** stdlib */
  export function attributedTextClear(...args: any[]): any;
  /** stdlib */
  export function bloomViewGetHwnd(...args: any[]): any;
  /** stdlib */
  export function bloomViewGetNativeHandle(...args: any[]): any;
  /** stdlib */
  export function blur(...args: any[]): any;
  /** stdlib */
  export function bottomNavAddItem(...args: any[]): any;
  /** stdlib */
  export function bottomNavSetBadge(...args: any[]): any;
  /** stdlib */
  export function bottomNavSetSelected(...args: any[]): any;
  /** stdlib */
  export function bottomNavSetTintColor(...args: any[]): any;
  /** stdlib */
  export function bottomNavSetUnselectedTintColor(...args: any[]): any;
  /** stdlib */
  export function cameraFreeze(...args: any[]): any;
  /** stdlib */
  export function cameraRegisterFrameCallback(...args: any[]): any;
  /** stdlib */
  export function cameraSampleColor(...args: any[]): any;
  /** stdlib */
  export function cameraSetOnTap(...args: any[]): any;
  /** stdlib */
  export function cameraStart(...args: any[]): any;
  /** stdlib */
  export function cameraStop(...args: any[]): any;
  /** stdlib */
  export function cameraUnfreeze(...args: any[]): any;
  /** stdlib */
  export function cameraUnregisterFrameCallback(...args: any[]): any;
  /** stdlib */
  export function clipboardRead(...args: any[]): any;
  /** stdlib */
  export function clipboardWrite(...args: any[]): any;
  /** stdlib */
  export function currentModifiers(...args: any[]): any;
  /** stdlib */
  export function embedNSView(...args: any[]): any;
  /** stdlib */
  export function focus(...args: any[]): any;
  /** stdlib */
  export function frameSplitAddChild(...args: any[]): any;
  /** stdlib */
  export function frameSplitCreate(...args: any[]): any;
  /** stdlib */
  export function imageGalleryAddImage(...args: any[]): any;
  /** stdlib */
  export function imageGallerySetIndex(...args: any[]): any;
  /** stdlib */
  export function isKeyDown(...args: any[]): any;
  /** stdlib */
  export function lazyvstackEndRefreshing(...args: any[]): any;
  /** stdlib */
  export function lazyvstackSetRefreshControl(...args: any[]): any;
  /** stdlib */
  export function lazyvstackSetScrollEndCallback(...args: any[]): any;
  /** stdlib */
  export function loadImage(...args: any[]): any;
  /** stdlib */
  export function menuAddItem(...args: any[]): any;
  /** stdlib */
  export function menuAddItemWithShortcut(...args: any[]): any;
  /** stdlib */
  export function menuAddSeparator(...args: any[]): any;
  /** stdlib */
  export function menuAddStandardAction(...args: any[]): any;
  /** stdlib */
  export function menuAddSubmenu(...args: any[]): any;
  /** stdlib */
  export function menuBarAddMenu(...args: any[]): any;
  /** stdlib */
  export function menuBarAttach(...args: any[]): any;
  /** stdlib */
  export function menuBarCreate(...args: any[]): any;
  /** stdlib */
  export function menuClear(...args: any[]): any;
  /** stdlib */
  export function menuCreate(...args: any[]): any;
  /** stdlib */
  export function onActivate(...args: any[]): any;
  /** stdlib */
  export function onAppKeyDown(...args: any[]): any;
  /** stdlib */
  export function onAppKeyUp(...args: any[]): any;
  /** stdlib */
  export function onKeyDown(...args: any[]): any;
  /** stdlib */
  export function onKeyUp(...args: any[]): any;
  /** stdlib */
  export function onTerminate(...args: any[]): any;
  /** stdlib */
  export function openFileDialog(...args: any[]): any;
  /** stdlib */
  export function openFolderDialog(...args: any[]): any;
  /** stdlib */
  export function pollOpenFile(...args: any[]): any;
  /** stdlib */
  export function registerGlobalHotkey(...args: any[]): any;
  /** stdlib */
  export function saveFileDialog(...args: any[]): any;
  /** stdlib */
  export function scrollViewSetScrollEndCallback(...args: any[]): any;
  /** stdlib */
  export function scrollviewSetScrollEndCallback(...args: any[]): any;
  /** stdlib */
  export function setText(...args: any[]): any;
  /** stdlib */
  export function sheetCreate(...args: any[]): any;
  /** stdlib */
  export function sheetDismiss(...args: any[]): any;
  /** stdlib */
  export function sheetPresent(...args: any[]): any;
  /** stdlib */
  export function showToast(...args: any[]): any;
  /** stdlib */
  export function toolbarAddItem(...args: any[]): any;
  /** stdlib */
  export function toolbarAttach(...args: any[]): any;
  /** stdlib */
  export function toolbarCreate(...args: any[]): any;
  /** stdlib */
  export function trayAttachMenu(...args: any[]): any;
  /** stdlib */
  export function trayCreate(...args: any[]): any;
  /** stdlib */
  export function trayDestroy(...args: any[]): any;
  /** stdlib */
  export function trayOnClick(...args: any[]): any;
  /** stdlib */
  export function traySetIcon(...args: any[]): any;
  /** stdlib */
  export function traySetTooltip(...args: any[]): any;
  /** stdlib */
  export function webviewCanGoBack(...args: any[]): any;
  /** stdlib */
  export function webviewClearCookies(...args: any[]): any;
  /** stdlib */
  export function webviewEvaluateJs(...args: any[]): any;
  /** stdlib */
  export function webviewGoBack(...args: any[]): any;
  /** stdlib */
  export function webviewGoForward(...args: any[]): any;
  /** stdlib */
  export function webviewLoadUrl(...args: any[]): any;
  /** stdlib */
  export function webviewReload(...args: any[]): any;
}

declare module "perry/updater" {
  /** stdlib */
  export function clearSentinel(...args: any[]): any;
  /** stdlib */
  export function compareVersions(...args: any[]): any;
  /** stdlib */
  export function computeFileSha256(...args: any[]): any;
  /** stdlib */
  export function getBackupPath(...args: any[]): any;
  /** stdlib */
  export function getExePath(...args: any[]): any;
  /** stdlib */
  export function getSentinelPath(...args: any[]): any;
  /** stdlib */
  export function installUpdate(...args: any[]): any;
  /** stdlib */
  export function performRollback(...args: any[]): any;
  /** stdlib */
  export function readSentinel(...args: any[]): any;
  /** stdlib */
  export function relaunch(...args: any[]): any;
  /** stdlib */
  export function verifyHash(...args: any[]): any;
  /** stdlib */
  export function verifySignature(...args: any[]): any;
  /** stdlib */
  export function verifySignatureV2(...args: any[]): any;
  /** stdlib */
  export function writeSentinel(...args: any[]): any;
}

declare module "perry/widget" {
  /** stdlib */
  export function Widget(...args: any[]): any;
}

declare module "perry/workloads" {
  /** stdlib */
  export const policy: any;
  /** stdlib */
  export const runtime: any;
  /** stdlib */
  export function graph(...args: any[]): any;
  /** stdlib */
  export function inspectGraph(...args: any[]): any;
  /** stdlib */
  export function node(...args: any[]): any;
  /** stdlib */
  export function runGraph(...args: any[]): any;
}

declare module "perry/yoga" {
  /** stdlib */
  export function calculateLayout(...args: any[]): any;
  /** stdlib */
  export function childCount(...args: any[]): any;
  /** stdlib */
  export function getComputed(...args: any[]): any;
  /** stdlib */
  export function getComputedEdge(...args: any[]): any;
  /** stdlib */
  export function insertChild(...args: any[]): any;
  /** stdlib */
  export function nodeFree(...args: any[]): any;
  /** stdlib */
  export function nodeNew(...args: any[]): any;
  /** stdlib */
  export function removeChild(...args: any[]): any;
  /** stdlib */
  export function setEdge(...args: any[]): any;
  /** stdlib */
  export function setEnum(...args: any[]): any;
  /** stdlib */
  export function setGap(...args: any[]): any;
  /** stdlib */
  export function setMeasureFunc(...args: any[]): any;
  /** stdlib */
  export function setNumber(...args: any[]): any;
  /** stdlib */
  export function unsetMeasureFunc(...args: any[]): any;
}

declare module "pg" {
  /** stdlib */
  export class Client { [key: string]: any; }
  /** stdlib */
  export class Pool { [key: string]: any; }
  /** stdlib */
  export function Pool(p0: any): any;
  /** stdlib */
  export function connect(p0: any): any;
}

declare module "process" {
  /** stdlib */
  export const _eval: any;
  /** stdlib */
  export const _events: any;
  /** stdlib */
  export const _eventsCount: any;
  /** stdlib */
  export const _exiting: any;
  /** stdlib */
  export const _maxListeners: any;
  /** stdlib */
  export const _preload_modules: any;
  /** stdlib */
  export const allowedNodeEnvironmentFlags: any;
  /** stdlib */
  export const arch: any;
  /** stdlib */
  export const argv: any;
  /** stdlib */
  export const argv0: any;
  /** stdlib */
  export const config: any;
  /** stdlib */
  export const debugPort: any;
  /** stdlib */
  export const domain: any;
  /** stdlib */
  export const env: any;
  /** stdlib */
  export const execArgv: any;
  /** stdlib */
  export const execPath: any;
  /** stdlib */
  export const features: any;
  /** stdlib */
  export const finalization: any;
  /** stdlib */
  export const moduleLoadList: any;
  /** stdlib */
  export const permission: any;
  /** stdlib */
  export const pid: any;
  /** stdlib */
  export const platform: any;
  /** stdlib */
  export const ppid: any;
  /** stdlib */
  export const release: any;
  /** stdlib */
  export const report: any;
  /** stdlib */
  export const stderr: any;
  /** stdlib */
  export const stdin: any;
  /** stdlib */
  export const stdout: any;
  /** stdlib */
  export const title: any;
  /** stdlib */
  export const version: any;
  /** stdlib */
  export const versions: any;
  /** stdlib */
  export function _debugEnd(...args: any[]): any;
  /** stdlib */
  export function _debugProcess(...args: any[]): any;
  /** stdlib */
  export function _fatalException(...args: any[]): any;
  /** stdlib */
  export function _getActiveHandles(...args: any[]): any;
  /** stdlib */
  export function _getActiveRequests(...args: any[]): any;
  /** stdlib */
  export function _kill(...args: any[]): any;
  /** stdlib */
  export function _linkedBinding(...args: any[]): any;
  /** stdlib */
  export function _rawDebug(...args: any[]): any;
  /** stdlib */
  export function _startProfilerIdleNotifier(...args: any[]): any;
  /** stdlib */
  export function _stopProfilerIdleNotifier(...args: any[]): any;
  /** stdlib */
  export function _tickCallback(...args: any[]): any;
  /** stdlib */
  export function abort(...args: any[]): any;
  /** stdlib */
  export function addUncaughtExceptionCaptureCallback(...args: any[]): any;
  /** stdlib */
  export function availableMemory(...args: any[]): any;
  /** stdlib */
  export function binding(...args: any[]): any;
  /** stdlib */
  export function chdir(...args: any[]): any;
  /** stdlib */
  export function constrainedMemory(...args: any[]): any;
  /** stdlib */
  export function cpuUsage(...args: any[]): any;
  /** stdlib */
  export function cwd(...args: any[]): any;
  /** stdlib */
  export function dlopen(...args: any[]): any;
  /** stdlib */
  export function emitWarning(...args: any[]): any;
  /** stdlib */
  export function execve(...args: any[]): any;
  /** stdlib */
  export function exit(...args: any[]): any;
  /** stdlib */
  export function getActiveResourcesInfo(...args: any[]): any;
  /** stdlib */
  export function getBuiltinModule(...args: any[]): any;
  /** stdlib */
  export function getegid(...args: any[]): any;
  /** stdlib */
  export function geteuid(...args: any[]): any;
  /** stdlib */
  export function getgid(...args: any[]): any;
  /** stdlib */
  export function getgroups(...args: any[]): any;
  /** stdlib */
  export function getuid(...args: any[]): any;
  /** stdlib */
  export function hasUncaughtExceptionCaptureCallback(...args: any[]): any;
  /** stdlib */
  export function hrtime(...args: any[]): any;
  /** stdlib */
  export function initgroups(...args: any[]): any;
  /** stdlib */
  export function kill(...args: any[]): any;
  /** stdlib */
  export function loadEnvFile(path?: any): void;
  /** stdlib */
  export function memoryUsage(...args: any[]): any;
  /** stdlib */
  export function nextTick(...args: any[]): any;
  /** stdlib */
  export function openStdin(...args: any[]): any;
  /** stdlib */
  export function reallyExit(...args: any[]): any;
  /** stdlib */
  export function ref(...args: any[]): any;
  /** stdlib */
  export function resourceUsage(...args: any[]): any;
  /** stdlib */
  export function setSourceMapsEnabled(...args: any[]): any;
  /** stdlib */
  export function setUncaughtExceptionCaptureCallback(...args: any[]): any;
  /** stdlib */
  export function setegid(...args: any[]): any;
  /** stdlib */
  export function seteuid(...args: any[]): any;
  /** stdlib */
  export function setgid(...args: any[]): any;
  /** stdlib */
  export function setgroups(...args: any[]): any;
  /** stdlib */
  export function setuid(...args: any[]): any;
  /** stdlib */
  export function sourceMapsEnabled(...args: any[]): any;
  /** stdlib */
  export function threadCpuUsage(...args: any[]): any;
  /** stdlib */
  export function umask(...args: any[]): any;
  /** stdlib */
  export function unref(...args: any[]): any;
  /** stdlib */
  export function uptime(...args: any[]): any;
}

declare module "punycode" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const ucs2: any;
  /** stdlib */
  export const version: any;
  /** stdlib */
  export function decode(...args: any[]): any;
  /** stdlib */
  export function encode(...args: any[]): any;
  /** stdlib */
  export function toASCII(...args: any[]): any;
  /** stdlib */
  export function toUnicode(...args: any[]): any;
}

declare module "querystring" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function decode(...args: any[]): any;
  /** stdlib */
  export function encode(...args: any[]): any;
  /** stdlib */
  export function escape(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function stringify(...args: any[]): any;
  /** stdlib */
  export function unescape(...args: any[]): any;
  /** stdlib */
  export function unescapeBuffer(...args: any[]): any;
}

declare module "rate-limiter-flexible" {
  /** stdlib */
  export class RateLimiterAbstract { [key: string]: any; }
  /** stdlib */
  export class RateLimiterMemory { [key: string]: any; }
}

declare module "readline" {
  /** stdlib */
  export function clearLine(...args: any[]): any;
  /** stdlib */
  export function clearScreenDown(...args: any[]): any;
  /** stdlib */
  export function createInterface(p0: any): any;
  /** stdlib */
  export function cursorTo(...args: any[]): any;
  /** stdlib */
  export function emitKeypressEvents(...args: any[]): any;
  /** stdlib */
  export function moveCursor(...args: any[]): any;
}

declare module "readline/promises" {
  /** stdlib */
  export class Interface { [key: string]: any; }
  /** stdlib */
  export class Readline { [key: string]: any; }
  /** stdlib */
  export function createInterface(p0: any): any;
}

declare module "redis" {
  /** stdlib */
  export class Redis { [key: string]: any; }
  /** stdlib */
  export function createClient(...args: any[]): any;
}

declare module "repl" {
  /** stdlib */
  export class REPLServer { [key: string]: any; }
  /** stdlib */
  export class Recoverable { [key: string]: any; }
  /** stdlib */
  export const REPL_MODE_SLOPPY: any;
  /** stdlib */
  export const REPL_MODE_STRICT: any;
  /** stdlib */
  export const builtinModules: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib @perryStub REPLServer shape only: never reads the input stream, and .write() evaluates just numeric literals, context lookups, and a single '+'; no real JS eval loop (#4916) */
  export function REPLServer(...args: any[]): any;
  /** stdlib */
  export function Recoverable(...args: any[]): any;
  /** stdlib @perryStub REPLServer shape only: never reads the input stream, and .write() evaluates just numeric literals, context lookups, and a single '+'; no real JS eval loop (#4916) */
  export function start(...args: any[]): any;
}

declare module "sea" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function getAsset(...args: any[]): any;
  /** stdlib */
  export function getAssetAsBlob(...args: any[]): any;
  /** stdlib */
  export function getAssetKeys(...args: any[]): any;
  /** stdlib */
  export function getRawAsset(...args: any[]): any;
  /** stdlib */
  export function isSea(...args: any[]): any;
}

declare module "sharp" {
  /** stdlib */
  export default function (p0: string): any;
  /** stdlib */
  export function sharp(p0: string): any;
}

declare module "slugify" {
  /** stdlib */
  export default function (p0: string, p1?: any): string;
  /** stdlib */
  export function slugify(p0: string, p1?: any): string;
}

declare module "sqlite" {
  /** stdlib */
  export class DatabaseSync { [key: string]: any; }
  /** stdlib */
  export class SQLTagStore { [key: string]: any; }
  /** stdlib */
  export class Session { [key: string]: any; }
  /** stdlib */
  export class StatementSync { [key: string]: any; }
  /** stdlib */
  export const constants: any;
  /** stdlib */
  export function DatabaseSync(...args: any[]): any;
  /** stdlib */
  export function Session(...args: any[]): any;
  /** stdlib */
  export function StatementSync(...args: any[]): any;
  /** stdlib */
  export function backup(...args: any[]): any;
}

declare module "stream" {
  /** stdlib */
  export class Duplex { [key: string]: any; }
  /** stdlib */
  export class PassThrough { [key: string]: any; }
  /** stdlib */
  export class Readable { [key: string]: any; }
  /** stdlib */
  export class Stream { [key: string]: any; }
  /** stdlib */
  export class Transform { [key: string]: any; }
  /** stdlib */
  export class Writable { [key: string]: any; }
  /** stdlib */
  export const promises: any;
  /** stdlib */
  export const promises: any;
  /** stdlib */
  export function _isArrayBufferView(...args: any[]): any;
  /** stdlib */
  export function _isUint8Array(...args: any[]): any;
  /** stdlib */
  export function _uint8ArrayToBuffer(...args: any[]): any;
  /** stdlib */
  export function addAbortSignal(...args: any[]): any;
  /** stdlib */
  export function compose(...args: any[]): any;
  /** stdlib */
  export default function (...args: any[]): any;
  /** stdlib */
  export function duplexPair(...args: any[]): any;
  /** stdlib */
  export function finished(...args: any[]): any;
  /** stdlib */
  export function getDefaultHighWaterMark(...args: any[]): any;
  /** stdlib */
  export function isDestroyed(...args: any[]): any;
  /** stdlib */
  export function isDisturbed(...args: any[]): any;
  /** stdlib */
  export function isErrored(...args: any[]): any;
  /** stdlib */
  export function isReadable(...args: any[]): any;
  /** stdlib */
  export function isWritable(...args: any[]): any;
  /** stdlib */
  export function pipeline(...args: any[]): any;
  /** stdlib */
  export function setDefaultHighWaterMark(...args: any[]): any;
}

declare module "stream/consumers" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function arrayBuffer(...args: any[]): any;
  /** stdlib */
  export function blob(...args: any[]): any;
  /** stdlib */
  export function buffer(...args: any[]): any;
  /** stdlib */
  export function bytes(...args: any[]): any;
  /** stdlib */
  export function json(...args: any[]): any;
  /** stdlib */
  export function text(...args: any[]): any;
}

declare module "stream/promises" {
  /** stdlib */
  export function finished(...args: any[]): any;
  /** stdlib */
  export function pipeline(...args: any[]): any;
}

declare module "stream/web" {
  /** stdlib */
  export class ByteLengthQueuingStrategy { [key: string]: any; }
  /** stdlib */
  export class CompressionStream { [key: string]: any; }
  /** stdlib */
  export class CountQueuingStrategy { [key: string]: any; }
  /** stdlib */
  export class DecompressionStream { [key: string]: any; }
  /** stdlib */
  export class ReadableByteStreamController { [key: string]: any; }
  /** stdlib */
  export class ReadableStream { [key: string]: any; }
  /** stdlib */
  export class ReadableStreamBYOBReader { [key: string]: any; }
  /** stdlib */
  export class ReadableStreamBYOBRequest { [key: string]: any; }
  /** stdlib */
  export class ReadableStreamDefaultController { [key: string]: any; }
  /** stdlib */
  export class ReadableStreamDefaultReader { [key: string]: any; }
  /** stdlib */
  export class TextDecoderStream { [key: string]: any; }
  /** stdlib */
  export class TextEncoderStream { [key: string]: any; }
  /** stdlib */
  export class TransformStream { [key: string]: any; }
  /** stdlib */
  export class TransformStreamDefaultController { [key: string]: any; }
  /** stdlib */
  export class WritableStream { [key: string]: any; }
  /** stdlib */
  export class WritableStreamDefaultController { [key: string]: any; }
  /** stdlib */
  export class WritableStreamDefaultWriter { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
}

declare module "streams" {
  /** stdlib */
  export class ByteLengthQueuingStrategy { [key: string]: any; }
  /** stdlib */
  export class CountQueuingStrategy { [key: string]: any; }
  /** stdlib */
  export class DecompressionStream { [key: string]: any; }
  /** stdlib */
  export class ReadableStream { [key: string]: any; }
  /** stdlib */
  export class TextDecoder { [key: string]: any; }
  /** stdlib */
  export class TextEncoder { [key: string]: any; }
  /** stdlib */
  export class TransformStream { [key: string]: any; }
  /** stdlib */
  export class WritableStream { [key: string]: any; }
}

declare module "string_decoder" {
  /** stdlib */
  export class StringDecoder { [key: string]: any; }
}

declare module "sys" {
  /** stdlib */
  export class MIMEParams { [key: string]: any; }
  /** stdlib */
  export class MIMEType { [key: string]: any; }
  /** stdlib */
  export class TextDecoder { [key: string]: any; }
  /** stdlib */
  export class TextEncoder { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const types: any;
  /** stdlib */
  export function MIMEParams(...args: any[]): any;
  /** stdlib */
  export function MIMEType(...args: any[]): any;
  /** stdlib */
  export function _errnoException(...args: any[]): any;
  /** stdlib */
  export function _exceptionWithHostPort(...args: any[]): any;
  /** stdlib */
  export function _extend(...args: any[]): any;
  /** stdlib */
  export function aborted(...args: any[]): any;
  /** stdlib */
  export function callbackify(...args: any[]): any;
  /** stdlib */
  export function convertProcessSignalToExitCode(...args: any[]): any;
  /** stdlib */
  export function debug(...args: any[]): any;
  /** stdlib */
  export function debuglog(...args: any[]): any;
  /** stdlib */
  export function deprecate(...args: any[]): any;
  /** stdlib */
  export function diff(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function formatWithOptions(...args: any[]): any;
  /** stdlib */
  export function getCallSites(...args: any[]): any;
  /** stdlib */
  export function getSystemErrorMap(...args: any[]): any;
  /** stdlib */
  export function getSystemErrorMessage(...args: any[]): any;
  /** stdlib */
  export function getSystemErrorName(...args: any[]): any;
  /** stdlib */
  export function inherits(...args: any[]): any;
  /** stdlib */
  export function inspect(...args: any[]): any;
  /** stdlib */
  export function isArray(value: any): boolean;
  /** stdlib */
  export function isDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function parseArgs(...args: any[]): any;
  /** stdlib */
  export function parseEnv(...args: any[]): any;
  /** stdlib */
  export function promisify(...args: any[]): any;
  /** stdlib */
  export function setTraceSigInt(...args: any[]): any;
  /** stdlib */
  export function stripVTControlCharacters(...args: any[]): any;
  /** stdlib */
  export function styleText(...args: any[]): any;
  /** stdlib */
  export function toUSVString(...args: any[]): any;
  /** stdlib */
  export function transferableAbortController(...args: any[]): any;
  /** stdlib */
  export function transferableAbortSignal(...args: any[]): any;
}

declare module "test" {
  /** stdlib */
  export const assert: any;
  /** stdlib */
  export const mock: any;
  /** stdlib */
  export const snapshot: any;
  /** stdlib */
  export function after(...args: any[]): any;
  /** stdlib */
  export function afterEach(...args: any[]): any;
  /** stdlib */
  export function before(...args: any[]): any;
  /** stdlib */
  export function beforeEach(...args: any[]): any;
  /** stdlib */
  export default function (...args: any[]): any;
  /** stdlib */
  export function describe(...args: any[]): any;
  /** stdlib */
  export function expectFailure(...args: any[]): any;
  /** stdlib */
  export function it(...args: any[]): any;
  /** stdlib */
  export function only(...args: any[]): any;
  /** stdlib */
  export function run(...args: any[]): any;
  /** stdlib */
  export function skip(...args: any[]): any;
  /** stdlib */
  export function suite(...args: any[]): any;
  /** stdlib */
  export function test(...args: any[]): any;
  /** stdlib */
  export function todo(...args: any[]): any;
}

declare module "test/reporters" {
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function dot(...args: any[]): any;
  /** stdlib */
  export function junit(...args: any[]): any;
  /** stdlib */
  export function lcov(...args: any[]): any;
  /** stdlib */
  export function spec(...args: any[]): any;
  /** stdlib */
  export function tap(...args: any[]): any;
}

declare module "timers" {
  /** stdlib */
  export const promises: any;
  /** stdlib */
  export function clearImmediate(...args: any[]): any;
  /** stdlib */
  export function clearInterval(...args: any[]): any;
  /** stdlib */
  export function clearTimeout(...args: any[]): any;
  /** stdlib */
  export function setImmediate(...args: any[]): any;
  /** stdlib */
  export function setInterval(...args: any[]): any;
  /** stdlib */
  export function setTimeout(...args: any[]): any;
}

declare module "timers/promises" {
  /** stdlib */
  export const scheduler: any;
  /** stdlib */
  export function setImmediate(...args: any[]): any;
  /** stdlib */
  export function setInterval(...args: any[]): any;
  /** stdlib */
  export function setTimeout(...args: any[]): any;
}

declare module "tls" {
  /** stdlib */
  export class SecureContext { [key: string]: any; }
  /** stdlib */
  export const CLIENT_RENEG_LIMIT: any;
  /** stdlib */
  export const CLIENT_RENEG_WINDOW: any;
  /** stdlib */
  export const DEFAULT_CIPHERS: any;
  /** stdlib */
  export const DEFAULT_ECDH_CURVE: any;
  /** stdlib */
  export const DEFAULT_MAX_VERSION: any;
  /** stdlib */
  export const DEFAULT_MIN_VERSION: any;
  /** stdlib */
  export const rootCertificates: any;
  /** stdlib */
  export function SecureContext(options: any): any;
  /** stdlib */
  export function Server(options: any, secureConnectionListener: any): any;
  /** stdlib */
  export function TLSSocket(socket: any, options: any): any;
  /** stdlib */
  export function checkServerIdentity(hostname: any, cert: any): any;
  /** stdlib */
  export function connect(p0: any, p1: any, p2: any, p3: any): any;
  /** stdlib */
  export function createSecureContext(options: any): any;
  /** stdlib */
  export function createServer(options: any, secureConnectionListener: any): any;
  /** stdlib */
  export function getCACertificates(type: any): any;
  /** stdlib */
  export function getCiphers(...args: any[]): any;
  /** stdlib */
  export function setDefaultCACertificates(certs: any): any;
}

declare module "tty" {
  /** stdlib */
  export class ReadStream { [key: string]: any; }
  /** stdlib */
  export class WriteStream { [key: string]: any; }
  /** stdlib */
  export function ReadStream(...args: any[]): any;
  /** stdlib */
  export function WriteStream(...args: any[]): any;
  /** stdlib */
  export function isatty(...args: any[]): any;
}

declare module "tursodb" {
  /** stdlib */
  export function open(...args: any[]): any;
}

declare module "url" {
  /** stdlib */
  export class URL { [key: string]: any; }
  /** stdlib */
  export class URLPattern { [key: string]: any; }
  /** stdlib */
  export class URLSearchParams { [key: string]: any; }
  /** stdlib */
  export class Url { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function Url(...args: any[]): any;
  /** stdlib */
  export function domainToASCII(...args: any[]): any;
  /** stdlib */
  export function domainToUnicode(...args: any[]): any;
  /** stdlib */
  export function fileURLToPath(...args: any[]): any;
  /** stdlib */
  export function fileURLToPathBuffer(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function pathToFileURL(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
  /** stdlib */
  export function resolveObject(...args: any[]): any;
  /** stdlib */
  export function urlToHttpOptions(...args: any[]): any;
}

declare module "util" {
  /** stdlib */
  export class MIMEParams { [key: string]: any; }
  /** stdlib */
  export class MIMEType { [key: string]: any; }
  /** stdlib */
  export class TextDecoder { [key: string]: any; }
  /** stdlib */
  export class TextEncoder { [key: string]: any; }
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export const types: any;
  /** stdlib */
  export function MIMEParams(...args: any[]): any;
  /** stdlib */
  export function MIMEType(...args: any[]): any;
  /** stdlib */
  export function _errnoException(...args: any[]): any;
  /** stdlib */
  export function _exceptionWithHostPort(...args: any[]): any;
  /** stdlib */
  export function _extend(...args: any[]): any;
  /** stdlib */
  export function aborted(...args: any[]): any;
  /** stdlib */
  export function callbackify(...args: any[]): any;
  /** stdlib */
  export function convertProcessSignalToExitCode(...args: any[]): any;
  /** stdlib */
  export function debug(...args: any[]): any;
  /** stdlib */
  export function debuglog(...args: any[]): any;
  /** stdlib */
  export function deprecate(...args: any[]): any;
  /** stdlib */
  export function diff(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function formatWithOptions(...args: any[]): any;
  /** stdlib */
  export function getCallSites(...args: any[]): any;
  /** stdlib */
  export function getSystemErrorMap(...args: any[]): any;
  /** stdlib */
  export function getSystemErrorMessage(...args: any[]): any;
  /** stdlib */
  export function getSystemErrorName(...args: any[]): any;
  /** stdlib */
  export function inherits(...args: any[]): any;
  /** stdlib */
  export function inspect(...args: any[]): any;
  /** stdlib */
  export function isArray(value: any): boolean;
  /** stdlib */
  export function isDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function parseArgs(...args: any[]): any;
  /** stdlib */
  export function parseEnv(...args: any[]): any;
  /** stdlib */
  export function promisify(...args: any[]): any;
  /** stdlib */
  export function setTraceSigInt(...args: any[]): any;
  /** stdlib */
  export function stripVTControlCharacters(...args: any[]): any;
  /** stdlib */
  export function styleText(...args: any[]): any;
  /** stdlib */
  export function toUSVString(...args: any[]): any;
  /** stdlib */
  export function transferableAbortController(...args: any[]): any;
  /** stdlib */
  export function transferableAbortSignal(...args: any[]): any;
}

declare module "util/types" {
  /** stdlib */
  export function isAnyArrayBuffer(...args: any[]): any;
  /** stdlib */
  export function isArgumentsObject(...args: any[]): any;
  /** stdlib */
  export function isArrayBuffer(...args: any[]): any;
  /** stdlib */
  export function isArrayBufferView(...args: any[]): any;
  /** stdlib */
  export function isAsyncFunction(...args: any[]): any;
  /** stdlib */
  export function isBigInt64Array(...args: any[]): any;
  /** stdlib */
  export function isBigIntObject(...args: any[]): any;
  /** stdlib */
  export function isBigUint64Array(...args: any[]): any;
  /** stdlib */
  export function isBooleanObject(...args: any[]): any;
  /** stdlib */
  export function isBoxedPrimitive(...args: any[]): any;
  /** stdlib */
  export function isCryptoKey(...args: any[]): any;
  /** stdlib */
  export function isDataView(...args: any[]): any;
  /** stdlib */
  export function isDate(...args: any[]): any;
  /** stdlib */
  export function isExternal(...args: any[]): any;
  /** stdlib */
  export function isFloat16Array(...args: any[]): any;
  /** stdlib */
  export function isFloat32Array(...args: any[]): any;
  /** stdlib */
  export function isFloat64Array(...args: any[]): any;
  /** stdlib */
  export function isGeneratorFunction(...args: any[]): any;
  /** stdlib */
  export function isGeneratorObject(...args: any[]): any;
  /** stdlib */
  export function isInt16Array(...args: any[]): any;
  /** stdlib */
  export function isInt32Array(...args: any[]): any;
  /** stdlib */
  export function isInt8Array(...args: any[]): any;
  /** stdlib */
  export function isKeyObject(...args: any[]): any;
  /** stdlib */
  export function isMap(...args: any[]): any;
  /** stdlib */
  export function isMapIterator(...args: any[]): any;
  /** stdlib */
  export function isModuleNamespaceObject(...args: any[]): any;
  /** stdlib */
  export function isNativeError(...args: any[]): any;
  /** stdlib */
  export function isNumberObject(...args: any[]): any;
  /** stdlib */
  export function isPromise(...args: any[]): any;
  /** stdlib */
  export function isProxy(...args: any[]): any;
  /** stdlib */
  export function isRegExp(...args: any[]): any;
  /** stdlib */
  export function isSet(...args: any[]): any;
  /** stdlib */
  export function isSetIterator(...args: any[]): any;
  /** stdlib */
  export function isSharedArrayBuffer(...args: any[]): any;
  /** stdlib */
  export function isStringObject(...args: any[]): any;
  /** stdlib */
  export function isSymbolObject(...args: any[]): any;
  /** stdlib */
  export function isTypedArray(...args: any[]): any;
  /** stdlib */
  export function isUint16Array(...args: any[]): any;
  /** stdlib */
  export function isUint32Array(...args: any[]): any;
  /** stdlib */
  export function isUint8Array(...args: any[]): any;
  /** stdlib */
  export function isUint8ClampedArray(...args: any[]): any;
  /** stdlib */
  export function isWeakMap(...args: any[]): any;
  /** stdlib */
  export function isWeakSet(...args: any[]): any;
}

declare module "uuid" {
  /** stdlib */
  export function v1(): string;
  /** stdlib */
  export function v3(name: string, namespace: string): string;
  /** stdlib */
  export function v4(): string;
  /** stdlib */
  export function v5(name: string, namespace: string): string;
  /** stdlib */
  export function v7(): string;
  /** stdlib */
  export function validate(id: string): boolean;
  /** stdlib */
  export function version(id: string): number;
}

declare module "v8" {
  /** stdlib */
  export class DefaultDeserializer { [key: string]: any; }
  /** stdlib */
  export class DefaultSerializer { [key: string]: any; }
  /** stdlib */
  export class Deserializer { [key: string]: any; }
  /** stdlib */
  export class GCProfiler { [key: string]: any; }
  /** stdlib */
  export class Serializer { [key: string]: any; }
  /** stdlib */
  export const promiseHooks: any;
  /** stdlib */
  export const startupSnapshot: any;
  /** stdlib */
  export function cachedDataVersionTag(...args: any[]): any;
  /** stdlib */
  export function deserialize(...args: any[]): any;
  /** stdlib */
  export function getCppHeapStatistics(...args: any[]): any;
  /** stdlib @perryStub all fields 0; Perry compiles AOT, there is no JIT code heap (#4916) */
  export function getHeapCodeStatistics(...args: any[]): any;
  /** stdlib */
  export function getHeapSnapshot(...args: any[]): any;
  /** stdlib @perryStub Node space names with all live usage attributed to old_space from Perry arenas; other spaces report 0 (#4916) */
  export function getHeapSpaceStatistics(...args: any[]): any;
  /** stdlib @perryStub Node shape, Perry numbers: total_heap_size/used_heap_size/malloced_memory/total_allocated_bytes from Perry arenas, total_physical_size=RSS, heap_size_limit fixed ~2GB (not enforced); *_executable, external_memory, global-handles and zap fields are 0 (#4916) */
  export function getHeapStatistics(...args: any[]): any;
  /** stdlib */
  export function isStringOneByteRepresentation(...args: any[]): any;
  /** stdlib */
  export function queryObjects(...args: any[]): any;
  /** stdlib */
  export function serialize(...args: any[]): any;
  /** stdlib */
  export function setFlagsFromString(...args: any[]): any;
  /** stdlib */
  export function setHeapSnapshotNearHeapLimit(...args: any[]): any;
  /** stdlib */
  export function startCpuProfile(...args: any[]): any;
  /** stdlib */
  export function stopCoverage(...args: any[]): any;
  /** stdlib */
  export function takeCoverage(...args: any[]): any;
  /** stdlib */
  export function writeHeapSnapshot(...args: any[]): any;
}

declare module "validator" {
  /** stdlib */
  export function isEmail(s: string): boolean;
  /** stdlib */
  export function isEmpty(s: string): boolean;
  /** stdlib */
  export function isJSON(s: string): boolean;
  /** stdlib */
  export function isURL(s: string): boolean;
  /** stdlib */
  export function isUUID(s: string): boolean;
}

declare module "vm" {
  /** stdlib */
  export class Script { [key: string]: any; }
  /** stdlib */
  export const constants: any;
  /** stdlib */
  const _default: any;
  export default _default;
  /** stdlib */
  export function compileFunction(...args: any[]): any;
  /** stdlib */
  export function createContext(p0: any): any;
  /** stdlib */
  export function createScript(...args: any[]): any;
  /** stdlib */
  export function isContext(...args: any[]): any;
  /** stdlib */
  export function measureMemory(...args: any[]): any;
  /** stdlib */
  export function runInContext(...args: any[]): any;
  /** stdlib */
  export function runInNewContext(...args: any[]): any;
  /** stdlib */
  export function runInThisContext(...args: any[]): any;
}

declare module "wasi" {
  /** stdlib */
  export class WASI { [key: string]: any; }
  /** stdlib */
  export const wasiImport: any;
  /** stdlib */
  export function WASI(...args: any[]): any;
}

declare module "worker_threads" {
  /** stdlib */
  export class BroadcastChannel { [key: string]: any; }
  /** stdlib */
  export class MessageChannel { [key: string]: any; }
  /** stdlib */
  export class MessagePort { [key: string]: any; }
  /** stdlib */
  export class Worker { [key: string]: any; }
  /** stdlib */
  export const SHARE_ENV: any;
  /** stdlib */
  export const isInternalThread: any;
  /** stdlib */
  export const isMainThread: any;
  /** stdlib */
  export const locks: any;
  /** stdlib */
  export const parentPort: any;
  /** stdlib */
  export const resourceLimits: any;
  /** stdlib */
  export const threadId: any;
  /** stdlib */
  export const threadName: any;
  /** stdlib */
  export const workerData: any;
  /** stdlib */
  export function BroadcastChannel(p0: any): any;
  /** stdlib */
  export function MessageChannel(...args: any[]): any;
  /** stdlib */
  export function getEnvironmentData(p0: any): any;
  /** stdlib */
  export function isMarkedAsUntransferable(p0: any): boolean;
  /** stdlib */
  export function markAsUncloneable(p0: any): void;
  /** stdlib */
  export function markAsUntransferable(p0: any): void;
  /** stdlib */
  export function moveMessagePortToContext(p0: any, p1: any): any;
  /** stdlib */
  export function postMessageToThread(p0: any, p1: any, p2: any, p3: any): any;
  /** stdlib */
  export function receiveMessageOnPort(p0: any): any;
  /** stdlib */
  export function setEnvironmentData(p0: any, p1: any): void;
}

declare module "ws" {
  /** stdlib */
  export class Client { [key: string]: any; }
  /** stdlib */
  export class WebSocket { [key: string]: any; }
  /** stdlib */
  export class WebSocketServer { [key: string]: any; }
  /** stdlib */
  export const CLOSED: any;
  /** stdlib */
  export const CLOSING: any;
  /** stdlib */
  export const CONNECTING: any;
  /** stdlib */
  export const OPEN: any;
  /** stdlib */
  export function Server(p0: any): any;
  /** stdlib */
  export function WebSocket(p0: string): any;
  /** stdlib */
  export function closeClient(p0: any): void;
  /** stdlib */
  export function sendToClient(p0: any, p1: string): void;
}

declare module "zlib" {
  /** stdlib */
  export class BrotliCompress { [key: string]: any; }
  /** stdlib */
  export class BrotliCompress { [key: string]: any; }
  /** stdlib */
  export class BrotliDecompress { [key: string]: any; }
  /** stdlib */
  export class BrotliDecompress { [key: string]: any; }
  /** stdlib */
  export class Deflate { [key: string]: any; }
  /** stdlib */
  export class Deflate { [key: string]: any; }
  /** stdlib */
  export class DeflateRaw { [key: string]: any; }
  /** stdlib */
  export class DeflateRaw { [key: string]: any; }
  /** stdlib */
  export class Gunzip { [key: string]: any; }
  /** stdlib */
  export class Gunzip { [key: string]: any; }
  /** stdlib */
  export class Gzip { [key: string]: any; }
  /** stdlib */
  export class Gzip { [key: string]: any; }
  /** stdlib */
  export class Inflate { [key: string]: any; }
  /** stdlib */
  export class Inflate { [key: string]: any; }
  /** stdlib */
  export class InflateRaw { [key: string]: any; }
  /** stdlib */
  export class InflateRaw { [key: string]: any; }
  /** stdlib */
  export class Unzip { [key: string]: any; }
  /** stdlib */
  export class Unzip { [key: string]: any; }
  /** stdlib */
  export class ZstdCompress { [key: string]: any; }
  /** stdlib */
  export class ZstdDecompress { [key: string]: any; }
  /** stdlib */
  export const codes: any;
  /** stdlib */
  export const constants: any;
  /** stdlib */
  export function brotliCompress(buffer: any, callback: any): void;
  /** stdlib */
  export function brotliCompressSync(p0: string): Buffer;
  /** stdlib */
  export function brotliDecompress(buffer: any, callback: any): void;
  /** stdlib */
  export function brotliDecompressSync(p0: string): Buffer;
  /** stdlib */
  export function crc32(p0: string, seed?: number): number;
  /** stdlib @perryStub params/quality options accepted but ignored, warns once (#4917) */
  export function createBrotliCompress(options?: any): any;
  /** stdlib @perryStub params/quality options accepted but ignored, warns once (#4917) */
  export function createBrotliDecompress(options?: any): any;
  /** stdlib @perryStub level honored; strategy/memLevel validated but not applied (#4917) */
  export function createDeflate(options?: any): any;
  /** stdlib @perryStub level honored; strategy/memLevel validated but not applied (#4917) */
  export function createDeflateRaw(options?: any): any;
  /** stdlib */
  export function createGunzip(options?: any): any;
  /** stdlib @perryStub level honored; strategy/memLevel validated but not applied (#4917) */
  export function createGzip(options?: any): any;
  /** stdlib */
  export function createInflate(options?: any): any;
  /** stdlib */
  export function createInflateRaw(options?: any): any;
  /** stdlib */
  export function createUnzip(options?: any): any;
  /** stdlib @perryStub params/quality options accepted but ignored, warns once (#4917) */
  export function createZstdCompress(options?: any): any;
  /** stdlib @perryStub params/quality options accepted but ignored, warns once (#4917) */
  export function createZstdDecompress(options?: any): any;
  /** stdlib */
  export function deflate(buffer: any, callback: any): void;
  /** stdlib */
  export function deflateRaw(buffer: any, callback: any): void;
  /** stdlib */
  export function deflateRawSync(p0: any, options?: any): Buffer;
  /** stdlib */
  export function deflateSync(p0: any, options?: any): Buffer;
  /** stdlib */
  export function gunzip(buffer: any, callback: any): void;
  /** stdlib */
  export function gunzipSync(p0: any): Buffer;
  /** stdlib */
  export function gzip(buffer: any, callback: any): void;
  /** stdlib */
  export function gzipSync(p0: any, options?: any): Buffer;
  /** stdlib */
  export function inflate(buffer: any, callback: any): void;
  /** stdlib */
  export function inflateRaw(buffer: any, callback: any): void;
  /** stdlib */
  export function inflateRawSync(p0: string): Buffer;
  /** stdlib */
  export function inflateSync(p0: any): Buffer;
  /** stdlib */
  export function unzip(buffer: any, callback: any): void;
  /** stdlib */
  export function unzipSync(p0: string): Buffer;
  /** stdlib */
  export function zstdCompress(buffer: any, callback: any): void;
  /** stdlib */
  export function zstdCompressSync(p0: any, options?: any): Buffer;
  /** stdlib */
  export function zstdDecompress(buffer: any, callback: any): void;
  /** stdlib */
  export function zstdDecompressSync(p0: any, options?: any): Buffer;
}
