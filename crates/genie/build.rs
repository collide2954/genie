fn main() {
    println!("cargo:rerun-if-changed=assets/icon/icon.res");
    let target = |key: &str, value: &str| std::env::var(key).is_ok_and(|found| found == value);
    if target("CARGO_CFG_TARGET_OS", "windows") && target("CARGO_CFG_TARGET_ENV", "msvc") {
        let folder = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        println!("cargo:rustc-link-arg-bins={folder}/assets/icon/icon.res");
    }
}
