//! Minimal Tor ControlPort foundation layer for runtime use.
//!
//! This module intentionally stays small at this stage:
//! - it opens an authenticated TCP ControlPort session
//! - it sends one raw command at a time and waits for the synchronous reply
//! - it only supports single-line replies (`250 OK`) and same-code continuations
//!   (`250-...` followed by a final `250 ...`)
//!
//! It does not yet implement:
//! - asynchronous `650` event handling
//! - `250+` data replies used by richer commands such as `GETINFO`
//! - higher-level control-plane commands like `SIGNAL NEWNYM`

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorControlAuth {
    Null,
    Cookie { cookie_path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorControlConfig {
    pub host: String,
    pub port: u16,
    pub auth: TorControlAuth,
}

impl TorControlConfig {
    pub fn new(host: impl Into<String>, port: u16, auth: TorControlAuth) -> Self {
        Self {
            host: host.into(),
            port,
            auth,
        }
    }

    pub fn localhost(port: u16, auth: TorControlAuth) -> Self {
        Self::new("127.0.0.1", port, auth)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorControlReply {
    code: u16,
    lines: Vec<String>,
}

impl TorControlReply {
    pub fn code(&self) -> u16 {
        self.code
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn message(&self) -> String {
        self.lines.join("\n")
    }

    fn is_success(&self) -> bool {
        (200..300).contains(&self.code)
    }
}

#[derive(Debug, Error)]
pub enum TorControlError {
    #[error("Tor ControlPort is not configured in TorRuntimeConfig")]
    MissingConfig,
    #[error("failed to connect to Tor ControlPort at {host}:{port}: {source}")]
    Connect {
        host: String,
        port: u16,
        #[source]
        source: io::Error,
    },
    #[error("control port authentication failed with Tor code {code}: {message}")]
    AuthenticationFailed { code: u16, message: String },
    #[error("Tor ControlPort command failed with code {code}: {message}")]
    CommandFailed { code: u16, message: String },
    #[error("malformed ControlPort response: {message}")]
    MalformedResponse { message: String },
    #[error("failed to read ControlPort cookie from {path}: {source}")]
    CookieRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("ControlPort cookie at {path} must be exactly 32 bytes")]
    InvalidCookieLength { path: PathBuf },
    #[error("ControlPort I/O error: {0}")]
    Io(#[from] io::Error),
}

pub struct TorControlClient {
    config: TorControlConfig,
    connection: Option<ControlConnection>,
    authenticated: bool,
}

impl TorControlClient {
    pub fn new(config: TorControlConfig) -> Self {
        Self {
            config,
            connection: None,
            authenticated: false,
        }
    }

    pub fn config(&self) -> &TorControlConfig {
        &self.config
    }

    pub async fn connect(&mut self) -> Result<(), TorControlError> {
        if self.connection.is_none() {
            self.open_connection().await?;
        }

        if self.authenticated {
            return Ok(());
        }

        if let Err(error) = self.authenticate().await {
            self.connection = None;
            self.authenticated = false;
            return Err(error);
        }

        Ok(())
    }

    pub async fn authenticate(&mut self) -> Result<(), TorControlError> {
        if self.authenticated {
            return Ok(());
        }

        if self.connection.is_none() {
            self.open_connection().await?;
        }

        let command = authenticate_command(&self.config.auth).await?;
        let reply = self.send_command_internal(&command).await;

        match reply {
            Ok(_) => {
                self.authenticated = true;
                Ok(())
            }
            Err(TorControlError::CommandFailed { code, message }) => {
                Err(TorControlError::AuthenticationFailed { code, message })
            }
            Err(error) => Err(error),
        }
    }

    pub async fn send_command(
        &mut self,
        command: &str,
    ) -> Result<TorControlReply, TorControlError> {
        self.send_raw_command(command).await
    }

    pub async fn send_raw_command(
        &mut self,
        command: &str,
    ) -> Result<TorControlReply, TorControlError> {
        if !self.authenticated {
            self.connect().await?;
        }

        self.send_command_internal(command).await
    }

    async fn send_command_internal(
        &mut self,
        command: &str,
    ) -> Result<TorControlReply, TorControlError> {
        let connection =
            self.connection
                .as_mut()
                .ok_or_else(|| TorControlError::MalformedResponse {
                    message: "no active ControlPort connection".to_string(),
                })?;

        if command.contains(['\r', '\n']) {
            return Err(TorControlError::MalformedResponse {
                message: "commands must be single-line ControlPort frames".to_string(),
            });
        }

        connection.write_command(command).await?;
        let reply = connection.read_reply().await?;

        if reply.is_success() {
            Ok(reply)
        } else {
            Err(TorControlError::CommandFailed {
                code: reply.code(),
                message: reply.message(),
            })
        }
    }

    async fn open_connection(&mut self) -> Result<(), TorControlError> {
        let stream = TcpStream::connect((self.config.host.as_str(), self.config.port))
            .await
            .map_err(|source| TorControlError::Connect {
                host: self.config.host.clone(),
                port: self.config.port,
                source,
            })?;

        self.connection = Some(ControlConnection::new(stream));
        self.authenticated = false;
        Ok(())
    }
}

struct ControlConnection {
    stream: BufReader<TcpStream>,
}

impl ControlConnection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }

    async fn write_command(&mut self, command: &str) -> Result<(), TorControlError> {
        self.stream.get_mut().write_all(command.as_bytes()).await?;
        self.stream.get_mut().write_all(b"\r\n").await?;
        self.stream.get_mut().flush().await?;
        Ok(())
    }

    async fn read_reply(&mut self) -> Result<TorControlReply, TorControlError> {
        let first_line = read_protocol_line(&mut self.stream).await?;
        let first = parse_reply_line(&first_line)?;

        if first.separator == '+' {
            return Err(TorControlError::MalformedResponse {
                message: "ControlPort data replies are not supported by this foundation layer"
                    .to_string(),
            });
        }

        let code = first.code;
        let mut lines = vec![first.text];

        if first.separator == ' ' {
            return Ok(TorControlReply { code, lines });
        }

        loop {
            let next_line = read_protocol_line(&mut self.stream).await?;
            let next = parse_reply_line(&next_line)?;

            if next.code != code {
                return Err(TorControlError::MalformedResponse {
                    message: format!(
                        "expected reply code {code} in continuation, got {}",
                        next.code
                    ),
                });
            }

            if next.separator == '+' {
                return Err(TorControlError::MalformedResponse {
                    message: "ControlPort data replies are not supported by this foundation layer"
                        .to_string(),
                });
            }

            let is_final = next.separator == ' ';
            lines.push(next.text);

            if is_final {
                return Ok(TorControlReply { code, lines });
            }
        }
    }
}

#[derive(Debug)]
struct ParsedReplyLine {
    code: u16,
    separator: char,
    text: String,
}

async fn authenticate_command(auth: &TorControlAuth) -> Result<String, TorControlError> {
    match auth {
        TorControlAuth::Null => Ok("AUTHENTICATE".to_string()),
        TorControlAuth::Cookie { cookie_path } => {
            let cookie =
                fs::read(cookie_path)
                    .await
                    .map_err(|source| TorControlError::CookieRead {
                        path: cookie_path.clone(),
                        source,
                    })?;

            if cookie.len() != 32 {
                return Err(TorControlError::InvalidCookieLength {
                    path: cookie_path.clone(),
                });
            }

            Ok(format!("AUTHENTICATE {}", encode_hex(&cookie)))
        }
    }
}

async fn read_protocol_line(stream: &mut BufReader<TcpStream>) -> Result<String, TorControlError> {
    let mut line = String::new();
    let bytes_read = stream.read_line(&mut line).await?;

    if bytes_read == 0 {
        return Err(TorControlError::MalformedResponse {
            message: "unexpected EOF while reading ControlPort response".to_string(),
        });
    }

    Ok(line)
}

fn parse_reply_line(line: &str) -> Result<ParsedReplyLine, TorControlError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let bytes = line.as_bytes();

    if bytes.len() < 4 {
        return Err(TorControlError::MalformedResponse {
            message: format!("reply line is too short: {line:?}"),
        });
    }

    let code = line[..3]
        .parse::<u16>()
        .map_err(|_| TorControlError::MalformedResponse {
            message: format!("reply line does not start with a numeric code: {line:?}"),
        })?;

    let separator = bytes[3] as char;
    if !matches!(separator, ' ' | '-' | '+') {
        return Err(TorControlError::MalformedResponse {
            message: format!("unsupported reply separator {separator:?} in {line:?}"),
        });
    }

    Ok(ParsedReplyLine {
        code,
        separator,
        text: line[4..].to_string(),
    })
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02X}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{parse_reply_line, TorControlError, TorControlReply};

    #[test]
    fn parses_single_line_success_reply() {
        let parsed = parse_reply_line("250 OK\r\n").unwrap();

        assert_eq!(parsed.code, 250);
        assert_eq!(parsed.separator, ' ');
        assert_eq!(parsed.text, "OK");
    }

    #[test]
    fn parses_error_reply_line() {
        let parsed = parse_reply_line("515 Authentication required\r\n").unwrap();

        assert_eq!(parsed.code, 515);
        assert_eq!(parsed.separator, ' ');
        assert_eq!(parsed.text, "Authentication required");
    }

    #[test]
    fn parses_continuation_reply_line() {
        let parsed = parse_reply_line("250-version=0.4.8.12\r\n").unwrap();

        assert_eq!(parsed.code, 250);
        assert_eq!(parsed.separator, '-');
        assert_eq!(parsed.text, "version=0.4.8.12");
    }

    #[test]
    fn reply_message_preserves_multiple_lines() {
        let reply = TorControlReply {
            code: 250,
            lines: vec!["version=0.4.8.12".to_string(), "OK".to_string()],
        };

        assert_eq!(reply.code(), 250);
        assert_eq!(reply.message(), "version=0.4.8.12\nOK");
    }

    #[test]
    fn rejects_malformed_reply_line() {
        let error = parse_reply_line("oops\r\n").unwrap_err();

        assert!(matches!(error, TorControlError::MalformedResponse { .. }));
    }

    #[test]
    fn parses_data_reply_marker() {
        let parsed = parse_reply_line("250+config-text\r\n").unwrap();

        assert_eq!(parsed.code, 250);
        assert_eq!(parsed.separator, '+');
        assert_eq!(parsed.text, "config-text");
    }
}
