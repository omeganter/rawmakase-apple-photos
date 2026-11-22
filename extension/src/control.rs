//! A client of RAWmakase's control socket (docs/automation.md, protocol 1),
//! used only for what the extension protocol doesn't offer yet: making
//! collections.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

pub struct Control {
    port: u16,
    token: String,
    next: u64,
}

impl Control {
    /// Reads `control.json` in RAWmakase's data folder: there only while
    /// external control is on.
    pub fn open(data: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(data.join("control.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        Some(Self {
            port: u16::try_from(v["port"].as_u64()?).ok()?,
            token: v["token"].as_str()?.to_string(),
            next: 0,
        })
    }

    /// One request, one reply; the reply's `result`.
    pub fn request(&mut self, cmd: &str, fields: Value) -> Result<Value, String> {
        self.next += 1;
        let mut req = json!({"protocol": 1, "request_id": format!("apple-photos-{}", self.next),
                             "token": self.token, "cmd": cmd});
        if let (Some(req), Some(fields)) = (req.as_object_mut(), fields.as_object()) {
            req.extend(fields.clone());
        }
        let mut stream =
            TcpStream::connect(("127.0.0.1", self.port)).map_err(|e| format!("{cmd}: {e}"))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
        writeln!(stream, "{req}").map_err(|e| format!("{cmd}: {e}"))?;
        let mut line = String::new();
        BufReader::new(stream)
            .read_line(&mut line)
            .map_err(|e| format!("{cmd}: {e}"))?;
        let reply: Value = serde_json::from_str(&line).map_err(|e| format!("{cmd}: {e}"))?;
        if reply["ok"].as_bool() == Some(true) {
            Ok(reply["result"].clone())
        } else {
            Err(format!(
                "{cmd}: {}: {}",
                reply["code"].as_str().unwrap_or("error"),
                reply["error"].as_str().unwrap_or("")
            ))
        }
    }
}
