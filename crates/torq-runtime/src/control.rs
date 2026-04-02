//! Minimal Tor ControlPort foundation layer for runtime use.
//!
//! This module intentionally stays small at this stage:
//! - it opens an authenticated TCP ControlPort session
//! - it sends one raw command at a time and waits for the synchronous reply
//! - it only supports single-line replies (`250 OK`) and same-code continuations
//!   (`250-...` followed by a final `250 ...`)
//! - it provides minimal high-level helpers for `SIGNAL NEWNYM` and
//!   `GETINFO status/bootstrap-phase`
//!
//! It does not yet implement:
//! - asynchronous `650` event handling
//! - `250+` data replies used by richer commands such as `GETINFO`
//! - broader command-specific control-plane APIs beyond the current narrow
//!   runtime use cases

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const SIGNAL_NEWNYM_COMMAND: &str = "SIGNAL NEWNYM";
const GETINFO_BOOTSTRAP_PHASE_COMMAND: &str = "GETINFO status/bootstrap-phase";
const BOOTSTRAP_PHASE_REPLY_PREFIX: &str = "status/bootstrap-phase=";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorBootstrapPhase {
    pub(crate) progress: u8,
    pub(crate) tag: Option<String>,
    pub(crate) summary: Option<String>,
}

impl TorBootstrapPhase {
    pub fn progress(&self) -> u8 {
        self.progress
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
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

    /// Requests a new Tor identity over the already configured ControlPort.
    ///
    /// This is intentionally a thin wrapper over the raw command foundation:
    /// it reuses the existing authenticated connection model and sends only
    /// `SIGNAL NEWNYM`, without introducing a broader command framework.
    pub async fn signal_newnym(&mut self) -> Result<TorControlReply, TorControlError> {
        self.send_command(SIGNAL_NEWNYM_COMMAND).await
    }

    /// Reads Tor bootstrap state from the official control interface.
    ///
    /// This stays intentionally narrow: runtime only needs the synchronous
    /// `status/bootstrap-phase` snapshot for now, so we parse just that reply
    /// shape instead of introducing a generic `GETINFO` framework.
    pub async fn get_bootstrap_phase(&mut self) -> Result<TorBootstrapPhase, TorControlError> {
        let reply = self.send_command(GETINFO_BOOTSTRAP_PHASE_COMMAND).await?;
        parse_bootstrap_phase_reply(&reply)
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

fn parse_bootstrap_phase_reply(
    reply: &TorControlReply,
) -> Result<TorBootstrapPhase, TorControlError> {
    let line = reply
        .lines()
        .iter()
        .find(|line| line.starts_with(BOOTSTRAP_PHASE_REPLY_PREFIX))
        .ok_or_else(|| TorControlError::MalformedResponse {
            message: "missing status/bootstrap-phase line in GETINFO reply".to_string(),
        })?;

    let payload = &line[BOOTSTRAP_PHASE_REPLY_PREFIX.len()..];
    let mut progress = None;
    let mut tag = None;
    let mut summary = None;

    for token in split_control_kv_tokens(payload)? {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };

        if key.eq_ignore_ascii_case("PROGRESS") {
            let parsed = value
                .parse::<u8>()
                .map_err(|_| TorControlError::MalformedResponse {
                    message: format!("invalid bootstrap progress value: {value:?}"),
                })?;

            if parsed > 100 {
                return Err(TorControlError::MalformedResponse {
                    message: format!("bootstrap progress out of range: {parsed}"),
                });
            }

            progress = Some(parsed);
        } else if key.eq_ignore_ascii_case("TAG") {
            tag = Some(value.to_string());
        } else if key.eq_ignore_ascii_case("SUMMARY") {
            summary = Some(value.to_string());
        }
    }

    let progress = progress.ok_or_else(|| TorControlError::MalformedResponse {
        message: "missing PROGRESS in status/bootstrap-phase reply".to_string(),
    })?;

    Ok(TorBootstrapPhase {
        progress,
        tag,
        summary,
    })
}

fn split_control_kv_tokens(input: &str) -> Result<Vec<String>, TorControlError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\\' => {
                let escaped = chars
                    .next()
                    .ok_or_else(|| TorControlError::MalformedResponse {
                        message: "dangling escape in control reply".to_string(),
                    })?;
                current.push(escaped);
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if in_quotes {
        return Err(TorControlError::MalformedResponse {
            message: "unterminated quote in control reply".to_string(),
        });
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    use super::{
        parse_bootstrap_phase_reply, parse_reply_line, split_control_kv_tokens, TorBootstrapPhase,
        TorControlAuth, TorControlClient, TorControlConfig, TorControlError, TorControlReply,
        GETINFO_BOOTSTRAP_PHASE_COMMAND, SIGNAL_NEWNYM_COMMAND,
    };

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

    #[test]
    fn parses_bootstrap_phase_reply() {
        let phase = parse_bootstrap_phase_reply(&TorControlReply {
            code: 250,
            lines: vec![
                "status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=73 TAG=loading_descriptors SUMMARY=\"Loading relay descriptors\"".to_string(),
                "OK".to_string(),
            ],
        })
        .unwrap();

        assert_eq!(
            phase,
            TorBootstrapPhase {
                progress: 73,
                tag: Some("loading_descriptors".to_string()),
                summary: Some("Loading relay descriptors".to_string()),
            }
        );
    }

    #[test]
    fn rejects_bootstrap_phase_reply_without_progress() {
        let error = parse_bootstrap_phase_reply(&TorControlReply {
            code: 250,
            lines: vec![
                "status/bootstrap-phase=NOTICE BOOTSTRAP TAG=loading_descriptors".to_string(),
                "OK".to_string(),
            ],
        })
        .unwrap_err();

        assert!(matches!(error, TorControlError::MalformedResponse { .. }));
    }

    #[test]
    fn rejects_bootstrap_phase_reply_with_out_of_range_progress() {
        let error = parse_bootstrap_phase_reply(&TorControlReply {
            code: 250,
            lines: vec![
                "status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=101".to_string(),
                "OK".to_string(),
            ],
        })
        .unwrap_err();

        assert!(matches!(error, TorControlError::MalformedResponse { .. }));
    }

    #[test]
    fn splits_control_reply_tokens_with_quoted_summary() {
        let tokens = split_control_kv_tokens(
            "NOTICE BOOTSTRAP PROGRESS=20 TAG=conn_done SUMMARY=\"Connected to a relay \\\"alpha\\\"\"",
        )
        .unwrap();

        assert_eq!(
            tokens,
            vec![
                "NOTICE",
                "BOOTSTRAP",
                "PROGRESS=20",
                "TAG=conn_done",
                "SUMMARY=Connected to a relay \"alpha\"",
            ]
        );
    }

    #[tokio::test]
    async fn signal_newnym_sends_authenticated_command() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(socket);

            assert_eq!(read_trimmed_line(&mut stream).await, "AUTHENTICATE");
            stream.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            assert_eq!(read_trimmed_line(&mut stream).await, SIGNAL_NEWNYM_COMMAND);
            stream.get_mut().write_all(b"250 OK\r\n").await.unwrap();
        });

        let mut client = TorControlClient::new(local_test_config(address));
        let reply = client.signal_newnym().await.unwrap();

        assert_eq!(reply.code(), 250);
        assert_eq!(reply.message(), "OK");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn signal_newnym_surfaces_tor_command_failure() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(socket);

            assert_eq!(read_trimmed_line(&mut stream).await, "AUTHENTICATE");
            stream.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            assert_eq!(read_trimmed_line(&mut stream).await, SIGNAL_NEWNYM_COMMAND);
            stream
                .get_mut()
                .write_all(b"552 Unrecognized signal\r\n")
                .await
                .unwrap();
        });

        let mut client = TorControlClient::new(local_test_config(address));
        let error = client.signal_newnym().await.unwrap_err();

        assert!(matches!(
            error,
            TorControlError::CommandFailed { code: 552, .. }
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn get_bootstrap_phase_sends_getinfo_and_parses_reply() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(socket);

            assert_eq!(read_trimmed_line(&mut stream).await, "AUTHENTICATE");
            stream.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            assert_eq!(
                read_trimmed_line(&mut stream).await,
                GETINFO_BOOTSTRAP_PHASE_COMMAND
            );
            stream
                .get_mut()
                .write_all(
                    b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=45 TAG=loading_descriptors SUMMARY=\"Loading relay descriptors\"\r\n250 OK\r\n",
                )
                .await
                .unwrap();
        });

        let mut client = TorControlClient::new(local_test_config(address));
        let phase = client.get_bootstrap_phase().await.unwrap();

        assert_eq!(phase.progress(), 45);
        assert_eq!(phase.tag(), Some("loading_descriptors"));
        assert_eq!(phase.summary(), Some("Loading relay descriptors"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn get_bootstrap_phase_rejects_malformed_reply() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(socket);

            assert_eq!(read_trimmed_line(&mut stream).await, "AUTHENTICATE");
            stream.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            assert_eq!(
                read_trimmed_line(&mut stream).await,
                GETINFO_BOOTSTRAP_PHASE_COMMAND
            );
            stream
                .get_mut()
                .write_all(
                    b"250-status/bootstrap-phase=NOTICE BOOTSTRAP TAG=starting\r\n250 OK\r\n",
                )
                .await
                .unwrap();
        });

        let mut client = TorControlClient::new(local_test_config(address));
        let error = client.get_bootstrap_phase().await.unwrap_err();

        assert!(matches!(error, TorControlError::MalformedResponse { .. }));
        server.await.unwrap();
    }

    fn local_test_config(address: SocketAddr) -> TorControlConfig {
        TorControlConfig::new(
            address.ip().to_string(),
            address.port(),
            TorControlAuth::Null,
        )
    }

    async fn read_trimmed_line(stream: &mut BufReader<tokio::net::TcpStream>) -> String {
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        line.trim_end_matches(['\r', '\n']).to_string()
    }
}
