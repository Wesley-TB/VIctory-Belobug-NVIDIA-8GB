//! File discovery shared by `shader-info` and `shaders`.

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

pub(crate) struct FileScan {
    limit: u64,
    verbose: bool,
    files_read: usize,
    walk_errors: usize,
    metadata_errors: usize,
    read_errors: usize,
    over_limit: usize,
}

impl FileScan {
    pub(crate) fn new(limit: u64, verbose: bool) -> Self {
        Self {
            limit,
            verbose,
            files_read: 0,
            walk_errors: 0,
            metadata_errors: 0,
            read_errors: 0,
            over_limit: 0,
        }
    }

    pub(crate) fn files(&mut self, root: &Path) -> impl Iterator<Item = (PathBuf, Vec<u8>)> {
        WalkDir::new(root).into_iter().filter_map(move |entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    // A failed walk may hide an entire subtree of unknown size.
                    self.walk_errors += 1;
                    self.detail(format_args!("walking {}: {error}", root.display()));
                    return None;
                }
            };
            if !entry.file_type().is_file() {
                return None;
            }
            let data = self.read_file(entry.path(), entry.metadata().map_err(io::Error::from))?;
            Some((entry.into_path(), data))
        })
    }

    fn read_file(&mut self, path: &Path, metadata: io::Result<fs::Metadata>) -> Option<Vec<u8>> {
        let size = match metadata {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                self.metadata_errors += 1;
                self.detail(format_args!("reading metadata for {}: {error}", path.display()));
                return None;
            }
        };
        if size > self.limit {
            self.over_limit += 1;
            self.detail(format_args!("skipping large file {} ({size} bytes)", path.display()));
            return None;
        }
        // The file can grow after the metadata check. Read at most the limit
        // into memory, then probe one extra byte without growing the buffer.
        let read = fs::File::open(path).and_then(|mut file| {
            let mut data = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
            file.by_ref().take(self.limit).read_to_end(&mut data)?;
            if data.len() as u64 == self.limit {
                match file.read_exact(&mut [0]) {
                    Ok(()) => return Ok(None),
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(Some(data))
        });
        match read {
            Ok(Some(data)) => {
                self.files_read += 1;
                Some(data)
            }
            Ok(None) => {
                self.over_limit += 1;
                self.detail(format_args!(
                    "skipping {}: file exceeded {} bytes during the read",
                    path.display(),
                    self.limit
                ));
                None
            }
            Err(error) => {
                self.read_errors += 1;
                self.detail(format_args!("reading {}: {error}", path.display()));
                None
            }
        }
    }

    fn detail(&self, message: fmt::Arguments<'_>) {
        if self.verbose {
            eprintln!("{message}");
        } else {
            tracing::debug!("{message}");
        }
    }
}

impl fmt::Display for FileScan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let errors = self.walk_errors + self.metadata_errors + self.read_errors;
        let status = match (errors > 0, self.over_limit > 0) {
            (false, false) => "complete",
            (false, true) => "size-limited",
            (true, false) => "incomplete (I/O errors)",
            (true, true) => "incomplete (I/O errors and size limit)",
        };
        write!(
            f,
            "file scan {status}: files read {}, skipped {} (walk errors {}, metadata errors {}, \
             read errors {}, over {} bytes: {})",
            self.files_read, errors + self.over_limit, self.walk_errors, self.metadata_errors,
            self.read_errors, self.limit, self.over_limit,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn summary(scan: &FileScan) -> String {
        let text = scan.to_string();
        println!("{text}");
        text
    }

    #[test]
    fn reads_regular_files_including_empty_files_and_the_limit_itself() {
        let root = tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let empty = root.path().join("empty.bin");
        let full = nested.join("full.bin");
        fs::write(&empty, []).unwrap();
        fs::write(&full, b"abcd").unwrap();

        let mut scan = FileScan::new(4, false);
        let files: std::collections::BTreeMap<_, _> = scan.files(root.path()).collect();
        assert_eq!(files[&full], b"abcd");
        assert!(files[&empty].is_empty());
        assert_eq!(
            summary(&scan),
            "file scan complete: files read 2, skipped 0 (walk errors 0, metadata errors 0, \
             read errors 0, over 4 bytes: 0)"
        );
    }

    #[test]
    fn empty_directory_counts_no_files_or_skips() {
        let root = tempdir().unwrap();
        let mut scan = FileScan::new(4, false);
        assert_eq!(scan.files(root.path()).count(), 0);
        assert_eq!(
            summary(&scan),
            "file scan complete: files read 0, skipped 0 (walk errors 0, metadata errors 0, \
             read errors 0, over 4 bytes: 0)"
        );
    }

    #[test]
    fn missing_root_counts_one_walk_error_and_later_roots_are_read() {
        let root = tempdir().unwrap();
        let file = root.path().join("ok.bin");
        fs::write(&file, b"ok").unwrap();
        let mut scan = FileScan::new(4, true);
        assert_eq!(scan.files(&root.path().join("missing")).count(), 0);
        assert_eq!(scan.files(&file).count(), 1);
        assert_eq!(
            summary(&scan),
            "file scan incomplete (I/O errors): files read 1, skipped 1 (walk errors 1, \
             metadata errors 0, read errors 0, over 4 bytes: 0)"
        );
    }

    #[test]
    fn failed_metadata_skips_even_a_readable_file() {
        let root = tempdir().unwrap();
        let file = root.path().join("readable.bin");
        fs::write(&file, b"ok").unwrap();
        let mut scan = FileScan::new(4, true);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(scan.read_file(&file, Err(denied)).is_none());
        assert_eq!(
            summary(&scan),
            "file scan incomplete (I/O errors): files read 0, skipped 1 (walk errors 0, \
             metadata errors 1, read errors 0, over 4 bytes: 0)"
        );
    }

    #[test]
    fn file_removed_after_metadata_counts_one_read_error() {
        let root = tempdir().unwrap();
        let file = root.path().join("removed.bin");
        fs::write(&file, b"ok").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        fs::remove_file(&file).unwrap();
        let mut scan = FileScan::new(4, true);
        assert!(scan.read_file(&file, Ok(metadata)).is_none());
        assert_eq!(
            summary(&scan),
            "file scan incomplete (I/O errors): files read 0, skipped 1 (walk errors 0, \
             metadata errors 0, read errors 1, over 4 bytes: 0)"
        );
    }

    #[test]
    fn file_growing_past_limit_is_still_skipped() {
        let root = tempdir().unwrap();
        let file = root.path().join("growing.bin");
        fs::write(&file, b"abcd").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        fs::write(&file, b"abcde").unwrap();

        let mut scan = FileScan::new(4, false);
        assert!(scan.read_file(&file, Ok(metadata)).is_none());
        assert_eq!(scan.over_limit, 1);
        assert_eq!(scan.files_read, 0);
    }

    #[test]
    fn size_skips_are_distinct_from_io_errors_even_in_the_same_scan() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("large.bin"), b"12345").unwrap();
        let mut scan = FileScan::new(4, true);
        assert_eq!(scan.files(root.path()).count(), 0);
        assert_eq!(
            summary(&scan),
            "file scan size-limited: files read 0, skipped 1 (walk errors 0, metadata errors 0, \
             read errors 0, over 4 bytes: 1)"
        );
        assert_eq!(scan.files(&root.path().join("missing")).count(), 0);
        assert_eq!(
            summary(&scan),
            "file scan incomplete (I/O errors and size limit): files read 0, skipped 2 \
             (walk errors 1, metadata errors 0, read errors 0, over 4 bytes: 1)"
        );
    }
}
