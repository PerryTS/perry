// Auto-generated from Perry's API manifest (#465). Do not edit by hand.
// Source: perry-api-manifest::API_MANIFEST
// Perry version: 0.5.598
// Coverage: 397 entries across 45 modules

declare module "argon2" {
  /** stdlib */
  export function hash(password: string): any;
  /** stdlib */
  export function verify(hash: string, password: string): any;
}

declare module "async_hooks" {
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

declare module "buffer" {
  /** stdlib */
  export class Buffer { [key: string]: any; }
}

declare module "cheerio" {
  /** stdlib */
  export function load(p0: string): any;
}

declare module "commander" {
}

declare module "cron" {
  /** stdlib */
  export function describe(expr: string): string;
  /** stdlib */
  export function schedule(expr: string, handler: any): any;
  /** stdlib */
  export function validate(expr: string): boolean;
}

declare module "crypto" {
  /** stdlib */
  export function createHash(...args: any[]): any;
  /** stdlib */
  export function createHmac(...args: any[]): any;
  /** stdlib */
  export function getRandomValues(...args: any[]): any;
  /** stdlib */
  export function md5(...args: any[]): any;
  /** stdlib */
  export function pbkdf2(...args: any[]): any;
  /** stdlib */
  export function pbkdf2Sync(...args: any[]): any;
  /** stdlib */
  export function randomBytes(...args: any[]): any;
  /** stdlib */
  export function randomUUID(...args: any[]): any;
  /** stdlib */
  export function sha256(...args: any[]): any;
}

declare module "dayjs" {
  /** stdlib */
  export function dayjs(...args: any[]): any;
  /** stdlib */
  export default function (...args: any[]): any;
}

declare module "decimal.js" {
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
  export function EventEmitter(...args: any[]): any;
}

declare module "exponential-backoff" {
  /** stdlib */
  export function backOff(p0: any, p1: any): any;
}

declare module "fastify" {
  /** stdlib */
  export default function (p0: any): any;
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
  export function sign(payload: any, secret: string, options: any): any;
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
  export function kebabCase(p0: string): string;
  /** stdlib */
  export function last(p0: any): any;
  /** stdlib */
  export function range(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function reverse(p0: any): any;
  /** stdlib */
  export function size(p0: any): any;
  /** stdlib */
  export function snakeCase(p0: string): string;
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

declare module "moment" {
  /** stdlib */
  export default function (...args: any[]): any;
  /** stdlib */
  export function moment(...args: any[]): any;
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
  export class Server { [key: string]: any; }
  /** stdlib */
  export class Socket { [key: string]: any; }
  /** stdlib */
  export function Socket(...args: any[]): any;
  /** stdlib */
  export function connect(p0: any, p1: string): any;
  /** stdlib */
  export function createConnection(p0: any, p1: string): any;
}

declare module "nodemailer" {
  /** stdlib */
  export function createTransport(p0: any): any;
}

declare module "os" {
  /** stdlib */
  export const EOL: any;
  /** stdlib */
  export function arch(...args: any[]): any;
  /** stdlib */
  export function cpus(...args: any[]): any;
  /** stdlib */
  export function freemem(...args: any[]): any;
  /** stdlib */
  export function homedir(...args: any[]): any;
  /** stdlib */
  export function hostname(...args: any[]): any;
  /** stdlib */
  export function networkInterfaces(...args: any[]): any;
  /** stdlib */
  export function platform(...args: any[]): any;
  /** stdlib */
  export function release(...args: any[]): any;
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
}

declare module "path" {
  /** stdlib */
  export const delimiter: any;
  /** stdlib */
  export const posix: any;
  /** stdlib */
  export const sep: any;
  /** stdlib */
  export const win32: any;
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
  export function normalize(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function relative(...args: any[]): any;
  /** stdlib */
  export function resolve(...args: any[]): any;
}

declare module "perry/thread" {
  /** stdlib */
  export function parallelFilter(p0: any, p1: any): any;
  /** stdlib */
  export function parallelMap(p0: any, p1: any): any;
  /** stdlib */
  export function spawn(p0: any): any;
}

declare module "perry/tui" {
  /** stdlib */
  export function Box(...args: any[]): any;
  /** stdlib */
  export function Input(p0: string): any;
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
  export function Text(p0: string): any;
  /** stdlib */
  export function TextArea(p0: string): any;
  /** stdlib */
  export function boxSetAlignItems(p0: any, p1: string): void;
  /** stdlib */
  export function boxSetFlexDirection(p0: any, p1: string): void;
  /** stdlib */
  export function boxSetFlexGrow(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetGap(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetHeight(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetJustifyContent(p0: any, p1: string): void;
  /** stdlib */
  export function boxSetPadding(p0: any, p1: any): void;
  /** stdlib */
  export function boxSetWidth(p0: any, p1: any): void;
  /** stdlib */
  export function enter(): void;
  /** stdlib */
  export function exit(): void;
  /** stdlib */
  export function render(p0: any): void;
  /** stdlib */
  export function run(p0: any): void;
  /** stdlib */
  export function state(p0: any): any;
  /** stdlib */
  export function useInput(p0: any): void;
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
  export const arch: any;
  /** stdlib */
  export const argv: any;
  /** stdlib */
  export const env: any;
  /** stdlib */
  export const pid: any;
  /** stdlib */
  export const platform: any;
  /** stdlib */
  export const ppid: any;
  /** stdlib */
  export const stderr: any;
  /** stdlib */
  export const stdin: any;
  /** stdlib */
  export const stdout: any;
  /** stdlib */
  export const version: any;
  /** stdlib */
  export const versions: any;
}

declare module "readline" {
  /** stdlib */
  export function createInterface(p0: any): any;
}

declare module "sharp" {
  /** stdlib */
  export default function (p0: string): any;
  /** stdlib */
  export function sharp(p0: string): any;
}

declare module "slugify" {
  /** stdlib */
  export default function (p0: string, p1: string, p2: string): string;
  /** stdlib */
  export function slugify(p0: string, p1: string, p2: string): string;
}

declare module "tls" {
  /** stdlib */
  export function connect(p0: string, p1: any, p2: string, p3: any): any;
}

declare module "tursodb" {
  /** stdlib */
  export function open(...args: any[]): any;
}

declare module "url" {
  /** stdlib */
  export class URL { [key: string]: any; }
  /** stdlib */
  export class URLSearchParams { [key: string]: any; }
}

declare module "uuid" {
  /** stdlib */
  export function v1(): string;
  /** stdlib */
  export function v4(): string;
  /** stdlib */
  export function v7(): string;
  /** stdlib */
  export function validate(id: string): boolean;
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

declare module "worker_threads" {
  /** stdlib */
  export function getWorkerData(...args: any[]): any;
  /** stdlib */
  export function parentPort(...args: any[]): any;
  /** stdlib */
  export function workerData(...args: any[]): any;
}

declare module "ws" {
  /** stdlib */
  export class WebSocket { [key: string]: any; }
  /** stdlib */
  export class WebSocketServer { [key: string]: any; }
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
  export function deflateSync(p0: string): string;
  /** stdlib */
  export function gunzip(p0: string): any;
  /** stdlib */
  export function gunzipSync(p0: string): string;
  /** stdlib */
  export function gzip(p0: string): any;
  /** stdlib */
  export function gzipSync(p0: string): string;
  /** stdlib */
  export function inflateSync(p0: string): string;
}

