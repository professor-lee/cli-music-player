/// Deprecated: libasound error-hooking requires C-variadic callbacks, which are
/// not available on stable Rust.
///
/// Use `utils::stderr_filter::install_alsa_stderr_filter()` instead.
#[allow(dead_code)]
pub fn suppress_libasound_errors() {
    crate::utils::stderr_filter::install_alsa_stderr_filter();
}
