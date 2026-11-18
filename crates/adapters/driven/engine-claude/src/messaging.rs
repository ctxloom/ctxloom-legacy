//! claude's cross-session messaging socket, as the terminal wake posts to it
//! (owner ruling 2026-11-18). claude listens on a socket and exports where
//! (`CLAUDE_CODE_MESSAGING_SOCKET`), and a token (`..._TOKEN`), to every
//! process it starts, ctxloom's own MCP server among them. A post there is
//! NDJSON: an auth line when there is a token, then a user line, which
//! starts a turn in an idle TUI (queued as the next prompt in a busy one).
//! The wire is undocumented: `just validate-wake-claude` measures it on each
//! claude bump (2.1.325, Linux: the wire as Go ctxloom read it; a post from
//! a process that is not claude's descendant is not refused, "auth line
//! optional here" in claude's own log).
//!
//! The token never leaves this value: never in a `Debug`, a failure or a log.
//! Only a unix socket is posted to (claude's Windows endpoint is not
//! measured): elsewhere a post fails, said.

use std::fmt;

use run_ports::WakePost;
use surface_ports::Json;

/// Where claude says its messaging socket is.
pub const SOCKET_VARIABLE: &str = "CLAUDE_CODE_MESSAGING_SOCKET";

/// The token claude admits a sender by, when it exports one.
pub const TOKEN_VARIABLE: &str = "CLAUDE_CODE_MESSAGING_TOKEN";

/// One session's messaging socket, bound from what claude exported.
pub struct MessagingWake {
    socket: String,
    token: String,
}

impl fmt::Debug for MessagingWake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "claude messaging wake at {}", self.socket)
    }
}

/// The wake claude exported to this process (`exported` reads a variable);
/// none where it exported no socket.
pub fn messaging_wake(exported: &dyn Fn(&str) -> Option<String>) -> Option<MessagingWake> {
    let socket = exported(SOCKET_VARIABLE).filter(|socket| !socket.is_empty())?;
    Some(MessagingWake {
        socket,
        token: exported(TOKEN_VARIABLE).unwrap_or_default(),
    })
}

/// The post of `line`: the auth line with `token` (none when it is empty),
/// then the user line, each a JSON object on a line of its own.
pub fn wire(token: &str, line: &str) -> String {
    let mut lines = Vec::new();
    if !token.is_empty() {
        lines.push(Json::object([
            ("type", Json::text("auth")),
            ("token", Json::text(token)),
        ]));
    }
    lines.push(Json::object([
        ("type", Json::text("user")),
        (
            "message",
            Json::object([("role", Json::text("user")), ("content", Json::text(line))]),
        ),
    ]));
    lines
        .iter()
        .map(|doc| crate::codec::compact(doc) + "\n")
        .collect()
}

impl WakePost for MessagingWake {
    fn post(&self, line: &str) -> Result<(), String> {
        send(&self.socket, wire(&self.token, line).as_bytes())
            .map_err(|e| format!("claude's messaging socket {}: {e}", self.socket))
    }
}

/// `bytes` written to the unix socket at `path`, which is then closed.
#[cfg(unix)]
fn send(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut socket = std::os::unix::net::UnixStream::connect(path)?;
    socket.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
    socket.write_all(bytes)?;
    socket.shutdown(std::net::Shutdown::Write)
}

/// No socket but a unix one is posted to.
#[cfg(not(unix))]
fn send(_: &str, _: &[u8]) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "only a unix socket is posted to",
    ))
}

#[cfg(test)]
#[path = "messaging.test.rs"]
mod tests;
