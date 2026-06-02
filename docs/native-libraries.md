# Native Library Implementations

Perry provides native Rust implementations of popular npm packages. When you import these packages in your TypeScript code, they are compiled directly to native code using high-performance Rust crates - no Node.js runtime required.

## Overview

**27 npm packages** are supported with native implementations, organized by category:

### Database & Storage
| npm Package | Rust Backend | Description |
|-------------|--------------|-------------|
| `mysql2` | [sqlx](https://crates.io/crates/sqlx) | MySQL/MariaDB client with connection pooling |
| `pg` | [sqlx](https://crates.io/crates/sqlx) | PostgreSQL client with connection pooling |
| `mongodb` | [mongodb](https://crates.io/crates/mongodb) | MongoDB driver with full CRUD support |
| `better-sqlite3` | [rusqlite](https://crates.io/crates/rusqlite) | Synchronous SQLite3 with prepared statements |
| `ioredis` | [redis](https://crates.io/crates/redis) | Redis client with all common operations |

### Security & Authentication
| npm Package | Rust Backend | Description |
|-------------|--------------|-------------|
| `bcrypt` | [bcrypt](https://crates.io/crates/bcrypt) | Password hashing (bcrypt algorithm) |
| `argon2` | [argon2](https://crates.io/crates/argon2) | Password hashing (Argon2id, more secure) |
| `jsonwebtoken` | [jsonwebtoken](https://crates.io/crates/jsonwebtoken) | JWT signing and verification |
| `crypto` | [sha2](https://crates.io/crates/sha2), [aes](https://crates.io/crates/aes), [pbkdf2](https://crates.io/crates/pbkdf2), [scrypt](https://crates.io/crates/scrypt) | Hashing, encryption, key derivation |

### HTTP & Networking
| npm Package | Rust Backend | Description |
|-------------|--------------|-------------|
| `axios` | [reqwest](https://crates.io/crates/reqwest) | HTTP client with full method support |
| `node-fetch` | [reqwest](https://crates.io/crates/reqwest) | Fetch API implementation |
| `ws` | [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite) | WebSocket client |
| `nodemailer` | [lettre](https://crates.io/crates/lettre) | SMTP email sending |

### Data Processing
| npm Package | Rust Backend | Description |
|-------------|--------------|-------------|
| `cheerio` | [scraper](https://crates.io/crates/scraper) | HTML parsing with jQuery-like API |
| `sharp` | [image](https://crates.io/crates/image) | Image processing (resize, convert, transform) |
| `zlib` | [flate2](https://crates.io/crates/flate2) | Gzip/deflate compression |
| `lodash` | Native Rust | Utility functions for arrays/strings |

### Date & Time
| npm Package | Rust Backend | Description |
|-------------|--------------|-------------|
| `dayjs` | [chrono](https://crates.io/crates/chrono) | Lightweight date manipulation |
| `moment` | [chrono](https://crates.io/crates/chrono) | Full-featured date library |
| `date-fns` | [chrono](https://crates.io/crates/chrono) | Functional date utilities |
| `node-cron` | [cron](https://crates.io/crates/cron) | Cron job scheduling |

### Utilities
| npm Package | Rust Backend | Description |
|-------------|--------------|-------------|
| `uuid` | [uuid](https://crates.io/crates/uuid) | UUID generation (v1, v4, v7) |
| `nanoid` | [nanoid](https://crates.io/crates/nanoid) | Compact unique ID generation |
| `slugify` | Native Rust | URL-friendly string conversion |
| `validator` | [validator](https://crates.io/crates/validator) | String validation (email, URL, etc.) |
| `dotenv` | Native Rust | Environment variable loading |
| `rate-limiter-flexible` | [dashmap](https://crates.io/crates/dashmap) | In-memory rate limiting |

## Quick Start

Using native libraries requires no special setup - just import and use:

```typescript
// Your TypeScript code works exactly like Node.js
import mysql from 'mysql2/promise';
import bcrypt from 'bcrypt';
import { v4 as uuidv4 } from 'uuid';

async function main() {
  // Connect to MySQL
  const conn = await mysql.createConnection({
    host: 'localhost',
    user: 'root',
    password: 'secret',
    database: 'myapp'
  });

  // Hash a password
  const hashedPassword = await bcrypt.hash('user-password', 10);

  // Generate a UUID
  const id = uuidv4();

  // Insert user
  await conn.execute(
    'INSERT INTO users (id, password) VALUES (?, ?)',
    [id, hashedPassword]
  );

  await conn.end();
}

main();
```

Compile and run:
```bash
perry app.ts -o app && ./app
```

The resulting binary has **no Node.js dependency** - it's a standalone native executable.

## Detailed API Reference

Click any library name to jump to its documentation:

| Library | Category | Jump |
|---------|----------|------|
| axios | HTTP Client | [docs](#axios) |
| argon2 | Security | [docs](#argon2) |
| bcrypt | Security | [docs](#bcrypt) |
| better-sqlite3 | Database | [docs](#better-sqlite3) |
| cheerio | HTML Parsing | [docs](#cheerio) |
| crypto | Security | [docs](#crypto) |
| date-fns | Date/Time | [docs](#date-fns) |
| dayjs | Date/Time | [docs](#dayjs) |
| dotenv | Config | [docs](#dotenv) |
| ioredis | Database | [docs](#ioredis) |
| jsonwebtoken | Security | [docs](#jsonwebtoken) |
| lodash | Utilities | [docs](#lodash) |
| moment | Date/Time | [docs](#moment) |
| mongodb | Database | [docs](#mongodb) |
| mysql2 | Database | [docs](#mysql2) |
| nanoid | Utilities | [docs](#nanoid) |
| node-cron | Scheduling | [docs](#node-cron) |
| node-fetch | HTTP Client | [docs](#node-fetch) |
| nodemailer | Email | [docs](#nodemailer) |
| pg | Database | [docs](#pg) |
| rate-limiter-flexible | Rate Limiting | [docs](#rate-limiter-flexible) |
| sharp | Image Processing | [docs](#sharp) |
| slugify | Utilities | [docs](#slugify) |
| uuid | Utilities | [docs](#uuid) |
| validator | Validation | [docs](#validator) |
| ws | WebSocket | [docs](#ws) |
| zlib | Compression | [docs](#zlib) |

---

## uuid

**npm package:** [uuid](https://www.npmjs.com/package/uuid)
**Rust backend:** [uuid](https://crates.io/crates/uuid) v1.11

### Supported API

```typescript
import * as uuid from 'uuid';

// Generate UUIDs
const id1 = uuid.v4();      // Random UUID (most common)
const id2 = uuid.v1();      // Timestamp + MAC-based UUID
const id3 = uuid.v7();      // Unix timestamp-based UUID (sortable)

// Validate and inspect
const isValid = uuid.validate(id1);  // Returns true/false
const version = uuid.version(id1);   // Returns version number (1, 4, 7, etc.)
```

### Notes
- `uuid.v1()` uses a fixed node ID (`01:23:45:67:89:ab`) since MAC address access is not available
- All UUIDs are lowercase, hyphenated format: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`

---

## mysql2

**npm package:** [mysql2](https://www.npmjs.com/package/mysql2)
**Rust backend:** [sqlx](https://crates.io/crates/sqlx) v0.8

### Supported API

```typescript
import mysql from 'mysql2/promise';

// Create connection
const conn = await mysql.createConnection({
  host: 'localhost',
  user: 'root',
  password: 'password',
  database: 'mydb'
});

// Execute queries
const [rows, fields] = await conn.query('SELECT * FROM users');
const [result] = await conn.execute('INSERT INTO users (name) VALUES (?)', ['Alice']);

// Transactions
await conn.beginTransaction();
await conn.query('UPDATE accounts SET balance = balance - 100 WHERE id = 1');
await conn.commit();
// or: await conn.rollback();

// Close connection
await conn.end();
```

### Connection Pooling

```typescript
const pool = await mysql.createPool({
  host: 'localhost',
  user: 'root',
  password: 'password',
  database: 'mydb',
  connectionLimit: 10
});

const [rows] = await pool.query('SELECT * FROM users');
await pool.end();
```

### Notes
- Only Promise-based API is supported (use `mysql2/promise`)
- Prepared statements use `?` placeholders
- Connection options: `host`, `port`, `user`, `password`, `database`

---

## pg

**npm package:** [pg](https://www.npmjs.com/package/pg)
**Rust backend:** [sqlx](https://crates.io/crates/sqlx) v0.8

### Supported API

```typescript
import { Client, Pool } from 'pg';

// Create client connection
const client = new Client({
  host: 'localhost',
  port: 5432,
  user: 'postgres',
  password: 'password',
  database: 'mydb'
});

await client.connect();

// Execute queries
const result = await client.query('SELECT * FROM users');
console.log(result.rows);
console.log(result.rowCount);

// Parameterized queries
const result2 = await client.query(
  'SELECT * FROM users WHERE id = $1',
  [123]
);

// Close connection
await client.end();
```

### Connection Pooling

```typescript
const pool = new Pool({
  host: 'localhost',
  port: 5432,
  user: 'postgres',
  password: 'password',
  database: 'mydb',
  max: 10  // connection limit
});

// Query via pool
const result = await pool.query('SELECT * FROM users');

// Parameterized query
const result2 = await pool.query('SELECT * FROM users WHERE id = $1', [123]);

// Close pool
await pool.end();
```

### Query Result Properties

- `result.rows` - Array of row objects
- `result.rowCount` - Number of rows affected/returned
- `result.fields` - Field metadata array
- `result.command` - SQL command executed (SELECT, INSERT, etc.)

### Notes
- PostgreSQL uses `$1`, `$2`, etc. for parameterized queries (not `?`)
- Connection options: `host`, `port`, `user`, `password`, `database`, `max`
- Uses sqlx under the hood with async/await

---

## bcrypt

**npm package:** [bcrypt](https://www.npmjs.com/package/bcrypt)
**Rust backend:** [bcrypt](https://crates.io/crates/bcrypt) v0.15

### Supported API

```typescript
import bcrypt from 'bcrypt';

// Hash a password
const saltRounds = 10;
const hash = await bcrypt.hash('myPassword', saltRounds);

// Verify a password
const match = await bcrypt.compare('myPassword', hash);
if (match) {
  console.log('Password correct!');
}

// Generate salt separately (optional)
const salt = await bcrypt.genSalt(10);
const hash2 = await bcrypt.hash('myPassword', salt);
```

### Notes
- `saltRounds` (cost factor) typically ranges from 10-12 for production
- Higher values = more secure but slower
- Hashes are 60 characters in the standard bcrypt format

---

## ioredis

**npm package:** [ioredis](https://www.npmjs.com/package/ioredis)
**Rust backend:** [redis](https://crates.io/crates/redis) v0.25

### Supported API

```typescript
import Redis from 'ioredis';

// Connect to Redis
const redis = new Redis();                    // localhost:6379
const redis2 = new Redis(6380);               // localhost:6380
const redis3 = new Redis(6379, '192.168.1.1'); // custom host
const redis4 = new Redis({
  host: 'localhost',
  port: 6379,
  password: 'secret',
  db: 0
});

// String operations
await redis.set('key', 'value');
await redis.set('key', 'value', 'EX', 60);    // with expiry (60 seconds)
const value = await redis.get('key');

// Delete keys
await redis.del('key');
await redis.del('key1', 'key2', 'key3');

// Check existence
const exists = await redis.exists('key');

// Expiration
await redis.expire('key', 60);                // seconds
await redis.ttl('key');                       // get remaining TTL

// Increment/Decrement
await redis.incr('counter');
await redis.incrby('counter', 5);
await redis.decr('counter');
await redis.decrby('counter', 5);

// Hash operations
await redis.hset('user:1', 'name', 'Alice');
await redis.hget('user:1', 'name');
await redis.hgetall('user:1');
await redis.hdel('user:1', 'name');

// List operations
await redis.lpush('list', 'value');
await redis.rpush('list', 'value');
await redis.lpop('list');
await redis.rpop('list');
await redis.lrange('list', 0, -1);

// Set operations
await redis.sadd('set', 'member');
await redis.srem('set', 'member');
await redis.smembers('set');
await redis.sismember('set', 'member');

// Disconnect
await redis.quit();
```

### Notes
- Async/Promise-based API only
- Cluster mode not yet supported
- Pub/Sub not yet supported

---

## crypto

**npm package:** [crypto](https://nodejs.org/api/crypto.html) (Node.js built-in)
**Rust backend:** [sha2](https://crates.io/crates/sha2), [md5](https://crates.io/crates/md-5), [hmac](https://crates.io/crates/hmac), [aes](https://crates.io/crates/aes), [pbkdf2](https://crates.io/crates/pbkdf2), [scrypt](https://crates.io/crates/scrypt), [rand](https://crates.io/crates/rand)

### Supported API

```typescript
import crypto from 'crypto';

// Hash functions
const sha256Hash = crypto.sha256('hello world');        // Returns hex string
const md5Hash = crypto.md5('hello world');              // Returns hex string

// HMAC
const hmac = crypto.hmacSha256('message', 'secret-key'); // Returns hex string

// Random data
const randomHex = crypto.randomBytes(16);               // Returns hex string (32 chars for 16 bytes)
const uuid = crypto.randomUUID();                       // Returns UUID v4 string
```

### AES-256-CBC Encryption

```typescript
// Key must be 32 bytes, IV must be 16 bytes
const key = '01234567890123456789012345678901';  // 32 bytes
const iv = '0123456789012345';                    // 16 bytes

// Encrypt (returns base64 string)
const encrypted = crypto.aes256Encrypt('secret message', key, iv);

// Decrypt (expects base64 input)
const decrypted = crypto.aes256Decrypt(encrypted, key, iv);
```

### Key Derivation Functions

```typescript
// PBKDF2 (Password-Based Key Derivation Function 2)
const key = crypto.pbkdf2('password', 'salt', 100000, 32);
// Returns: 32-byte hex string derived from password
// Parameters: password, salt, iterations, keyLength

// Scrypt (memory-hard KDF)
const key2 = crypto.scrypt('password', 'salt', 32);
// Returns: 32-byte hex string with default params (N=16384, r=8, p=1)

// Scrypt with custom parameters
const key3 = crypto.scryptCustom('password', 'salt', 32, 14, 8, 1);
// Parameters: password, salt, keyLength, logN, r, p
// logN = log2(N), so 14 means N=16384
```

### Notes
- All hash functions return lowercase hex strings
- `randomBytes(n)` returns `2*n` hex characters
- Uses cryptographically secure random number generator
- AES encryption uses PKCS7 padding
- Key derivation functions return hex-encoded output

---

## zlib

**npm package:** [zlib](https://nodejs.org/api/zlib.html) (Node.js built-in)
**Rust backend:** [flate2](https://crates.io/crates/flate2) v1.0

### Supported API

```typescript
import zlib from 'zlib';

// Gzip compression (sync)
const compressed = zlib.gzipSync(data);
const decompressed = zlib.gunzipSync(compressed);

// Deflate compression (sync)
const deflated = zlib.deflateSync(data);
const inflated = zlib.inflateSync(deflated);

// Async versions
const compressedAsync = await zlib.gzip(data);
const decompressedAsync = await zlib.gunzip(compressedAsync);
```

### Notes
- Input/output are treated as byte buffers (strings work for text data)
- Default compression level used
- Async versions run compression in background thread

---

## node-fetch

**npm package:** [node-fetch](https://www.npmjs.com/package/node-fetch)
**Rust backend:** [reqwest](https://crates.io/crates/reqwest) v0.12

### Supported API

```typescript
import fetch from 'node-fetch';

// Simple GET request
const response = await fetch('https://jsonplaceholder.typicode.com/posts/1');
const text = await response.text();
const json = await response.json();

// Check response status
if (response.ok) {
  console.log('Success:', response.status);
}

// Convenience function for text
const text2 = await fetch.fetchText('https://jsonplaceholder.typicode.com/posts/1');
```

### Response Properties

- `response.status` - HTTP status code (number)
- `response.statusText` - HTTP status text (string)
- `response.ok` - true if status is 200-299 (boolean)
- `response.text()` - Get body as text (Promise<string>)
- `response.json()` - Get body as JSON (Promise<object>)

### Notes
- HTTPS supported via rustls (no OpenSSL dependency)
- Currently supports GET requests; POST coming soon
- Redirects are followed automatically

---

## ws

**npm package:** [ws](https://www.npmjs.com/package/ws)
**Rust backend:** [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite) v0.24

### Supported API

```typescript
import WebSocket from 'ws';

// Connect to WebSocket server
const ws = await WebSocket.connect('wss://echo.websocket.org');

// Check connection status
if (ws.isOpen()) {
  // Send a message
  ws.send('Hello, server!');
}

// Receive messages
const message = ws.receive();          // Get next message (null if none)
const count = ws.messageCount();       // Number of pending messages

// Wait for message with timeout
const msg = await ws.waitForMessage(5000);  // 5 second timeout, null if timeout

// Close connection
ws.close();
```

### Notes
- WebSocket Secure (wss://) supported via rustls
- Messages are queued and can be polled with `receive()`
- Text messages only (binary messages not yet supported)
- Connection events (onopen, onclose, onerror) not yet supported - use polling pattern

---

## dotenv

**npm package:** [dotenv](https://www.npmjs.com/package/dotenv)
**Rust backend:** Native implementation

### Supported API

```typescript
import dotenv from 'dotenv';

// Load .env file (from current directory)
dotenv.config();

// Load from custom path
dotenv.config({ path: '.env.local' });

// Parse env content string
const parsed = dotenv.parse('KEY=value\nFOO=bar');
// Returns: { KEY: 'value', FOO: 'bar' }
```

### Notes
- Supports comments (lines starting with `#`)
- Supports quoted values (single and double quotes)
- Supports escape sequences (`\n`, `\t`) in double-quoted values
- Missing .env file is not an error (silent fail)

---

## jsonwebtoken

**npm package:** [jsonwebtoken](https://www.npmjs.com/package/jsonwebtoken)
**Rust backend:** [jsonwebtoken](https://crates.io/crates/jsonwebtoken) v9.3

### Supported API

```typescript
import jwt from 'jsonwebtoken';

const secret = 'your-secret-key';

// Sign a token
const token = jwt.sign({ userId: 123 }, secret, 3600);  // 3600 seconds = 1 hour

// Verify a token (returns payload or null if invalid)
const payload = jwt.verify(token, secret);
if (payload) {
  console.log('Valid token:', payload);
}

// Decode without verification (unsafe, for debugging)
const decoded = jwt.decode(token);
```

### Notes
- Uses HS256 algorithm
- `sign()` third parameter is expiration time in seconds (0 = no expiration)
- `verify()` returns null for invalid/expired tokens
- Payload is JSON stringified/parsed automatically

---

## nanoid

**npm package:** [nanoid](https://www.npmjs.com/package/nanoid)
**Rust backend:** [nanoid](https://crates.io/crates/nanoid) v0.4

### Supported API

```typescript
import { nanoid, customAlphabet } from 'nanoid';

// Generate default ID (21 characters, URL-safe)
const id = nanoid();

// Generate ID with custom length
const shortId = nanoid(10);

// Generate with custom alphabet
const customId = customAlphabet('0123456789abcdef', 12);  // 12 hex chars
```

### Notes
- Default alphabet is URL-safe: `A-Za-z0-9_-`
- Default length is 21 characters
- IDs are cryptographically random

---

## slugify

**npm package:** [slugify](https://www.npmjs.com/package/slugify)
**Rust backend:** Native implementation

### Supported API

```typescript
import slugify from 'slugify';

// Basic usage
slugify('Hello World');           // 'hello-world'
slugify('Héllo Wörld');           // 'hello-world' (accents converted)
slugify('Hello  World!');         // 'hello-world' (special chars removed)

// Strict mode (only alphanumeric)
slugify.strict('Hello_World');    // 'hello-world'
```

### Notes
- Converts to lowercase
- Replaces spaces and common separators (`_`, `/`, `\`) with hyphens
- Converts common accented characters to ASCII equivalents
- Removes non-alphanumeric characters (except separator)
- Trims leading/trailing separators

---

## validator

**npm package:** [validator](https://www.npmjs.com/package/validator)
**Rust backend:** [validator](https://crates.io/crates/validator) v0.18

### Supported API

```typescript
import validator from 'validator';

// Email validation
validator.isEmail('user@example.com');      // true
validator.isEmail('invalid');               // false

// URL validation
validator.isURL('https://example.com');     // true

// UUID validation
validator.isUUID('550e8400-e29b-41d4-a716-446655440000');  // true

// Character type checks
validator.isAlpha('Hello');                 // true
validator.isAlphanumeric('Hello123');       // true
validator.isNumeric('12345');               // true
validator.isNumeric('-123');                // true (allows leading +/-)
validator.isHexadecimal('0xFF');            // true
validator.isHexadecimal('abc123');          // true

// Number validation
validator.isInt('42');                      // true
validator.isFloat('3.14');                  // true

// String checks
validator.isEmpty('   ');                   // true (after trim)
validator.isJSON('{"key": "value"}');       // true
validator.isLowercase('hello');             // true
validator.isUppercase('HELLO');             // true

// String comparison
validator.contains('hello world', 'world'); // true
validator.equals('hello', 'hello');         // true

// Length validation
validator.isLength('hello', 1, 10);         // true (min 1, max 10)
```

### Notes
- All functions return boolean (true/false)
- Email validation uses the validator crate's implementation
- URL validation requires protocol (http://, https://)

---

## nodemailer

**npm package:** [nodemailer](https://www.npmjs.com/package/nodemailer)
**Rust backend:** [lettre](https://crates.io/crates/lettre) v0.11

### Supported API

```typescript
import nodemailer from 'nodemailer';

// Create transporter with SMTP config
const transporter = nodemailer.createTransport({
  host: 'smtp.example.com',
  port: 587,
  secure: false,  // true for 465, false for other ports
  auth: {
    user: 'username',
    pass: 'password'
  }
});

// Send email
const info = await transporter.sendMail({
  from: 'sender@example.com',
  to: 'recipient@example.com',
  subject: 'Hello',
  text: 'Plain text body',
  html: '<p>HTML body</p>'  // optional, overrides text
});

console.log('Message sent:', info.messageId);

// Verify connection
const isValid = await transporter.verify();
```

### Configuration Options

- `host` - SMTP server hostname
- `port` - SMTP port (default: 587)
- `secure` - Use TLS (true for port 465)
- `auth.user` - SMTP username
- `auth.pass` - SMTP password

### Mail Options

- `from` - Sender email address
- `to` - Recipient email address
- `subject` - Email subject line
- `text` - Plain text body
- `html` - HTML body (optional)

### Notes
- Uses STARTTLS for non-secure connections
- Returns `{ messageId, response }` on success
- `verify()` tests SMTP connection without sending

---

## dayjs

**npm package:** [dayjs](https://www.npmjs.com/package/dayjs)
**Rust backend:** [chrono](https://crates.io/crates/chrono) v0.4

### Supported API

```typescript
import dayjs from 'dayjs';

// Create instances
const now = dayjs();                        // Current time
const fromTimestamp = dayjs(1609459200000); // From Unix timestamp (ms)
const parsed = dayjs('2021-01-01');         // Parse ISO string

// Format dates
const formatted = now.format('YYYY-MM-DD HH:mm:ss');
const iso = now.toISOString();              // ISO 8601 format

// Get values
now.valueOf();      // Unix timestamp (ms)
now.unix();         // Unix timestamp (seconds)
now.year();         // Year (e.g., 2024)
now.month();        // Month (0-11)
now.date();         // Day of month (1-31)
now.day();          // Day of week (0=Sunday)
now.hour();         // Hour (0-23)
now.minute();       // Minute (0-59)
now.second();       // Second (0-59)
now.millisecond();  // Millisecond (0-999)

// Manipulate dates
const tomorrow = now.add(1, 'day');
const lastWeek = now.subtract(7, 'day');
const startOfMonth = now.startOf('month');
const endOfYear = now.endOf('year');

// Compare dates
const diff = now.diff(parsed, 'day');       // Difference in days
now.isBefore(parsed);                       // Returns boolean
now.isAfter(parsed);                        // Returns boolean
now.isSame(parsed, 'day');                  // Same day?
now.isValid();                              // Is valid date?
```

### Supported Units

For `add()`, `subtract()`, `startOf()`, `endOf()`, `diff()`, `isSame()`:
- `year`, `years`, `y`
- `month`, `months`, `M`
- `day`, `days`, `d`
- `hour`, `hours`, `h`
- `minute`, `minutes`, `m`
- `second`, `seconds`, `s`
- `millisecond`, `milliseconds`, `ms`

### Notes
- All dates are UTC internally
- Immutable API (methods return new instances)
- Format tokens: `YYYY`, `MM`, `DD`, `HH`, `mm`, `ss`, etc.

---

## date-fns

**npm package:** [date-fns](https://www.npmjs.com/package/date-fns)
**Rust backend:** [chrono](https://crates.io/crates/chrono) v0.4

### Supported API

```typescript
import {
  format,
  parseISO,
  addDays,
  addMonths,
  addYears,
  differenceInDays,
  differenceInHours,
  differenceInMinutes,
  isAfter,
  isBefore,
  startOfDay,
  endOfDay
} from 'date-fns';

// Format a date
const formatted = format(new Date(), 'yyyy-MM-dd HH:mm:ss');

// Parse ISO string
const date = parseISO('2021-01-01T00:00:00Z');

// Add time
const tomorrow = addDays(new Date(), 1);
const nextMonth = addMonths(new Date(), 1);
const nextYear = addYears(new Date(), 1);

// Calculate differences
const daysDiff = differenceInDays(date1, date2);
const hoursDiff = differenceInHours(date1, date2);
const minutesDiff = differenceInMinutes(date1, date2);

// Compare dates
const after = isAfter(date1, date2);
const before = isBefore(date1, date2);

// Day boundaries
const dayStart = startOfDay(new Date());
const dayEnd = endOfDay(new Date());
```

### Notes
- Uses Unix timestamps (milliseconds) internally
- Format tokens differ from dayjs: `yyyy` (not `YYYY`), etc.
- Functions are pure (no mutation)
- All times are UTC

---

## axios

**npm package:** [axios](https://www.npmjs.com/package/axios)
**Rust backend:** [reqwest](https://crates.io/crates/reqwest) v0.12

### Supported API

```typescript
import axios from 'axios';

// Simple requests
const response = await axios.get('https://jsonplaceholder.typicode.com/posts/1');
const postResponse = await axios.post('https://jsonplaceholder.typicode.com/posts', { title: 'hello' });
const putResponse = await axios.put('https://jsonplaceholder.typicode.com/posts/1', { title: 'updated' });
const deleteResponse = await axios.delete('https://jsonplaceholder.typicode.com/posts/1');

// Full request with config
const response2 = await axios.request({
  method: 'POST',
  url: 'https://jsonplaceholder.typicode.com/posts',
  headers: { 'Content-Type': 'application/json' },
  data: { title: 'hello' }
});

// Create instance with defaults
const api = axios.create({
  baseURL: 'https://jsonplaceholder.typicode.com',
  timeout: 5000,
  headers: { 'Authorization': 'Bearer token' }
});
```

### Response Properties

- `response.status` - HTTP status code
- `response.statusText` - HTTP status text
- `response.data` - Response body (JSON parsed)
- `response.headers` - Response headers

### Notes
- HTTPS supported via rustls
- JSON bodies automatically serialized/parsed
- Timeouts supported via config

---

## argon2

**npm package:** [argon2](https://www.npmjs.com/package/argon2)
**Rust backend:** [argon2](https://crates.io/crates/argon2) v0.5

### Supported API

```typescript
import argon2 from 'argon2';

// Hash a password (with default options)
const hash = await argon2.hash('myPassword');

// Hash with custom options
const hash2 = await argon2.hash('myPassword', {
  type: 2,           // 0=Argon2d, 1=Argon2i, 2=Argon2id (default)
  memoryCost: 65536, // Memory in KB (default: 65536 = 64MB)
  timeCost: 3,       // Iterations (default: 3)
  parallelism: 4     // Threads (default: 4)
});

// Verify a password
const isValid = await argon2.verify(hash, 'myPassword');
if (isValid) {
  console.log('Password correct!');
}
```

### Notes
- Argon2id is recommended (type: 2) and used by default
- Memory cost is in KB, not bytes
- Hash format is PHC string format (contains all parameters)
- More secure than bcrypt for new applications

---

## mongodb

**npm package:** [mongodb](https://www.npmjs.com/package/mongodb)
**Rust backend:** [mongodb](https://crates.io/crates/mongodb) v2.8

### Supported API

```typescript
import { MongoClient } from 'mongodb';

// Connect to MongoDB
const client = await MongoClient.connect('mongodb://localhost:27017');

// Get database and collection
const db = client.db('mydb');
const users = db.collection('users');

// Insert documents
const result = await users.insertOne({ name: 'Alice', age: 30 });
console.log('Inserted:', result.insertedId);

const bulkResult = await users.insertMany([
  { name: 'Bob', age: 25 },
  { name: 'Charlie', age: 35 }
]);

// Find documents
const user = await users.findOne({ name: 'Alice' });
const allUsers = await users.find({ age: { $gte: 25 } });

// Update documents
const updateResult = await users.updateOne(
  { name: 'Alice' },
  { $set: { age: 31 } }
);

await users.updateMany(
  { age: { $lt: 30 } },
  { $inc: { age: 1 } }
);

// Delete documents
await users.deleteOne({ name: 'Bob' });
await users.deleteMany({ age: { $gt: 50 } });

// Count documents
const count = await users.countDocuments({ age: { $gte: 25 } });

// Close connection
await client.close();
```

### Notes
- Filters and updates are passed as JSON strings internally
- Supports standard MongoDB query operators ($eq, $gt, $lt, $gte, $lte, $in, etc.)
- Connection pooling is automatic
- All operations are async/Promise-based

---

## better-sqlite3

**npm package:** [better-sqlite3](https://www.npmjs.com/package/better-sqlite3)
**Rust backend:** [rusqlite](https://crates.io/crates/rusqlite) v0.31

### Supported API

```typescript
import Database from 'better-sqlite3';

// Open database (creates if not exists)
const db = Database.open('mydb.sqlite');

// Execute raw SQL
db.exec('CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT)');

// Prepare statements
const insert = db.prepare('INSERT INTO users (name) VALUES (?)');
const result = insert.run(['Alice']);  // Returns { changes, lastInsertRowid }

// Query single row
const select = db.prepare('SELECT * FROM users WHERE id = ?');
const user = select.get([1]);  // Returns object or null

// Query all rows
const selectAll = db.prepare('SELECT * FROM users');
const users = selectAll.all([]);  // Returns array of objects

// Transactions
const tx = db.transaction();
try {
  insert.run(['Bob']);
  insert.run(['Charlie']);
  tx.commit();
} catch (e) {
  tx.rollback();
}

// Close database
db.close();
```

### Notes
- Synchronous API (no await needed for queries)
- Parameters passed as JSON array
- Results returned as JSON objects
- SQLite bundled (no external dependency)

---

## sharp

**npm package:** [sharp](https://www.npmjs.com/package/sharp)
**Rust backend:** [image](https://crates.io/crates/image) v0.25

### Supported API

```typescript
import sharp from 'sharp';

// Load image from file
const image = sharp.fromFile('input.jpg');

// Load from buffer
const buffer = /* ... */;
const image2 = sharp.fromBuffer(buffer, buffer.length);

// Resize
const resized = image.resize(800, 600);  // width, height

// Transformations (chainable)
const processed = image
  .resize(800, 600)
  .rotate(90)
  .blur(2.5)
  .grayscale()
  .flip()      // vertical flip
  .flop()      // horizontal flip
  .negate();

// Set output format
const jpeg = image.toFormat('jpeg');
const png = image.toFormat('png');
const webp = image.toFormat('webp');

// Set quality (1-100)
const highQuality = image.quality(90);

// Save to file
await image.toFile('output.jpg');

// Get buffer
const outputBuffer = await image.toBuffer();

// Get metadata
const meta = image.metadata();
// Returns: { width, height, format }
```

### Notes
- Chainable API (all methods return new image handle)
- Supported formats: JPEG, PNG, WebP, GIF
- Blur sigma: 0.3-1000 (higher = more blur)
- Quality only affects JPEG and WebP

---

## cheerio

**npm package:** [cheerio](https://www.npmjs.com/package/cheerio)
**Rust backend:** [scraper](https://crates.io/crates/scraper) v0.19

### Supported API

```typescript
import cheerio from 'cheerio';

// Load HTML
const $ = cheerio.load('<html><body><h1>Hello</h1><p class="intro">World</p></body></html>');

// Load fragment (no <html>/<body> wrapper)
const $frag = cheerio.loadFragment('<div><p>Hello</p></div>');

// Select elements
const selection = $.select('p.intro');

// Get text content
const text = selection.text();           // 'World'

// Get HTML content
const html = selection.html();           // Inner HTML

// Get attributes
const className = selection.attr('class'); // 'intro'

// Get length
const count = selection.length;          // 1

// Navigation
const first = selection.first();
const last = selection.last();
const nth = selection.eq(0);
const parent = selection.parent();
const children = selection.children();
const found = selection.find('span');

// Predicates
const hasClass = selection.hasClass('intro');  // true
const matches = selection.is('.intro');        // true

// Iteration
const textArray = selection.texts();     // Array of text contents
const htmlArray = selection.toArray();   // Array of HTML strings
const attrs = selection.attrs('href');   // Array of attribute values
```

### Notes
- CSS selectors supported (class, id, tag, attribute, combinators)
- Immutable API (methods return new selections)
- No DOM manipulation (read-only parsing)

---

## lodash

**npm package:** [lodash](https://www.npmjs.com/package/lodash)
**Rust backend:** Native Rust implementation

### Supported API

```typescript
import _ from 'lodash';

// Array functions
_.chunk([1, 2, 3, 4], 2);           // [[1, 2], [3, 4]]
_.compact([0, 1, false, 2, '', 3]); // [1, 2, 3]
_.concat([1], [2], [3]);            // [1, 2, 3]
_.difference([1, 2, 3], [2, 3]);    // [1]
_.drop([1, 2, 3], 2);               // [3]
_.dropRight([1, 2, 3], 2);          // [1]
_.first([1, 2, 3]);                 // 1 (alias: head)
_.last([1, 2, 3]);                  // 3
_.flatten([[1], [2, [3]]]);         // [1, 2, [3]]
_.initial([1, 2, 3]);               // [1, 2]
_.tail([1, 2, 3]);                  // [2, 3]
_.take([1, 2, 3], 2);               // [1, 2]
_.takeRight([1, 2, 3], 2);          // [2, 3]
_.uniq([1, 2, 1, 3, 2]);            // [1, 2, 3]
_.reverse([1, 2, 3]);               // [3, 2, 1]
_.size([1, 2, 3]);                  // 3

// String functions
_.camelCase('foo bar');             // 'fooBar'
_.capitalize('hello');              // 'Hello'
_.kebabCase('foo bar');             // 'foo-bar'
_.lowerCase('FOO BAR');             // 'foo bar'
_.snakeCase('foo bar');             // 'foo_bar'
_.startCase('foo bar');             // 'Foo Bar'
_.upperCase('foo bar');             // 'FOO BAR'
_.upperFirst('hello');              // 'Hello'
_.lowerFirst('Hello');              // 'hello'
_.trim('  hello  ');                // 'hello'
_.trimStart('  hello');             // 'hello'
_.trimEnd('hello  ');               // 'hello'
_.pad('hi', 6);                     // '  hi  '
_.padStart('hi', 6);                // '    hi'
_.padEnd('hi', 6);                  // 'hi    '
_.repeat('ab', 3);                  // 'ababab'
_.truncate('hello world', 8);       // 'hello...'
_.startsWith('hello', 'he');        // true
_.endsWith('hello', 'lo');          // true
_.includes('hello', 'ell');         // true
_.split('a,b,c', ',');              // ['a', 'b', 'c']
_.replace('hello', 'l', 'L');       // 'heLlo'
_.escape('<div>');                  // '&lt;div&gt;'
_.unescape('&lt;div&gt;');          // '<div>'

// Number functions
_.clamp(5, 0, 3);                   // 3
_.inRange(3, 2, 4);                 // true
_.random(0, 10);                    // Random number 0-10
```

### Notes
- Subset of full lodash API implemented
- Functions work on primitive arrays
- All functions are pure (no mutation)

---

## moment

**npm package:** [moment](https://www.npmjs.com/package/moment)
**Rust backend:** [chrono](https://crates.io/crates/chrono) v0.4

### Supported API

```typescript
import moment from 'moment';

// Create instances
const now = moment();                       // Current time
const fromTimestamp = moment.fromTimestamp(1609459200000);
const parsed = moment.parse('2021-01-01');  // Parse ISO string

// Format dates
const formatted = now.format('YYYY-MM-DD HH:mm:ss');

// Get values
now.valueOf();       // Unix timestamp (ms)
now.unix();          // Unix timestamp (seconds)
now.year();          // Year
now.month();         // Month (0-11)
now.date();          // Day of month
now.day();           // Day of week (0=Sunday)
now.hour();          // Hour
now.minute();        // Minute
now.second();        // Second
now.millisecond();   // Millisecond
now.isValid();       // Is valid date?

// Manipulate dates
const tomorrow = now.add(1, 'day');
const lastWeek = now.subtract(7, 'day');
const startOfMonth = now.startOf('month');
const endOfYear = now.endOf('year');

// Compare dates
const diff = now.diff(parsed, 'day');  // Difference in days
```

### Notes
- Similar API to dayjs (moment-compatible)
- Immutable operations (returns new instances)
- All times are UTC internally
- Consider using dayjs for new projects (smaller footprint)

---

## node-cron

**npm package:** [node-cron](https://www.npmjs.com/package/node-cron)
**Rust backend:** [cron](https://crates.io/crates/cron) v0.12

### Supported API

```typescript
import cron from 'node-cron';

// Validate cron expression
const isValid = cron.validate('* * * * *');  // true

// Schedule a job
const job = cron.schedule('*/5 * * * *', () => {
  console.log('Running every 5 minutes');
});

// Control the job
job.start();           // Start the job
job.stop();            // Stop the job
const running = job.isRunning();  // Check if running

// Get next execution times
const next = job.nextDate();      // ISO string of next run
const nextFive = job.nextDates(5); // Array of next 5 run times

// Get human-readable description
const desc = cron.describe('0 0 * * *');
// "At second 0 minute 0 of hour *, on day * of month *, on weekday *"

// Timer helpers (setTimeout/setInterval alternative)
const interval = cron.setInterval(() => {
  console.log('Every second');
}, 1000);
cron.clearInterval(interval);

const timeout = cron.setTimeout(() => {
  console.log('After 5 seconds');
}, 5000);
cron.clearTimeout(timeout);
```

### Cron Expression Format

```
┌────────────── second (0-59) [optional]
│ ┌──────────── minute (0-59)
│ │ ┌────────── hour (0-23)
│ │ │ ┌──────── day of month (1-31)
│ │ │ │ ┌────── month (1-12)
│ │ │ │ │ ┌──── day of week (0-6, Sunday=0)
│ │ │ │ │ │
* * * * * *
```

### Notes
- Both 5-field and 6-field (with seconds) formats supported
- Callback ID is used internally (actual callbacks not yet implemented)
- Job timing is based on UTC

---

## rate-limiter-flexible

**npm package:** [rate-limiter-flexible](https://www.npmjs.com/package/rate-limiter-flexible)
**Rust backend:** [governor](https://crates.io/crates/governor) v0.6

### Supported API

```typescript
import { RateLimiterMemory } from 'rate-limiter-flexible';

// Create rate limiter
const limiter = RateLimiterMemory.create({
  points: 10,        // Number of points
  duration: 1        // Per second
});

// Consume points
try {
  const result = await limiter.consume('user_123', 1);
  console.log('Remaining points:', result.remainingPoints);
} catch (rateLimiterRes) {
  console.log('Rate limited! Retry after:', rateLimiterRes.msBeforeNext);
}

// Get current state
const info = await limiter.get('user_123');

// Delete key
await limiter.delete('user_123');

// Block a key for duration
await limiter.block('user_123', 60);  // Block for 60 seconds

// Add penalty points
await limiter.penalty('user_123', 2);  // Consume 2 extra points

// Reward points (restore consumed points)
await limiter.reward('user_123', 1);   // Give back 1 point
```

### Result Properties

- `remainingPoints` - Points remaining in current window
- `msBeforeNext` - Milliseconds until points reset
- `consumedPoints` - Points consumed so far
- `isFirstInDuration` - Whether this is first request in window

### Notes
- Memory-based storage (resets on restart)
- Uses token bucket algorithm
- Thread-safe for concurrent requests
- Consider Redis-backed limiter for distributed systems

---

## Implementation Notes

### How It Works

When you import a supported npm package, perry:

1. **Detects the import** - Recognizes `mysql2`, `bcrypt`, etc. as native modules
2. **Emits FFI calls** - Generates calls to `js_mysql2_*`, `js_bcrypt_*` functions
3. **Links native code** - The `libperry_stdlib.a` library provides implementations
4. **Runs natively** - No JavaScript runtime, no Node.js, pure native code

### Architecture

```
TypeScript                    Perry                     Native Binary
─────────────────────────────────────────────────────────────────────────
import mysql from 'mysql2'  → HIR marks as native import  → Links to stdlib
await mysql.createConn()    → Emits js_mysql2_connect()   → Calls sqlx
```

### Data Passing

Data crosses the FFI boundary using:
- **JSValue** - NaN-boxed 64-bit values for primitives
- **StringHeader** - Length-prefixed UTF-8 strings
- **ArrayHeader** - Length-prefixed arrays of JSValues
- **ObjectHeader** - Field count + JSValue pairs
- **Handle** - Opaque i64 pointers for complex objects (connections, etc.)

### Async Support

Async operations use a tokio runtime bridge:
1. TypeScript `await` creates a Promise handle
2. Rust spawns work on the tokio runtime
3. Completion resolves/rejects the Promise
4. Control returns to compiled code

---

## Adding New Libraries

To add support for a new npm package:

### 1. Add Rust Dependencies

```toml
# crates/perry-stdlib/Cargo.toml
[dependencies]
your-crate = "1.0"
```

### 2. Implement FFI Module

```rust
// crates/perry-stdlib/src/your_library.rs

use perry_runtime::{JSValue, StringHeader, js_string_from_bytes};
use crate::common::{get_handle, register_handle, Handle};

/// your_library.method()
#[no_mangle]
pub unsafe extern "C" fn js_your_library_method(
    arg: *const StringHeader
) -> JSValue {
    // Implementation using your Rust crate
}
```

### 3. Export in lib.rs

```rust
// crates/perry-stdlib/src/lib.rs
pub mod your_library;
```

### 4. Add Codegen Support

```rust
// crates/perry-codegen/src/codegen.rs

// In declare_*_functions():
{
    let mut sig = self.module.make_signature();
    sig.params.push(AbiParam::new(types::I64));
    sig.returns.push(AbiParam::new(types::I64));
    let func_id = self.module.declare_function(
        "js_your_library_method",
        Linkage::Import,
        &sig
    )?;
    self.extern_funcs.insert("js_your_library_method".to_string(), func_id);
}

// In NativeMethodCall handling:
("your_library", false, "method") => "js_your_library_method",
```

### Guidelines

- **Match the npm API** - Users expect familiar interfaces
- **Use proven Rust crates** - Don't reinvent wheels
- **Handle errors gracefully** - Return null/undefined, don't panic
- **Document the API** - Add a section to this file

Contributions welcome! See the [contributing guide](../CONTRIBUTING.md) for details.

---

# Perry Native Framework

Beyond npm package compatibility, perry includes a native HTTP/WebSocket server framework optimized for maximum performance.

## HTTP Server (Low-Level API)

**Rust backend:** [hyper](https://crates.io/crates/hyper) + tokio

### Basic Example

```typescript
// Type declarations (will be auto-generated in future)
declare function js_http_server_create(port: number): number;
declare function js_http_server_accept_v2(server: number): number;
declare function js_http_request_method(req: number): string;
declare function js_http_request_path(req: number): string;
declare function js_http_request_body(req: number): string;
declare function js_http_respond_text(req: number, status: number, body: string): boolean;
declare function js_http_respond_json(req: number, status: number, body: string): boolean;
declare function js_http_respond_not_found(req: number): boolean;

// Create server
const server = js_http_server_create(3000);
console.log('Server started on http://localhost:3000');

// Request loop
while (true) {
  const req = js_http_server_accept_v2(server);
  const method = js_http_request_method(req);
  const path = js_http_request_path(req);

  if (path === '/' && method === 'GET') {
    js_http_respond_text(req, 200, 'Hello World');
  } else if (path === '/api/data' && method === 'GET') {
    js_http_respond_json(req, 200, '{"status":"ok"}');
  } else {
    js_http_respond_not_found(req);
  }
}
```

### FFI Functions

**Server:**
| Function | Description |
|----------|-------------|
| `js_http_server_create(port)` | Create server on port, returns handle |
| `js_http_server_accept_v2(server)` | Block until next request, returns request handle |
| `js_http_server_close(server)` | Close the server |

**Request:**
| Function | Description |
|----------|-------------|
| `js_http_request_method(req)` | Get HTTP method (GET, POST, etc.) |
| `js_http_request_path(req)` | Get URL path |
| `js_http_request_query(req)` | Get query string |
| `js_http_request_body(req)` | Get request body as string |
| `js_http_request_header(req, name)` | Get specific header |
| `js_http_request_query_param(req, name)` | Get specific query parameter |

**Response:**
| Function | Description |
|----------|-------------|
| `js_http_respond_text(req, status, body)` | Send text/plain response |
| `js_http_respond_json(req, status, body)` | Send application/json response |
| `js_http_respond_html(req, status, body)` | Send text/html response |
| `js_http_respond_redirect(req, url, permanent)` | Send 301/302 redirect |
| `js_http_respond_not_found(req)` | Send 404 response |
| `js_http_respond_error(req, status, message)` | Send error response |

## JSON Handling

**Rust backend:** [serde_json](https://crates.io/crates/serde_json)

| Function | Description |
|----------|-------------|
| `js_json_parse(text)` | Parse JSON string to JSValue |
| `js_json_stringify_string(str)` | Convert string to JSON string |
| `js_json_stringify_number(num)` | Convert number to JSON string |
| `js_json_stringify_bool(bool)` | Convert boolean to JSON string |
| `js_json_is_valid(text)` | Check if string is valid JSON |
| `js_json_get_string(json, key)` | Get string value from JSON object |
| `js_json_get_number(json, key)` | Get number value from JSON object |

## Performance Targets

| Metric | Target | vs Node.js |
|--------|--------|------------|
| Requests/sec | 500k+ | ~15x faster |
| Latency p99 | <1ms | ~5-10x faster |
| Memory | 10MB | ~5-10x less |
| Cold start | <10ms | ~10-50x faster |
| Binary size | <5MB | No runtime needed |

## Future: High-Level Router API

The target high-level API (coming soon):

```typescript
import { serve, router, cors, logger } from 'perry/http';

const app = router()
  .use(logger())
  .use(cors({ origin: '*' }))
  .get('/', (c) => c.text('Hello World'))
  .get('/api/users/:id', async (c) => {
    const id = c.param('id');
    return c.json({ id, name: 'Alice' });
  })
  .post('/api/users', async (c) => {
    const body = await c.json();
    return c.json({ created: true }, 201);
  });

serve(app, { port: 3000 });
```

See [PerryTS/perry-examples](https://github.com/PerryTS/perry-examples) for working examples.
