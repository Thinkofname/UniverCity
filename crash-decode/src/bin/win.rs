extern crate rustc_demangle;
extern crate winapi;

use std::env;
use std::fs::File;
use std::io::{BufReader, BufRead};
use std::ptr;
use std::mem::{self, zeroed};

use winapi::um::dbghelp::*;
use winapi::um::winnt::*;
use winapi::um::errhandlingapi::GetLastError;
use winapi::shared::minwindef::*;
use winapi::shared::basetsd::DWORD64;
use winapi::shared::guiddef::GUID;

use std::ffi::*;
use std::os::windows::ffi::*;
use std::slice;

extern "C" {
    fn SymLoadModuleExW(process: HANDLE, file: HANDLE, image_name: PCWSTR, module_name: PCWSTR, base_of_dll: DWORD64, size: DWORD, data: PMODLOAD_DATA, flags: DWORD) -> DWORD64;
    fn SymUnloadModule64(process: HANDLE, base: DWORD64) -> BOOL;
    fn SymSetOptions(options: DWORD) -> DWORD;
    fn SymSetSearchPathW(process: HANDLE, search_path: PCWSTR) -> BOOL;
    fn SymGetSearchPathW(process: HANDLE, search_path: PCWSTR, len: DWORD64) -> BOOL;
    fn SymGetLineFromAddrW64(process: HANDLE, addr: DWORD64, displacement: *mut DWORD, line: *mut IMAGEHLP_LINEW64) -> BOOL;
    fn SymFromNameW(process: HANDLE, name: PCWSTR, symbol: *mut SYMBOL_INFOW) -> BOOL;
}

fn main() {
    let mut args = env::args();
    args.next();

    let binary_name = args.next().expect("Missing binary");
    let report = args.next().expect("Missing report name");
    let file = BufReader::new(File::open(&report).unwrap());

    let mut binary = env::current_dir().unwrap();
    binary.push(binary_name);

    let mut lines = file.lines().skip(1);

    println!("{}", lines.next().and_then(|v| v.ok()).unwrap());
    println!("{}", lines.next().and_then(|v| v.ok()).unwrap());

    let base_anchor = {
        let line = lines.next().and_then(|v| v.ok()).unwrap();
        let mut parts = line.split(' ');
        parts.next();
        let base = parts.next().unwrap();
        let base = u64::from_str_radix(&base[2..], 16).unwrap();
        base
    };

    let mut stack = vec![];

    for line in lines
        .filter_map(|v| v.ok())
        .filter(|v| v.starts_with("BT: "))
    {
        let mut parts = line.split(' ');
        parts.next();
        let ip = parts.next().unwrap();
        let ip = u64::from_str_radix(&ip[2..], 16).unwrap();
        let sym = parts.next().unwrap();
        let sym = u64::from_str_radix(&sym[2..], 16).unwrap();
        stack.push((ip, sym));
    }

    stack.reverse();

    // Now print the backtrace
    unsafe {
        let process = winapi::um::processthreadsapi::GetCurrentProcess();

        SymSetOptions(0x10 | 0x80000000 | 0x40);
        if SymInitializeW(process, ptr::null(), 0) == 0 {
            panic!("Err: {:?}", GetLastError());
        }

        let mut path = vec![0; 1024*1024];
        if SymGetSearchPathW(process, path.as_mut_ptr(), path.len() as _) == 0 {
            panic!("Err: {:?}", GetLastError());
        }
        let path = OsString::from_wide(&path[..path.iter().position(|&v| v == 0).unwrap_or(0)]);
        let mut search_path = path.to_string_lossy().into_owned();
        search_path.push(';');
        search_path.push_str(&env::current_dir().unwrap().to_string_lossy());

        let search_path: Vec<u16> = OsStr::new(&search_path).encode_wide().chain(Some(0)).collect();
        if SymSetSearchPathW(process, search_path.as_ptr()) == 0 {
            panic!("Err: {:?}", GetLastError());
        }

        let binary_w: Vec<u16> = OsStr::new(&binary).encode_wide().chain(Some(0)).collect();
        let module_base = SymLoadModuleExW(process, ptr::null_mut(), binary_w.as_ptr(), ptr::null(), 0, 0, ptr::null_mut(), 0);
        if module_base == 0 {
            panic!("Err: {:?}", GetLastError());
        }

        let diff = {
            let mut buffer = vec![0; mem::size_of::<SYMBOL_INFOW>() + MAX_SYM_NAME * mem::size_of::<WCHAR>()];
            let info: &mut SYMBOL_INFOW = &mut *(buffer.as_mut_ptr() as *mut _);
            info.SizeOfStruct = mem::size_of::<SYMBOL_INFOW>() as u32;
            info.MaxNameLen = MAX_SYM_NAME as u32;
            let base_anchor_name: Vec<u16> = OsStr::new("base_anchor").encode_wide().chain(Some(0)).collect();
            if SymFromNameW(process, base_anchor_name.as_ptr(), info) == 0 {
                panic!("Failed to get base symbol: {}", GetLastError());
            }
            base_anchor - info.Address
        };

        for ip in stack.iter().cloned() {
            let ip = ip.0;

            let mut line: IMAGEHLP_LINEW64 = zeroed();
            line.SizeOfStruct = mem::size_of::<IMAGEHLP_LINEW64>() as _;
            let mut displacement = 0;
            let (file, line_no) = if SymGetLineFromAddrW64(process, ip - diff, &mut displacement, &mut line) != 0 {
                let mut len = 0;
                while *line.FileName.offset(len) != 0 {
                    len += 1;
                }
                let name: &[u16] = slice::from_raw_parts(line.FileName, len as usize);
                let name = OsString::from_wide(name);
                (
                    name.to_string_lossy().into_owned(),
                    line.LineNumber.to_string(),
                )
            } else {
                ("??".into(), "?".into())
            };

            let mut buffer = vec![0; mem::size_of::<SYMBOL_INFOW>() + MAX_SYM_NAME * mem::size_of::<WCHAR>()];
            let info: &mut SYMBOL_INFOW = &mut *(buffer.as_mut_ptr() as *mut _);
            info.SizeOfStruct = mem::size_of::<SYMBOL_INFOW>() as u32;
            info.MaxNameLen = MAX_SYM_NAME as u32;

            let name = if SymFromAddrW(process, ip as DWORD64 - diff, ptr::null_mut(), info) == 0 {
                "unknown".into()
            } else {
                let name: &[u16] = slice::from_raw_parts(info.Name.as_ptr(), info.NameLen as usize);
                let name = OsString::from_wide(name);
                name.to_string_lossy().into_owned()
            };


            println!("{}", name);
            println!("    at: {}:{}", file, line_no);
            println!("    ip: 0x{:x}", ip);
        }

        SymUnloadModule64(process, module_base);
        SymCleanup(process);
    }
}