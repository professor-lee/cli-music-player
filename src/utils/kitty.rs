use std::env;

/// Best-effort detection for Kitty Graphics Protocol support.
///
/// We avoid active query/reply probing to keep input handling simple.
pub fn kitty_graphics_supported() -> bool {
    // kitty sets TERM=xterm-kitty and KITTY_WINDOW_ID.
    if env::var("KITTY_WINDOW_ID").is_ok() {
        return true;
    }

    if let Ok(term) = env::var("TERM") {
        let term_lc = term.to_ascii_lowercase();
        if term_lc.contains("kitty") {
            return true;
        }
    }

    // A few other terminals implement the protocol; keep this conservative.
    if let Ok(tp) = env::var("TERM_PROGRAM") {
        let tp_lc = tp.to_ascii_lowercase();
        if tp_lc.contains("wezterm") || tp_lc.contains("ghostty") || tp_lc.contains("warp") {
            return true;
        }
    }

    false
}
