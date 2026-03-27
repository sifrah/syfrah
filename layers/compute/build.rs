use std::path::Path;

fn main() {
    // Read CLOUD_HYPERVISOR_VERSION from repo root and expose it as a compile-time env var.
    // The path is relative to the crate's Cargo.toml (layers/compute/).
    let version_file = Path::new("../../CLOUD_HYPERVISOR_VERSION");

    println!("cargo:rerun-if-changed={}", version_file.display());

    if let Ok(contents) = std::fs::read_to_string(version_file) {
        let version = contents.trim();
        println!("cargo:rustc-env=CLOUD_HYPERVISOR_VERSION={version}");
    } else {
        // Fallback: if the file is missing, fail the build with a clear message.
        panic!(
            "CLOUD_HYPERVISOR_VERSION file not found at repo root. \
             Expected at: {}",
            version_file.display()
        );
    }
}
