//! Delivering a message straight into a running Claude Code session.
//!
//! Claude Code 2.1 gives every session an inbox: a Unix socket it registers
//! in `~/.claude/sessions/<pid>.json` next to a `<pid>.<hash>.key` file
//! holding the token a peer must present. A line of JSON on that socket
//! lands exactly as a prompt typed at the terminal: it starts a turn when
//! Claude is idle and waits its turn when Claude is busy. That makes it the
//! right channel for the composer when a terminal session is behind the
//! transcript: no hooks, no second writer, and the message reaches the
//! transcript as an ordinary user row.
//!
//! The wire is newline-delimited JSON, one connection per message:
//!
//! ```text
//! {"type": "auth", "token": "<peerToken from the .key file>"}
//! {"type": "user", "message": {"role": "user", "content": "..."}}
//! ```
//!
//! The content is wrapped in Claude Code's own `<cross-session-message>`
//! envelope, which is what makes the transcript row carry `origin.name` and
//! a clean `origin.body`. The envelope deliberately asserts no permission
//! mode: Claude Code holds a message that asserts none when the recipient
//! runs with permissions bypassed and asks in the terminal first. That check
//! stops a less trusted process from steering a more trusted session, and
//! Emaki is such a process as far as Claude Code can tell. The user's own
//! switch is `"crossSessionInbound": "accept"` in their settings.
//!
//! The token is read at send time and never stored or logged.
//!
//! The socket is a Unix socket, so the channel exists only on Unix. Elsewhere
//! `registry` is empty and `send` refuses, and the window falls through to
//! the driver: how Claude Code exposes an inbox on Windows is not known yet.

use std::collections::HashMap;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use serde_json::json;
use serde_json::Value;

use crate::paths;

#[cfg(unix)]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to listen for a receipt after the message is written. The inbox
/// usually answers within milliseconds; silence is not failure.
#[cfg(unix)]
const REPLY_WAIT: Duration = Duration::from_millis(1500);

/// Whether this build can talk to an inbox at all.
pub fn available() -> bool {
    cfg!(unix)
}

/// The name the envelope carries. The builder keys on it (`origin.name`).
pub const SENDER_NAME: &str = "emaki";
const ENVELOPE_TAG: &str = "cross-session-message";
/// Permission modes under which Claude Code holds an inbound message for
/// approval in the terminal instead of delivering it straight away.
pub const HELD_MODES: &[&str] = &["bypassPermissions", "auto"];

/// A live session's inbox, as published in the registry.
#[derive(Debug, Clone)]
pub struct Peer {
    pub session_id: String,
    pub pid: i32,
    pub socket_path: String,
    pub cwd: String,
    pub kind: String,
    pub name: String,
    /// `idle` or `busy`, as Claude Code keeps it: whether a turn is running.
    pub status: String,
    /// When the status last changed, Unix seconds; 0 when the record has none.
    pub status_at: f64,
    pub proc_start: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct Delivery {
    pub msg_id: String,
    pub status: String,
    pub receipts: Vec<Value>,
}

/// Wrap `text` exactly as Claude Code's own sender would. The receiver
/// re-serialises what it parsed and compares byte for byte, so the attribute
/// order and the newlines are not negotiable. A closing tag inside the body
/// is defused the same way Claude Code does it.
pub fn envelope(text: &str) -> String {
    let clean: String = SENDER_NAME.chars().filter(|c| !matches!(c, '"' | '<' | '>')).collect();
    let head = if clean.trim().is_empty() { String::new() } else { format!(" from-name=\"{}\"", clean.trim()) };
    let body = text.replace(&format!("</{ENVELOPE_TAG}"), "<\\");
    format!("<{ENVELOPE_TAG}{head}>\n{body}\n</{ENVELOPE_TAG}>")
}

pub fn sessions_dir() -> PathBuf {
    paths::claude_home().join("sessions")
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // Signal 0 probes without delivering. EPERM still means a process exists.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
/// No inbox can be reached from here, so no record counts as live.
#[cfg(not(unix))]
fn alive(_pid: i32) -> bool {
    false
}

/// Every session on this machine that currently has an inbox, by id.
///
/// A record whose process is gone, whose socket is missing, or whose key
/// file's `procStart` disagrees with the record (a reused pid) is skipped:
/// delivering to the wrong process is worse than not delivering.
pub fn registry() -> HashMap<String, Peer> {
    let folder = sessions_dir();
    let mut peers = HashMap::new();
    let Ok(entries) = std::fs::read_dir(&folder) else { return peers };
    let names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    let mut keys: HashMap<i32, String> = HashMap::new();
    for name in &names {
        if let Some(stem) = name.strip_suffix(".key") {
            if let Some((head, _)) = stem.split_once('.') {
                if let Ok(pid) = head.parse::<i32>() {
                    keys.insert(pid, folder.join(name).to_string_lossy().to_string());
                }
            }
        }
    }
    for name in &names {
        if !name.ends_with(".json") {
            continue;
        }
        let Some(record) = paths::read_json(&folder.join(name)) else { continue };
        let Some(obj) = record.as_object() else { continue };
        let pid = obj.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let sid = obj.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sock = obj.get("messagingSocketPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if sid.is_empty() || sock.is_empty() || !alive(pid) || !std::path::Path::new(&sock).exists() {
            continue;
        }
        let s = |k: &str| obj.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        peers.insert(
            sid.clone(),
            Peer { session_id: sid, pid, socket_path: sock, cwd: s("cwd"), kind: s("kind"), name: s("name"), status: s("status"), status_at: obj.get("statusUpdatedAt").and_then(|v| v.as_f64()).unwrap_or(0.0) / 1000.0, proc_start: s("procStart"), key_path: keys.get(&pid).cloned().unwrap_or_default() },
        );
    }
    peers
}

#[cfg(unix)]
fn token(peer: &Peer) -> String {
    if peer.key_path.is_empty() {
        return String::new();
    }
    let Some(data) = paths::read_json(std::path::Path::new(&peer.key_path)) else { return String::new() };
    let start = data.get("procStart").and_then(|v| v.as_str()).unwrap_or("");
    if !peer.proc_start.is_empty() && !start.is_empty() && start != peer.proc_start {
        return String::new();
    }
    data.get("peerToken").and_then(|v| v.as_str()).unwrap_or("").to_string()
}

/// Put `text` in front of the session. `Ok` means the inbox accepted the
/// connection and read the message; a refusal carries the reason the inbox
/// gave, or ours if it never got that far.
#[cfg(not(unix))]
pub fn send(_peer: &Peer, _text: &str) -> Result<Delivery, String> {
    Err("the inbox channel needs a Unix socket, which this platform does not have yet".into())
}

#[cfg(unix)]
pub fn send(peer: &Peer, text: &str) -> Result<Delivery, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty message".into());
    }
    let token = token(peer);
    if token.is_empty() {
        return Err("no inbox key for this session".into());
    }
    let msg_id = format!("cc-msg-{}", uuid_hex());
    let frames = [
        json!({"type": "auth", "token": token}),
        json!({"type": "user", "msg_id": msg_id, "session_id": peer.session_id, "message": {"role": "user", "content": envelope(text)}}),
    ];
    let mut payload = String::new();
    for f in &frames {
        payload.push_str(&f.to_string());
        payload.push('\n');
    }

    let mut sock = UnixStream::connect(&peer.socket_path).map_err(|e| format!("inbox unreachable: {e}"))?;
    sock.set_write_timeout(Some(CONNECT_TIMEOUT)).ok();
    sock.write_all(payload.as_bytes()).map_err(|e| format!("inbox unreachable: {e}"))?;
    let _ = sock.shutdown(std::net::Shutdown::Write);
    sock.set_read_timeout(Some(Duration::from_millis(250))).ok();

    let mut delivery = Delivery { msg_id, ..Default::default() };
    let deadline = Instant::now() + REPLY_WAIT;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 65536];
    while Instant::now() < deadline {
        match sock.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => break,
        }
    }
    for line in String::from_utf8_lossy(&buf).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => delivery.receipts.push(v),
            Err(_) => delivery.receipts.push(json!({"raw": line.chars().take(200).collect::<String>()})),
        }
    }
    for frame in &delivery.receipts {
        if let Some(reason) = frame.get("drop_reason").or(frame.get("error")).and_then(|v| v.as_str()) {
            return Err(reason.to_string());
        }
        if let Some(status) = frame.get("status").and_then(|v| v.as_str()) {
            delivery.status = status.to_string();
        }
    }
    Ok(delivery)
}

#[cfg(unix)]
fn uuid_hex() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = sha1_smol::Sha1::new();
    h.update(nanos.to_string().as_bytes());
    h.update(std::process::id().to_string().as_bytes());
    h.digest().to_string()[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in inbox: checks the auth frame and the envelope, answers
    /// with a receipt, so the wire is tested without Claude Code.
    #[cfg(unix)]
    #[test]
    fn send_speaks_the_inbox_wire() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("inbox.sock");
        let key = dir.path().join("1234.abcd.key");
        std::fs::write(&key, r#"{"peerToken":"tok-1","procStart":"s1"}"#).unwrap();
        let listener = UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut lines = Vec::new();
            for line in BufReader::new(stream.try_clone().unwrap()).lines() {
                lines.push(line.unwrap());
            }
            stream.write_all(b"{\"status\":\"delivered\"}\n").unwrap();
            lines
        });
        let peer = Peer { session_id: "sid".into(), pid: 1234, socket_path: sock.to_string_lossy().into(), cwd: String::new(), kind: String::new(), name: String::new(), status: String::new(), status_at: 0.0, proc_start: "s1".into(), key_path: key.to_string_lossy().into() };
        let d = send(&peer, "hello there").unwrap();
        assert_eq!(d.status, "delivered");
        let lines = server.join().unwrap();
        assert_eq!(lines.len(), 2);
        let auth: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(auth["type"], "auth");
        assert_eq!(auth["token"], "tok-1");
        let user: Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(user["type"], "user");
        assert_eq!(user["session_id"], "sid");
        assert_eq!(user["message"]["content"], envelope("hello there"));

        // A reused pid: the key's procStart disagrees, so no token, no send.
        let stale = Peer { proc_start: "other".into(), ..peer.clone() };
        assert!(send(&stale, "x").is_err());
    }

    #[test]
    fn envelope_matches_claude_codes_own() {
        let e = envelope("hello");
        assert_eq!(e, "<cross-session-message from-name=\"emaki\">\nhello\n</cross-session-message>");
        assert!(envelope("a </cross-session-message> b").contains("a <\\> b"));
    }
}
