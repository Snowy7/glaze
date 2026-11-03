//! `system.stats`: CPU, memory, GPU, network and disk usage, sampled by one
//! shared timer for every subscriber, and not at all while nobody listens or
//! the host is held.

use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW,
};
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::core::{PCWSTR, w};

use crate::bridge::Target;

/// CPU and network usage are deltas, so the first sample needs a second one.
const WARMUP: Duration = Duration::from_millis(500);
/// Subscribers due this close together share one sample.
const COALESCE: Duration = Duration::from_millis(150);
/// Free space barely moves; don't ask the file system every second.
const DISK_REFRESH: Duration = Duration::from_secs(10);
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

fn read_counters() -> Counters {
    let (mut idle, mut kernel, mut user) = Default::default();
    // SAFETY: the out-pointers are valid FILETIMEs for the call.
    let _ = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
    Counters {
        at: Instant::now(),
        idle: ticks(idle),
        total: ticks(kernel) + ticks(user),
        net: net_totals(),
    }
}

fn net_totals() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table, which FreeMibTable releases
    // below; rows are read only while it is alive.
    unsafe {
        if GetIfTable2(&mut table) != ERROR_SUCCESS || table.is_null() {
            return None;
        }
        let rows =
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let totals = rows
            .iter()
            .filter(|row| {
                let flags = row.InterfaceAndOperStatusFlags._bitfield;
                let hardware = flags & 1 != 0;
                let filter = flags & 2 != 0;
                hardware
                    && !filter
                    && row.OperStatus == IfOperStatusUp
                    && row.Type != IF_TYPE_SOFTWARE_LOOPBACK
            })
            .fold((0, 0), |(rx, tx), row| {
                (rx + row.InOctets, tx + row.OutOctets)
            });
        FreeMibTable(table.cast());
        Some(totals)
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
        Some(Self { query, counter })
    }

    fn read(&mut self) -> Option<f64> {
        // SAFETY: two-call pattern: ask for the size, then fill a buffer of
        // that many bytes (aligned for the items); names point into it.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut bytes, mut count) = (0u32, 0u32);
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                None,
            );
            if status != PDH_MORE_DATA || bytes == 0 {
                return None;
            }
            let item = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buffer =
                vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (bytes as usize).div_ceil(item)];
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                Some(buffer.as_mut_ptr()),
            );
            if status != 0 {
                return None;
            }
            let items = buffer[..count as usize].iter().filter_map(|item| {
                let name = item.szName.to_string().ok()?;
                Some((name, item.FmtValue.Anonymous.doubleValue))
            });
            let named: Vec<(String, f64)> = items.collect();
            busiest_engine(named.iter().map(|(name, value)| (name.as_str(), *value)))
        }
    }
}

impl Drop for GpuCounters {
    fn drop(&mut self) {
        // SAFETY: we opened this query.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Reads the system; alive only while someone listens.
struct Sampler {
    last: Counters,
    gpu: Option<GpuCounters>,
    disks: Option<(Instant, Vec<Disk>)>,
}

impl Sampler {
    fn new() -> Self {
        Self {
            last: read_counters(),
            gpu: GpuCounters::open(),
            disks: None,
        }
    }

    fn sample(&mut self) -> SystemStats {
        let now = read_counters();
        let stats = SystemStats {
            cpu: cpu_percent(&self.last, &now),
            memory: memory(),
            gpu: self.gpu.as_mut().and_then(GpuCounters::read),
            net: net_rates(&self.last, &now),
            disks: Some(self.disks().to_vec()),
        };
        self.last = now;
        stats
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
    sub: u64,
    interval: Duration,
    due: Instant,
}

/// What a tick produced.
pub enum Delivery {
    /// The answer to a `system.stats` call.
    Reply(Target, SystemStats),
    /// A `system.stats` event for subscription `sub`.
    Event {
        instance: String,
        sub: u64,
        stats: SystemStats,
    },
}

/// Every `system.stats` request and subscription, driven by one timer.
#[derive(Default)]
pub struct Stats {
    sampler: Option<Sampler>,
    /// The sampler can't answer before this (see [`WARMUP`]).
    ready_at: Option<Instant>,
    subs: Vec<Sub>,
    once: Vec<Target>,
    next_sub: u64,
    held: bool,
}

impl Stats {
    fn wake(&mut self, now: Instant) {
        if self.sampler.is_none() && !self.held && !(self.subs.is_empty() && self.once.is_empty()) {
            self.sampler = Some(Sampler::new());
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

    /// Starts a subscription and returns its id.
    pub fn subscribe(&mut self, instance: &str, interval: Duration, now: Instant) -> u64 {
        self.next_sub += 1;
        self.subs.push(Sub {
            instance: instance.to_owned(),
            sub: self.next_sub,
            interval,
            due: now,
        });
        self.wake(now);
        self.next_sub
    }

    pub fn unsubscribe(&mut self, instance: &str, sub: u64) {
        self.subs
            .retain(|s| !(s.instance == instance && s.sub == sub));
        self.sleep_if_idle();
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
        let stats = sampler.sample();
        let mut out: Vec<Delivery> = self
            .once
            .drain(..)
            .map(|target| Delivery::Reply(target, stats.clone()))
            .collect();
        for sub in self.subs.iter_mut().filter(|s| s.due <= now + COALESCE) {
            sub.due = now + sub.interval;
            out.push(Delivery::Event {
                instance: sub.instance.clone(),
                sub: sub.sub,
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

    fn counters(at: Instant, idle: u64, total: u64, net: Option<(u64, u64)>) -> Counters {
        Counters {
            at,
            idle,
            total,
            net,
        }
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
        let a = stats.subscribe("a-1", Duration::from_secs(1), now);
        stats.subscribe("b-1", Duration::from_secs(5), now);
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

        stats.unsubscribe("a-1", a);
        stats.forget("b-1");
        assert_eq!(stats.deadline(), None);
        assert!(
            stats.sampler.is_none(),
            "the sampler goes away with the last listener"
        );
    }

    #[test]
    fn holding_pauses_everything() {
        let now = Instant::now();
        let mut stats = Stats::default();
        stats.subscribe("a-1", Duration::from_secs(1), now);
        stats.set_held(true, now);
        assert_eq!(stats.deadline(), None);
        stats.set_held(false, now);
        assert_eq!(stats.deadline(), Some(now + WARMUP));
    }
}
