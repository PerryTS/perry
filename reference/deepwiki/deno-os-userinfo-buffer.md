# DeepWiki Response

Repository: `denoland/deno`
Question: How does Deno's Node compatibility implementation of node:os userInfo handle the options.encoding='buffer' case? Please identify the source files and tests showing which userInfo fields are returned as Buffers versus numbers/null, and how invalid encodings are validated.
Search: https://deepwiki.com/search/how-does-denos-node-compatibil_e57c1e37-73ad-4b87-a745-f20b561c30fc

**Question**: How does Deno's Node compatibility implementation of node:os userInfo handle the options.encoding='buffer' case? Please identify the source files and tests showing which userInfo fields are returned as Buffers versus numbers/null, and how invalid encodings are validated.

Deno's Node compatibility implementation of `node:os`'s `userInfo` function handles the `options.encoding='buffer'` case by returning `homedir`, `shell`, and `username` fields as `Buffer` objects, while `uid` and `gid` are returned as numbers. Invalid encodings are not explicitly validated within the `userInfo` function itself, as the `encoding` option is specifically checked for the string `'buffer'`.

### `userInfo` Implementation and Buffer Handling
The `userInfo` function is located in [1](#ref-q1-1) . When `options.encoding` is set to `'buffer'`, the `homedir`, `shell`, and `username` fields are converted to `Buffer` objects using `Buffer.from()` [2](#ref-q1-2) .

The `userInfo` function retrieves user information by calling the Rust operation `op_node_os_user_info` [3](#ref-q1-3) . This operation returns the `username`, `homedir`, and `shell` as strings. The `uid` and `gid` are obtained directly from `Deno.uid()` and `Deno.gid()` respectively [4](#ref-q1-4) . On Windows, `uid` and `gid` are explicitly set to `-1` [5](#ref-q1-5) .

The `op_node_os_user_info` Rust operation is defined in [6](#ref-q1-6) . This Rust function retrieves the `username` and `homedir` from the operating system. The `shell` field is currently returned as `None` (null) on Windows [7](#ref-q1-7) .

### Returned Fields
When `options.encoding='buffer'`, the `userInfo` function returns an object with the following types for its fields:
*   `uid`: `number` [8](#ref-q1-8) 
*   `gid`: `number` [9](#ref-q1-9) 
*   `homedir`: `Buffer` or `null` (if the original `homedir` was null) [10](#ref-q1-10) 
*   `shell`: `Buffer` or `null` (if the original `shell` was null) [11](#ref-q1-11) 
*   `username`: `Buffer` [12](#ref-q1-12) 

### Invalid Encoding Validation
The `userInfo` function explicitly checks if `options?.encoding` is equal to the string `"buffer"` [10](#ref-q1-10) . If it's not `"buffer"`, it defaults to `"utf-8"` [13](#ref-q1-13) . There is no explicit validation for other invalid encoding strings within the `userInfo` function itself. However, the `Buffer.from()` method, which is used for conversion, handles various encodings and would likely throw an error for an unsupported encoding if it were passed directly. The `Buffer` object is imported from [14](#ref-q1-14) .

### Tests
The Node.js compatibility test suite includes `parallel/test-os-userinfo-handles-getter-errors.js` [15](#ref-q1-15) , which is relevant to the `os.userInfo` functionality. While this specific test focuses on error handling for getter errors, it indicates that the `userInfo` function's behavior is covered by the Node.js compatibility tests.

## Notes
The `validateEncoding` function found in [16](#ref-q1-16)  is not directly used by `os.userInfo`. It is used for `fs.readdir` and demonstrates how encoding validation might be handled in other parts of the Node compatibility layer, but it is not part of the `userInfo` implementation.

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)
- [Process, TTY, and OS APIs (denoland/deno)](/wiki/denoland/deno#7.7)

View this search on DeepWiki: https://deepwiki.com/search/how-does-denos-node-compatibil_e57c1e37-73ad-4b87-a745-f20b561c30fc

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/os.ts:233-258`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L233-L258)

```typescript
function userInfo(
  options = { encoding: "utf-8" },
) {
  let uid = Deno.uid();
  let gid = Deno.gid();

  if (isWindows) {
    uid = -1;
    gid = -1;
  }
  let { username, homedir: hd, shell } = op_node_os_user_info(uid);

  if (options?.encoding === "buffer") {
    hd = hd ? Buffer.from(hd) : hd;
    shell = shell ? Buffer.from(shell) : shell;
    username = Buffer.from(username);
  }

  return {
    uid,
    gid,
    homedir: hd,
    shell,
    username,
  };
}
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/os.ts:245-248`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L245-L248)

```typescript
  if (options?.encoding === "buffer") {
    hd = hd ? Buffer.from(hd) : hd;
    shell = shell ? Buffer.from(shell) : shell;
    username = Buffer.from(username);
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/os.ts:243`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L243)

```typescript
  let { username, homedir: hd, shell } = op_node_os_user_info(uid);
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/os.ts:236-237`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L236-L237)

```typescript
  let uid = Deno.uid();
  let gid = Deno.gid();
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/os.ts:239-242`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L239-L242)

```typescript
  if (isWindows) {
    uid = -1;
    gid = -1;
  }
```

<a id="ref-q1-6"></a>
### [6] `ext/node/ops/os/mod.rs:134-207`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/mod.rs#L134-L207)

```rust
#[cfg(windows)]
fn get_user_info(_uid: u32) -> Result<UserInfo, OsError> {
  use std::ffi::OsString;
  use std::os::windows::ffi::OsStringExt;

  use windows_sys::Win32::Foundation::CloseHandle;
  use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
  use windows_sys::Win32::Foundation::GetLastError;
  use windows_sys::Win32::Foundation::HANDLE;
  use windows_sys::Win32::System::Threading::GetCurrentProcess;
  use windows_sys::Win32::System::Threading::OpenProcessToken;
  use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;
  struct Handle(HANDLE);
  impl Drop for Handle {
    fn drop(&mut self) {
      // SAFETY: win32 call
      unsafe {
        CloseHandle(self.0);
      }
    }
  }
  let mut token: MaybeUninit<HANDLE> = MaybeUninit::uninit();

  // Get a handle to the current process
  // SAFETY: win32 call
  unsafe {
    if OpenProcessToken(
      GetCurrentProcess(),
      windows_sys::Win32::Security::TOKEN_READ,
      token.as_mut_ptr(),
    ) == 0
    {
      return Err(
        OsError::FailedToGetUserInfo(std::io::Error::last_os_error()),
      );
    }
  }

  // SAFETY: initialized by call above
  let token = Handle(unsafe { token.assume_init() });

  let mut bufsize = 0;
  // get the size for the homedir buf (it'll end up in `bufsize`)
  // SAFETY: win32 call
  unsafe {
    GetUserProfileDirectoryW(token.0, std::ptr::null_mut(), &mut bufsize);
    let err = GetLastError();
    if err != ERROR_INSUFFICIENT_BUFFER {
      return Err(OsError::FailedToGetUserInfo(
        std::io::Error::from_raw_os_error(err as i32),
      ));
    }
  }
  let mut path = vec![0; bufsize as usize];
  // Actually get the homedir
  // SAFETY: path is `bufsize` elements
  unsafe {
    if GetUserProfileDirectoryW(token.0, path.as_mut_ptr(), &mut bufsize) == 0 {
      return Err(
        OsError::FailedToGetUserInfo(std::io::Error::last_os_error()),
      );
    }
  }
  // remove trailing nul
  path.pop();
  let homedir_wide = OsString::from_wide(&path);
  let homedir = homedir_wide.to_string_lossy().into_owned();

  Ok(UserInfo {
    username: deno_whoami::username(),
    homedir,
    shell: None,
  })
}
```

<a id="ref-q1-7"></a>
### [7] `ext/node/ops/os/mod.rs:205`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/mod.rs#L205)

```rust
    shell: None,
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/os.ts:252`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L252)

```typescript
    uid,
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/os.ts:253`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L253)

```typescript
    gid,
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/os.ts:245`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L245)

```typescript
  if (options?.encoding === "buffer") {
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/os.ts:246`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L246)

```typescript
    hd = hd ? Buffer.from(hd) : hd;
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/os.ts:247`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L247)

```typescript
    shell = shell ? Buffer.from(shell) : shell;
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/os.ts:234`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L234)

```typescript
  options = { encoding: "utf-8" },
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/os.ts:39`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L39)

```typescript
const { Buffer } = core.loadExtScript("ext:deno_node/internal/buffer.mjs");
```

<a id="ref-q1-15"></a>
### [15] `tests/node_compat/config.jsonc:2558`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L2558)

```
    "parallel/test-os-userinfo-handles-getter-errors.js": {},
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/_fs/_fs_readdir.ts:45-52`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_fs/_fs_readdir.ts#L45-L52)

```typescript
function validateEncoding(encoding: string | undefined) {
  if (!encoding || encoding === "buffer") return;
  if (!Buffer.isEncoding(encoding)) {
    throw new Error(
      `TypeError [ERR_INVALID_OPT_VALUE_ENCODING]: The value "${encoding}" is invalid for option "encoding"`,
    );
  }
}
```
