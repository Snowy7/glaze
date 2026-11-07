//! Immutable query summaries backed by ordinary files, so their pages need no private
//! commit and Windows can reclaim them. Delete-on-close also cleans up a killed helper.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::ops::Deref;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use bytemuck::Pod;
use memmap2::Mmap;
use windows::Win32::Storage::FileSystem::FILE_FLAG_DELETE_ON_CLOSE;

pub(crate) enum Backing<T> {
    Owned(Vec<T>),
    Mapped {
        // Drop the mapping before its delete-on-close file handle.
        map: Mmap,
        _file: File,
    },
}

impl<T> Default for Backing<T> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<T: Pod> Deref for Backing<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match self {
            Self::Owned(values) => values,
            Self::Mapped { map, .. } => bytemuck::cast_slice(map),
        }
    }
}

impl<T: Pod> Backing<T> {
    pub fn freeze(
        values: Vec<T>,
        directory: Option<&Path>,
        quiet: Option<&crate::quiet::Quiet>,
    ) -> Result<Self> {
        let Some(directory) = directory.filter(|_| !values.is_empty()) else {
            return Ok(Self::Owned(values));
        };
        std::fs::create_dir_all(directory)?;
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = directory.join(format!("query-{}-{stamp}-{serial}.bin", std::process::id()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
            .open(path)?;
        {
            let mut writer = BufWriter::with_capacity(1024 * 1024, &file);
            for chunk in bytemuck::cast_slice::<T, u8>(&values).chunks(1024 * 1024) {
                if let Some(quiet) = quiet {
                    quiet.wait();
                }
                writer.write_all(chunk)?;
            }
            writer.flush()?;
        }
        // Free private storage before mapping its replacement, avoiding two resident copies.
        drop(values);
        // SAFETY: the uniquely created file is complete and immutable from here on. The
        // file handle remains owned until after the read-only mapping is dropped.
        let map = unsafe { Mmap::map(&file)? };
        Ok(Self::Mapped { map, _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_array_round_trip_and_delete_on_close() {
        let directory = std::env::temp_dir().join(format!(
            "mypaper-query-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let values: Vec<u64> = (0..10000).collect();
        let backing = Backing::freeze(values.clone(), Some(&directory), None).unwrap();
        assert!(matches!(backing, Backing::Mapped { .. }));
        assert_eq!(&*backing, &values);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        drop(backing);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        std::fs::remove_dir(directory).unwrap();
    }
}
