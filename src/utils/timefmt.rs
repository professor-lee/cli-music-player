use std::time::Duration;

pub fn mmss(d: Duration) -> String {
    let secs = d.as_secs();
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}
