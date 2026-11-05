//! GPU-engine busy time from cached raw PDH counters. Names are owned and matched across
//! counter insertion/reordering; stable samples need no allocation or hashing per instance.
use std::collections::HashMap;
use windows::Win32::System::Performance::{
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PDH_RAW_COUNTER, PDH_RAW_COUNTER_ITEM_W,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetRawCounterArrayW,
    PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

#[derive(Default)]
struct Row {
    name: Vec<u16>,
    engine: Option<usize>,
    previous: PDH_RAW_COUNTER,
    valid: bool,
}
#[derive(Default)]
struct Values {
    rows: Vec<Row>,
    engines: HashMap<Vec<u16>, usize>,
    totals: Vec<f64>,
}
impl Values {
    /// # Safety
    /// Every non-null name points into a live, complete PDH raw-counter array.
    unsafe fn update(&mut self, readings: &[PDH_RAW_COUNTER_ITEM_W]) -> Option<f64> {
        unsafe fn name(item: &PDH_RAW_COUNTER_ITEM_W) -> &[u16] {
            if item.szName.is_null() {
                &[][..]
            } else {
                // SAFETY: guaranteed by the caller for the duration of this update.
                unsafe { item.szName.as_wide() }
            }
        }
        // SAFETY: the caller guarantees all names remain valid throughout this update.
        unsafe {
            if self.rows.len() != readings.len()
                || !self
                    .rows
                    .iter()
                    .zip(readings)
                    .all(|(old, new)| old.name == name(new))
            {
                let mut old: HashMap<_, _> = self
                    .rows
                    .drain(..)
                    .map(|mut row| (std::mem::take(&mut row.name), row))
                    .collect();
                const LUID: [u16; 5] = [108, 117, 105, 100, 95];
                for item in readings {
                    let name = name(item);
                    let row = if let Some((name, mut row)) = old.remove_entry(name) {
                        row.name = name;
                        row
                    } else {
                        let engine = name.windows(5).position(|part| part == LUID).map(|at| {
                            let key = &name[at..];
                            if let Some(&id) = self.engines.get(key) {
                                id
                            } else {
                                let id = self.totals.len();
                                self.totals.push(0.0);
                                self.engines.insert(key.to_vec(), id);
                                id
                            }
                        });
                        Row {
                            name: name.to_vec(),
                            engine,
                            ..Default::default()
                        }
                    };
                    self.rows.push(row);
                }
            }
            self.totals.fill(0.0);
            let mut supported = false;
            for (row, item) in self.rows.iter_mut().zip(readings) {
                let raw = item.RawValue;
                let valid = !item.szName.is_null() && raw.CStatus <= 1 && raw.SecondValue > 0;
                supported |= valid;
                if valid
                    && row.valid
                    && let Some(engine) = row.engine
                {
                    // GPU Engine uses PERF_100NSEC_TIMER: busy-time delta / timestamp delta.
                    let busy = raw
                        .FirstValue
                        .checked_sub(row.previous.FirstValue)
                        .filter(|n| *n >= 0);
                    let elapsed = raw
                        .SecondValue
                        .checked_sub(row.previous.SecondValue)
                        .filter(|n| *n > 0);
                    if let (Some(busy), Some(elapsed)) = (busy, elapsed) {
                        self.totals[engine] +=
                            (100.0 * busy as f64 / elapsed as f64).clamp(0.0, 100.0);
                    }
                }
                row.previous = raw;
                row.valid = valid;
            }
            supported.then(|| {
                self.totals
                    .iter()
                    .copied()
                    .reduce(f64::max)
                    .unwrap_or(0.0)
                    .clamp(0.0, 100.0)
            })
        }
    }
}

pub(super) struct GpuCounters {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    buffer: Vec<PDH_RAW_COUNTER_ITEM_W>,
    values: Values,
}
impl GpuCounters {
    pub(super) fn open() -> Option<Self> {
        let (mut query, mut counter) = (PDH_HQUERY::default(), PDH_HCOUNTER::default());
        // SAFETY: valid out-pointers; the query is closed on failure or in Drop.
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            if PdhAddEnglishCounterW(
                query,
                w!(r"\GPU Engine(*)\Utilization Percentage"),
                0,
                &mut counter,
            ) != 0
                || PdhCollectQueryData(query) != 0
            {
                PdhCloseQuery(query);
                return None;
            }
        }
        let mut counters = Self {
            query,
            counter,
            buffer: Vec::new(),
            values: Values::default(),
        };
        let _ = counters.raw();
        Some(counters)
    }
    pub(super) fn read(&mut self) -> Option<f64> {
        // SAFETY: this object owns the query throughout collection and reading.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return None;
        }
        self.raw()
    }
    fn raw(&mut self) -> Option<f64> {
        let item_size = size_of::<PDH_RAW_COUNTER_ITEM_W>();
        let (mut bytes, mut count) = ((self.buffer.len() * item_size) as u32, 0);
        // SAFETY: the aligned buffer covers the requested byte length, including all names.
        unsafe {
            let mut status = PdhGetRawCounterArrayW(
                self.counter,
                &mut bytes,
                &mut count,
                (!self.buffer.is_empty()).then_some(self.buffer.as_mut_ptr()),
            );
            if status == PDH_MORE_DATA {
                self.buffer.resize(
                    (bytes as usize).div_ceil(item_size),
                    PDH_RAW_COUNTER_ITEM_W::default(),
                );
                status = PdhGetRawCounterArrayW(
                    self.counter,
                    &mut bytes,
                    &mut count,
                    Some(self.buffer.as_mut_ptr()),
                );
            }
            if status != 0 || count as usize > self.buffer.len() {
                return None;
            }
            self.values.update(&self.buffer[..count as usize])
        }
    }
}
impl Drop for GpuCounters {
    fn drop(&mut self) {
        // SAFETY: this object owns the query.
        unsafe { PdhCloseQuery(self.query) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{HSTRING, PWSTR};
    #[test]
    fn raw_rates_keep_identity_when_counters_reorder_appear_reset_or_become_invalid() {
        let names: Vec<_> = [
            "pid_1_luid_A_eng_0",
            "pid_2_luid_A_eng_0",
            "pid_3_luid_B_eng_0",
        ]
        .map(HSTRING::from)
        .into();
        let items = |values: &[(usize, i64, i64, u32)]| {
            values
                .iter()
                .map(|&(id, value, at, status)| PDH_RAW_COUNTER_ITEM_W {
                    szName: PWSTR(names[id].as_ptr().cast_mut()),
                    RawValue: PDH_RAW_COUNTER {
                        CStatus: status,
                        FirstValue: value,
                        SecondValue: at,
                        ..Default::default()
                    },
                })
                .collect::<Vec<_>>()
        };
        let mut state = Values::default();
        // SAFETY: all names are backed by HSTRINGs held until the test ends.
        unsafe {
            assert_eq!(
                state.update(&items(&[(0, 10, 100, 0), (1, 20, 100, 0)])),
                Some(0.0)
            );
            assert_eq!(
                state.update(&items(&[(1, 40, 200, 0), (0, 20, 200, 0)])),
                Some(30.0)
            );
            assert_eq!(
                state.update(&items(&[(2, 900, 300, 0), (0, 30, 300, 0)])),
                Some(10.0)
            );
            assert_eq!(
                state.update(&items(&[(0, 1, 400, 0), (2, 920, 400, 0)])),
                Some(20.0)
            );
            assert_eq!(state.update(&items(&[(0, 2, 500, 2)])), None);
            assert_eq!(state.update(&items(&[(0, 9, 600, 0)])), Some(0.0));
        }
    }
    #[test]
    #[ignore = "compares real GPU reads with the Windows formatter under the desktop lock"]
    fn raw_gpu_rates_match_the_windows_formatter() {
        use windows::Win32::System::Performance::{
            PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PdhGetFormattedCounterArrayW,
        };
        let mut counters = GpuCounters::open().expect("GPU Engine counters");
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let raw = counters.read().unwrap();
            let (mut bytes, mut count) = (0, 0);
            // SAFETY: documented size/fill pattern on the same collected query as the raw read.
            unsafe {
                assert_eq!(
                    PdhGetFormattedCounterArrayW(
                        counters.counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        None
                    ),
                    PDH_MORE_DATA
                );
                let mut data = vec![
                    PDH_FMT_COUNTERVALUE_ITEM_W::default();
                    (bytes as usize)
                        .div_ceil(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>())
                ];
                assert_eq!(
                    PdhGetFormattedCounterArrayW(
                        counters.counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        Some(data.as_mut_ptr())
                    ),
                    0
                );
                let values: Vec<_> = data[..count as usize]
                    .iter()
                    .filter(|item| item.FmtValue.CStatus <= 1)
                    .map(|item| {
                        (
                            item.szName.to_string().unwrap(),
                            item.FmtValue.Anonymous.doubleValue,
                        )
                    })
                    .collect();
                let formatted = super::super::busiest_engine(
                    values.iter().map(|(name, value)| (name.as_str(), *value)),
                )
                .unwrap()
                .clamp(0.0, 100.0);
                println!("raw={raw:.6} formatted={formatted:.6}");
                assert!((raw - formatted).abs() < 0.0001);
            }
        }
    }
}
