use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::thread;

/// Installs a stderr filter that drops libasound (ALSA) spam lines like:
/// `ALSA lib pcm.c:... underrun occurred`
///
/// This prevents stderr output from corrupting the TUI screen while keeping
/// other stderr logs (including env_logger) working.
pub fn install_alsa_stderr_filter() {
    // Only install once.
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        // Duplicate the original stderr fd so we can forward non-ALSA lines.
        let orig_fd = libc::dup(libc::STDERR_FILENO);
        if orig_fd < 0 {
            return;
        }

        // Create a pipe and redirect stderr to its write end.
        let mut fds = [0i32; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            let _ = libc::close(orig_fd);
            return;
        }
        let read_fd = fds[0];
        let write_fd = fds[1];

        if libc::dup2(write_fd, libc::STDERR_FILENO) < 0 {
            let _ = libc::close(orig_fd);
            let _ = libc::close(read_fd);
            let _ = libc::close(write_fd);
            return;
        }

        // Close the extra write end; stderr now points to the pipe.
        let _ = libc::close(write_fd);

        // Move fds into Rust std types.
        let reader_file = File::from_raw_fd(read_fd);
        let mut orig_file = File::from_raw_fd(orig_fd);

        thread::spawn(move || {
            let mut reader = BufReader::new(reader_file);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        // Drop typical libasound spam.
                        // Some backends may prefix with brackets or other chars.
                        if line.contains("ALSA lib") {
                            continue;
                        }
                        let _ = orig_file.write_all(line.as_bytes());
                        let _ = orig_file.flush();
                    }
                    Err(_) => break,
                }
            }
        });

        // Prevent accidental close of the now-redirected stderr by any File drop.
        // (stderr is managed by the OS; we only own orig_file in the thread.)
        let _ = File::from_raw_fd(libc::STDERR_FILENO).into_raw_fd();
    });
}
