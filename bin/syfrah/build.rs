fn main() {
    // Allow overriding the version at build time via SYFRAH_VERSION env var
    if let Ok(v) = std::env::var("SYFRAH_VERSION") {
        println!("cargo:rustc-env=CARGO_PKG_VERSION={v}");
    }
}
