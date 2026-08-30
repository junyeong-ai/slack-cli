/// Hands the URL to whatever the platform opens links with, reporting only
/// whether that worked: the caller prints the URL either way, so a failure
/// carries nothing the fallback does not already say.
pub fn open(url: &str) -> bool {
    open::that(url).is_ok()
}
