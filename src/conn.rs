// Connection handling: hold the TCP stream with zero reads + periodic dribble.
// Logging is here and supports structured output for flexibility (text or json).
// IMPORTANT: We NEVER read from the stream. This is the core security property.

use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::sleep;

pub async fn hold(mut stream: TcpStream, dribble: u64) {
    let _ = stream.set_nodelay(true);
    let d = if dribble == 0 { 60 } else { dribble };
    loop {
        sleep(std::time::Duration::from_secs(d)).await;
        // dribble one byte; NEVER read their input -> their sends jam our tiny rcvbuf -> zero window
        if stream.write_all(b" ").await.is_err() {
            break;
        }
        let _ = stream.flush().await;
    }
}

/// Log an accepted connection. Format is controlled by log_format ("text" or "json").
/// Keeps backward compat with simple text while allowing structured logs.
pub fn log_conn(path: &str, ip: IpAddr, log_format: &str) {
    use std::io::Write;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|x| x.as_secs())
        .unwrap_or(0);

    let line = if log_format == "json" {
        format!(r#"{{"ts":{},"ip":"{}"}}"#, now, ip)
    } else {
        format!("{} {}", now, ip)
    };

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_log_text_format() {
        let tmp = std::env::temp_dir().join("tinypit_test_log_text.log");
        let _ = fs::remove_file(&tmp);
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10));
        log_conn(tmp.to_str().unwrap(), ip, "text");

        let content = fs::read_to_string(&tmp).unwrap_or_default();
        assert!(content.contains("203.0.113.10"));
        assert!(!content.contains("\"ip\"")); // not json
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_log_json_format() {
        let tmp = std::env::temp_dir().join("tinypit_test_log_json.log");
        let _ = fs::remove_file(&tmp);
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 11));
        log_conn(tmp.to_str().unwrap(), ip, "json");

        let content = fs::read_to_string(&tmp).unwrap_or_default();
        assert!(content.contains(r#""ip":"203.0.113.11""#));
        assert!(content.contains(r#""ts":"#));
        let _ = fs::remove_file(&tmp);
    }
}
