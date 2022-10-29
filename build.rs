fn main() {
    let is_tag = format!(
        "\"RBK-ICAP-V-{}{}{}\"",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    println!("cargo:rustc-env=DEFAULT_IS_TAG={}", is_tag);
}
