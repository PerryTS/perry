// Auto-generated from Perry's API manifest (#465). Do not edit by hand.
// Source: perry-api-manifest::API_MANIFEST
// Coverage: 928 entries across 74 modules

declare module "@perryts/google-auth" {
  /** stdlib */
  export function js_google_auth_sign_in(...args: any[]): any;
  /** stdlib */
  export function js_google_auth_sign_out(...args: any[]): any;
  /** stdlib */
  export function js_google_auth_silent_sign_in(...args: any[]): any;
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

declare module "async_hooks" {
  /** stdlib */
  export class AsyncLocalStorage { [key: string]: any; }
  /** stdlib */
  export class AsyncResource { [key: string]: any; }
  /** stdlib */
  export function createHook(...args: any[]): any;
  /** stdlib */
  export function executionAsyncId(...args: any[]): any;
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
  export class Buffer { [key: string]: any; }
  /** stdlib */
  export function alloc(...args: any[]): any;
  /** stdlib */
  export function allocUnsafe(...args: any[]): any;
  /** stdlib */
  export function byteLength(...args: any[]): any;
  /** stdlib */
  export function concat(...args: any[]): any;
  /** stdlib */
  export function from(...args: any[]): any;
  /** stdlib */
  export function isBuffer(...args: any[]): any;
}

declare module "cheerio" {
  /** stdlib */
  export function load(p0: string): any;
}

declare module "child_process" {
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
  export const constants: any;
  /** stdlib */
  export const subtle: any;
  /** stdlib */
  export function createHash(...args: any[]): any;
  /** stdlib */
  export function createHmac(...args: any[]): any;
  /** stdlib */
  export function createSecretKey(...args: any[]): any;
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
  export function randomFillSync(...args: any[]): any;
  /** stdlib */
  export function randomUUID(...args: any[]): any;
  /** stdlib */
  export function sha256(...args: any[]): any;
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
  /** stdlib */
  export function getEventListeners(...args: any[]): any;
  /** stdlib */
  export function getMaxListeners(...args: any[]): any;
  /** stdlib */
  export function listenerCount(...args: any[]): any;
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
  export const constants: any;
  /** stdlib */
  export const promises: any;
  /** stdlib */
  export function accessSync(...args: any[]): any;
  /** stdlib */
  export function appendFile(...args: any[]): any;
  /** stdlib */
  export function appendFileSync(...args: any[]): any;
  /** stdlib */
  export function chmodSync(...args: any[]): any;
  /** stdlib */
  export function copyFileSync(...args: any[]): any;
  /** stdlib */
  export function createReadStream(...args: any[]): any;
  /** stdlib */
  export function createWriteStream(...args: any[]): any;
  /** stdlib */
  export function existsSync(...args: any[]): any;
  /** stdlib */
  export function lstatSync(...args: any[]): any;
  /** stdlib */
  export function mkdir(...args: any[]): any;
  /** stdlib */
  export function mkdirSync(...args: any[]): any;
  /** stdlib */
  export function mkdtempSync(...args: any[]): any;
  /** stdlib */
  export function readFile(...args: any[]): any;
  /** stdlib */
  export function readFileSync(...args: any[]): any;
  /** stdlib */
  export function readdir(...args: any[]): any;
  /** stdlib */
  export function readdirSync(...args: any[]): any;
  /** stdlib */
  export function realpathSync(...args: any[]): any;
  /** stdlib */
  export function renameSync(...args: any[]): any;
  /** stdlib */
  export function rm(...args: any[]): any;
  /** stdlib */
  export function rmSync(...args: any[]): any;
  /** stdlib */
  export function rmdirSync(...args: any[]): any;
  /** stdlib */
  export function stat(...args: any[]): any;
  /** stdlib */
  export function statSync(...args: any[]): any;
  /** stdlib */
  export function unlink(...args: any[]): any;
  /** stdlib */
  export function unlinkSync(...args: any[]): any;
  /** stdlib */
  export function unwatchFile(...args: any[]): any;
  /** stdlib */
  export function watchFile(...args: any[]): any;
  /** stdlib */
  export function writeFile(...args: any[]): any;
  /** stdlib */
  export function writeFileSync(...args: any[]): any;
}

declare module "http" {
  /** stdlib */
  export class ClientRequest { [key: string]: any; }
  /** stdlib */
  export class IncomingMessage { [key: string]: any; }
  /** stdlib */
  export class IncomingMessage { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class ServerResponse { [key: string]: any; }
  /** stdlib */
  export class ServerResponse { [key: string]: any; }
  /** stdlib */
  export const METHODS: any;
  /** stdlib */
  export const STATUS_CODES: any;
  /** stdlib */
  export function createServer(...args: any[]): any;
  /** stdlib */
  export function get(...args: any[]): any;
  /** stdlib */
  export function request(...args: any[]): any;
}

declare module "http2" {
  /** stdlib */
  export class Http2SecureServer { [key: string]: any; }
  /** stdlib */
  export class Http2ServerRequest { [key: string]: any; }
  /** stdlib */
  export class Http2ServerResponse { [key: string]: any; }
  /** stdlib */
  export function createSecureServer(...args: any[]): any;
}

declare module "https" {
  /** stdlib */
  export class ClientRequest { [key: string]: any; }
  /** stdlib */
  export class IncomingMessage { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class Server { [key: string]: any; }
  /** stdlib */
  export class ServerResponse { [key: string]: any; }
  /** stdlib */
  export function createServer(...args: any[]): any;
  /** stdlib */
  export function get(...args: any[]): any;
  /** stdlib */
  export function request(...args: any[]): any;
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
  export function connect(p0: any, p1: any, p2: any): any;
  /** stdlib */
  export function createConnection(p0: any, p1: any, p2: any): any;
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
  export class Headers { [key: string]: any; }
  /** stdlib */
  export class Request { [key: string]: any; }
  /** stdlib */
  export class Response { [key: string]: any; }
  /** stdlib */
  export default function (...args: any[]): any;
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
  export function js_ads_rewarded_load(...args: any[]): any;
  /** stdlib */
  export function js_ads_rewarded_show(...args: any[]): any;
}

declare module "perry/background" {
  /** stdlib */
  export function cancel(...args: any[]): any;
  /** stdlib */
  export function registerTask(...args: any[]): any;
  /** stdlib */
  export function schedule(...args: any[]): any;
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
  export function spawn(p0: any): any;
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
  export function clipboardRead(...args: any[]): any;
  /** stdlib */
  export function clipboardWrite(...args: any[]): any;
  /** stdlib */
  export function embedNSView(...args: any[]): any;
  /** stdlib */
  export function frameSplitAddChild(...args: any[]): any;
  /** stdlib */
  export function frameSplitCreate(...args: any[]): any;
  /** stdlib */
  export function imageGalleryAddImage(...args: any[]): any;
  /** stdlib */
  export function imageGallerySetIndex(...args: any[]): any;
  /** stdlib */
  export function lazyvstackEndRefreshing(...args: any[]): any;
  /** stdlib */
  export function lazyvstackSetRefreshControl(...args: any[]): any;
  /** stdlib */
  export function lazyvstackSetScrollEndCallback(...args: any[]): any;
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

declare module "rate-limiter-flexible" {
  /** stdlib */
  export class RateLimiterAbstract { [key: string]: any; }
  /** stdlib */
  export class RateLimiterMemory { [key: string]: any; }
}

declare module "readline" {
  /** stdlib */
  export function createInterface(p0: any): any;
}

declare module "redis" {
  /** stdlib */
  export class Redis { [key: string]: any; }
  /** stdlib */
  export function createClient(...args: any[]): any;
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

declare module "stream" {
  /** stdlib */
  export class Duplex { [key: string]: any; }
  /** stdlib */
  export class PassThrough { [key: string]: any; }
  /** stdlib */
  export class Readable { [key: string]: any; }
  /** stdlib */
  export class Transform { [key: string]: any; }
  /** stdlib */
  export class Writable { [key: string]: any; }
  /** stdlib */
  export const prototype: any;
  /** stdlib */
  export function finished(...args: any[]): any;
  /** stdlib */
  export function from(...args: any[]): any;
  /** stdlib */
  export function pipeline(...args: any[]): any;
}

declare module "streams" {
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
  /** stdlib */
  export const lastChar: any;
  /** stdlib */
  export const lastNeed: any;
  /** stdlib */
  export const lastTotal: any;
}

declare module "tls" {
  /** stdlib */
  export function connect(p0: string, p1: any, p2: string, p3: any): any;
}

declare module "tty" {
  /** stdlib */
  export class ReadStream { [key: string]: any; }
  /** stdlib */
  export class WriteStream { [key: string]: any; }
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
  export class URLSearchParams { [key: string]: any; }
  /** stdlib */
  export function fileURLToPath(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function parse(...args: any[]): any;
  /** stdlib */
  export function pathToFileURL(...args: any[]): any;
}

declare module "util" {
  /** stdlib */
  export class TextDecoder { [key: string]: any; }
  /** stdlib */
  export class TextEncoder { [key: string]: any; }
  /** stdlib */
  export const types: any;
  /** stdlib */
  export function callbackify(...args: any[]): any;
  /** stdlib */
  export function deprecate(...args: any[]): any;
  /** stdlib */
  export function format(...args: any[]): any;
  /** stdlib */
  export function formatWithOptions(...args: any[]): any;
  /** stdlib */
  export function inherits(...args: any[]): any;
  /** stdlib */
  export function inspect(...args: any[]): any;
  /** stdlib */
  export function isDeepStrictEqual(...args: any[]): any;
  /** stdlib */
  export function promisify(...args: any[]): any;
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
  export class Client { [key: string]: any; }
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
  export const constants: any;
  /** stdlib */
  export function createBrotliDecompress(options?: any): any;
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

