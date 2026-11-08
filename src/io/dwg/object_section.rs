//! The object section (`AcDb:AcDbObjects`) as the object reader reads it.
//!
//! An R2004 and later file stores the section as compressed pages of at
//! most 0x7400 bytes each. The whole decompressed section is often many
//! times the file: one 33 MB drawing holds a 297 MB section, most of it
//! records a reader may skip. A paged section keeps the pages compressed
//! and decompresses a page only when a record on it is read, keeping the
//! most recently used pages in a small cache. Records are read roughly in
//! offset order, so each page is decompressed about once.
//!
//! Other versions, and readers built from bytes in memory, hold the
//! section whole.

use super::decompressor_ac18::decompress_ac18;
use crate::error::{DxfError, Result};
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::{Arc, Mutex};

/// Decompressed pages kept for reuse: 256 pages of 0x7400 bytes, 7.6 MB.
const CACHED_PAGES: usize = 256;

/// A record at least this large in a paged section is read from the pages
/// as the reader reaches its bytes, not copied whole: one dictionary of a
/// drawing converted from MicroStation is a 120 MB record.
const WINDOW_RECORD_BYTES: usize = 1 << 20;

/// The bytes of an object section.
pub(crate) enum ObjectSection {
    /// The decompressed section.
    Whole(Arc<Vec<u8>>),
    /// The compressed pages, decompressed as they are read.
    Paged(PagedSection),
}

impl From<Arc<Vec<u8>>> for ObjectSection {
    fn from(bytes: Arc<Vec<u8>>) -> Self {
        Self::Whole(bytes)
    }
}

impl From<Vec<u8>> for ObjectSection {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Whole(Arc::new(bytes))
    }
}

impl ObjectSection {
    /// The section's decompressed length.
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Whole(bytes) => bytes.len(),
            Self::Paged(paged) => paged.len,
        }
    }

    /// Copies the bytes from `start` into `out`, fewer where the section
    /// ends, and returns how many it copied.
    pub(crate) fn head(&self, start: usize, out: &mut [u8]) -> usize {
        let len = out.len().min(self.len().saturating_sub(start));
        let out = &mut out[..len];
        match self {
            Self::Whole(bytes) => out.copy_from_slice(&bytes[start..start + len]),
            Self::Paged(paged) => {
                out.fill(0);
                paged.copy_into(start, out);
            }
        }
        len
    }

    /// `len` bytes from `start` as one shared buffer. The caller checks the
    /// range against [`Self::len`].
    pub(crate) fn shared(&self, start: usize, len: usize) -> Arc<[u8]> {
        match self {
            Self::Whole(bytes) => Arc::from(&bytes[start..start + len]),
            Self::Paged(paged) => {
                let mut out: Arc<[MaybeUninit<u8>]> = Arc::new_uninit_slice(len);
                let slots = Arc::get_mut(&mut out).expect("a new buffer has no other holder");
                for slot in slots.iter_mut() {
                    slot.write(0);
                }
                // SAFETY: every byte was written above.
                let mut out = unsafe { out.assume_init() };
                let bytes = Arc::get_mut(&mut out).expect("a new buffer has no other holder");
                paged.copy_into(start, bytes);
                out
            }
        }
    }
}

impl ObjectSection {
    /// The bytes of the record at `start`, `len` long. The caller checks
    /// the range against [`Self::len`].
    pub(crate) fn record(section: &Arc<Self>, start: usize, len: usize) -> RecordBytes {
        match &**section {
            Self::Paged(_) if len >= WINDOW_RECORD_BYTES => RecordBytes::Window(Window {
                section: Arc::clone(section),
                start,
                len,
                page_start: 0,
                page: Arc::from(&[][..]),
            }),
            _ => RecordBytes::Shared(section.shared(start, len)),
        }
    }
}

/// The bytes of one object record, as a bit reader reads them.
#[derive(Clone)]
pub(crate) enum RecordBytes {
    /// The record's bytes.
    Shared(Arc<[u8]>),
    /// A record of a paged section, read a page at a time.
    Window(Window),
}

/// A record's range of a paged section and the page last read.
#[derive(Clone)]
pub(crate) struct Window {
    section: Arc<ObjectSection>,
    start: usize,
    len: usize,
    /// Where `page` starts in the section.
    page_start: usize,
    page: Arc<[u8]>,
}

impl From<Arc<[u8]>> for RecordBytes {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self::Shared(bytes)
    }
}

impl RecordBytes {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Shared(bytes) => bytes.len(),
            Self::Window(window) => window.len,
        }
    }

    /// The byte at `pos`, which the caller keeps below [`Self::len`].
    #[inline]
    pub(crate) fn byte(&mut self, pos: usize) -> u8 {
        match self {
            Self::Shared(bytes) => bytes[pos],
            Self::Window(window) => window.byte(pos),
        }
    }

    /// Copies the bytes from `start` into `out`; the caller keeps the range
    /// inside the record.
    pub(crate) fn copy(&mut self, start: usize, out: &mut [u8]) {
        match self {
            Self::Shared(bytes) => out.copy_from_slice(&bytes[start..start + out.len()]),
            Self::Window(window) => {
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = window.byte(start + i);
                }
            }
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<u8> {
        match self {
            Self::Shared(bytes) => bytes.to_vec(),
            Self::Window(window) => window.section.shared(window.start, window.len).to_vec(),
        }
    }
}

impl Window {
    fn byte(&mut self, pos: usize) -> u8 {
        let at = self.start + pos;
        if at < self.page_start || at - self.page_start >= self.page.len() {
            let ObjectSection::Paged(paged) = &*self.section else {
                unreachable!("a window is made over a paged section only");
            };
            match paged.page_at(at) {
                Some((start, page)) => {
                    self.page_start = start;
                    self.page = page;
                }
                // A byte no page holds reads as zero.
                None => return 0,
            }
        }
        self.page[at - self.page_start]
    }
}

/// One compressed page and where its bytes go in the section.
pub(crate) struct Page {
    /// The page's first byte in the decompressed section.
    pub(crate) start: usize,
    /// The page's bytes as the file holds them, decrypted.
    pub(crate) stored: Box<[u8]>,
}

/// A section kept as its compressed pages.
pub(crate) struct PagedSection {
    len: usize,
    compressed: bool,
    page_size: usize,
    /// Sorted by `start`.
    pages: Vec<Page>,
    cache: Mutex<PageCache>,
}

impl PagedSection {
    /// `pages` in any order; bytes no page covers read as zero, as a
    /// whole section reads a skipped page.
    pub(crate) fn new(
        len: usize,
        compressed: bool,
        page_size: usize,
        mut pages: Vec<Page>,
    ) -> Self {
        pages.sort_by_key(|page| page.start);
        Self {
            len,
            compressed,
            page_size,
            pages,
            cache: Mutex::new(PageCache::default()),
        }
    }

    /// The most the cache holds.
    pub(crate) fn cache_bytes(page_size: usize) -> usize {
        CACHED_PAGES.saturating_mul(page_size)
    }

    /// Copies the section's bytes from `start` into `out`.
    fn copy_into(&self, start: usize, out: &mut [u8]) {
        let end = start + out.len();
        // The last page starting at or before `start`.
        let first = self
            .pages
            .partition_point(|page| page.start <= start)
            .saturating_sub(1);
        for index in first..self.pages.len() {
            let page_start = self.pages[index].start;
            if page_start >= end {
                break;
            }
            let bytes = self.page(index);
            let from = start.max(page_start);
            let to = end.min(page_start + bytes.len());
            if from < to {
                out[from - start..to - start]
                    .copy_from_slice(&bytes[from - page_start..to - page_start]);
            }
        }
    }

    /// The page holding byte `at` and where it starts, if a page does.
    fn page_at(&self, at: usize) -> Option<(usize, Arc<[u8]>)> {
        let index = self
            .pages
            .partition_point(|page| page.start <= at)
            .checked_sub(1)?;
        let start = self.pages[index].start;
        let bytes = self.page(index);
        (at - start < bytes.len()).then_some((start, bytes))
    }

    /// The decompressed bytes of page `index`.
    fn page(&self, index: usize) -> Arc<[u8]> {
        if let Some(bytes) = self.cache.lock().map(|mut c| c.get(index)).unwrap_or(None) {
            return bytes;
        }
        // Decompressed outside the lock, so readers on other threads go on.
        let stored = &self.pages[index].stored;
        let bytes: Arc<[u8]> = if self.compressed {
            decompress_ac18(stored, self.page_size).into()
        } else {
            Arc::from(&stored[..])
        };
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(index, Arc::clone(&bytes));
        }
        bytes
    }
}

/// The most recently used pages.
#[derive(Default)]
struct PageCache {
    pages: HashMap<usize, (Arc<[u8]>, u64)>,
    tick: u64,
}

impl PageCache {
    fn get(&mut self, index: usize) -> Option<Arc<[u8]>> {
        self.tick += 1;
        let tick = self.tick;
        self.pages.get_mut(&index).map(|(bytes, used)| {
            *used = tick;
            Arc::clone(bytes)
        })
    }

    fn put(&mut self, index: usize, bytes: Arc<[u8]>) {
        if self.pages.len() >= CACHED_PAGES && !self.pages.contains_key(&index) {
            if let Some(&oldest) = self
                .pages
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(index, _)| index)
            {
                self.pages.remove(&oldest);
            }
        }
        self.tick += 1;
        self.pages.insert(index, (bytes, self.tick));
    }
}

/// Reads past the section's end are refused before they reach it.
pub(crate) fn check_range(section: &ObjectSection, start: usize, len: usize) -> Result<()> {
    if start > section.len() || len > section.len() - start {
        return Err(DxfError::Parse(format!(
            "Object record extends past data: offset={start}, size={len}, data_len={}",
            section.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paged(bytes: &[u8], page: usize) -> ObjectSection {
        let pages = bytes
            .chunks(page)
            .enumerate()
            .rev()
            .map(|(i, chunk)| Page {
                start: i * page,
                stored: chunk.into(),
            })
            .collect();
        ObjectSection::Paged(PagedSection::new(bytes.len(), false, page, pages))
    }

    #[test]
    fn a_paged_section_reads_the_bytes_a_whole_one_does() {
        let bytes: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let whole = ObjectSection::from(bytes.clone());
        let paged = paged(&bytes, 64);
        assert_eq!(paged.len(), whole.len());
        for (start, len) in [
            (0, 10),
            (60, 10),
            (63, 130),
            (900, 100),
            (999, 1),
            (0, 1000),
        ] {
            assert_eq!(&paged.shared(start, len)[..], &whole.shared(start, len)[..]);
            let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
            assert_eq!(paged.head(start, &mut a), whole.head(start, &mut b));
            assert_eq!(a, b);
        }
        assert_eq!(paged.head(995, &mut [0u8; 16]), 5);
    }

    #[test]
    fn a_large_record_reads_from_the_pages_as_a_copy_does() {
        let bytes: Vec<u8> = (0..(3 << 20) as u32).map(|i| (i % 253) as u8).collect();
        let section = Arc::new(paged(&bytes, 0x7400));
        let (start, len) = (100, WINDOW_RECORD_BYTES + 12_345);
        let mut window = ObjectSection::record(&section, start, len);
        assert!(matches!(window, RecordBytes::Window(_)));
        assert_eq!(window.len(), len);
        for pos in [0, 1, 0x7400 - 101, 0x7400 - 100, len / 2, 7, len - 1] {
            assert_eq!(window.byte(pos), bytes[start + pos], "byte {pos}");
        }
        let mut out = [0u8; 40];
        window.copy(0x7400 - 120, &mut out);
        assert_eq!(&out[..], &bytes[start + 0x7400 - 120..start + 0x7400 - 80]);
        assert_eq!(window.to_vec(), &bytes[start..start + len]);
        let small = ObjectSection::record(&section, start, 64);
        assert!(matches!(small, RecordBytes::Shared(_)));
    }

    #[test]
    fn bytes_no_page_holds_read_as_zero() {
        let pages = vec![Page {
            start: 8,
            stored: vec![1u8; 8].into(),
        }];
        let section = ObjectSection::Paged(PagedSection::new(24, false, 8, pages));
        assert_eq!(
            &section.shared(0, 24)[..],
            &[[0u8; 8], [1u8; 8], [0u8; 8]].concat()[..]
        );
    }

    #[test]
    fn the_cache_keeps_the_pages_last_used() {
        let mut cache = PageCache::default();
        for i in 0..CACHED_PAGES {
            cache.put(i, Arc::from(&[i as u8][..]));
        }
        assert!(cache.get(0).is_some());
        cache.put(CACHED_PAGES, Arc::from(&[0u8][..]));
        assert!(cache.get(0).is_some(), "used last, kept");
        assert!(cache.get(1).is_none(), "least recently used, dropped");
        assert_eq!(cache.pages.len(), CACHED_PAGES);
    }
}
