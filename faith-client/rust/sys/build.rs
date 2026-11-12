use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn watch(path: &Path) {
    for entry in fs::read_dir(path).expect("read native source directory") {
        let path = entry.expect("native source entry").path();
        if path.is_dir() {
            watch(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../../..").canonicalize().unwrap();
    let client = root.join("faith-client");
    let proto = root.join("faith-proto");
    for path in [
        client.join("src"),
        client.join("include"),
        proto.join("src"),
        proto.join("include"),
    ] {
        watch(&path);
    }
    println!("cargo:rerun-if-changed=wrapper.h");
    let mut native = cc::Build::new();
    native
        .include(client.join("include"))
        .include(proto.join("include"))
        .include(proto.join("include/faith-proto"))
        .include(root.join("third_party"))
        .file(client.join("src/client.c"))
        .flag_if_supported("-std=gnu11")
        .flag_if_supported("-Wno-unused-function");
    for dir in [proto.join("src/codec"), proto.join("src/core")] {
        let mut sources: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "c"))
            .collect();
        sources.sort();
        native.files(sources);
    }
    native.compile("faith_client");
    for lib in ["ssl", "crypto", "pthread"] {
        println!("cargo:rustc-link-lib={lib}");
    }
    let bindings = bindgen::Builder::default()
        .header(manifest.join("wrapper.h").to_str().unwrap())
        .clang_arg(format!("-I{}", client.join("include").display()))
        .clang_arg(format!("-I{}", proto.join("include").display()))
        .clang_arg(format!("-I{}", root.join("third_party").display()))
        .allowlist_function("faith_client_.*")
        .allowlist_function("faith_(event_name|group_.*|random_bytes)")
        .allowlist_type("faith_.*")
        .allowlist_var("FAITH_.*")
        .prepend_enum_name(false)
        .derive_default(true)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("generate faith-client bindings");
    bindings
        .write_to_file(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("write faith-client bindings");
}
