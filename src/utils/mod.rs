pub mod ascii_art;
pub mod input;
pub mod kitty;
#[cfg(target_os = "linux")]
pub mod stderr_filter;
#[cfg(not(target_os = "linux"))]
pub mod stderr_filter {
	pub fn install_alsa_stderr_filter() {}
}
pub mod system_volume;
pub mod timefmt;
