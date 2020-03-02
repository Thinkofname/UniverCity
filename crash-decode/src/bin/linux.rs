extern crate rustc_demangle;

use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};

use symbolic::debuginfo::*;

fn main() {
    let mut args = env::args();
    args.next();

    let binary = args.next().expect("Missing binary");
    let report = args.next().expect("Missing report name");
    let file = BufReader::new(File::open(&report).unwrap());

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
    stack.remove(0);

    // First find the `base_anchor` address to use as a base
    let data = std::fs::read(&binary).unwrap();
    let obj = Object::parse(&data).unwrap();
    let base_addr = obj
        .symbols()
        .find(|v| v.name() == Some("base_anchor"))
        .map(|v| v.address)
        .unwrap();

    let diff = base_anchor - base_addr;

    // Now print the backtrace
    let debug = obj.debug_session().unwrap();

    for ip in stack.iter().cloned() {
        let ip = ip.0 - diff;

        let func = debug
            .functions()
            .filter_map(|v| v.ok())
            .find(|v| ip >= v.address && ip <= v.address + v.size);
        if let Some(func) = func {
            let sym = func.name.as_str();
            if let Ok(name) = rustc_demangle::try_demangle(&sym) {
                println!("{}", name);
            } else {
                println!("{}", sym);
            }
            if let Some(line) = func
                .lines
                .iter()
                .skip_while(|v| v.address < ip || v.line == 0)
                .next()
            {
                println!("    at: {}:{}", line.file.path_str(), line.line);
            }
        } else {
            println!("Missing info");
        }
        println!("    ip: 0x{:x}", ip);
    }
}
