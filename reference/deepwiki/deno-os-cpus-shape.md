# DeepWiki Response

Repository: `denoland/deno`
Question: In Deno's node:os compatibility implementation, how is os.cpus() implemented and what object shape does it return? Which Rust or JS files show model, speed, and times fields, and what fallback behavior is used when host CPU details are unavailable?
Search: https://deepwiki.com/search/in-denos-nodeos-compatibility_e2f09965-eb50-45b4-8912-9b0850eb675f

**Question**: In Deno's node:os compatibility implementation, how is os.cpus() implemented and what object shape does it return? Which Rust or JS files show model, speed, and times fields, and what fallback behavior is used when host CPU details are unavailable?

In Deno's `node:os` compatibility layer, the `os.cpus()` function is implemented by calling the Rust `op_cpus` operation, which gathers CPU information from the operating system [1](#ref-q1-1)  [2](#ref-q1-2) . This function returns an array of `CpuInfo` objects, each containing `model`, `speed`, and `times` fields [3](#ref-q1-3) .

The `model`, `speed`, and `times` fields are defined in the Rust struct `CpuInfo` in `ext/node/ops/os/cpus.rs` [4](#ref-q1-4) . The `CpuTimes` struct, also in `ext/node/ops/os/cpus.rs`, defines the `user`, `nice`, `sys`, `idle`, and `irq` fields for the `times` object [5](#ref-q1-5) .

### Implementation of `os.cpus()`

The JavaScript `os.cpus()` function in `ext/node/polyfills/os.ts` directly calls the `op_cpus` Rust function [1](#ref-q1-1) .

The `op_cpus` Rust function, located in `ext/node/ops/os/mod.rs`, first performs a permission check for "cpus" [6](#ref-q1-6) . It then calls the `cpu_info()` function from the `cpus` module and returns its result, defaulting to an empty vector if `cpu_info()` returns `None` [7](#ref-q1-7) .

The `cpu_info()` function in `ext/node/ops/os/cpus.rs` has platform-specific implementations:
*   **macOS**: It uses `sysctlbyname` to get the CPU model (`machdep.cpu.brand_string` or `hw.model`) and CPU frequency (`hw.cpufrequency`). It also uses `host_processor_info` to get CPU load information. If `hw.cpufrequency` is unavailable (e.g., on `darwin/arm64`), it hardcodes a plausible speed of 2.4 GHz [8](#ref-q1-8) .
*   **Windows**: It uses `GetSystemInfo` to get the number of processors and `NtQuerySystemInformation` to get system processor performance information. It queries the Windows Registry for CPU speed (`~MHz`) and model (`ProcessorNameString`) [9](#ref-q1-9) .
*   **Linux/Android**: It reads `/proc/stat` to get CPU time statistics (user, nice, sys, idle, irq) and `/proc/cpuinfo` to get the CPU model name. It also attempts to read the current CPU frequency from `/sys/devices/system/cpu/cpuX/cpufreq/scaling_cur_freq` [10](#ref-q1-10) .
*   **OpenBSD**: It provides a stub implementation that returns dummy values for `model`, `speed`, and `times` fields, as full implementation is not feasible due to missing libc bindings [11](#ref-q1-11) .

### Object Shape Returned by `os.cpus()`

The `os.cpus()` function returns an array of `CpuInfo` objects [12](#ref-q1-12) . Each `CpuInfo` object has the following structure [3](#ref-q1-3) :
*   `model`: A string representing the CPU model [13](#ref-q1-13) .
*   `speed`: A number representing the CPU speed in MHz [14](#ref-q1-14) .
*   `times`: An object containing CPU time statistics [15](#ref-q1-15) :
    *   `user`: The number of milliseconds the CPU has spent in user mode [16](#ref-q1-16) .
    *   `nice`: The number of milliseconds the CPU has spent in nice mode (POSIX-only, always 0 on Windows) [17](#ref-q1-17) .
    *   `sys`: The number of milliseconds the CPU has spent in sys mode [18](#ref-q1-18) .
    *   `idle`: The number of milliseconds the CPU has spent in idle mode [19](#ref-q1-19) .
    *   `irq`: The number of milliseconds the CPU has spent in irq mode [20](#ref-q1-20) .

### Fallback Behavior

When host CPU details are unavailable, the `cpu_info()` function in `ext/node/ops/os/cpus.rs` returns `None` [21](#ref-q1-21)  [22](#ref-q1-22)  [23](#ref-q1-23)  [24](#ref-q1-24)  [25](#ref-q1-25)  [26](#ref-q1-26)  [27](#ref-q1-27)  [28](#ref-q1-28)  [29](#ref-q1-29)  [30](#ref-q1-30)  [31](#ref-q1-31)  [32](#ref-q1-32)  [33](#ref-q1-33)  [34](#ref-q1-34)  [35](#ref-q1-35)  [36](#ref-q1-36)  [37](#ref-q1-37)  [38](#ref-q1-38)  [39](#ref-q1-39)  [40](#ref-q1-40)  [41](#ref-q1-41)  [42](#ref-q1-42)  [43](#ref-q1-43)  [44](#ref-q1-44)  [45](#ref-q1-45)  [46](#ref-q1-46)  [47](#ref-q1-47)  [48](#ref-q1-48)  [49](#ref-q1-49)  [50](#ref-q1-50)  [51](#ref-q1-51)  [52](#ref-q1-52)  [53](#ref-q1-53)  [54](#ref-q1-54)  [55](#ref-q1-55)  [56](#ref-q1-56)  [57](#ref-q1-57)  [58](#ref-q1-58)  [59](#ref-q1-59)  [60](#ref-q1-60)  [61](#ref-q1-61)  [62](#ref-q1-62)  [63](#ref-q1-63)  [64](#ref-q1-64)  [65](#ref-q1-65)  [66](#ref-q1-66)  [67](#ref-q1-67)  [68](#ref-q1-68)  [69](#ref-q1-69)  [70](#ref-q1-70)  [71](#ref-q1-71)  [72](#ref-q1-72)  [73](#ref-q1-73)  [74](#ref-q1-74)  [75](#ref-q1-75)  [76](#ref-q1-76)  [77](#ref-q1-77)  [78](#ref-q1-78)  [79](#ref-q1-79)  [80](#ref-q1-80)  [81](#ref-q1-81) . In such cases, `op_cpus` returns an empty vector [7](#ref-q1-7) . For OpenBSD, a stub

Wiki pages you might want to explore:
- [Glossary (denoland/deno)](/wiki/denoland/deno#12)
- [Process, TTY, and OS APIs (denoland/deno)](/wiki/denoland/deno#7.7)

View this search on DeepWiki: https://deepwiki.com/search/in-denos-nodeos-compatibility_e2f09965-eb50-45b4-8912-9b0850eb675f

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/os.ts:75-77`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/os.ts#L75-L77)

```typescript
function cpus() {
  return op_cpus();
}
```

<a id="ref-q1-2"></a>
### [2] `ext/node/ops/os/mod.rs:293-302`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/mod.rs#L293-L302)

```rust
#[op2(stack_trace)]
#[serde]
pub fn op_cpus(state: &mut OpState) -> Result<Vec<cpus::CpuInfo>, OsError> {
  {
    let permissions = state.borrow_mut::<PermissionsContainer>();
    permissions.check_sys("cpus", "node:os.cpus()")?;
  }

  Ok(cpus::cpu_info().unwrap_or_default())
}
```

<a id="ref-q1-3"></a>
### [3] `cli/tsc/dts/node/os.d.cts:11-26`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L11-L26)

```
    interface CpuInfo {
        model: string;
        speed: number;
        times: {
            /** The number of milliseconds the CPU has spent in user mode. */
            user: number;
            /** The number of milliseconds the CPU has spent in nice mode. */
            nice: number;
            /** The number of milliseconds the CPU has spent in sys mode. */
            sys: number;
            /** The number of milliseconds the CPU has spent in idle mode. */
            idle: number;
            /** The number of milliseconds the CPU has spent in irq mode. */
            irq: number;
        };
    }
```

<a id="ref-q1-4"></a>
### [4] `ext/node/ops/os/cpus.rs:14-20`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L14-L20)

```rust
#[derive(Debug, Default, Serialize, Clone)]
pub struct CpuInfo {
  pub model: String,
  /* in MHz */
  pub speed: u64,
  pub times: CpuTimes,
}
```

<a id="ref-q1-5"></a>
### [5] `ext/node/ops/os/cpus.rs:5-11`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L5-L11)

```rust
#[derive(Debug, Default, Serialize, Clone)]
pub struct CpuTimes {
  pub user: u64,
  pub nice: u64,
  pub sys: u64,
  pub idle: u64,
  pub irq: u64,
```

<a id="ref-q1-6"></a>
### [6] `ext/node/ops/os/mod.rs:297-298`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/mod.rs#L297-L298)

```rust
    let permissions = state.borrow_mut::<PermissionsContainer>();
    permissions.check_sys("cpus", "node:os.cpus()")?;
```

<a id="ref-q1-7"></a>
### [7] `ext/node/ops/os/mod.rs:301`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/mod.rs#L301)

```rust
  Ok(cpus::cpu_info().unwrap_or_default())
```

<a id="ref-q1-8"></a>
### [8] `ext/node/ops/os/cpus.rs:29-125`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L29-L125)

```rust
pub fn cpu_info() -> Option<Vec<CpuInfo>> {
  let mut model: [u8; 512] = [0; 512];
  let mut size = std::mem::size_of_val(&model);

  // Safety: Assumes correct behavior of platform-specific syscalls and data structures.
  // Relies on specific sysctl names and sysconf parameter existence.
  unsafe {
    let ticks = libc::sysconf(libc::_SC_CLK_TCK);
    let multiplier = 1000u64 / ticks as u64;
    if libc::sysctlbyname(
      c"machdep.cpu.brand_string".as_ptr() as *const libc::c_char,
      model.as_mut_ptr() as _,
      &mut size,
      std::ptr::null_mut(),
      0,
    ) != 0
      && libc::sysctlbyname(
        c"hw.model".as_ptr() as *const libc::c_char,
        model.as_mut_ptr() as _,
        &mut size,
        std::ptr::null_mut(),
        0,
      ) != 0
    {
      return None;
    }

    let mut cpu_speed: u64 = 0;
    let mut cpu_speed_size = std::mem::size_of_val(&cpu_speed);

    libc::sysctlbyname(
      c"hw.cpufrequency".as_ptr() as *const libc::c_char,
      &mut cpu_speed as *mut _ as *mut libc::c_void,
      &mut cpu_speed_size,
      std::ptr::null_mut(),
      0,
    );

    if cpu_speed == 0 {
      // https://github.com/libuv/libuv/pull/3679
      //
      // hw.cpufrequency sysctl seems to be missing on darwin/arm64
      // so we instead hardcode a plausible value. This value matches
      // what the mach kernel will report when running Rosetta apps.
      cpu_speed = 2_400_000_000;
    }

    unsafe extern "C" {
      fn mach_host_self() -> std::ffi::c_uint;
      static mut mach_task_self_: std::ffi::c_uint;
    }

    let mut num_cpus: libc::natural_t = 0;
    let mut info: *mut libc::processor_cpu_load_info_data_t =
      std::ptr::null_mut();
    let mut msg_type: libc::mach_msg_type_number_t = 0;
    if libc::host_processor_info(
      mach_host_self(),
      libc::PROCESSOR_CPU_LOAD_INFO,
      &mut num_cpus,
      &mut info as *mut _ as *mut libc::processor_info_array_t,
      &mut msg_type,
    ) != 0
    {
      return None;
    }

    let mut cpus = vec![CpuInfo::new(); num_cpus as usize];

    let info = std::slice::from_raw_parts(info, num_cpus as usize);
    let model = std::ffi::CStr::from_ptr(model.as_ptr() as _)
      .to_string_lossy()
      .into_owned();
    for (i, cpu) in cpus.iter_mut().enumerate() {
      cpu.times.user =
        info[i].cpu_ticks[libc::CPU_STATE_USER as usize] as u64 * multiplier;
      cpu.times.nice =
        info[i].cpu_ticks[libc::CPU_STATE_NICE as usize] as u64 * multiplier;
      cpu.times.sys =
        info[i].cpu_ticks[libc::CPU_STATE_SYSTEM as usize] as u64 * multiplier;
      cpu.times.idle =
        info[i].cpu_ticks[libc::CPU_STATE_IDLE as usize] as u64 * multiplier;

      cpu.times.irq = 0;

      cpu.model.clone_from(&model);
      cpu.speed = cpu_speed / 1000000;
    }

    libc::vm_deallocate(
      mach_task_self_,
      info.as_ptr() as libc::vm_address_t,
      msg_type as _,
    );

    Some(cpus)
  }
```

<a id="ref-q1-9"></a>
### [9] `ext/node/ops/os/cpus.rs:128-241`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L128-L241)

```rust
#[cfg(target_os = "windows")]
pub fn cpu_info() -> Option<Vec<CpuInfo>> {
  use std::os::windows::ffi::OsStrExt;
  use std::os::windows::ffi::OsStringExt;

  use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
  use windows_sys::Wdk::System::SystemInformation::SystemProcessorPerformanceInformation;
  use windows_sys::Win32::System::WindowsProgramming::SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION;

  fn encode_wide(s: &str) -> Vec<u16> {
    std::ffi::OsString::from(s)
      .encode_wide()
      .chain(Some(0))
      .collect()
  }

  // Safety: Assumes correct behavior of platform-specific syscalls and data structures.
  unsafe {
    let mut system_info: winapi::um::sysinfoapi::SYSTEM_INFO =
      std::mem::zeroed();
    winapi::um::sysinfoapi::GetSystemInfo(&mut system_info);

    let cpu_count = system_info.dwNumberOfProcessors as usize;

    let mut cpus = vec![CpuInfo::new(); cpu_count];

    let mut sppi: Vec<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION> =
      vec![std::mem::zeroed(); cpu_count];

    let sppi_size = std::mem::size_of_val(&sppi[0]) * cpu_count;
    let mut result_size = 0;

    let status = NtQuerySystemInformation(
      SystemProcessorPerformanceInformation,
      sppi.as_mut_ptr() as *mut _,
      sppi_size as u32,
      &mut result_size,
    );
    if status != 0 {
      return None;
    }

    assert_eq!(result_size, sppi_size as u32);

    for i in 0..cpu_count {
      let key_name =
        format!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\{}", i);
      let key_name = encode_wide(&key_name);

      let mut processor_key: windows_sys::Win32::System::Registry::HKEY =
        std::mem::zeroed();
      let err = windows_sys::Win32::System::Registry::RegOpenKeyExW(
        windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        key_name.as_ptr(),
        0,
        windows_sys::Win32::System::Registry::KEY_QUERY_VALUE,
        &mut processor_key,
      );

      if err != 0 {
        return None;
      }

      let mut cpu_speed = 0;
      let mut cpu_speed_size = std::mem::size_of_val(&cpu_speed) as u32;

      let err = windows_sys::Win32::System::Registry::RegQueryValueExW(
        processor_key,
        encode_wide("~MHz").as_ptr() as *mut _,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut cpu_speed as *mut _ as *mut _,
        &mut cpu_speed_size,
      );

      if err != 0 {
        return None;
      }

      let cpu_brand: [u16; 512] = [0; 512];
      let mut cpu_brand_size = std::mem::size_of_val(&cpu_brand) as u32;

      let err = windows_sys::Win32::System::Registry::RegQueryValueExW(
        processor_key,
        encode_wide("ProcessorNameString").as_ptr() as *mut _,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        cpu_brand.as_ptr() as *mut _,
        &mut cpu_brand_size,
      );
      windows_sys::Win32::System::Registry::RegCloseKey(processor_key);

      if err != 0 {
        return None;
      }

      let cpu_brand =
        std::ffi::OsString::from_wide(&cpu_brand[..cpu_brand_size as usize])
          .into_string()
          .unwrap();

      cpus[i].model = cpu_brand;
      cpus[i].speed = cpu_speed as u64;

      cpus[i].times.user = sppi[i].UserTime as u64 / 10000;
      cpus[i].times.sys =
        (sppi[i].KernelTime - sppi[i].IdleTime) as u64 / 10000;
      cpus[i].times.idle = sppi[i].IdleTime as u64 / 10000;
      /* InterruptTime is Reserved1[1] */
      cpus[i].times.irq = sppi[i].Reserved1[1] as u64 / 10000;
      cpus[i].times.nice = 0;
    }
    Some(cpus)
  }
```

<a id="ref-q1-10"></a>
### [10] `ext/node/ops/os/cpus.rs:244-314`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L244-L314)

```rust
#[cfg(any(target_os = "android", target_os = "linux"))]
pub fn cpu_info() -> Option<Vec<CpuInfo>> {
  use std::io::BufRead;

  let mut cpus = vec![CpuInfo::new(); 8192]; /* Kernel maximum */

  let fp = std::fs::File::open("/proc/stat").ok()?;
  let reader = std::io::BufReader::new(fp);

  let mut count = 0;
  // Skip the first line which tracks total CPU time across all cores
  for (i, line) in reader.lines().skip(1).enumerate() {
    let line = line.ok()?;
    if !line.starts_with("cpu") {
      break;
    }
    count = i + 1;
    let mut fields = line.split_whitespace();
    fields.next()?;
    let user = fields.next()?.parse::<u64>().ok()?;
    let nice = fields.next()?.parse::<u64>().ok()?;
    let sys = fields.next()?.parse::<u64>().ok()?;
    let idle = fields.next()?.parse::<u64>().ok()?;
    let _iowait = fields.next()?.parse::<u64>().ok()?;
    let irq = fields.next()?.parse::<u64>().ok()?;

    // sysconf(_SC_CLK_TCK) is fixed at 100 Hz, therefore the
    // multiplier is always 1000/100 = 10
    cpus[i].times.user = user * 10;
    cpus[i].times.nice = nice * 10;
    cpus[i].times.sys = sys * 10;
    cpus[i].times.idle = idle * 10;
    cpus[i].times.irq = irq * 10;
  }

  let fp = std::fs::File::open("/proc/cpuinfo").ok()?;
  let reader = std::io::BufReader::new(fp);

  let mut j = 0;
  for line in reader.lines() {
    let line = line.ok()?;
    if !line.starts_with("model name") {
      continue;
    }
    let mut fields = line.splitn(2, ':');
    fields.next()?;
    let model = fields.next()?.trim();

    cpus[j].model = model.to_string();

    if let Ok(fp) = std::fs::File::open(format!(
      "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq",
      j
    )) {
      let mut reader = std::io::BufReader::new(fp);
      let mut speed = String::new();
      reader.read_line(&mut speed).ok()?;

      cpus[j].speed = speed.trim().parse::<u64>().ok()? / 1000;
    }

    j += 1;
  }

  while j < count {
    cpus[j].model = "unknown".to_string();
    j += 1;
  }

  cpus.truncate(count);
  Some(cpus)
```

<a id="ref-q1-11"></a>
### [11] `ext/node/ops/os/cpus.rs:317-358`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L317-L358)

```rust
#[cfg(target_os = "openbsd")]
pub fn cpu_info() -> Option<Vec<CpuInfo>> {
  // Stub implementation for OpenBSD that returns an array of the correct size
  // but with dummy values.
  // Rust's OpenBSD libc bindings don't contain all the symbols needed for a
  // full implementation, and including them is not planned.
  let mut mib = [libc::CTL_HW, libc::HW_NCPUONLINE];

  // SAFETY: Assumes correct behavior of platform-specific
  // sysctls and data structures. Relies on specific sysctl
  // names and parameter existence.
  unsafe {
    let mut ncpu: libc::c_uint = 0;
    let mut size = std::mem::size_of_val(&ncpu) as libc::size_t;

    // Get number of CPUs online
    let res = libc::sysctl(
      mib.as_mut_ptr(),
      mib.len() as _,
      &mut ncpu as *mut _ as *mut _,
      &mut size,
      std::ptr::null_mut(),
      0,
    );
    // If res == 0, the sysctl call was succesful and
    // ncpuonline contains the number of online CPUs.
    if res != 0 {
      return None;
    } else {
      let mut cpus = vec![CpuInfo::new(); ncpu as usize];

      for (_, cpu) in cpus.iter_mut().enumerate() {
        cpu.model = "Undisclosed CPU".to_string();
        // Return 1 as a dummy value so the tests won't
        // fail.
        cpu.speed = 1;
        cpu.times.user = 1;
        cpu.times.nice = 1;
        cpu.times.sys = 1;
        cpu.times.idle = 1;
        cpu.times.irq = 1;
      }
```

<a id="ref-q1-12"></a>
### [12] `cli/tsc/dts/node/os.d.cts:143`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L143)

```
    function cpus(): CpuInfo[];
```

<a id="ref-q1-13"></a>
### [13] `cli/tsc/dts/node/os.d.cts:12`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L12)

```
        model: string;
```

<a id="ref-q1-14"></a>
### [14] `cli/tsc/dts/node/os.d.cts:13`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L13)

```
        speed: number;
```

<a id="ref-q1-15"></a>
### [15] `cli/tsc/dts/node/os.d.cts:14`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L14)

```
        times: {
```

<a id="ref-q1-16"></a>
### [16] `cli/tsc/dts/node/os.d.cts:15`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L15)

```
            /** The number of milliseconds the CPU has spent in user mode. */
```

<a id="ref-q1-17"></a>
### [17] `cli/tsc/dts/node/os.d.cts:16`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L16)

```
            user: number;
```

<a id="ref-q1-18"></a>
### [18] `cli/tsc/dts/node/os.d.cts:17`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L17)

```
            /** The number of milliseconds the CPU has spent in nice mode. */
```

<a id="ref-q1-19"></a>
### [19] `cli/tsc/dts/node/os.d.cts:18`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L18)

```
            nice: number;
```

<a id="ref-q1-20"></a>
### [20] `cli/tsc/dts/node/os.d.cts:19`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/os.d.cts#L19)

```
            /** The number of milliseconds the CPU has spent in sys mode. */
```

<a id="ref-q1-21"></a>
### [21] `ext/node/ops/os/cpus.rs:53-54`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L53-L54)

```rust
      return None;
    }
```

<a id="ref-q1-22"></a>
### [22] `ext/node/ops/os/cpus.rs:93-94`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L93-L94)

```rust
      return None;
    }
```

<a id="ref-q1-23"></a>
### [23] `ext/node/ops/os/cpus.rs:167-168`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L167-L168)

```rust
      return None;
    }
```

<a id="ref-q1-24"></a>
### [24] `ext/node/ops/os/cpus.rs:188-189`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L188-L189)

```rust
        return None;
      }
```

<a id="ref-q1-25"></a>
### [25] `ext/node/ops/os/cpus.rs:203-204`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L203-L204)

```rust
      if err != 0 {
        return None;
```

<a id="ref-q1-26"></a>
### [26] `ext/node/ops/os/cpus.rs:220-221`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L220-L221)

```rust
      if err != 0 {
        return None;
```

<a id="ref-q1-27"></a>
### [27] `ext/node/ops/os/cpus.rs:250`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L250)

```rust
  let fp = std::fs::File::open("/proc/stat").ok()?;
```

<a id="ref-q1-28"></a>
### [28] `ext/node/ops/os/cpus.rs:256`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L256)

```rust
    let line = line.ok()?;
```

<a id="ref-q1-29"></a>
### [29] `ext/node/ops/os/cpus.rs:263`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L263)

```rust
    let user = fields.next()?.parse::<u64>().ok()?;
```

<a id="ref-q1-30"></a>
### [30] `ext/node/ops/os/cpus.rs:264`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L264)

```rust
    let nice = fields.next()?.parse::<u64>().ok()?;
```

<a id="ref-q1-31"></a>
### [31] `ext/node/ops/os/cpus.rs:265`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L265)

```rust
    let sys = fields.next()?.parse::<u64>().ok()?;
```

<a id="ref-q1-32"></a>
### [32] `ext/node/ops/os/cpus.rs:266`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L266)

```rust
    let idle = fields.next()?.parse::<u64>().ok()?;
```

<a id="ref-q1-33"></a>
### [33] `ext/node/ops/os/cpus.rs:268`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L268)

```rust
    let irq = fields.next()?.parse::<u64>().ok()?;
```

<a id="ref-q1-34"></a>
### [34] `ext/node/ops/os/cpus.rs:270`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L270)

```rust
    // sysconf(_SC_CLK_TCK) is fixed at 100 Hz, therefore the
```

<a id="ref-q1-35"></a>
### [35] `ext/node/ops/os/cpus.rs:271`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L271)

```rust
    // multiplier is always 1000/100 = 10
```

<a id="ref-q1-36"></a>
### [36] `ext/node/ops/os/cpus.rs:272`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L272)

```rust
    cpus[i].times.user = user * 10;
```

<a id="ref-q1-37"></a>
### [37] `ext/node/ops/os/cpus.rs:273`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L273)

```rust
    cpus[i].times.nice = nice * 10;
```

<a id="ref-q1-38"></a>
### [38] `ext/node/ops/os/cpus.rs:274`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L274)

```rust
    cpus[i].times.sys = sys * 10;
```

<a id="ref-q1-39"></a>
### [39] `ext/node/ops/os/cpus.rs:275`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L275)

```rust
    cpus[i].times.idle = idle * 10;
```

<a id="ref-q1-40"></a>
### [40] `ext/node/ops/os/cpus.rs:276`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L276)

```rust
    cpus[i].times.irq = irq * 10;
```

<a id="ref-q1-41"></a>
### [41] `ext/node/ops/os/cpus.rs:277`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L277)

```rust
  }
```

<a id="ref-q1-42"></a>
### [42] `ext/node/ops/os/cpus.rs:278`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L278)

<a id="ref-q1-43"></a>
### [43] `ext/node/ops/os/cpus.rs:279`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L279)

```rust
  let fp = std::fs::File::open("/proc/cpuinfo").ok()?;
```

<a id="ref-q1-44"></a>
### [44] `ext/node/ops/os/cpus.rs:280`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L280)

```rust
  let reader = std::io::BufReader::new(fp);
```

<a id="ref-q1-45"></a>
### [45] `ext/node/ops/os/cpus.rs:281`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L281)

<a id="ref-q1-46"></a>
### [46] `ext/node/ops/os/cpus.rs:282`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L282)

```rust
  let mut j = 0;
```

<a id="ref-q1-47"></a>
### [47] `ext/node/ops/os/cpus.rs:283`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L283)

```rust
  for line in reader.lines() {
```

<a id="ref-q1-48"></a>
### [48] `ext/node/ops/os/cpus.rs:284`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L284)

```rust
    let line = line.ok()?;
```

<a id="ref-q1-49"></a>
### [49] `ext/node/ops/os/cpus.rs:285`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L285)

```rust
    if !line.starts_with("model name") {
```

<a id="ref-q1-50"></a>
### [50] `ext/node/ops/os/cpus.rs:286`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L286)

```rust
      continue;
```

<a id="ref-q1-51"></a>
### [51] `ext/node/ops/os/cpus.rs:287`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L287)

```rust
    }
```

<a id="ref-q1-52"></a>
### [52] `ext/node/ops/os/cpus.rs:288`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L288)

```rust
    let mut fields = line.splitn(2, ':');
```

<a id="ref-q1-53"></a>
### [53] `ext/node/ops/os/cpus.rs:289`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L289)

```rust
    fields.next()?;
```

<a id="ref-q1-54"></a>
### [54] `ext/node/ops/os/cpus.rs:290`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L290)

```rust
    let model = fields.next()?.trim();
```

<a id="ref-q1-55"></a>
### [55] `ext/node/ops/os/cpus.rs:291`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L291)

<a id="ref-q1-56"></a>
### [56] `ext/node/ops/os/cpus.rs:292`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L292)

```rust
    cpus[j].model = model.to_string();
```

<a id="ref-q1-57"></a>
### [57] `ext/node/ops/os/cpus.rs:293`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L293)

<a id="ref-q1-58"></a>
### [58] `ext/node/ops/os/cpus.rs:294`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L294)

```rust
    if let Ok(fp) = std::fs::File::open(format!(
```

<a id="ref-q1-59"></a>
### [59] `ext/node/ops/os/cpus.rs:295`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L295)

```rust
      "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq",
```

<a id="ref-q1-60"></a>
### [60] `ext/node/ops/os/cpus.rs:296`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L296)

```rust
      j
```

<a id="ref-q1-61"></a>
### [61] `ext/node/ops/os/cpus.rs:297`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L297)

```rust
    )) {
```

<a id="ref-q1-62"></a>
### [62] `ext/node/ops/os/cpus.rs:298`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L298)

```rust
      let mut reader = std::io::BufReader::new(fp);
```

<a id="ref-q1-63"></a>
### [63] `ext/node/ops/os/cpus.rs:299`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L299)

```rust
      let mut speed = String::new();
```

<a id="ref-q1-64"></a>
### [64] `ext/node/ops/os/cpus.rs:300`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L300)

```rust
      reader.read_line(&mut speed).ok()?;
```

<a id="ref-q1-65"></a>
### [65] `ext/node/ops/os/cpus.rs:301`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L301)

<a id="ref-q1-66"></a>
### [66] `ext/node/ops/os/cpus.rs:302`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L302)

```rust
      cpus[j].speed = speed.trim().parse::<u64>().ok()? / 1000;
```

<a id="ref-q1-67"></a>
### [67] `ext/node/ops/os/cpus.rs:303`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L303)

```rust
    }
```

<a id="ref-q1-68"></a>
### [68] `ext/node/ops/os/cpus.rs:304`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L304)

<a id="ref-q1-69"></a>
### [69] `ext/node/ops/os/cpus.rs:305`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L305)

```rust
    j += 1;
```

<a id="ref-q1-70"></a>
### [70] `ext/node/ops/os/cpus.rs:306`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L306)

```rust
  }
```

<a id="ref-q1-71"></a>
### [71] `ext/node/ops/os/cpus.rs:307`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L307)

<a id="ref-q1-72"></a>
### [72] `ext/node/ops/os/cpus.rs:308`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L308)

```rust
  while j < count {
```

<a id="ref-q1-73"></a>
### [73] `ext/node/ops/os/cpus.rs:309`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L309)

```rust
    cpus[j].model = "unknown".to_string();
```

<a id="ref-q1-74"></a>
### [74] `ext/node/ops/os/cpus.rs:310`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L310)

```rust
    j += 1;
```

<a id="ref-q1-75"></a>
### [75] `ext/node/ops/os/cpus.rs:311`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L311)

```rust
  }
```

<a id="ref-q1-76"></a>
### [76] `ext/node/ops/os/cpus.rs:312`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L312)

<a id="ref-q1-77"></a>
### [77] `ext/node/ops/os/cpus.rs:313`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L313)

```rust
  cpus.truncate(count);
```

<a id="ref-q1-78"></a>
### [78] `ext/node/ops/os/cpus.rs:314`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L314)

```rust
  Some(cpus)
```

<a id="ref-q1-79"></a>
### [79] `ext/node/ops/os/cpus.rs:315`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L315)

```rust
}
```

<a id="ref-q1-80"></a>
### [80] `ext/node/ops/os/cpus.rs:316`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L316)

<a id="ref-q1-81"></a>
### [81] `ext/node/ops/os/cpus.rs:344-345`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/os/cpus.rs#L344-L345)

```rust
      return None;
    } else {
```
