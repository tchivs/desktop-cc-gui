use std::collections::HashMap;
use tokio::process::Command;

const GEMINI_PROXY_CONNECT_TIMEOUT_MS: u64 = 180;
const GEMINI_PROXY_ENV_KEYS: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

fn is_proxy_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "http_proxy" | "https_proxy" | "all_proxy" | "no_proxy"
    )
}

fn parse_proxy_host_port(proxy: &str) -> Option<(String, u16)> {
    let trimmed = proxy.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = if let Some(index) = trimmed.find("://") {
        &trimmed[(index + 3)..]
    } else {
        trimmed
    };
    let without_path = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('?')
        .next()
        .unwrap_or(without_scheme)
        .split('#')
        .next()
        .unwrap_or(without_scheme);
    let authority = if let Some(index) = without_path.rfind('@') {
        &without_path[(index + 1)..]
    } else {
        without_path
    };
    if authority.is_empty() {
        return None;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = rest[..end].trim().to_string();
        if host.is_empty() {
            return None;
        }
        let tail = rest[(end + 1)..].trim();
        let Some(port_raw) = tail.strip_prefix(':') else {
            return None;
        };
        let port = port_raw.parse::<u16>().ok()?;
        return Some((host, port));
    }
    let separator = authority.rfind(':')?;
    let host = authority[..separator].trim().to_string();
    if host.is_empty() {
        return None;
    }
    let port = authority[(separator + 1)..].trim().parse::<u16>().ok()?;
    Some((host, port))
}

fn is_loopback_proxy_host(host: &str) -> bool {
    let normalized = host.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "127.0.0.1" | "localhost" | "::1")
}

fn is_loopback_proxy_reachable(host: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        if !addr.ip().is_loopback() {
            continue;
        }
        if TcpStream::connect_timeout(
            &addr,
            std::time::Duration::from_millis(GEMINI_PROXY_CONNECT_TIMEOUT_MS),
        )
        .is_ok()
        {
            return true;
        }
    }
    false
}

pub(crate) fn apply_dead_loopback_proxy_guard(
    cmd: &mut Command,
    vendor_env: &HashMap<String, String>,
) {
    if vendor_env.keys().any(|key| is_proxy_key(key)) {
        return;
    }

    let mut disabled_keys: Vec<String> = Vec::new();
    for key in GEMINI_PROXY_ENV_KEYS {
        let Some(value) = std::env::var(key)
            .ok()
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
        else {
            continue;
        };
        let Some((host, port)) = parse_proxy_host_port(&value) else {
            continue;
        };
        if !is_loopback_proxy_host(&host) {
            continue;
        }
        if is_loopback_proxy_reachable(&host, port) {
            continue;
        }
        disabled_keys.push(key.to_string());
    }

    if disabled_keys.is_empty() {
        return;
    }

    for key in GEMINI_PROXY_ENV_KEYS {
        cmd.env_remove(key);
    }
    log::warn!(
        "[gemini/send] disabled unreachable local proxy env for this turn: {}",
        disabled_keys.join(",")
    );
}
