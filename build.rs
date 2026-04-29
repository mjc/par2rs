use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(parpar_compare_embedded)");

    if env::var_os("CARGO_FEATURE_PARPAR_COMPARE").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let parpar = manifest_dir.join("par2cmdline-turbo/parpar");
    let hasher = parpar.join("hasher");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let md5_final = hasher.join("md5-final.c");

    if target_arch != "x86_64" || !md5_final.exists() {
        return;
    }

    println!("cargo:rustc-cfg=parpar_compare_embedded");

    for path in [
        "src/ffi/wrapper.cpp",
        "par2cmdline-turbo/parpar/hasher/hasher_input.cpp",
        "par2cmdline-turbo/parpar/hasher/hasher_md5crc.cpp",
        "par2cmdline-turbo/parpar/hasher/hasher_scalar.cpp",
        "par2cmdline-turbo/parpar/hasher/hasher_sse.cpp",
        "par2cmdline-turbo/parpar/hasher/hasher_bmi1.cpp",
        "par2cmdline-turbo/parpar/hasher/hasher_clmul.cpp",
        "par2cmdline-turbo/parpar/hasher/tables.cpp",
        "par2cmdline-turbo/parpar/hasher/crc_zeropad.c",
        "par2cmdline-turbo/parpar/hasher/hasher_input_impl.h",
        "par2cmdline-turbo/parpar/hasher/hasher_input.h",
        "par2cmdline-turbo/parpar/hasher/hasher_md5crc.h",
        "par2cmdline-turbo/parpar/hasher/hasher_md5crc_impl.h",
        "par2cmdline-turbo/parpar/hasher/hasher_input_base.h",
        "par2cmdline-turbo/parpar/hasher/md5x2-base.h",
        "par2cmdline-turbo/parpar/hasher/md5x2-scalar.h",
        "par2cmdline-turbo/parpar/hasher/md5x2-sse.h",
        "par2cmdline-turbo/parpar/hasher/crc_slice4.h",
        "par2cmdline-turbo/parpar/hasher/crc_clmul.h",
        "par2cmdline-turbo/parpar/hasher/crc_zeropad.h",
        "par2cmdline-turbo/parpar/src/platform.h",
        "par2cmdline-turbo/parpar/src/hedley.h",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let mut c_build = cc::Build::new();
    c_build.include(&manifest_dir);
    c_build.include(&parpar);
    c_build.include(&hasher);
    c_build.file(&md5_final);
    c_build.file(hasher.join("crc_zeropad.c"));
    c_build.compile("parpar_helpers");

    let mut cpp_build = cc::Build::new();
    cpp_build.cpp(true);
    cpp_build.std("c++17");
    cpp_build.define("PARPAR_ENABLE_HASHER_MD5CRC", None);
    cpp_build.include(&manifest_dir);
    cpp_build.include(&parpar);
    cpp_build.include(&hasher);

    if target_arch == "x86_64" {
        cpp_build.flag_if_supported("-msse2");
        cpp_build.flag_if_supported("-mssse3");
        cpp_build.flag_if_supported("-msse4.1");
        cpp_build.flag_if_supported("-mpclmul");
        cpp_build.flag_if_supported("-mbmi");
        cpp_build.flag_if_supported("-mbmi2");
        cpp_build.flag_if_supported("-mavx");
    }

    cpp_build
        .file(manifest_dir.join("src/ffi/wrapper.cpp"))
        .file(hasher.join("hasher_input.cpp"))
        .file(hasher.join("hasher_md5crc.cpp"))
        .file(hasher.join("hasher_scalar.cpp"))
        .file(hasher.join("hasher_sse.cpp"))
        .file(hasher.join("hasher_bmi1.cpp"))
        .file(hasher.join("hasher_clmul.cpp"))
        .file(hasher.join("tables.cpp"))
        .compile("parpar_wrapper");

    let mut avx512_build = cc::Build::new();
    avx512_build.cpp(true);
    avx512_build.std("c++17");
    avx512_build.include(&manifest_dir);
    avx512_build.include(&parpar);
    avx512_build.include(&hasher);
    avx512_build.flag_if_supported("-msse2");
    avx512_build.flag_if_supported("-mssse3");
    avx512_build.flag_if_supported("-msse4.1");
    avx512_build.flag_if_supported("-mpclmul");
    avx512_build.flag_if_supported("-mbmi");
    avx512_build.flag_if_supported("-mavx512f");
    avx512_build.flag_if_supported("-mavx512vl");
    avx512_build.flag_if_supported("-mavx512bw");
    avx512_build.flag_if_supported("-mavx512dq");
    avx512_build.flag_if_supported("-mbmi2");
    avx512_build
        .file(hasher.join("hasher_avx512.cpp"))
        .file(hasher.join("hasher_avx512vl.cpp"))
        .compile("parpar_avx512");
}
