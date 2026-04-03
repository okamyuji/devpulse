pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.1}GB", bytes as f64 / 1_000_000_000.0) }
    else if bytes >= 1_000_000 { format!("{:.0}MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.0}KB", bytes as f64 / 1_000.0) }
    else { format!("{}B", bytes) }
}
