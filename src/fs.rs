//! A set of utilities that extend [std::fs]
use std::fs::{self, DirEntry, ReadDir};
use std::{io, path::Path};

pub use fs::create_dir_all;
pub use fs::remove_dir_all;

/// An interator that can be used to generate all of the directory entries
/// recursively. This is similar to [`std::fs::ReadDir`], except it steps into
/// each directory as they are encountered.
pub struct RecursiveReadDir {
    stack: Vec<ReadDir>,
}

impl RecursiveReadDir {
    /// Create a new RecursiveReadDir. Note this will result in an
    /// [io::Error] if the path that is provided is not accessible or is
    /// not a directory.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        fs::read_dir(path).map(|rd| RecursiveReadDir { stack: vec![rd] })
    }
}

impl Iterator for RecursiveReadDir {
    type Item = std::result::Result<DirEntry, io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.stack.last_mut();
            if let Some(c) = current {
                let next = c.next();
                match next {
                    Some(Ok(ref de)) => {
                        let path = de.path();
                        if path.is_dir() {
                            let rrd = fs::read_dir(path);

                            match rrd {
                                Ok(rd) => {
                                    self.stack.push(rd);
                                    return next;
                                }
                                Err(_) => return Some(Err(rrd.unwrap_err())),
                            }
                        } else {
                            return next;
                        }
                    }
                    Some(Err(_)) => return next,
                    None => {
                        self.stack.pop();
                    }
                }
            } else {
                return None;
            }
        }
    }
}

/// Get an iterator that produces directory entries for a given directory,
/// recusrively through subdirectories. This is similar to [`fs::read_dir`].
///
/// # Examples
///
/// ```no_run
/// use bloggo::fs::recursive_read_dir;
///
/// recursive_read_dir("/some/path").map(|rrd| {
///     for rde in rrd {
///         match rde {
///             Ok(de) => println!("Entry: {}", de.path().display()),
///             Err(e) => {
///                 // handle error
///             }
///         }
///     }
/// });
/// ```
pub fn recursive_read_dir(path: impl AsRef<Path>) -> io::Result<RecursiveReadDir> {
    RecursiveReadDir::new(path)
}
