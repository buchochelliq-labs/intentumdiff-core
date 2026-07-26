use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // lib.rs uses include_str!(concat!(env!("OUT_DIR"), "/stdlib-symbols.txt"))
    fs::copy(
        PathBuf::from(&manifest).join("src/wasm/stdlib-symbols.txt"),
        out_dir.join("stdlib-symbols.txt"),
    )
    .expect("failed to copy stdlib-symbols.txt");

    let manifest_path = PathBuf::from(&manifest);
    let include_path = manifest_path.join("include");
    let src_path = manifest_path.join("src");

    if env::var("TARGET").as_deref() == Ok("wasm32-wasip2") {
        println!("cargo:rustc-link-lib=static=tree_sitter");
        println!("cargo:rustc-link-search=native={}/lib", manifest);
        println!("cargo:rerun-if-changed=lib/libtree_sitter.a");
    } else {
        let wasm_path = src_path.join("wasm");
        for entry in fs::read_dir(&src_path).expect("failed to read tree-sitter src") {
            let path = entry.expect("failed to read tree-sitter src entry").path();
            println!("cargo:rerun-if-changed={}", path.display());
        }

        cc::Build::new()
            .flag_if_supported("-std=c11")
            .flag_if_supported("-fvisibility=hidden")
            .flag_if_supported("-Wshadow")
            .flag_if_supported("-Wno-unused-parameter")
            .flag_if_supported("-Wno-incompatible-pointer-types")
            .include(&src_path)
            .include(&wasm_path)
            .include(&include_path)
            .define("_POSIX_C_SOURCE", "200112L")
            .define("_DEFAULT_SOURCE", None)
            .define("_DARWIN_C_SOURCE", None)
            .warnings(false)
            .file(src_path.join("lib.c"))
            .compile("tree-sitter");
    }

    // Downstream grammar build scripts may read DEP_TREE_SITTER_INCLUDE.
    println!("cargo:include={}", include_path.display());
}
