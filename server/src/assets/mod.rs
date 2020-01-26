//! Asset management for a collection of packages.
//!
//! Assets are stored in packages. Packages contain a
//! colleciton of files in module sub-folders. One of
//! these sub-folders should share the same name as the
//! module itself e.g: `assets/base/base/...` where base
//! is repeated twice. This is done to allow overriding of
//! assets from another module loaded beforehand, a
//! module `mod2` could contain a folder `mod1` to override
//! the assets from `mod1` with its own. An example
//! layout is provided below where the module `test_mod`
//! overrides some textures from the `base` module:
//!
//! ```ignore
//! - assets
//! -- base
//! --- base
//! ---- textures/...
//! ---- rooms/...
//! -- test_mod
//! --- base
//! ---- textures/...
//! --- test_mod
//! ---- textures/...
//! ---- rooms/...
//! ```
//!
//! Resources are generally represented as a module and a
//! resource name in the form `module:resource`. `ModuleKey`
//! is the `module` part and the `ResourceKey` is the
//! combined `module` and `resource`. In some cases it
//! is normally a good idea to allow the module to be
//! left off where it can be inferred to avoid repeating
//! youself, `LazyResourceKey` may be used for this case.

use std::sync::{Arc, Mutex};
use std::io::{self, SeekFrom, Write, Read, Seek};
use std::path::{Path, PathBuf};
use std::fs;
use std::any::{TypeId, Any};
use std::time::SystemTime;
use std::fmt::{self, Debug};

use byteorder::{LittleEndian, ReadBytesExt};
use crate::util::*;
use delta_encode::DeltaEncodable;
use crate::errors;
use crate::prelude::*;
use memmap;

mod arcstr;
use self::arcstr::ArcStr;

/// A key that can be used to reference a module.
///
/// This can either be owned or a reference to reduce
/// cloning.
#[derive(Clone, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
// The eq would be the same as the generated one
// ours just handles borrowed keys better
#[allow(clippy::derive_hash_xor_eq)]
pub struct ModuleKey<'a> {
    key: ArcStr<'a>,
}

impl <'a> ModuleKey<'a> {
    /// Creates a module key that references the named module.
    pub fn new<S: Into<ArcStr<'a>>>(module: S) -> ModuleKey<'a> {
        ModuleKey {
            key: module.into(),
        }
    }

    /// Returns the module name of this key
    pub fn module(&self) -> &str {
        &self.key
    }

    /// Returns a owned version of the module key.
    ///
    /// If the module key was already owned this is a no-op.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::collections::HashMap;
    /// # use univercity_server::assets::ModuleKey;
    /// # let mod_name = "hello";
    /// let m = ModuleKey::new(mod_name);
    /// let mut map = HashMap::new();
    /// map.insert(m.into_owned(), 5);
    /// ```
    pub fn into_owned(self) -> ModuleKey<'static> {
        ModuleKey {
            key: self.key.into_owned(),
        }
    }

    /// Creates a `ModuleKey` that references the same
    /// data as this key.
    pub fn borrow(&'a self) -> ModuleKey<'a> {
        ModuleKey {
            key: self.key.borrow()
        }
    }
}

impl <'a, 'b> PartialEq<ModuleKey<'a>> for ModuleKey<'b> {
    fn eq(&self, other: &ModuleKey<'a>) -> bool {
        self.key == other.key
    }
}

impl <'a> Debug for ModuleKey<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "Module({})", self.key)
    }
}

impl <'a, T> From<T> for ModuleKey<'a> where T: Into<ArcStr<'a>> {
    fn from(v: T) -> ModuleKey<'a> {
        ModuleKey {
            key: v.into(),
        }
    }
}

/// References a resource from a module
///
/// This can either be owned or a reference to reduce
/// cloning.
// The eq would be the same as the generated one
// ours just handles borrowed keys better
#[allow(clippy::derive_hash_xor_eq)]
#[derive(Clone, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceKey<'a> {
    module: ModuleKey<'a>,
    resource: ArcStr<'a>,
}

impl <'a> ResourceKey<'a> {
    /// Creates a resource key that references the named resource.
    ///
    /// Inherits the lifetime of its parameters
    pub fn new<'b: 'a, 'c: 'a,  M, S>(module: M, resource: S) -> ResourceKey<'a>
        where S: Into<ArcStr<'b>>,
            M: Into<ModuleKey<'c>>
    {
        ResourceKey {
            module: module.into(),
            resource: resource.into(),
        }

    }

    /// Parses the passed string into a resource key.
    ///
    /// Returns `None` if the string doesn't contain a `:` (module)
    ///
    /// # Example
    ///
    /// ```rust
    /// # use univercity_server::assets::*;
    /// assert_eq!(ResourceKey::parse("base:key"), Some(ResourceKey::new("base", "key")));
    /// assert_eq!(ResourceKey::parse("key"), None);
    /// ```
    pub fn parse(val: &'a str) -> Option<ResourceKey<'a>> {
        if let Some(pos) = val.char_indices().find(|v| v.1 == ':') {
            let (module, res) = val.split_at(pos.0);
            Some(ResourceKey {
                module: ModuleKey::new(module),
                resource: ArcStr::Borrowed(&res[1..]),
            })
        } else {
            None
        }
    }

    /// Returns the module name of this key
    pub fn module(&self) -> &str {
        &self.module.key
    }

    /// Returns the module key of this key
    pub fn module_key(&'a self) -> ModuleKey<'a> {
        self.module.borrow()
    }

    /// Returns the resource of this key
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Returns a owned version of the resource key.
    ///
    /// If the resource key was already owned this is a no-op.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::collections::HashMap;
    /// # use univercity_server::assets::ResourceKey;
    /// # let mod_name = "hello";
    /// let r = ResourceKey::new(mod_name, "testing");
    /// let mut map = HashMap::new();
    /// map.insert(r.into_owned(), 5);
    /// ```
    pub fn into_owned(self) -> ResourceKey<'static> {
        ResourceKey {
            module: self.module.into_owned(),
            resource: self.resource.into_owned(),
        }
    }

    /// Creates a resource key that references the same data
    /// as this key.
    pub fn borrow(&'a self) -> ResourceKey<'a> {
        ResourceKey {
            module: self.module.borrow(),
            resource: self.resource.borrow(),
        }
    }

    /// Returns a string that represents both the module and the
    /// resource.
    ///
    /// This can be passed back into `parse` to get
    /// the same resource key
    pub fn as_string(&self) -> String {
        format!("{}:{}", self.module.key, self.resource)
    }

    /// Stores the string representation of the resource key
    /// into the passed buffer
    pub fn store_string_buf(&self, buf: &mut [u8]) -> usize {
        let m = self.module.key.as_bytes();
        let r = self.resource.as_bytes();
        buf[..m.len()].copy_from_slice(m);
        buf[m.len()] = b':';
        buf[m.len() + 1 .. m.len() + 1 + r.len()].copy_from_slice(r);
        m.len() + r.len() + 1
    }

    /// Tests this key against the other key to see
    /// if they match.
    ///
    /// If this key contains '*' at the end then this
    /// will perform a wildcard match allowing any key
    /// that starts with the part before the '*' to pass
    /// the match.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use univercity_server::assets::*;
    /// let tools = ResourceKey::new("base", "tools/*");
    /// let hammer = ResourceKey::new("base", "tools/hammer");
    /// assert!(tools.weak_match(&hammer));
    /// assert!(tools.weak_match(&ResourceKey::new("base", "tools/screwdriver")));
    /// assert!(!tools.weak_match(&ResourceKey::new("magic", "tools/screwdriver")));
    /// assert!(hammer.weak_match(&hammer));
    /// assert!(!hammer.weak_match(&ResourceKey::new("base", "tools/screwdriver")));
    /// ```
    pub fn weak_match(&self, other: &ResourceKey<'_>) -> bool {
        if self.module() != other.module() {
            false
        } else if self.resource.ends_with('*') {
            other.resource.starts_with(&self.resource[..self.resource.len() - 1])
        } else {
            self.resource == other.resource
        }
    }
}

impl <'a, 'b> PartialEq<ResourceKey<'a>> for ResourceKey<'b> {
    fn eq(&self, other: &ResourceKey<'a>) -> bool {
        self.module == other.module && self.resource == other.resource
    }
}

impl <'a> Debug for ResourceKey<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "Resource({}:{})", self.module.key, self.resource)
    }
}

impl <'a> DeltaEncodable for ResourceKey<'static> {
    fn encode<W>(&self, base: Option<&Self>, w: &mut bitio::Writer<W>) -> io::Result<()>
        where W: Write
    {
        bitio::encode_str(&self.module.key, base.map(|v| v.module.key.as_ref()), w)?;
        bitio::encode_str(&self.resource, base.map(|v| v.resource.as_ref()), w)
    }

    fn decode<R>(base: Option<&Self>, r: &mut bitio::Reader<R>) -> io::Result<Self>
        where R: Read
    {
        Ok(ResourceKey::new(
            bitio::decode_string(base.map(|v| v.module.key.as_ref()), r)?,
            bitio::decode_string(base.map(|v| v.resource.as_ref()), r)?
        ))
    }
}


/// References a resource from a module. The module
/// can be `None`
///
/// This can either be owned or a reference to reduce
/// cloning.
#[derive(Clone, Eq, Hash)]
// The eq would be the same as the generated one
// ours just handles borrowed keys better
#[allow(clippy::derive_hash_xor_eq)]
pub struct LazyResourceKey<'a> {
    module: Option<ModuleKey<'a>>,
    resource: ArcStr<'a>,
}

impl <'a> LazyResourceKey<'a> {

    /// Parses the passed string into a lazy resource key.
    ///
    /// If the string contains a `:` the value before it
    /// will be the module and the value after will be the
    /// resource. Otherwise the passed string simply becomes
    /// the resource and the module will be `None`
    ///
    /// # Example
    ///
    /// ```rust
    /// # use univercity_server::assets::*;
    /// let base_hammer = LazyResourceKey::parse("base:hammer");
    /// let gen_hammer = LazyResourceKey::parse("hammer");
    /// assert_eq!(base_hammer, ResourceKey::new("base", "hammer"));
    /// assert_ne!(gen_hammer, ResourceKey::new("base", "hammer"));
    /// ```
    pub fn parse(val: &'a str) -> LazyResourceKey<'a> {
        if let Some(pos) = val.char_indices().find(|v| v.1 == ':') {
            let (module, res) = val.split_at(pos.0);
            LazyResourceKey {
                module: Some(ModuleKey::new(module)),
                resource: ArcStr::Borrowed(&res[1..]),
            }
        } else {
            LazyResourceKey {
                module: None,
                resource: ArcStr::Borrowed(val),
            }
        }
    }

    /// Creates a lazy resource key that references the named resource.
    ///
    /// Inherits the lifetime of its parameters
    pub fn new<M, S>(module: M, resource: S) -> LazyResourceKey<'a>
        where S: Into<ArcStr<'a>>,
            M: Into<Option<ModuleKey<'a>>> {
        LazyResourceKey {
            module: module.into(),
            resource: resource.into(),
        }
    }

    /// Returns the module name (if defined) of this key
    pub fn module(&self) -> Option<&str> {
        self.module.as_ref().map(|v| &*v.key)
    }

    /// Returns the module key (if defined) of this key
    pub fn module_key(&'a self) -> Option<ModuleKey<'a>> {
        self.module.as_ref().map(|v| v.borrow())
    }

    /// Returns the resource of this key
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Returns whether this key has a module defined
    pub fn has_module(&self) -> bool {
        self.module.is_some()
    }

    /// Converts this lazy resource key to a resource key.
    ///
    /// If this key doesn't have a module then the passed module will be used
    /// instead
    ///
    /// # Example
    ///
    /// ```rust
    /// # use univercity_server::assets::*;
    /// let base_hammer = LazyResourceKey::parse("base:hammer");
    /// let gen_hammer = LazyResourceKey::parse("hammer");
    ///
    /// let base = ModuleKey::new("base");
    /// let cake = ModuleKey::new("cake");
    ///
    /// assert_eq!(base_hammer.borrow().or_module(base.borrow()), ResourceKey::new("base", "hammer"));
    /// assert_eq!(gen_hammer.borrow().or_module(base.borrow()), ResourceKey::new("base", "hammer"));
    /// assert_eq!(gen_hammer.borrow().or_module(cake.borrow()), ResourceKey::new("cake", "hammer"));
    ///
    /// assert_ne!(base_hammer.borrow().or_module(cake.borrow()), ResourceKey::new("cake", "hammer"));
    /// ```
    pub fn or_module(self, module: ModuleKey<'a>) -> ResourceKey<'a> {
        ResourceKey {
            module: self.module.unwrap_or(module),
            resource: self.resource,
        }
    }

    /// Returns a owned version of the resource key.
    ///
    /// If the resource key was already owned this is a no-op.
    pub fn into_owned(self) -> LazyResourceKey<'static> {
        LazyResourceKey {
            module: if let Some(m) = self.module { Some(m.into_owned()) } else { None },
            resource: self.resource.into_owned(),
        }
    }

    /// Creates a reference to the resource key.
    pub fn borrow(&'a self) -> LazyResourceKey<'a> {
        LazyResourceKey {
            module: if let Some(ref m) = self.module { Some(m.borrow()) } else { None },
            resource: self.resource.borrow(),
        }
    }

    /// Creates a reference to the resource key using the passed module
    /// if this key doesn't have one defined
    pub fn borrow_module(&'a self, module: ModuleKey<'a>) -> ResourceKey<'a> {
        ResourceKey {
            module: if let Some(ref m) = self.module { m.borrow() } else { module },
            resource: self.resource.borrow(),
        }
    }


    /// Returns a string that represents both the module (if it has one)
    /// and the resource.
    ///
    /// This can be passed back into `parse` to get
    /// the same resource key
    pub fn as_string(&self) -> String {
        if let Some(module) = self.module.as_ref() {
            format!("{}:{}", module.key, self.resource)
        } else {
            self.resource.to_string()
        }
    }
}

impl <'a, 'b> PartialEq<LazyResourceKey<'a>> for LazyResourceKey<'b> {
    fn eq(&self, other: &LazyResourceKey<'a>) -> bool {
        if self.resource != other.resource {
            return false;
        }
        match (self.module.as_ref(), other.module.as_ref()) {
            (Some(m), Some(mo)) => m == mo,
            (None, None) => true,
            _ => false
        }
    }
}

impl <'a, 'b> PartialEq<LazyResourceKey<'a>> for ResourceKey<'b> {
    fn eq(&self, other: &LazyResourceKey<'a>) -> bool {
        if self.resource != other.resource {
            return false;
        }
        match (&self.module, other.module.as_ref()) {
            (m, Some(mo)) => m == mo,
            _ => false
        }
    }
}

impl <'a, 'b> PartialEq<ResourceKey<'a>> for LazyResourceKey<'b> {
    fn eq(&self, other: &ResourceKey<'a>) -> bool {
        if self.resource != other.resource {
            return false;
        }
        match (&other.module, self.module.as_ref()) {
            (m, Some(mo)) => m == mo,
            _ => false
        }
    }
}

impl <'a> Debug for LazyResourceKey<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "LazyResource({:?}:{})", self.module, self.resource)
    }
}

impl <'a> From<ResourceKey<'a>> for LazyResourceKey<'a> {
    fn from(v: ResourceKey<'a>) -> LazyResourceKey<'a> {
        LazyResourceKey {
            module: Some(v.module),
            resource: v.resource,
        }
    }
}

/// Used to create an instance of an asset manager
pub struct AssetsBuilder {
    store: Store,
    loader_data: FNVMap<TypeId, Mutex<Box<dyn Any + Send>>>,
}

impl AssetsBuilder {

    /// Registers a new asset loader
    pub fn register<'a, L: AssetLoader<'a>>(mut self) -> AssetsBuilder {
        self.loader_data.insert(TypeId::of::<L::LoaderData>(), Mutex::new(Box::new(L::init(
            &self.store
        ))));
        self
    }

    /// Creates a new asset manager
    pub fn build(self) -> AssetManager {
        AssetManager {
            inner: Arc::new(AssetInner {
                store: self.store,
                loader_data: self.loader_data,
            })
        }
    }
}

/// Provides access to assets whether they are in a packed or
/// unpacked format. Also handles multiple 'packs' of assets
/// to allow overriding of files.
///
/// Can be cloned to create multiple handles to the asset manager.
/// All clones will have the same 'packs' registered even if one
/// is added/removed via one of the handles.
#[derive(Clone)]
pub struct AssetManager {
    inner: Arc<AssetInner>,
}

struct AssetInner {
    store: Store,

    loader_data: FNVMap<TypeId, Mutex<Box<dyn Any + Send>>>,
}

impl AssetManager {
    /// Creates a builder used to register loaders and then create
    /// an asset amanger
    pub fn with_packs(log: &Logger, packs: &[String]) -> AssetsBuilder {
        let log = log.new(o!(
            "packs" => packs.join(","),
        ));
        let assets = packs.iter()
            .map(|name| if name.starts_with("workshop:") {
                    let path = assume!(log, PathBuf::from(&name["workshop:".len()..]).canonicalize());
                    let info: crate::ModMeta = assume!(log, fs::File::open(path.join("meta.json"))
                        .map_err(errors::Error::from)
                        .and_then(|v| serde_json::from_reader(v).map_err(errors::Error::from)));
                    (
                        ModuleKey::new(info.main),
                        Box::new(DirFetcher(path)) as Box<dyn Fetcher + Send + Sync>,
                    )
                } else if fs::metadata(&format!("./assets/{}", name)).is_ok() {
                    (
                        ModuleKey::new(&**name).into_owned(),
                        Box::new(DirFetcher(
                            assume!(log, PathBuf::from(&format!("./assets/{}/", name)).canonicalize())
                        )) as Box<dyn Fetcher + Send + Sync>,
                    )
                } else {
                    (
                        ModuleKey::new(&**name).into_owned(),
                        Box::new(assume!(log, PackedFetcher::new(name))) as Box<dyn Fetcher + Send + Sync>,
                    )
                }
            )
            .collect();

        #[cfg(feature = "debugutil")]
        let assets = {
            let mut a: Vec<_> = assets;
            a.push((ModuleKey::new("debug"), Box::new(DirFetcher(
                    assume!(log, PathBuf::from("./assets/debug/").canonicalize())
                )) as Box<dyn Fetcher + Send + Sync>));
            a
        };

        AssetsBuilder {
            store: Store {
                assets,
                log,
            },
            loader_data: FNVMap::default(),
        }
    }

    /// Returns a vector containing all loaded packs names
    pub fn get_packs(&self) -> Vec<ModuleKey<'static>> {
        self.inner.store.get_packs()
    }

    /// Opens the named asset from the named pack
    ///
    /// This is case sensitive and paths should not start
    /// with `/` (or any other prefix). This does not support
    /// complex paths (e.g. with `..` or `.`) on all types of
    /// 'packs'.
    pub fn open_from_pack<'a, M>(&self, module: M, name: &str) -> errors::Result<Asset>
        where M: Into<ModuleKey<'a>>
    {
        self.inner.store.open_from_pack(module.into(), name)
    }

    /// Returns the modified time of the named file if the
    /// pack supports it.
    ///
    /// This is case sensitive and paths should not start
    /// with `/` (or any other prefix). This does not support
    /// complex paths (e.g. with `..` or `.`) on all types of
    /// 'packs'.
    pub fn modified_time<'a>(&self, module: ModuleKey<'a>, name: &str) -> Option<SystemTime> {
        self.inner.store.modified_time(module, name)
    }

    /// Opens the named asset using the specified loader.
    ///
    /// Loaders can be used to load assets in the background
    /// without blocking the main thread or some other loader
    /// specific system. See the docs for the loader itself for
    /// more information.
    pub fn loader_open<'a, L: AssetLoader<'a>>(&self, res: L::Key) -> UResult<L::Return> {
        let mut loader_data = assume!(self.inner.store.log, self.inner.loader_data[&TypeId::of::<L::LoaderData>()]
            .lock());
        let data = assume!(self.inner.store.log, loader_data.downcast_mut::<L::LoaderData>());
        L::load(data, self, res)
    }
}

/// A loader provides a way to load and process an asset at
/// the same time.
///
/// This can be used to provide a async loading interface
/// so that loading doesn't block the main thread.
pub trait AssetLoader<'a> {
    /// Loader specific data.
    ///
    /// The asset manager will pass this into every `load`
    /// call to allow the loader to access channels, caches
    /// etc.
    ///
    /// Loader's are keyed on this type. Multiple loaders can
    /// share a single data source.
    type LoaderData: Any + Send;
    /// The type of key used for the input of this loader
    type Key;
    /// This is the type that will be returned to the caller
    /// when trying to load an asset with the loader.
    type Return;

    /// Called once to create the initial `LoaderData` state.
    fn init(assets: &Store) -> Self::LoaderData;
    /// Loads an asset that matches the passed resource key.
    ///
    /// If this can fail then the return type must handle this
    /// instead of panicing.
    fn load(data: &mut Self::LoaderData, assets: &AssetManager, key: Self::Key) -> UResult<Self::Return>;
}

/// Collection of packs
pub struct Store {
    assets: Vec<(ModuleKey<'static>, Box<dyn Fetcher + Sync + Send>)>,
    /// The asset manager's logger
    pub log: Logger,
}

impl Store {

    /// Returns a vector containing all loaded packs names
    pub fn get_packs(&self) -> Vec<ModuleKey<'static>> {
        self.assets.iter()
            .map(|v| v.0.clone())
            .collect()
    }

    /// Opens the named asset from the named pack
    ///
    /// This is case sensitive and paths should not start
    /// with `/` (or any other prefix). This does not support
    /// complex paths (e.g. with `..` or `.`) on all types of
    /// 'packs'.
    pub fn open_from_pack<'a>(&self, module: ModuleKey<'a>, name: &str) -> errors::Result<Asset> {
        for asset in self.assets.iter().rev() {
            if let Some(file) = asset.1.open(module.borrow(), name) {
                return Ok(file);
            }
        }
        Err(format!("Missing file: {:?} : {}", module, name).into())
    }

    /// Returns the modified time of the named file if the
    /// pack supports it.
    ///
    /// This is case sensitive and paths should not start
    /// with `/` (or any other prefix). This does not support
    /// complex paths (e.g. with `..` or `.`) on all types of
    /// 'packs'.
    pub fn modified_time<'a>(&self, module: ModuleKey<'a>, name: &str) -> Option<SystemTime> {
        for asset in self.assets.iter().rev() {
            if let Some(time) = asset.1.modified_time(module.borrow(), name) {
                return Some(time);
            }
        }
        None
    }
}

/// An asset that has been loaded from an
/// asset module.
///
/// The type used to store the asset (e.g. buffer
/// or native file) depends on the type of pack
/// used.
pub enum Asset {
    /// An in memory buffer containing the asset.
    Buffer(io::Cursor<Vec<u8>>),
    /// A on disk file with the asset.
    File(fs::File),
    /// Memory mapped asset
    Mapped(MappedAsset),
}

/// A memory mapped asset
pub struct MappedAsset {
    // Keep the mmap active whilst this asset exists due
    // to the unsafe static pointer used
    _map: Arc<memmap::Mmap>,
    reader: io::Cursor<&'static [u8]>,
}

impl Read for Asset {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            Asset::Buffer(ref mut b) => b.read(buf),
            Asset::File(ref mut f) =>f.read(buf),
            Asset::Mapped(ref mut m) => m.reader.read(buf),
        }
    }
}

impl Seek for Asset {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match *self {
            Asset::Buffer(ref mut b) => b.seek(pos),
            Asset::File(ref mut f) => f.seek(pos),
            Asset::Mapped(ref mut m) => m.reader.seek(pos),
        }
    }
}

trait Fetcher: Sync + Send {
    fn open(&self, module: ModuleKey<'_>, name: &str) -> Option<Asset>;
    fn modified_time(&self, _module: ModuleKey<'_>, _name: &str) -> Option<SystemTime> {
        None
    }
}

struct DirFetcher(PathBuf);

impl DirFetcher {
    fn clamp_path(&self, module: ModuleKey<'_>, name: &str) -> Option<PathBuf> {
        // Block absolute paths to prevent modules from
        // reading other files
        if Path::new(name).is_absolute() {
            return None;
        }
        let mut path = self.0.clone();
        path.push(module.module());
        for part in name.split('/') {
            path.push(part);
        }
        let path = match path.canonicalize() {
            Ok(val) => val,
            Err(_) => return None,
        };
        // Ensure the path stays within the folder
        if !path.starts_with(&self.0) {
            return None;
        }
        Some(path)
    }
}

impl Fetcher for DirFetcher {
    fn open(&self, module: ModuleKey<'_>, name: &str) -> Option<Asset> {
        let path = match self.clamp_path(module, name) {
            Some(val) => val,
            None => return None,
        };
        if let Ok(file) = fs::File::open(path) {
            Some(Asset::File(file))
        } else {
            None
        }
    }

    fn modified_time(&self, module: ModuleKey<'_>, name: &str) -> Option<SystemTime> {
        let path = match self.clamp_path(module, name) {
            Some(val) => val,
            None => return None,
        };
        fs::metadata(path).and_then(|v| v.modified()).ok()
    }
}

struct PackedFetcher {
    index: FNVMap<ResourceKey<'static>, (u64, u64)>,
    map: Arc<memmap::Mmap>,
}

impl PackedFetcher {
    fn new(name: &str) -> UResult<PackedFetcher> {
        let mut index = FNVMap::default();
        let mut fi = fs::File::open(&format!("./assets/packed/{}.index", name))?;
        let len = fi.read_u32::<LittleEndian>()?;
        for _ in 0 .. len {
            let slen = fi.read_u16::<LittleEndian>()?;
            let mut buf = vec![0; slen as usize];
            fi.read_exact(&mut buf)?;
            let name = String::from_utf8(buf)?;
            let name = name.trim_start_matches('/');
            let pos = name.char_indices().find(|v| v.1 == '/')
                .ok_or_else::<ErrorKind, _>(|| "Invalid file path in index".into())?;
            let (module, res) = name.split_at(pos.0);
            let key = ResourceKey::new(
                module,
                &res[1..]
            );
            index.insert(key.into_owned(), (
                fi.read_u64::<LittleEndian>()?,
                fi.read_u64::<LittleEndian>()?
            ));
        }
        Ok(PackedFetcher {
            index,
            map: Arc::new(unsafe {
                memmap::MmapOptions::new()
                    .map(&fs::File::open(&format!("./assets/packed/{}.assets", name))?)?
            }),
        })
    }
}

impl Fetcher for PackedFetcher {
    fn open(&self, module: ModuleKey<'_>, name: &str) -> Option<Asset> {
        let offset = self.index.get(&ResourceKey::new(module, name))?;
        let data = self.map.get(offset.0 as usize .. offset.0 as usize + offset.1 as usize)?;
        let data = unsafe {
            &*(data as *const [u8] as *const [u8])
        };
        let mapped = MappedAsset {
            _map: self.map.clone(),
            reader: io::Cursor::new(data),
        };
        Some(Asset::Mapped(mapped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_compare() {
        let a = ResourceKey::new("test", "key1");
        let b = ResourceKey::new("test", "key1");
        let c = ResourceKey::new("test", "key2");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(c, b);
    }

    #[test]
    fn key_owned() {
        let stack = ModuleKey::new("cake");
        let test = ResourceKey::new(stack.borrow(), "test").into_owned();
        fn is_owned(_: ResourceKey<'static>) {}
        is_owned(test);
    }

    #[test]
    fn key_lazy() {
        let lazy = LazyResourceKey::parse("test:key");
        let lazy_part = LazyResourceKey::parse("key");
        let full = ResourceKey::new("test", "key");

        assert_eq!(lazy, lazy_part.borrow().or_module(ModuleKey::new("test")));
        assert_eq!(lazy.borrow().or_module(ModuleKey::new("cake")), lazy_part.borrow().or_module(ModuleKey::new("test")));
        assert_eq!(lazy, full);
        assert_ne!(lazy, lazy_part);
        assert_ne!(lazy_part, full);
    }

    #[test]
    fn key_hash() {
        let mut map: FNVMap<ResourceKey<'static>, i32> = FNVMap::default();

        map.insert(ResourceKey::new("base", "test"), 5);

        let module = ModuleKey::new("base");
        let m1 = module.borrow();
        let m2 = module.borrow();
        let m3 = module.borrow();

        let r1 = ResourceKey::new(m1, "test");
        let r2 = ResourceKey::new(m2, "test".to_owned());

        assert_eq!(map.get(&r1), Some(&5));
        assert_eq!(map.get(&r2), Some(&5));
        assert_eq!(map.get(&r1.borrow()), Some(&5));

        let l1 = LazyResourceKey::new(None, "test");
        assert_eq!(map.get(&l1.or_module(m3)), Some(&5));
    }
}