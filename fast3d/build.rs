fn main() {
    println!("cargo::rustc-check-cfg=cfg(fast3d_repository_tests)");

    let repository_tests = std::path::Path::new("src/tests/mod.rs");
    if repository_tests.is_file() {
        println!("cargo::rerun-if-changed=src/tests/mod.rs");
    } else {
        // Track an existing ancestor so packaged builds remain cacheable while a source checkout
        // still notices if the repository-only test module appears after being absent.
        println!("cargo::rerun-if-changed=src");
    }

    // The external test module and its data are intentionally repository-only. Keep the full
    // suite enabled in a source checkout, while allowing a packaged crate to compile and run the
    // inline unit tests that remain in its production source files.
    if repository_tests.is_file() {
        println!("cargo::rustc-cfg=fast3d_repository_tests");
    }
}
