use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-link-search=native=../build/");
    println!("cargo:rustc-link-lib=faith_client");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg("-I../include/")
        .clang_arg("-I../../third_party/")
        .clang_arg("-I../../faith-proto/include/")

        .allowlist_function("faith_client_.*")
        .allowlist_function("faith_event_name")
        .allowlist_type("faith_client_.*")
        .allowlist_type("faith_event_.*")
        .allowlist_type("faith_status_code_t")
        .allowlist_type("faith_auth_id_t")
        .allowlist_var("FAITH_.*")

        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("unable to generate faith-client bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("couldn't write bindings");
}
