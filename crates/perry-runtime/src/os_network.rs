//! `os.networkInterfaces()` support, split out of `os.rs` to keep that file
//! under the 2000-line source gate (#3006).

use crate::object::ObjectHeader;
use crate::string::js_string_from_bytes;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

const OS_NETWORK_IPV4_SHAPE_ID: u32 = 0x7FFF_FF27;
const OS_NETWORK_IPV6_SHAPE_ID: u32 = 0x7FFF_FF28;

struct OsNetworkAddress {
    address: String,
    netmask: String,
    family: &'static str,
    mac: String,
    internal: bool,
    cidr: String,
    scopeid: Option<u32>,
}

fn prefix_len_ipv4(netmask: Ipv4Addr) -> u32 {
    u32::from(netmask).count_ones()
}

fn prefix_len_ipv6(netmask: Ipv6Addr) -> u32 {
    netmask.octets().iter().map(|byte| byte.count_ones()).sum()
}

fn fallback_mac_address() -> String {
    "00:00:00:00:00:00".to_string()
}

// Callers are `collect_link_macs` (macOS/BSD), `collect_windows_adapters`
// (Windows), and the unit tests. On Linux/Android the MAC is read as a
// preformatted string from `/sys`, so this helper has no caller there and
// would trip `-D dead-code`; gate it to the union of its real callers.
#[cfg(any(
    windows,
    all(unix, not(any(target_os = "linux", target_os = "android"))),
    test
))]
fn format_mac_address(bytes: &[u8]) -> String {
    if bytes.len() != 6 {
        return fallback_mac_address();
    }
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(any(windows, test))]
fn ipv4_netmask_from_prefix(prefix: u8) -> Ipv4Addr {
    let prefix = prefix.min(32);
    let bits = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Ipv4Addr::from(bits)
}

#[cfg(any(windows, test))]
fn ipv6_netmask_from_prefix(prefix: u8) -> Ipv6Addr {
    let prefix = prefix.min(128);
    let mut octets = [0u8; 16];
    let full_bytes = usize::from(prefix / 8);
    octets[..full_bytes].fill(u8::MAX);
    let remaining_bits = prefix % 8;
    if remaining_bits != 0 {
        octets[full_bytes] = u8::MAX << (8 - remaining_bits);
    }
    Ipv6Addr::from(octets)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn interface_mac_address(name: &str) -> String {
    let path = format!("/sys/class/net/{name}/address");
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(fallback_mac_address)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn interface_mac_address(_name: &str) -> String {
    fallback_mac_address()
}

/// #3006 — on macOS/BSD the per-interface hardware address is exposed through
/// `getifaddrs` as an `AF_LINK` entry (`sockaddr_dl`), not via `/sys`. Walk the
/// list once and map interface name → formatted MAC string. Interfaces with no
/// link-layer address (or a zero-length one, e.g. tunnels) are left out and
/// fall back to the all-zero MAC, matching Node.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn collect_link_macs() -> HashMap<String, String> {
    let mut macs: HashMap<String, String> = HashMap::new();
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return macs;
    }
    let mut current = first;
    while !current.is_null() {
        let ifa = unsafe { &*current };
        if !ifa.ifa_addr.is_null() && !ifa.ifa_name.is_null() {
            let family = unsafe { (*ifa.ifa_addr).sa_family as i32 };
            if family == libc::AF_LINK {
                let sdl = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_dl) };
                let alen = sdl.sdl_alen as usize;
                if alen == 6 {
                    let nlen = sdl.sdl_nlen as usize;
                    // MAC bytes follow the interface name inside `sdl_data`.
                    let data = unsafe { (sdl.sdl_data.as_ptr() as *const u8).add(nlen) };
                    let bytes = unsafe { std::slice::from_raw_parts(data, alen) };
                    let mac = format_mac_address(bytes);
                    let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }
                        .to_string_lossy()
                        .into_owned();
                    macs.insert(name, mac);
                }
            }
        }
        current = ifa.ifa_next;
    }
    unsafe { libc::freeifaddrs(first) };
    macs
}

#[cfg(unix)]
unsafe fn ipv4_from_sockaddr(addr: *const libc::sockaddr) -> Ipv4Addr {
    let sin = &*(addr as *const libc::sockaddr_in);
    Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr))
}

#[cfg(unix)]
unsafe fn ipv6_from_sockaddr(addr: *const libc::sockaddr) -> Ipv6Addr {
    let sin6 = &*(addr as *const libc::sockaddr_in6);
    Ipv6Addr::from(sin6.sin6_addr.s6_addr)
}

#[cfg(unix)]
fn collect_network_interfaces() -> HashMap<String, Vec<OsNetworkAddress>> {
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return HashMap::new();
    }

    let mut interfaces: HashMap<String, Vec<OsNetworkAddress>> = HashMap::new();
    // #3006 — on macOS/BSD seed the per-interface MACs from the AF_LINK
    // entries up front; Linux/Android read them lazily from `/sys`.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let mut macs: HashMap<String, String> = collect_link_macs();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let mut macs: HashMap<String, String> = HashMap::new();
    let mut current = first;
    while !current.is_null() {
        let ifa = unsafe { &*current };
        if !ifa.ifa_addr.is_null() && !ifa.ifa_name.is_null() {
            let family = unsafe { (*ifa.ifa_addr).sa_family as i32 };
            let internal = (ifa.ifa_flags & (libc::IFF_LOOPBACK as libc::c_uint)) != 0;
            let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }
                .to_string_lossy()
                .into_owned();
            let mac = macs
                .entry(name.clone())
                .or_insert_with(|| interface_mac_address(&name))
                .clone();

            match family {
                libc::AF_INET => {
                    let address = unsafe { ipv4_from_sockaddr(ifa.ifa_addr) };
                    let netmask = if ifa.ifa_netmask.is_null() {
                        Ipv4Addr::UNSPECIFIED
                    } else {
                        unsafe { ipv4_from_sockaddr(ifa.ifa_netmask) }
                    };
                    let prefix = prefix_len_ipv4(netmask);
                    interfaces.entry(name).or_default().push(OsNetworkAddress {
                        address: address.to_string(),
                        netmask: netmask.to_string(),
                        family: "IPv4",
                        mac,
                        internal,
                        cidr: format!("{address}/{prefix}"),
                        scopeid: None,
                    });
                }
                libc::AF_INET6 => {
                    let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                    let address = unsafe { ipv6_from_sockaddr(ifa.ifa_addr) };
                    let netmask = if ifa.ifa_netmask.is_null() {
                        Ipv6Addr::UNSPECIFIED
                    } else {
                        unsafe { ipv6_from_sockaddr(ifa.ifa_netmask) }
                    };
                    let prefix = prefix_len_ipv6(netmask);
                    interfaces.entry(name).or_default().push(OsNetworkAddress {
                        address: address.to_string(),
                        netmask: netmask.to_string(),
                        family: "IPv6",
                        mac,
                        internal,
                        cidr: format!("{address}/{prefix}"),
                        scopeid: Some(sin6.sin6_scope_id),
                    });
                }
                _ => {}
            }
        }
        current = ifa.ifa_next;
    }

    unsafe { libc::freeifaddrs(first) };
    interfaces
}

#[cfg(windows)]
unsafe fn windows_wide_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }

    let mut len = 0;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

#[cfg(windows)]
unsafe fn collect_windows_adapters(
    mut adapter: *mut windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH,
) -> HashMap<String, Vec<OsNetworkAddress>> {
    use windows_sys::Win32::NetworkManagement::IpHelper::IF_TYPE_SOFTWARE_LOOPBACK;
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
    };

    let mut interfaces: HashMap<String, Vec<OsNetworkAddress>> = HashMap::new();
    while !adapter.is_null() {
        let current = unsafe { &*adapter };
        adapter = current.Next;
        if current.OperStatus != IfOperStatusUp || current.FirstUnicastAddress.is_null() {
            continue;
        }

        let name = unsafe { windows_wide_string(current.FriendlyName) };
        if name.is_empty() {
            continue;
        }
        let mac = if current.PhysicalAddressLength == 6 {
            format_mac_address(&current.PhysicalAddress[..6])
        } else {
            fallback_mac_address()
        };
        let internal = current.IfType == IF_TYPE_SOFTWARE_LOOPBACK;

        let mut unicast = current.FirstUnicastAddress;
        while !unicast.is_null() {
            let address_entry = unsafe { &*unicast };
            unicast = address_entry.Next;
            let socket_address = address_entry.Address;
            if socket_address.lpSockaddr.is_null()
                || socket_address.iSockaddrLength < size_of::<SOCKADDR>() as i32
            {
                continue;
            }

            let family = unsafe { (*socket_address.lpSockaddr).sa_family };
            match family {
                AF_INET if socket_address.iSockaddrLength >= size_of::<SOCKADDR_IN>() as i32 => {
                    let socket = unsafe { &*socket_address.lpSockaddr.cast::<SOCKADDR_IN>() };
                    let bytes = unsafe { socket.sin_addr.S_un.S_un_b };
                    let address = Ipv4Addr::new(bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4);
                    let prefix = address_entry.OnLinkPrefixLength.min(32);
                    let netmask = ipv4_netmask_from_prefix(prefix);
                    interfaces
                        .entry(name.clone())
                        .or_default()
                        .push(OsNetworkAddress {
                            address: address.to_string(),
                            netmask: netmask.to_string(),
                            family: "IPv4",
                            mac: mac.clone(),
                            internal,
                            cidr: format!("{address}/{prefix}"),
                            scopeid: None,
                        });
                }
                AF_INET6 if socket_address.iSockaddrLength >= size_of::<SOCKADDR_IN6>() as i32 => {
                    let socket = unsafe { &*socket_address.lpSockaddr.cast::<SOCKADDR_IN6>() };
                    let address = Ipv6Addr::from(unsafe { socket.sin6_addr.u.Byte });
                    let prefix = address_entry.OnLinkPrefixLength.min(128);
                    let netmask = ipv6_netmask_from_prefix(prefix);
                    interfaces
                        .entry(name.clone())
                        .or_default()
                        .push(OsNetworkAddress {
                            address: address.to_string(),
                            netmask: netmask.to_string(),
                            family: "IPv6",
                            mac: mac.clone(),
                            internal,
                            cidr: format!("{address}/{prefix}"),
                            scopeid: Some(unsafe { socket.Anonymous.sin6_scope_id }),
                        });
                }
                _ => {}
            }
        }
    }
    interfaces
}

#[cfg(windows)]
fn collect_network_interfaces() -> HashMap<String, Vec<OsNetworkAddress>> {
    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;

    const INITIAL_BUFFER_BYTES: u32 = 16 * 1024;
    const MAX_ATTEMPTS: usize = 4;

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut buffer_bytes = INITIAL_BUFFER_BYTES;

    for _ in 0..MAX_ATTEMPTS {
        // A byte Vec is not guaranteed to satisfy the adapter structure's
        // alignment. u64 storage supplies the required eight-byte alignment on
        // every Windows target while still giving the API a byte-sized buffer.
        let word_count = (buffer_bytes as usize).saturating_add(7) / 8;
        let mut buffer = vec![0u64; word_count];
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
                &mut buffer_bytes,
            )
        };

        if result == ERROR_SUCCESS {
            return unsafe {
                collect_windows_adapters(buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>())
            };
        }
        if result != ERROR_BUFFER_OVERFLOW || buffer_bytes == 0 {
            break;
        }
    }
    HashMap::new()
}

#[cfg(not(any(unix, windows)))]
fn collect_network_interfaces() -> HashMap<String, Vec<OsNetworkAddress>> {
    HashMap::new()
}

fn js_string_value(value: &str) -> crate::value::JSValue {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    crate::value::JSValue::string_ptr(ptr)
}

fn build_network_address_object(address: &OsNetworkAddress) -> *mut ObjectHeader {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let (shape_id, packed, field_count) = if address.family == "IPv6" {
        (
            OS_NETWORK_IPV6_SHAPE_ID,
            b"address\0netmask\0family\0mac\0internal\0cidr\0scopeid\0".as_slice(),
            7,
        )
    } else {
        (
            OS_NETWORK_IPV4_SHAPE_ID,
            b"address\0netmask\0family\0mac\0internal\0cidr\0".as_slice(),
            6,
        )
    };
    let obj =
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);

    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        0,
        js_string_value(&address.address),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        1,
        js_string_value(&address.netmask),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        2,
        js_string_value(address.family),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        3,
        js_string_value(&address.mac),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        4,
        JSValue::bool(address.internal),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        5,
        js_string_value(&address.cidr),
    );
    if let Some(scopeid) = address.scopeid {
        js_object_set_field(
            obj_handle.get_raw_mut_ptr(),
            6,
            JSValue::number(scopeid as f64),
        );
    }

    obj_handle.get_raw_mut_ptr()
}

/// Get network interfaces information
/// Returns an object with interface names as keys
#[no_mangle]
pub extern "C" fn js_os_network_interfaces() -> *mut ObjectHeader {
    use crate::object::{js_object_alloc, js_object_set_field_by_name};
    use crate::value::JSValue;

    let interfaces = collect_network_interfaces();
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = js_object_alloc(0, 0);
    let result_handle = scope.root_raw_mut_ptr(result);

    for (name, addresses) in interfaces {
        let arr = crate::array::js_array_alloc(addresses.len() as u32);
        let arr_handle = scope.root_raw_mut_ptr(arr);

        for address in addresses {
            let entry = build_network_address_object(&address);
            let entry_handle = scope.root_raw_mut_ptr(entry);
            let pushed = crate::array::js_array_push(
                arr_handle.get_raw_mut_ptr(),
                JSValue::pointer(entry_handle.get_raw_mut_ptr() as *const u8),
            );
            arr_handle.set_raw_mut_ptr(pushed);
        }

        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_handle = scope.root_string_ptr(key);
        js_object_set_field_by_name(
            result_handle.get_raw_mut_ptr(),
            key_handle.get_raw_const_ptr(),
            f64::from_bits(JSValue::pointer(arr_handle.get_raw_mut_ptr() as *const u8).bits()),
        );
    }

    result_handle.get_raw_mut_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mac_addresses_like_node() {
        assert_eq!(
            format_mac_address(&[0x00, 0x1a, 0x2b, 0xc3, 0xde, 0xff]),
            "00:1a:2b:c3:de:ff"
        );
        assert_eq!(format_mac_address(&[]), "00:00:00:00:00:00");
        assert_eq!(
            format_mac_address(&[0x00, 0x1a, 0x2b, 0xc3, 0xde]),
            "00:00:00:00:00:00"
        );
    }

    #[test]
    fn derives_ipv4_netmasks_from_windows_prefixes() {
        assert_eq!(ipv4_netmask_from_prefix(0), Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(
            ipv4_netmask_from_prefix(24),
            Ipv4Addr::new(255, 255, 255, 0)
        );
        assert_eq!(
            ipv4_netmask_from_prefix(32),
            Ipv4Addr::new(255, 255, 255, 255)
        );
        assert_eq!(
            ipv4_netmask_from_prefix(u8::MAX),
            Ipv4Addr::new(255, 255, 255, 255)
        );
    }

    #[test]
    fn derives_ipv6_netmasks_from_windows_prefixes() {
        assert_eq!(ipv6_netmask_from_prefix(0), Ipv6Addr::UNSPECIFIED);
        assert_eq!(
            ipv6_netmask_from_prefix(64),
            "ffff:ffff:ffff:ffff::".parse::<Ipv6Addr>().unwrap()
        );
        assert_eq!(
            ipv6_netmask_from_prefix(68),
            "ffff:ffff:ffff:ffff:f000::".parse::<Ipv6Addr>().unwrap()
        );
        assert_eq!(
            ipv6_netmask_from_prefix(128),
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"
                .parse::<Ipv6Addr>()
                .unwrap()
        );
        assert_eq!(
            ipv6_netmask_from_prefix(u8::MAX),
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"
                .parse::<Ipv6Addr>()
                .unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_live_network_interfaces() {
        let interfaces = collect_network_interfaces();
        assert!(
            !interfaces.is_empty(),
            "GetAdaptersAddresses should report at least the loopback adapter"
        );

        for (name, addresses) in interfaces {
            assert!(!name.is_empty());
            assert!(!addresses.is_empty());
            for entry in addresses {
                let prefix = match entry.family {
                    "IPv4" => prefix_len_ipv4(entry.netmask.parse().unwrap()),
                    "IPv6" => prefix_len_ipv6(entry.netmask.parse().unwrap()),
                    family => panic!("unexpected address family {family}"),
                };
                assert_eq!(entry.cidr, format!("{}/{prefix}", entry.address));
                assert_eq!(entry.scopeid.is_some(), entry.family == "IPv6");
                assert_eq!(entry.mac.len(), 17);
            }
        }
    }
}
