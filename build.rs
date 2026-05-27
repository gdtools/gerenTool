use std::{env, fs, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .expect("Failed to find target dir");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_src = Path::new(&manifest_dir).join("lib");

    let dlls = ["dav1d.dll"];
    for dll in &dlls {
        let src = lib_src.join(dll);
        let dst = target_dir.join(dll);
        if src.exists() {
            fs::copy(&src, &dst).ok();
        }
    }

    println!("cargo:rerun-if-changed=lib");
}
