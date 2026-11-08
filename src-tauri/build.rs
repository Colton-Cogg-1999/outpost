fn main() {
    // The built-in engine is named after the target triple (Tauri's sidecar convention).
    println!("cargo:rustc-env=OUTPOST_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    tauri_build::build()
}
