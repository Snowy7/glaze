//! `system.stats`: CPU, memory, GPU, network and disk usage, battery, uptime and the
//! connection, sampled by one shared timer for every subscriber, and not at all while
//! nobody listens or the host is held.

use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    CancelMibChangeNotify2, FreeMibTable, GetIfEntry2, GetIfTable2Ex, IF_TYPE_ETHERNET_CSMACD,
    IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
    MibIfTableRaw, NotifyIpInterfaceChange,
};
use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUp, MediaConnectStateConnected};
use windows::Win32::NetworkManagement::WiFi::{
    WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST, WlanCloseHandle, WlanEnumInterfaces,
    WlanFreeMemory, WlanOpenHandle, WlanQueryInterface, wlan_interface_state_connected,
    wlan_intf_opcode_current_connection,
};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW,
};
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::core::{PCWSTR, w};

use crate::bridge::Target;

/// CPU and network usage are deltas, so the first sample needs a second one.
const WARMUP: Duration = Duration::from_millis(500);
/// Subscribers due this close together share one sample.
const COALESCE: Duration = Duration::from_millis(150);
/// Free space barely moves; don't ask the file system every second.
const DISK_REFRESH: Duration = Duration::from_secs(10);
/// Nor does the Wi-Fi connection; asking the WLAN service costs an RPC round trip.
const WIFI_REFRESH: Duration = Duration::from_secs(5);
/// With no WLAN service (or no Wi-Fi adapter) opening it fails; don't retry every sample.
const WLAN_RETRY: Duration = Duration::from_secs(60);
const DRIVE_FIXED: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStats {
    /// Percent of the whole machine, 0–100.
    pub cpu: f64,
    pub memory: Usage,
    /// The busiest GPU engine, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<Net>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disks: Option<Vec<Disk>>,
    /// Absent on machines without a battery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<Battery>,
    /// Seconds since boot.
    pub uptime: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<Network>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Battery {
    /// 0–100.
    pub percent: u8,
    pub charging: bool,
}

/// How the machine is connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Network {
    pub kind: LinkKind,
    /// The Wi-Fi network's name (SSID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wi-Fi signal quality, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    Wifi,
    Ethernet,
    None,
}

/// A Wi-Fi connection as the WLAN service reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Wifi {
    ssid: String,
    signal: u8,
}

/// Bytes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Usage {
    pub used: u64,
    pub total: u64,
}

/// Bytes per second over physical interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Net {
    pub up: f64,
    pub down: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Disk {
    pub name: String,
    pub used: u64,
    pub total: u64,
}

/// One reading of the cumulative counters.
#[derive(Debug, Clone, Copy)]
struct Counters {
    at: Instant,
    idle: u64,
    /// Kernel (which includes idle) plus user time.
    total: u64,
    /// Received and sent bytes, if the interface table could be read.
    net: Option<(u64, u64)>,
    /// A wired adapter is connected (from the same interface table).
    ethernet: bool,
}

fn cpu_percent(before: &Counters, after: &Counters) -> f64 {
    let total = after.total.saturating_sub(before.total);
    let idle = after.idle.saturating_sub(before.idle);
    if total == 0 {
        return 0.0;
    }
    (100.0 * (1.0 - idle as f64 / total as f64)).clamp(0.0, 100.0)
}

fn net_rates(before: &Counters, after: &Counters) -> Option<Net> {
    let ((rx0, tx0), (rx1, tx1)) = (before.net?, after.net?);
    let secs = (after.at - before.at).as_secs_f64();
    // A counter that went backwards (an interface went away) reads as 0.
    let rate = |a: u64, b: u64| {
        if secs > 0.0 {
            b.saturating_sub(a) as f64 / secs
        } else {
            0.0
        }
    };
    Some(Net {
        up: rate(tx0, tx1),
        down: rate(rx0, rx1),
    })
}

/// The busiest engine from "GPU Engine" instances named like
/// `pid_1234_luid_0x0_0x1_phys_0_eng_0_engtype_3D`: sums each engine
/// (`luid_…`) over processes, then takes the maximum, as Task Manager does.
#[cfg(test)]
fn busiest_engine<'a>(items: impl IntoIterator<Item = (&'a str, f64)>) -> Option<f64> {
    let mut engines: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for (name, value) in items {
        if let Some(at) = name.find("luid_") {
            *engines.entry(&name[at..]).or_default() += value;
        }
    }
    engines
        .into_values()
        .reduce(f64::max)
        .map(|v| v.clamp(0.0, 100.0))
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn read_counters(interfaces: Option<&mut InterfaceCounters>) -> Counters {
    let (mut idle, mut kernel, mut user) = Default::default();
    // SAFETY: the out-pointers are valid FILETIMEs for the call.
    let _ = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
    let table = interfaces.and_then(InterfaceCounters::sample);
    Counters {
        at: Instant::now(),
        idle: ticks(idle),
        total: ticks(kernel) + ticks(user),
        net: table.map(|t| (t.rx, t.tx)),
        ethernet: table.is_some_and(|t| t.ethernet),
    }
}

/// What one pass over the interface table yields.
#[derive(Debug, Clone, Copy)]
struct Interfaces {
    rx: u64,
    tx: u64,
    ethernet: bool,
}

/// Enumerate physical adapters only when Windows reports a topology change. Counter reads
/// then query the cached adapter indices, avoiding the full IP helper table on every tick.
struct InterfaceCounters {
    indices: Vec<u32>,
    changed: Box<std::sync::atomic::AtomicBool>,
    notification: Option<HANDLE>,
}
impl InterfaceCounters {
    fn new() -> Self {
        let changed = Box::new(std::sync::atomic::AtomicBool::new(true));
        let mut handle = HANDLE::default();
        // SAFETY: the boxed flag has a stable address until CancelMibChangeNotify2 drains callbacks.
        let status = unsafe {
            NotifyIpInterfaceChange(
                windows::Win32::Networking::WinSock::AF_UNSPEC,
                Some(interface_changed),
                Some(std::ptr::from_ref(changed.as_ref()).cast()),
                false,
                &mut handle,
            )
        };
        Self {
            indices: Vec::new(),
            changed,
            notification: (status == ERROR_SUCCESS).then_some(handle),
        }
    }
    fn sample(&mut self) -> Option<Interfaces> {
        use std::sync::atomic::Ordering;
        if self.changed.swap(false, Ordering::AcqRel) || self.notification.is_none() {
            let mut table = std::ptr::null_mut();
            // SAFETY: the allocated table is released before returning, and its rows stay in bounds.
            unsafe {
                if GetIfTable2Ex(MibIfTableRaw, &mut table) != ERROR_SUCCESS || table.is_null() {
                    self.changed.store(true, Ordering::Release);
                    return None;
                }
                let rows = std::slice::from_raw_parts(
                    (*table).Table.as_ptr(),
                    (*table).NumEntries as usize,
                );
                self.indices = rows
                    .iter()
                    .filter(|row| {
                        row.InterfaceAndOperStatusFlags._bitfield & 3 == 1
                            && row.Type != IF_TYPE_SOFTWARE_LOOPBACK
                    })
                    .map(|row| row.InterfaceIndex)
                    .collect();
                FreeMibTable(table.cast());
            }
        }
        let mut found = Interfaces {
            rx: 0,
            tx: 0,
            ethernet: false,
        };
        for &index in &self.indices {
            let mut row = MIB_IF_ROW2 {
                InterfaceIndex: index,
                ..Default::default()
            };
            // SAFETY: GetIfEntry2 fills this owned row identified by its cached interface index.
            if unsafe { GetIfEntry2(&mut row) } != ERROR_SUCCESS {
                self.changed.store(true, Ordering::Release);
                continue;
            }
            if row.OperStatus != IfOperStatusUp {
                continue;
            }
            found.rx += row.InOctets;
            found.tx += row.OutOctets;
            found.ethernet |= row.Type == IF_TYPE_ETHERNET_CSMACD
                && row.MediaConnectState == MediaConnectStateConnected;
        }
        Some(found)
    }
}
impl Drop for InterfaceCounters {
    fn drop(&mut self) {
        if let Some(handle) = self.notification {
            // SAFETY: cancellation waits for pending callbacks before the boxed flag is dropped.
            let _ = unsafe { CancelMibChangeNotify2(handle) };
        }
    }
}
unsafe extern "system" fn interface_changed(
    context: *const std::ffi::c_void,
    _: *const MIB_IPINTERFACE_ROW,
    _: MIB_NOTIFICATION_TYPE,
) {
    // SAFETY: registered with the address of InterfaceCounters' boxed flag; cancellation joins us.
    unsafe { context.cast::<std::sync::atomic::AtomicBool>().as_ref() }
        .unwrap()
        .store(true, std::sync::atomic::Ordering::Release);
}

/// The battery from `GetSystemPowerStatus` fields: `None` when there isn't one (flag 128) or
/// the system can't tell (flag 255, percent 255).
fn battery_from(flag: u8, percent: u8) -> Option<Battery> {
    if flag & 128 != 0 || flag == 255 || percent > 100 {
        return None;
    }
    Some(Battery {
        percent,
        charging: flag & 8 != 0,
    })
}

fn battery() -> Option<Battery> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a valid out-parameter for the call.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    battery_from(status.BatteryFlag, status.BatteryLifePercent)
}

/// Seconds since boot.
fn uptime() -> f64 {
    // SAFETY: no preconditions.
    (unsafe { GetTickCount64() }) as f64 / 1000.0
}

/// The connection to report: a wired link wins (Windows routes over it first), then Wi-Fi.
fn classify(wifi: Option<&Wifi>, ethernet: bool) -> Network {
    match (ethernet, wifi) {
        (true, _) => Network {
            kind: LinkKind::Ethernet,
            name: None,
            signal: None,
        },
        (false, Some(wifi)) => Network {
            kind: LinkKind::Wifi,
            name: Some(wifi.ssid.clone()),
            signal: Some(wifi.signal),
        },
        (false, None) => Network {
            kind: LinkKind::None,
            name: None,
            signal: None,
        },
    }
}

/// An SSID is up to 32 bytes, usually UTF-8.
fn ssid_text(length: u32, bytes: &[u8; 32]) -> String {
    let len = (length as usize).min(bytes.len());
    String::from_utf8_lossy(&bytes[..len]).into_owned()
}

/// The WLAN service, open only while someone is sampling.
struct Wlan {
    client: Option<HANDLE>,
    /// When opening last failed (no service, no adapter).
    failed_at: Option<Instant>,
}

impl Wlan {
    fn new() -> Self {
        Self {
            client: None,
            failed_at: None,
        }
    }

    fn open(&mut self) -> Option<HANDLE> {
        if self.client.is_none() && self.failed_at.is_none_or(|at| at.elapsed() >= WLAN_RETRY) {
            let (mut version, mut client) = (0u32, HANDLE::default());
            // SAFETY: valid out-pointers; the handle is closed in `drop`.
            let status = unsafe { WlanOpenHandle(2, None, &mut version, &mut client) };
            if status == 0 {
                self.client = Some(client);
                self.failed_at = None;
            } else {
                self.failed_at = Some(Instant::now());
            }
        }
        self.client
    }

    /// The first connected Wi-Fi interface, if any.
    fn connection(&mut self) -> Option<Wifi> {
        let client = self.open()?;
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        // SAFETY: `client` is an open WLAN handle; the list WLAN allocates is read while alive
        // and freed with WlanFreeMemory below.
        unsafe {
            if WlanEnumInterfaces(client, None, &mut list) != 0 || list.is_null() {
                // The service went away (or the adapter was removed): reopen next time.
                self.close();
                return None;
            }
            let interfaces = std::slice::from_raw_parts(
                (*list).InterfaceInfo.as_ptr(),
                (*list).dwNumberOfItems as usize,
            );
            let mut found = None;
            for interface in interfaces
                .iter()
                .filter(|i| i.isState == wlan_interface_state_connected)
            {
                let (mut size, mut data) = (0u32, std::ptr::null_mut());
                let status = WlanQueryInterface(
                    client,
                    &interface.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                );
                if status != 0 || data.is_null() {
                    continue;
                }
                let attributes = &*data.cast::<WLAN_CONNECTION_ATTRIBUTES>();
                let association = &attributes.wlanAssociationAttributes;
                found = Some(Wifi {
                    ssid: ssid_text(
                        association.dot11Ssid.uSSIDLength,
                        &association.dot11Ssid.ucSSID,
                    ),
                    signal: association.wlanSignalQuality.min(100) as u8,
                });
                WlanFreeMemory(data.cast_const());
                break;
            }
            WlanFreeMemory(list.cast_const().cast());
            found
        }
    }

    fn close(&mut self) {
        if let Some(client) = self.client.take() {
            // SAFETY: a handle WlanOpenHandle gave us, closed once.
            unsafe { WlanCloseHandle(client, None) };
        }
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        self.close();
    }
}

fn memory() -> Usage {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` has dwLength set and outlives the call.
    let _ = unsafe { GlobalMemoryStatusEx(&mut status) };
    Usage {
        used: status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
        total: status.ullTotalPhys,
    }
}

fn fixed_disks() -> Vec<Disk> {
    let mut buffer = [0u16; 512];
    // SAFETY: the buffer outlives the call; the result is the length written.
    let len = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) } as usize;
    buffer[..len.min(buffer.len())]
        .split(|&c| c == 0)
        .filter(|root| !root.is_empty())
        .filter_map(|root| {
            let mut root = root.to_vec();
            root.push(0);
            let path = PCWSTR(root.as_ptr());
            let (mut total, mut free) = (0u64, 0u64);
            // SAFETY: `path` is a NUL-terminated drive root that outlives the calls.
            unsafe {
                if GetDriveTypeW(path) != DRIVE_FIXED {
                    return None;
                }
                GetDiskFreeSpaceExW(path, None, Some(&mut total), Some(&mut free)).ok()?;
            }
            let name = String::from_utf16_lossy(&root[..root.len() - 1]);
            Some(Disk {
                name: name.trim_end_matches('\\').to_owned(),
                used: total.saturating_sub(free),
                total,
            })
        })
        .collect()
}

/// The "GPU Engine" utilization counters, read through PDH.
struct GpuCounters {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    buffer: Vec<PDH_FMT_COUNTERVALUE_ITEM_W>,
    engines: std::collections::HashMap<Vec<u16>, f64>,
}

impl GpuCounters {
    fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        let mut counter = PDH_HCOUNTER::default();
        // SAFETY: out-pointers are valid; the query is closed on drop (or below on failure).
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let path = w!(r"\GPU Engine(*)\Utilization Percentage");
            if PdhAddEnglishCounterW(query, path, 0, &mut counter) != 0
                || PdhCollectQueryData(query) != 0
            {
                PdhCloseQuery(query);
                return None;
            }
        }
        Some(Self {
            query,
            counter,
            buffer: Vec::new(),
            engines: std::collections::HashMap::new(),
        })
    }

    fn read(&mut self) -> Option<f64> {
        // SAFETY: two-call pattern: ask for the size, then fill a buffer of
        // that many bytes (aligned for the items); names point into it.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let item_size = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut bytes = (self.buffer.len() * item_size) as u32;
            let mut count = 0;
            let mut status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                (!self.buffer.is_empty()).then_some(self.buffer.as_mut_ptr()),
            );
            if status == PDH_MORE_DATA {
                self.buffer.resize(
                    (bytes as usize).div_ceil(item_size),
                    PDH_FMT_COUNTERVALUE_ITEM_W::default(),
                );
                status = PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut bytes,
                    &mut count,
                    Some(self.buffer.as_mut_ptr()),
                );
            }
            if status != 0 || count as usize > self.buffer.len() {
                return None;
            }
            self.engines.values_mut().for_each(|n| *n = 0.0);
            const LUID: [u16; 5] = [108, 117, 105, 100, 95];
            let mut valid = false;
            for item in &self.buffer[..count as usize] {
                if item.FmtValue.CStatus > 1 || item.szName.is_null() {
                    continue;
                }
                let value = item.FmtValue.Anonymous.doubleValue;
                if !value.is_finite() {
                    continue;
                }
                valid = true;
                if value == 0.0 {
                    continue;
                }
                let name = item.szName.as_wide();
                let Some(at) = name.windows(5).position(|s| s == LUID) else {
                    continue;
                };
                let key = &name[at..];
                if let Some(total) = self.engines.get_mut(key) {
                    *total += value;
                } else {
                    self.engines.insert(key.to_vec(), value);
                }
            }
            self.engines
                .values()
                .copied()
                .reduce(f64::max)
                .map(|n| n.clamp(0.0, 100.0))
                .filter(|_| valid)
                .or_else(|| valid.then_some(0.0))
        }
    }
}

impl Drop for GpuCounters {
    fn drop(&mut self) {
        // SAFETY: we opened this query.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Expensive fields requested by visible consumers. CPU and memory are always cheap snapshots.
#[derive(Debug, Clone, Copy)]
pub struct SampleFields {
    pub gpu: bool,
    pub disks: bool,
    pub net: bool,
    pub battery: bool,
    pub network: bool,
}
impl SampleFields {
    pub const ALL: Self = Self {
        gpu: true,
        disks: true,
        net: true,
        battery: true,
        network: true,
    };
    fn union(self, other: Self) -> Self {
        Self {
            gpu: self.gpu || other.gpu,
            disks: self.disks || other.disks,
            net: self.net || other.net,
            battery: self.battery || other.battery,
            network: self.network || other.network,
        }
    }
}

/// Reads the system; alive only while someone listens.
struct Sampler {
    last: Counters,
    interfaces: Option<InterfaceCounters>,
    gpu: Option<GpuCounters>,
    disks: Option<(Instant, Vec<Disk>)>,
    wlan: Wlan,
    wifi: Option<(Instant, Option<Wifi>)>,
}

impl Sampler {
    fn new() -> Self {
        Self {
            last: read_counters(None),
            interfaces: None,
            gpu: None,
            disks: None,
            wlan: Wlan::new(),
            wifi: None,
        }
    }

    fn sample(&mut self, fields: SampleFields) -> SystemStats {
        let interfaces = if fields.net || fields.network {
            Some(self.interfaces.get_or_insert_with(InterfaceCounters::new))
        } else {
            None
        };
        let now = read_counters(interfaces);
        if fields.gpu && self.gpu.is_none() {
            self.gpu = GpuCounters::open();
        }
        if !fields.gpu {
            self.gpu = None;
        }
        let stats = SystemStats {
            cpu: cpu_percent(&self.last, &now),
            memory: memory(),
            gpu: fields
                .gpu
                .then(|| self.gpu.as_mut().and_then(GpuCounters::read))
                .flatten(),
            net: fields.net.then(|| net_rates(&self.last, &now)).flatten(),
            disks: fields.disks.then(|| self.disks().to_vec()),
            battery: fields.battery.then(battery).flatten(),
            uptime: uptime(),
            network: fields.network.then(|| self.network(now.ethernet)),
        };
        self.last = now;
        stats
    }

    /// The connection. A wired link needs no Wi-Fi query at all.
    fn network(&mut self, ethernet: bool) -> Network {
        if ethernet {
            return classify(None, true);
        }
        let fresh = self
            .wifi
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < WIFI_REFRESH);
        if !fresh {
            self.wifi = Some((Instant::now(), self.wlan.connection()));
        }
        let wifi = self.wifi.as_ref().and_then(|(_, wifi)| wifi.as_ref());
        classify(wifi, false)
    }

    fn disks(&mut self) -> &[Disk] {
        let fresh = self
            .disks
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < DISK_REFRESH);
        if !fresh {
            self.disks = Some((Instant::now(), fixed_disks()));
        }
        &self.disks.as_ref().expect("just filled").1
    }
}

struct Sub {
    instance: String,
    /// The widget's own id for the subscription.
    sub: String,
    interval: Duration,
    fields: SampleFields,
    due: Instant,
}

/// What a tick produced.
pub enum Delivery {
    /// The answer to a `system.stats` call.
    Reply(Target, SystemStats),
    /// A sample for subscription `sub`.
    Event {
        instance: String,
        sub: String,
        stats: SystemStats,
    },
}

/// Every `system.stats` request and `system` subscription, driven by one timer.
#[derive(Default)]
pub struct Stats {
    sampler: Option<Sampler>,
    /// The sampler can't answer before this (see [`WARMUP`]).
    ready_at: Option<Instant>,
    subs: Vec<Sub>,
    once: Vec<Target>,
    held: bool,
}

impl Stats {
    fn wake(&mut self, now: Instant) {
        if self.sampler.is_none() && !self.held && !(self.subs.is_empty() && self.once.is_empty()) {
            let mut sampler = Sampler::new();
            if !self.once.is_empty() || self.subs.iter().any(|sub| sub.fields.gpu) {
                sampler.gpu = GpuCounters::open();
            }
            if !self.once.is_empty()
                || self
                    .subs
                    .iter()
                    .any(|sub| sub.fields.net || sub.fields.network)
            {
                let mut interfaces = InterfaceCounters::new();
                let initial = read_counters(Some(&mut interfaces));
                sampler.last = initial;
                sampler.interfaces = Some(interfaces);
            }
            self.sampler = Some(sampler);
            self.ready_at = Some(now + WARMUP);
        }
    }

    fn sleep_if_idle(&mut self) {
        if self.held || (self.subs.is_empty() && self.once.is_empty()) {
            self.sampler = None;
            self.ready_at = None;
        }
    }

    /// Answers a `system.stats` call at the next sample.
    pub fn request(&mut self, target: Target, now: Instant) {
        self.once.push(target);
        self.wake(now);
    }

    /// Starts (or replaces) subscription `sub`: a sample right away, then
    /// one per `interval`.
    pub fn subscribe(&mut self, instance: &str, sub: &str, interval: Duration, now: Instant) {
        self.subscribe_fields(instance, sub, interval, now, SampleFields::ALL);
    }

    /// Subscribe only to fields the view actually displays; hidden disk/connection fields do no I/O.
    pub fn subscribe_fields(
        &mut self,
        instance: &str,
        sub: &str,
        interval: Duration,
        now: Instant,
        fields: SampleFields,
    ) {
        self.unsubscribe(instance, sub);
        self.subs.push(Sub {
            instance: instance.to_owned(),
            sub: sub.to_owned(),
            interval,
            fields,
            due: now,
        });
        self.wake(now);
    }

    pub fn unsubscribe(&mut self, instance: &str, sub: &str) {
        self.subs
            .retain(|s| !(s.instance == instance && s.sub == sub));
        self.sleep_if_idle();
    }

    /// Reopen device-dependent caches after Windows reports a device change.
    pub fn refresh_devices(&mut self) {
        if let Some(sampler) = &mut self.sampler {
            sampler.gpu = None;
            sampler.disks = None;
            sampler.wifi = None;
        }
    }
    /// Makes an instance's subscriptions due now (it just became visible).
    pub fn refresh(&mut self, instance: &str, now: Instant) {
        for sub in self.subs.iter_mut().filter(|s| s.instance == instance) {
            sub.due = now;
        }
    }

    /// Forgets everything an instance asked for (it reloaded or went away).
    pub fn forget(&mut self, instance: &str) {
        self.subs.retain(|s| s.instance != instance);
        self.once.retain(|t| t.instance != instance);
        self.sleep_if_idle();
    }

    /// While held nothing is sampled; subscriptions resume on release.
    pub fn set_held(&mut self, held: bool, now: Instant) {
        self.held = held;
        self.sleep_if_idle();
        self.wake(now);
    }

    pub fn deadline(&self) -> Option<Instant> {
        let ready = self.ready_at?;
        let first = self.once.first().map(|_| ready);
        let next = first
            .into_iter()
            .chain(self.subs.iter().map(|s| s.due))
            .min()?;
        Some(next.max(ready))
    }

    /// Samples once if anything is due, and hands out the results.
    pub fn tick(&mut self, now: Instant) -> Vec<Delivery> {
        if self.deadline().is_none_or(|due| now < due) {
            return Vec::new();
        }
        let Some(sampler) = &mut self.sampler else {
            return Vec::new();
        };
        let fields = if self.once.is_empty() {
            self.subs
                .iter()
                .filter(|s| s.due <= now + COALESCE)
                .map(|s| s.fields)
                .reduce(SampleFields::union)
        } else {
            Some(SampleFields::ALL)
        }
        .unwrap_or(SampleFields::ALL);
        let stats = sampler.sample(fields);
        let mut out: Vec<Delivery> = self
            .once
            .drain(..)
            .map(|target| Delivery::Reply(target, stats.clone()))
            .collect();
        for sub in self.subs.iter_mut().filter(|s| s.due <= now + COALESCE) {
            sub.due = now + sub.interval;
            out.push(Delivery::Event {
                instance: sub.instance.clone(),
                sub: sub.sub.clone(),
                stats: stats.clone(),
            });
        }
        self.sleep_if_idle();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "profiles the real OS samplers"]
    fn profile_sampler_cost() {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let busy = || {
            let (mut a, mut b, mut k, mut u) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            // SAFETY: own-process query into valid local FILETIMEs.
            unsafe {
                GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u).unwrap();
            }
            ticks(k) + ticks(u)
        };
        let mut sampler = Sampler::new();
        let _ = sampler.sample(SampleFields::ALL);
        for part in 0..5 {
            let start = Instant::now();
            let before = busy();
            for _ in 0..500 {
                match part {
                    0 => {
                        let _ = read_counters(sampler.interfaces.as_mut());
                    }
                    1 => {
                        let _ = memory();
                    }
                    2 => {
                        let _ = sampler.gpu.as_mut().and_then(GpuCounters::read);
                    }
                    3 => {
                        let _ = sampler.disks();
                    }
                    _ => {
                        let _ = sampler.network(false);
                        let _ = battery();
                    }
                }
            }
            println!(
                "sampler part={part} wall_us={} cpu_us={}",
                start.elapsed().as_micros() / 500,
                (busy() - before) / 5000
            );
        }
    }

    #[test]
    fn native_views_can_omit_expensive_fields_they_do_not_display() {
        let mut stats = Stats::default();
        let start = Instant::now();
        stats.subscribe_fields(
            "system-1",
            "system",
            Duration::from_secs(2),
            start,
            SampleFields {
                gpu: false,
                disks: false,
                net: false,
                battery: false,
                network: false,
            },
        );
        let out = stats.tick(start + WARMUP);
        let Delivery::Event {
            stats: snapshot, ..
        } = &out[0]
        else {
            panic!("subscription result")
        };
        assert!(
            snapshot.disks.is_none()
                && snapshot.gpu.is_none()
                && snapshot.net.is_none()
                && snapshot.network.is_none()
        );
        assert!(snapshot.memory.total > 0);
        stats.forget("system-1");
        assert!(stats.deadline().is_none());
    }

    fn counters(at: Instant, idle: u64, total: u64, net: Option<(u64, u64)>) -> Counters {
        Counters {
            at,
            idle,
            total,
            net,
            ethernet: false,
        }
    }

    #[test]
    fn battery_is_absent_without_one_and_reads_the_charging_flag() {
        assert_eq!(battery_from(128, 255), None, "no system battery");
        assert_eq!(battery_from(255, 255), None, "unknown status");
        assert_eq!(battery_from(1, 255), None, "unknown charge");
        assert_eq!(
            battery_from(1, 80),
            Some(Battery {
                percent: 80,
                charging: false
            })
        );
        assert_eq!(
            battery_from(1 | 8, 42),
            Some(Battery {
                percent: 42,
                charging: true
            })
        );
    }

    #[test]
    fn the_connection_prefers_a_wired_link_then_wifi() {
        let home = Wifi {
            ssid: "Home".into(),
            signal: 71,
        };
        assert_eq!(classify(Some(&home), true).kind, LinkKind::Ethernet);
        assert_eq!(classify(None, true).name, None);
        let wifi = classify(Some(&home), false);
        assert_eq!(
            (wifi.kind, wifi.name.as_deref(), wifi.signal),
            (LinkKind::Wifi, Some("Home"), Some(71))
        );
        assert_eq!(classify(None, false).kind, LinkKind::None);
        // On the wire: kind is lowercase, and unset fields are absent.
        let json = serde_json::to_value(classify(None, false)).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "none" }));
    }

    #[test]
    fn ssids_are_length_prefixed_and_may_be_unicode() {
        let mut bytes = [0u8; 32];
        let name = "Café".as_bytes(); // 5 bytes
        bytes[..name.len()].copy_from_slice(name);
        assert_eq!(ssid_text(5, &bytes), "Café");
        assert_eq!(ssid_text(0, &bytes), "");
        assert_eq!(
            ssid_text(99, &bytes).len(),
            32,
            "a bad length is clamped to the buffer"
        );
    }

    #[test]
    fn cpu_is_the_busy_share_of_the_delta() {
        let t = Instant::now();
        let before = counters(t, 1_000, 2_000, None);
        assert_eq!(cpu_percent(&before, &counters(t, 1_750, 3_000, None)), 25.0);
        assert_eq!(cpu_percent(&before, &before), 0.0);
    }

    #[test]
    fn net_is_bytes_per_second_and_survives_resets() {
        let t = Instant::now();
        let before = counters(t, 0, 0, Some((1_000, 500)));
        let after = counters(t + Duration::from_secs(2), 0, 0, Some((5_000, 300)));
        assert_eq!(
            net_rates(&before, &after),
            Some(Net {
                up: 0.0,
                down: 2_000.0
            })
        );
        assert_eq!(net_rates(&before, &counters(t, 0, 0, None)), None);
    }

    #[test]
    fn gpu_takes_the_busiest_engine_summed_over_processes() {
        let items = [
            ("pid_1_luid_0x0_0xA_phys_0_eng_0_engtype_3D", 30.0),
            ("pid_2_luid_0x0_0xA_phys_0_eng_0_engtype_3D", 25.0),
            ("pid_1_luid_0x0_0xA_phys_0_eng_3_engtype_VideoDecode", 40.0),
            ("pid_3_luid_0x0_0xB_phys_0_eng_0_engtype_3D", 70.0),
        ];
        assert_eq!(busiest_engine(items), Some(70.0));
        assert_eq!(
            busiest_engine([("pid_1_luid_x", 80.0), ("pid_2_luid_x", 50.0)]),
            Some(100.0)
        );
        assert_eq!(busiest_engine([]), None);
    }

    fn target(instance: &str) -> Target {
        Target {
            instance: instance.into(),
            id: 1.into(),
        }
    }

    #[test]
    fn one_timer_serves_everyone_and_stops_when_idle() {
        let now = Instant::now();
        let mut stats = Stats::default();
        assert_eq!(stats.deadline(), None, "nothing to do: no timer");
        stats.subscribe("a-1", "s1", Duration::from_secs(1), now);
        stats.subscribe("b-1", "s1", Duration::from_secs(5), now);
        stats.request(target("c-1"), now);
        assert_eq!(stats.deadline(), Some(now + WARMUP));
        assert!(stats.tick(now).is_empty(), "not warm yet");

        let first = stats.tick(now + WARMUP);
        assert_eq!(
            first.len(),
            3,
            "one sample for the reply and both subscribers"
        );
        assert_eq!(
            stats.deadline(),
            Some(now + WARMUP + Duration::from_secs(1))
        );

        stats.unsubscribe("a-1", "s1");
        stats.forget("b-1");
        assert_eq!(stats.deadline(), None);
        assert!(
            stats.sampler.is_none(),
            "the sampler goes away with the last listener"
        );
    }

    #[test]
    fn subscriptions_are_per_instance_and_refresh_on_demand() {
        let now = Instant::now();
        let mut stats = Stats::default();
        stats.subscribe("a-1", "s1", Duration::from_secs(5), now);
        stats.subscribe("a-1", "s1", Duration::from_secs(5), now); // replaces, not doubles
        stats.subscribe("b-1", "s1", Duration::from_secs(5), now);
        assert_eq!(stats.tick(now + WARMUP).len(), 2);
        let later = now + WARMUP + Duration::from_secs(1);
        assert!(stats.tick(later).is_empty());
        stats.refresh("a-1", later);
        let refreshed = stats.tick(later);
        assert!(
            matches!(refreshed.as_slice(), [Delivery::Event { instance, .. }] if instance == "a-1")
        );
    }

    #[test]
    fn holding_pauses_everything() {
        let now = Instant::now();
        let mut stats = Stats::default();
        stats.subscribe("a-1", "s1", Duration::from_secs(1), now);
        stats.set_held(true, now);
        assert_eq!(stats.deadline(), None);
        stats.set_held(false, now);
        assert_eq!(stats.deadline(), Some(now + WARMUP));
    }
}
