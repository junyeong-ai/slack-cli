/// The release asset this build updates itself from.
///
/// Derived from the compiler's own view of the host rather than probed at
/// runtime: a musl build must replace itself with a musl build, and only the
/// compiler knows which one this is.
pub const fn current() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some("aarch64-apple-darwin");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Some("x86_64-apple-darwin");
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    return Some("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
    return Some("aarch64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "musl"))]
    return Some("x86_64-unknown-linux-musl");
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "musl"))]
    return Some("aarch64-unknown-linux-musl");
    #[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
    return Some("x86_64-pc-windows-msvc");

    #[cfg(not(any(
        all(
            target_os = "macos",
            any(target_arch = "aarch64", target_arch = "x86_64")
        ),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64"),
            any(target_env = "gnu", target_env = "musl")
        ),
        all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
    )))]
    return None;
}

/// The file name a release publishes the executable itself under, as opposed
/// to the archive beside it.
pub fn binary_asset(version: &str, target: &str) -> String {
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    format!("slack-cli-v{version}-{target}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_build_knows_its_own_release_target() {
        let target = current().expect("a supported host");
        assert!(!target.is_empty());
        #[cfg(target_os = "macos")]
        assert!(target.ends_with("-apple-darwin"));
        #[cfg(target_os = "linux")]
        assert!(target.contains("-unknown-linux-"));
        #[cfg(target_os = "windows")]
        assert!(target.contains("-pc-windows-"));
    }

    #[test]
    fn the_windows_asset_carries_an_executable_extension() {
        assert_eq!(
            binary_asset("0.9.0", "x86_64-pc-windows-msvc"),
            "slack-cli-v0.9.0-x86_64-pc-windows-msvc.exe"
        );
        assert_eq!(
            binary_asset("0.9.0", "aarch64-apple-darwin"),
            "slack-cli-v0.9.0-aarch64-apple-darwin"
        );
    }
}
