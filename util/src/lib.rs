// # Clippy lints
#![allow(unknown_lints)]
// Mostly happens in the rendering code. Clippy's
// limit is pretty arbitrary.
#![allow(clippy::too_many_arguments)]
// Not a fan of this style
#![allow(clippy::new_without_default)]
// Not always bad, mainly style
#![allow(clippy::single_match)]
// Used to make sure debug is stripped out
#![allow(clippy::inline_always)]
// Clippy bug? https://github.com/Manishearth/rust-clippy/issues/1725
#![allow(clippy::get_unwrap)]
// Sometimes things get complex
#![allow(clippy::cyclomatic_complexity)]
// I generally use this correctly.
#![allow(clippy::float_cmp)]
// Not making a library
#![allow(clippy::should_implement_trait)]
#![allow(clippy::clone_on_ref_ptr)]
// Unwrap makes tracking crashes harder, use `assume!`
#![cfg_attr(not(test), deny(clippy::option_unwrap_used))]
#![cfg_attr(not(test), deny(clippy::result_unwrap_used))]

pub extern crate cgmath;
#[macro_use]
extern crate slog;
extern crate byteorder;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate delta_encode;
extern crate backtrace;
extern crate fnv;

mod location;
pub use self::location::*;
mod direction;
pub use self::direction::*;
mod bit;
pub use self::bit::*;
mod intmap;
pub use self::intmap::*;
mod result;
pub use self::result::*;
mod option;
pub use self::option::*;
mod ray;
pub use self::ray::*;
mod aabb;
pub use self::aabb::*;
mod color;
pub use self::color::*;
mod iter;
pub use self::iter::*;
mod angle;
pub use self::angle::*;

use slog::Logger;
use std::fmt::Debug;
use std::fs::OpenOptions;
use std::panic;
use std::time::{SystemTime, UNIX_EPOCH};

/// Alias for a `HashMap` using FNVHash as its hasher
pub type FNVMap<K, V> = fnv::FnvHashMap<K, V>;

/// Alias for a `HashSet` using FNVHash as its hasher
pub type FNVSet<K> = fnv::FnvHashSet<K>;

/// Alias for `fnv::FnvHasher` using the old name
pub type FNVHash = fnv::FnvHasher;

#[allow(dead_code)]
const REPORT_URL: &str = "http://localhost:8088/univercity/submit";

#[derive(Deserialize, Serialize)]
struct CrashReport {
    game_version: String,
    reason: String,
    file: String,
    line: u32,
    base_pointer: u64,
    backtrace: Vec<u64>,
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn base_anchor() {}

/// Logs panics to a log file before
pub fn log_panics(log: &Logger, game_version: &'static str, is_client: bool) {
    let log = log.new(o!("panic" => true));

    let old = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .cloned()
            .or_else(|| payload.downcast_ref::<String>().map(|v| v.as_str()))
            .unwrap_or("Box<Any>");
        let (file, line) = if let Some(loc) = info.location() {
            error!(log, "{}", msg; "file" => loc.file(), "line" => loc.line());
            (loc.file(), loc.line())
        } else {
            error!(log, "{}", msg);
            ("unknown", 0)
        };

        let mut frames = Vec::new();
        backtrace::trace(|frame| {
            frames.push(frame.ip() as u64);
            true
        });
        #[cfg(target_os = "windows")]
        let os = "windows";
        #[cfg(target_os = "linux")]
        let os = "linux";
        let report = CrashReport {
            game_version: format!(
                "{}-{}-{}",
                game_version,
                if is_client { "client" } else { "server" },
                os
            ),
            reason: msg.to_owned(),
            file: file.to_owned(),
            line: line,
            base_pointer: (base_anchor as *const ()) as u64,
            backtrace: frames,
        };
        // TODO:
        // let client = reqwest::Client::new();
        // let _ = client.post(REPORT_URL)
        //     .json(&report)
        //     .send();

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get the timestamp")
            .as_secs()
            / 10;
        let mut rf = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&format!("crash-report-{}.rpt", ts))
            .expect("Failed to create error report");
        serde_json::to_writer_pretty(&mut rf, &report)
            .expect("Failed to encode the report to json");
        rf.sync_all().expect("Failed to sync the report to disk");

        old(info);
    }));
}

/// Unwraps the passed value (`Result` or `Option`) assuming that it wont
/// fail. If it does fail the error will be logged and then a panic will be
/// triggered.
#[macro_export]
macro_rules! assume {
    ($log:expr, $e:expr) => {
        match $crate::Assumable::assume($e) {
            Ok(val) => val,
            Err(err) => $crate::Failable::fail(err, &$log, file!(), line!()),
        }
    };
}

#[doc(hidden)]
pub trait Assumable {
    type Value;
    type Error: Failable;

    fn assume(self) -> Result<Self::Value, Self::Error>;
}

#[doc(hidden)]
pub trait Failable {
    #[cold]
    fn fail(self, log: &Logger, file: &'static str, line: u32) -> !;
}

impl<V, E> Assumable for Result<V, E>
where
    E: Failable,
{
    type Value = V;
    type Error = E;

    #[inline(always)]
    fn assume(self) -> Result<Self::Value, Self::Error> {
        self
    }
}

impl<V> Assumable for Option<V> {
    type Value = V;
    type Error = ();

    #[inline(always)]
    fn assume(self) -> Result<Self::Value, Self::Error> {
        self.ok_or(())
    }
}

impl<T> Failable for T
where
    T: Debug,
{
    #[cold]
    #[inline(never)]
    fn fail(self, log: &Logger, file: &'static str, line: u32) -> ! {
        error!(log, "Assumption failed: {:?}", self; "file" => file, "line" => line, "assume" => true);
        panic!("{}:{}: Assumption failed: {:?}", file, line, self)
    }
}

#[test]
fn assume_test_result() {
    let log = Logger::root(slog::Discard, o!());

    let test: Result<i32, &'static str> = Ok(5);
    assume!(log, test);
}

#[test]
#[should_panic]
fn assume_test_result_fail() {
    let log = Logger::root(slog::Discard, o!());
    let test: Result<i32, &'static str> = Err("Should fail");
    assume!(log, test);
}

#[test]
fn assume_test_option() {
    let log = Logger::root(slog::Discard, o!());

    let test: Option<i32> = Some(5);
    assume!(log, test);
}

#[test]
#[should_panic]
fn assume_test_option_fail() {
    let log = Logger::root(slog::Discard, o!());
    let test: Option<i32> = None;
    assume!(log, test);
}
