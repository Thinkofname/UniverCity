
extern crate cc;

use std::path::{Path, PathBuf};
use std::env;
use std::process::{Stdio, Command};
use std::fs;

fn main() {
    let cur_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let cur_path = Path::new(&cur_dir);

    let triple = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();

    let out_dir = env::var("OUT_DIR").unwrap();
    let mut source_dir = PathBuf::from(out_dir).join("source");
    if let Ok(or) = env::var("LUAJIT_CUSTOM_PATH") {
        source_dir = PathBuf::from(or);
    } else {
        fs::create_dir_all(&source_dir).unwrap();
        if !source_dir.join(".git").exists() {
            fs::create_dir_all("./target").unwrap();
            assert!(Command::new("git")
                .arg("clone")
                .arg("-b").arg("v2.1")
                .arg("--single-branch")
                .arg("https://github.com/LuaJIT/LuaJIT.git")
                .arg(&source_dir)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .expect("Failed to run git clone").success());

            if triple.contains("windows") && host.contains("linux") {
                assert!(Command::new("git")
                    .current_dir(&source_dir)
                    .arg("am")
                    .arg(cur_path.join("patches/0001-clang-cl-build-fixes.patch"))
                    .status()
                    .expect("Failed to run git am").success());
            }
        }
        if triple.contains("windows") && host.contains("linux") {
            build_msvc(&source_dir);
        } else {
            build(cur_path, &source_dir);
        }
    }

    if triple.contains("windows") {
        println!("cargo:rustc-link-lib=static=lua51");
    } else {
        println!("cargo:rustc-link-lib=static=luajit");
    }

    println!("cargo:rustc-link-search=native={}", source_dir.join("src").to_string_lossy());
}

fn build_msvc(source_dir: &Path) {
    let src = source_dir.join("src");

    // Build minilua
    assert!(Command::new("cc")
        .arg(src.join("host/minilua.c"))
        .arg("-lm")
        .arg("-o")
        .arg(src.join("host/minilua"))
        .status()
        .unwrap()
        .success());

    // Run dynasm
    assert!(Command::new(src.join("host/minilua"))
        .current_dir(&src)
        .arg(src.join("../dynasm/dynasm.lua"))
        .arg("-LN")
        .arg("-D")
        .arg("WIN")
        .arg("-D")
        .arg("JIT")
        .arg("-D")
        .arg("FFI")
        .arg("-D")
        .arg("P64")
        .arg("-o")
        .arg("host/buildvm_arch.h")
        .arg("vm_x86.dasc")
        .status()
        .unwrap()
        .success());

    // Build buildvm
    assert!(Command::new("cc")
        .current_dir(&src)
        .arg(src.join("host/buildvm_asm.c"))
        .arg(src.join("host/buildvm_fold.c"))
        .arg(src.join("host/buildvm_lib.c"))
        .arg(src.join("host/buildvm_peobj.c"))
        .arg(src.join("host/buildvm.c"))
        .arg("-I")
        .arg(".")
        .arg("-I")
        .arg("../dynasm")
        .arg("-o")
        .arg(src.join("buildvm"))
        .status()
        .unwrap()
        .success());


    assert!(Command::new(src.join("buildvm"))
        .current_dir(&src)
        .arg("-m")
        .arg("peobj")
        .arg("-o")
        .arg("lj_vm.obj")
        .status()
        .unwrap()
        .success());

    for (m, h) in &[
        ("bcdef", "lj_bcdef.h"),
        ("ffdef", "lj_ffdef.h"),
        ("libdef", "lj_libdef.h"),
        ("recdef", "lj_recdef.h"),
        ("vmdef", "jit/vmdef.lua"),
    ] {
        assert!(Command::new(src.join("buildvm"))
            .current_dir(&src)
            .arg("-m")
            .arg(m)
            .arg("-o")
            .arg(h)
            .args(&[
                "lib_base.c", "lib_math.c",
                "lib_bit.c", "lib_string.c",
                "lib_table.c", "lib_io.c", "lib_os.c",
                "lib_package.c", "lib_debug.c",
                "lib_jit.c", "lib_ffi.c"])
            .status()
            .unwrap()
            .success());
    }

    assert!(Command::new(src.join("buildvm"))
        .current_dir(&src)
        .arg("-m")
        .arg("folddef")
        .arg("-o")
        .arg("lj_folddef.h")
        .arg("lj_opt_fold.c")
        .status()
        .unwrap()
        .success());

    cc::Build::new()
        .files(glob::glob(&src.join("lj_*.c").to_string_lossy()).unwrap().chain(glob::glob(&src.join("lib_*.c").to_string_lossy()).unwrap()).filter_map(Result::ok))
        .object(src.join("lj_vm.obj"))
        .flag("-fms-compatibility")
        .flag("/D_CRT_SECURE_NO_DEPRECATE")
        .flag("/D_CRT_STDIO_INLINE=__declspec(dllexport)__inline")
        .compile("lua51");
}

#[cfg(target_os = "windows")]
fn build(_cur_path: &Path, source_dir: &Path) {
    assert!(Command::new(source_dir.join("src").join("msvcbuild.bat"))
        .current_dir(source_dir.join("src"))
        .arg("static")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("Failed to run make").success());
}

#[cfg(not(target_os = "windows"))]
fn build(_cur_path: &Path, source_dir: &Path) {
    let target_triple = env::var("TARGET").unwrap();
    let target_sys = if target_triple.contains("windows") {
        "Windows"
    } else if target_triple.contains("linux") {
        "Linux"
    } else if target_triple.contains("darwin") {
        return;
    } else {
        panic!("Unsupported OS")
    };
    let source_dir = source_dir.canonicalize().unwrap();
    let mut cmd = Command::new("make");
    cmd.current_dir(&source_dir)
       .arg(format!("TARGET_SYS={}", target_sys));
    if target_triple != env::var("HOST").unwrap() {
        let config = cc::Build::new();
        let tool = config.get_compiler();
        let path = tool.path().to_string_lossy().to_owned();
        let mut cross_args = format!("{}", tool.args().iter()
                .map(|v| v.to_string_lossy())
                .fold("".to_owned(), |mut a, b| {a.push_str(&*b); a.push(' '); a}));
        if path == "cc" {
            cmd.arg(format!("CC=cc {}", cross_args));
        } else {
            if target_triple.contains("windows") && target_triple.contains("i686") {
                cross_args.push_str(" -static-libgcc");
            }
            cmd.arg(format!("CC=gcc"))
                .arg(format!("CROSS={}", path.replace("gcc", "")))
                .arg(format!("TARGET_CFLAGS={}", cross_args));
            if target_triple.contains("arm") || target_triple.contains("i686") {
                cmd.arg("HOST_CC=gcc -m32");
            }
        }
        cmd.arg("BUILDMODE=static");
        if target_triple.contains("musl") {
            cmd.arg(format!("TARGET_AR={}", "ar rcus"));
            cmd.arg("TARGET_STRIP=strip");
            cmd.arg("LDFLAGS=-static");
        }
    }
    if env::var("PROFILE") == Ok("debug".into()) {
        cmd.arg("XCFLAGS=-DLUA_USE_APICHECK");
    }

    println!("Executing: {:?}", cmd);
    assert!(cmd.stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("Failed to run make").success());
}