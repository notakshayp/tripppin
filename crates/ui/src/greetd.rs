//! greetd's login protocol: JSON messages over the Unix socket in `$GREETD_SOCK`, each
//! prefixed with its length as a native-endian u32. Written here rather than linking the
//! GPL-licensed client crate; the protocol is a handful of messages.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

pub struct Greetd {
    sock: UnixStream,
}

/// How a login attempt ended.
pub enum Outcome {
    /// The password was right and the desktop is starting.
    Started,
    /// greetd said no; the text says why, in its words.
    Refused(String),
}

impl Greetd {
    pub fn connect() -> Result<Self> {
        let path = std::env::var("GREETD_SOCK").context("GREETD_SOCK isn't set; is greetd running this?")?;
        Ok(Greetd { sock: UnixStream::connect(&path).with_context(|| format!("Connecting to {path}"))? })
    }

    fn send(&mut self, msg: &Value) -> Result<Value> {
        let body = msg.to_string();
        let len = u32::try_from(body.len())?;
        self.sock.write_all(&len.to_ne_bytes())?;
        self.sock.write_all(body.as_bytes())?;
        let mut len = [0u8; 4];
        self.sock.read_exact(&mut len)?;
        let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
        self.sock.read_exact(&mut buf)?;
        Ok(serde_json::from_slice(&buf)?)
    }

    /// Logs `user` in with `password` and starts `cmd` as their session.
    pub fn login(&mut self, user: &str, password: &str, cmd: &[&str]) -> Result<Outcome> {
        let mut reply = self.send(&json!({ "type": "create_session", "username": user }))?;
        let mut answered = false;
        loop {
            match reply["type"].as_str() {
                Some("auth_message") => {
                    // A secret prompt gets the password; info and error prompts just need an ack.
                    let response = match reply["auth_message_type"].as_str() {
                        Some("secret") | Some("visible") if !answered => {
                            answered = true;
                            Value::String(password.to_string())
                        }
                        Some("secret") | Some("visible") => {
                            self.cancel();
                            return Ok(Outcome::Refused("It asked for more than a password".into()));
                        }
                        _ => Value::Null,
                    };
                    reply = self.send(&json!({ "type": "post_auth_message_response", "response": response }))?;
                }
                Some("success") => {
                    let started = self.send(&json!({ "type": "start_session", "cmd": cmd, "env": [] }))?;
                    return match started["type"].as_str() {
                        Some("success") => Ok(Outcome::Started),
                        _ => Ok(Outcome::Refused(describe(&started))),
                    };
                }
                Some("error") => {
                    self.cancel();
                    return Ok(Outcome::Refused(describe(&reply)));
                }
                other => bail!("Greetd sent something unexpected: {other:?}"),
            }
        }
    }

    fn cancel(&mut self) {
        let _ = self.send(&json!({ "type": "cancel_session" }));
    }
}

fn describe(reply: &Value) -> String {
    match reply["error_type"].as_str() {
        Some("auth_error") => "That's not it. Try again.".into(),
        _ => reply["description"].as_str().unwrap_or("Something went wrong").to_lowercase(),
    }
}
