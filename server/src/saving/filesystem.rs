//! Wrapper around filesystems.
//!
//! Allows the use for virtual systems (e.g. steam cloud)

use chrono;
use std::fs;
use std::io::{Read, Result, Seek, Write};
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

/// A boxed filesystem
pub type BoxedFileSystem =
    Box<dyn FileSystem<Reader = Box<dyn ReadSeeker>, Writer = Box<dyn Write>>>;

#[doc(hidden)]
pub trait ReadSeeker: Read + Seek {}
impl<T> ReadSeeker for T where T: Read + Seek {}

/// A wrapper around a filesystem
pub trait FileSystem {
    /// The type returned to read files
    type Reader: Read + Seek;
    /// The type returned to Write files
    type Writer: Write;

    /// Opens a file for reading
    fn read(&self, name: &str) -> Result<Self::Reader>;
    /// Opens a file for writing
    fn write(&self, name: &str) -> Result<Self::Writer>;
    /// Deletes a file
    fn delete(&self, name: &str) -> Result<()>;
    /// Returns whether a file exists
    fn exists(&self, name: &str) -> bool;

    /// Returns the timestamp of the file
    fn timestamp(&self, name: &str) -> Result<chrono::DateTime<chrono::Local>>;

    /// Returns a list of files on the system
    fn files(&self) -> Vec<String>;

    /// Converts the filesystem into a boxed version erasing the
    /// type.
    fn into_boxed(self) -> BoxedFileSystem
    where
        Self: Sized + 'static,
    {
        Box::new(BoxedFS { inner: self })
    }
}

struct BoxedFS<Inner> {
    inner: Inner,
}

impl<Inner> FileSystem for BoxedFS<Inner>
where
    Inner: FileSystem,
    Inner::Reader: 'static,
    Inner::Writer: 'static,
{
    type Reader = Box<dyn ReadSeeker>;
    type Writer = Box<dyn Write>;

    fn read(&self, name: &str) -> Result<Self::Reader> {
        Ok(Box::new(self.inner.read(name)?))
    }

    fn write(&self, name: &str) -> Result<Self::Writer> {
        Ok(Box::new(self.inner.write(name).map(Box::new)?))
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.inner.delete(name)
    }

    fn exists(&self, name: &str) -> bool {
        self.inner.exists(name)
    }

    fn timestamp(&self, name: &str) -> Result<chrono::DateTime<chrono::Local>> {
        self.inner.timestamp(name)
    }

    fn files(&self) -> Vec<String> {
        self.inner.files()
    }
}

impl<W, R, F> FileSystem for Box<F>
where
    F: FileSystem<Reader = R, Writer = W> + ?Sized,
    W: Write + Sized,
    R: Read + Seek + Sized,
{
    type Reader = R;
    type Writer = W;

    fn read(&self, name: &str) -> Result<Self::Reader> {
        F::read(&*self, name)
    }

    fn write(&self, name: &str) -> Result<Self::Writer> {
        F::write(&*self, name)
    }

    fn delete(&self, name: &str) -> Result<()> {
        F::delete(&*self, name)
    }

    fn exists(&self, name: &str) -> bool {
        F::exists(&*self, name)
    }

    fn timestamp(&self, name: &str) -> Result<chrono::DateTime<chrono::Local>> {
        F::timestamp(&*self, name)
    }

    fn files(&self) -> Vec<String> {
        F::files(&*self)
    }
}

/// A view onto the native filesystem
#[derive(Clone)]
pub struct NativeFileSystem {
    path: PathBuf,
}

impl NativeFileSystem {
    /// Creates a native file system that is a view
    /// onto the passed folder.
    pub fn new(path: &Path) -> NativeFileSystem {
        NativeFileSystem {
            path: path.to_owned(),
        }
    }
}

/// A wrapper around `fs::File` that writes to a temp file
/// and moves to the target location once dropped.
///
/// This is done so that if the application crashes whilst
/// writing the previous file is still usable.
pub struct SafeFileWriter {
    file: ManuallyDrop<fs::File>,
    path: PathBuf,
    tmp_path: PathBuf,
}

impl Write for SafeFileWriter {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        self.file.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.file.write_all(buf)
    }
}

impl Drop for SafeFileWriter {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.file);
        }
        fs::rename(&self.tmp_path, &self.path).expect("Failed to move file");
    }
}

impl FileSystem for NativeFileSystem {
    type Reader = fs::File;
    type Writer = SafeFileWriter;

    fn read(&self, name: &str) -> Result<Self::Reader> {
        fs::File::open(self.path.join(name))
    }

    fn write(&self, name: &str) -> Result<Self::Writer> {
        // Do this here so that the directory isn't created if it isn't accessed
        fs::create_dir_all(&self.path).expect("Failed to create directory");

        let path = self.path.join(name);
        let tmp_path = path.with_extension(".tmp");
        let file = fs::File::create(&tmp_path)?;
        Ok(SafeFileWriter {
            file: ManuallyDrop::new(file),
            path: path.to_owned(),
            tmp_path: tmp_path.to_owned(),
        })
    }

    fn delete(&self, name: &str) -> Result<()> {
        fs::remove_file(self.path.join(name))
    }

    fn exists(&self, name: &str) -> bool {
        fs::metadata(self.path.join(name)).is_ok()
    }

    fn timestamp(&self, name: &str) -> Result<chrono::DateTime<chrono::Local>> {
        fs::metadata(self.path.join(name))
            .and_then(|v| v.modified())
            .map(Into::into)
    }

    fn files(&self) -> Vec<String> {
        fs::read_dir(&self.path)
            .ok()
            .into_iter()
            .flat_map(|v| v)
            .filter_map(|v| v.ok())
            .map(|v| v)
            .map(|v| v.path())
            .map(|v| {
                v.strip_prefix(&self.path)
                    .expect("File is missing prefix")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }
}

/// Targets a subfolder of the passed filesystem
#[derive(Clone)]
pub struct SubfolderFileSystem<I> {
    inner: I,
    sub_path: String,
}

impl<I> SubfolderFileSystem<I>
where
    I: FileSystem,
{
    /// Creates a view onto the subfolder of the passed filesystem.
    pub fn new<S>(fs: I, sub_path: S) -> SubfolderFileSystem<I>
    where
        S: Into<String>,
    {
        SubfolderFileSystem {
            inner: fs,
            sub_path: sub_path.into(),
        }
    }
}

impl<I> FileSystem for SubfolderFileSystem<I>
where
    I: FileSystem,
{
    type Reader = I::Reader;
    type Writer = I::Writer;

    fn read(&self, name: &str) -> Result<Self::Reader> {
        self.inner.read(&format!("{}/{}", self.sub_path, name))
    }

    fn write(&self, name: &str) -> Result<Self::Writer> {
        self.inner.write(&format!("{}/{}", self.sub_path, name))
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.inner.delete(&format!("{}/{}", self.sub_path, name))
    }

    fn exists(&self, name: &str) -> bool {
        self.inner.exists(&format!("{}/{}", self.sub_path, name))
    }

    fn timestamp(&self, name: &str) -> Result<chrono::DateTime<chrono::Local>> {
        self.inner.timestamp(&format!("{}/{}", self.sub_path, name))
    }

    fn files(&self) -> Vec<String> {
        let mut files = self.inner.files();
        let path = format!("{}/", self.sub_path);
        files.retain(|v| v.starts_with(&path));
        for f in &mut files {
            f.drain(..path.len());
        }
        files
    }
}

/// Reads/Writes to `A` falling back to `B` for reads if missing
#[derive(Clone)]
pub struct JoinedFileSystem<A, B> {
    a: A,
    b: B,
}

impl<A, B> JoinedFileSystem<A, B>
where
    A: FileSystem,
    B: FileSystem,
{
    /// Joins the two filesystems
    pub fn new(a: A, b: B) -> JoinedFileSystem<A, B> {
        JoinedFileSystem { a, b }
    }
}

impl<A, B> FileSystem for JoinedFileSystem<A, B>
where
    A: FileSystem + 'static,
    B: FileSystem + 'static,
    A::Reader: 'static,
    B::Reader: 'static,
    A::Writer: 'static,
{
    type Reader = Box<dyn ReadSeeker>;
    type Writer = Box<dyn Write>;

    fn read(&self, name: &str) -> Result<Self::Reader> {
        if let Ok(f) = self.a.read(name) {
            return Ok(Box::new(f));
        }
        Ok(Box::new(self.b.read(name)?))
    }

    fn write(&self, name: &str) -> Result<Self::Writer> {
        Ok(Box::new(self.a.write(name)?))
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.a.delete(name)?;
        self.b.delete(name)
    }

    fn exists(&self, name: &str) -> bool {
        self.a.exists(name) || self.b.exists(name)
    }

    fn timestamp(&self, name: &str) -> Result<chrono::DateTime<chrono::Local>> {
        if let Ok(f) = self.a.timestamp(name) {
            return Ok(f);
        }
        self.b.timestamp(name)
    }

    fn files(&self) -> Vec<String> {
        let mut files = self.a.files();
        files.append(&mut self.b.files());
        files.sort_unstable();
        files.dedup();
        files
    }

    fn into_boxed(self) -> BoxedFileSystem {
        Box::new(self)
    }
}
