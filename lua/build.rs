extern crate cc;

fn main() {
    use std::env;
    use std::path::PathBuf;

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    cc::Build::new()
        .file("src/lib.c")
        .compile("luahandler");
    println!("cargo:rustc-link-lib=static=luahandler");
    println!("cargo:rustc-link-search={}", out.display());
}