const NO_DEV_ERROR_CHECKS_CFG: &str = "smoldata_int_dev_error_checks";

fn main() {
    println!("cargo::rustc-check-cfg=cfg({})", NO_DEV_ERROR_CHECKS_CFG);

    #[cfg(not(any(
        feature = "no_dev_error_checks",
        all(not(debug_assertions), feature = "no_dev_error_checks_on_release")
    )))]
    {
        println!("cargo::rustc-cfg={}", NO_DEV_ERROR_CHECKS_CFG);
    }
}
