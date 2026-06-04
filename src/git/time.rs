use std::time::SystemTime;

/// Compact, Instagram-style relative time, e.g. `2d ago`, `3w ago`, `now`.
pub fn relative(t: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if secs < 60 {
        "now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else if secs < 604_800 {
        format!("{}d ago", secs / 86_400)
    } else if secs < 2_592_000 {
        format!("{}w ago", secs / 604_800)
    } else if secs < 31_536_000 {
        format!("{}mo ago", secs / 2_592_000)
    } else {
        format!("{}y ago", secs / 31_536_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ago(secs: u64) -> String {
        relative(SystemTime::now() - Duration::from_secs(secs))
    }

    #[test]
    fn formats_buckets() {
        assert_eq!(ago(10), "now");
        assert_eq!(ago(120), "2m ago");
        assert_eq!(ago(7_200), "2h ago");
        assert_eq!(ago(172_800), "2d ago");
        assert_eq!(ago(1_209_600), "2w ago");
    }
}
