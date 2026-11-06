//! Sparse, file-backed scratch space for a volume scan. Only written pages consume disk;
//! mappings avoid committing a private Vec for every name and record during reconciliation.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::Result;
use memmap2::MmapMut;
use std::os::windows::io::AsRawHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE};

use crate::store::Record;

struct Files(Vec<PathBuf>);
impl Drop for Files {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) struct Spool {
    records: MmapMut,
    names: MmapMut,
    pub count: usize,
    pub names_len: usize,
    // Declared last so mappings close before their backing files are removed on Windows.
    _files: Files,
}

impl Spool {
    pub fn new(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let mut files = Files(Vec::new());
        let mut create = |suffix: &str, length: u64| -> Result<MmapMut> {
            let path = directory.join(format!("scan-{}-{stamp}.{suffix}", std::process::id()));
            let file: File = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)?;
            files.0.push(path);
            let mut returned = 0;
            // SAFETY: owned file handle, no input requests sparse=true, valid result pointer.
            unsafe {
                DeviceIoControl(
                    HANDLE(file.as_raw_handle()),
                    FSCTL_SET_SPARSE,
                    None,
                    0,
                    None,
                    0,
                    Some(&mut returned),
                    None,
                )?;
            }
            file.set_len(length)?;
            // SAFETY: this process exclusively owns the scratch file; it is never truncated.
            Ok(unsafe { MmapMut::map_mut(&file)? })
        };
        let records = create("records", 64_000_000 * size_of::<Record>() as u64)?;
        let names = create("names", 2 * 1024 * 1024 * 1024)?;
        Ok(Self {
            records,
            names,
            count: 0,
            names_len: 0,
            _files: files,
        })
    }

    pub fn record(&self, id: usize) -> Record {
        bytemuck::cast_slice::<u8, Record>(&self.records)[id]
    }
    pub fn set(&mut self, id: usize, record: Record) {
        bytemuck::cast_slice_mut::<u8, Record>(&mut self.records)[id] = record;
    }
    pub fn push(&mut self, record: Record) {
        assert!(
            (self.count + 1) * size_of::<Record>() <= self.records.len(),
            "scan exceeds 64 million entries per volume"
        );
        self.set(self.count, record);
        self.count += 1;
    }
    pub fn append(&mut self, name: &[u8]) {
        let end = self.names_len + name.len();
        assert!(
            end <= self.names.len(),
            "scan name blob exceeds 2 GiB per volume"
        );
        self.names[self.names_len..end].copy_from_slice(name);
        self.names_len = end;
    }
    pub fn bytes(&self, start: usize, len: usize) -> &[u8] {
        &self.names[start..start + len]
    }
}

/// A killed core cannot run Drop. Remove only this service's recognizable scratch files,
/// after exclusive pipe ownership has ruled out another running index process.
pub(crate) fn cleanup(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("scan-")
            && (name.ends_with(".records") || name.ends_with(".names"))
            && entry.file_type().is_ok_and(|t| t.is_file())
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
