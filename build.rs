fn main() {
    // Set build timestamp
    println!(
        "cargo:rustc-env=BUILD_TIMESTAMP={}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
}
