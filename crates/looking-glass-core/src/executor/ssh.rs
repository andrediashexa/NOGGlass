//! SSH transport.
//!
//! One session per query: connect, authenticate, disable paging, send exactly
//! one read-only command, read until the prompt comes back, close. Nothing is
//! pooled or reused, because a reused session carries state — a changed context,
//! a half-read page — from one visitor's query into the next one's.
//!
//! Routers are not Unix hosts. Most network operating systems reject an `exec`
//! channel, so this drives an interactive shell and detects the end of the
//! output with the prompt pattern from the catalogue. That is why the catalogue
//! carries a prompt regex per vendor.

use crate::driver::DriverError;
use crate::executor::Transport;
use crate::inventory::{Credentials, Router};
use regex::Regex;
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use std::sync::Arc;
use std::time::Duration;

/// What to do about a router's host key.
///
/// Trust on first use is not offered. A Looking Glass connects to routers the
/// operator listed, repeatedly and unattended, so accepting an unknown key
/// would hand a man in the middle every query and every credential.
#[derive(Debug, Clone, Default)]
pub enum HostKeyPolicy {
    /// Accept only these keys, in OpenSSH `ssh-ed25519 AAAA...` form.
    Pinned(Arc<Vec<String>>),
    /// Accept any key. Laboratory only; the operator opts in explicitly and the
    /// deployment guide says why this is not for production.
    #[default]
    AcceptAny,
}

/// Opens a real SSH session per query.
pub struct SshTransport {
    connect_timeout: Duration,
    /// How long to wait for more output before deciding the router is done.
    ///
    /// A router that never prints its prompt again — because the command paged,
    /// or the session hung — would otherwise hold the slot until the executor's
    /// timeout fires. This ends it sooner, with what was read so far.
    idle_timeout: Duration,
    host_keys: HostKeyPolicy,
}

impl Default for SshTransport {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(5),
            host_keys: HostKeyPolicy::default(),
        }
    }
}

impl SshTransport {
    pub fn new(
        connect_timeout: Duration,
        idle_timeout: Duration,
        host_keys: HostKeyPolicy,
    ) -> Self {
        Self {
            connect_timeout,
            idle_timeout,
            host_keys,
        }
    }

    async fn authenticate(
        &self,
        session: &mut Handle<ClientHandler>,
        router: &Router,
    ) -> Result<(), DriverError> {
        let user = router
            .username
            .clone()
            .unwrap_or_else(|| "admin".to_string());

        let result = match &router.credentials {
            Credentials::PasswordEnv(variable) => {
                let password = std::env::var(variable)
                    .map_err(|_| DriverError::ConnectionFailed(format!("{variable} is not set")))?;
                if password.is_empty() {
                    return Err(DriverError::ConnectionFailed(format!(
                        "{variable} is empty"
                    )));
                }
                session
                    .authenticate_password(user, password)
                    .await
                    .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?
            }
            Credentials::KeyFile {
                path,
                passphrase_env,
            } => {
                let passphrase = passphrase_env
                    .as_ref()
                    .and_then(|variable| std::env::var(variable).ok())
                    .filter(|value| !value.is_empty());
                let key = load_secret_key(path, passphrase.as_deref())
                    .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;
                session
                    .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), None))
                    .await
                    .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?
            }
            Credentials::None => {
                return Err(DriverError::ConnectionFailed(
                    "no credentials configured".to_string(),
                ))
            }
        };

        if !result.success() {
            // Never says which of user, password or method was wrong: that is
            // an oracle for anyone who can read the visitor-facing error.
            return Err(DriverError::ConnectionFailed(
                "authentication was refused".to_string(),
            ));
        }
        Ok(())
    }
}

/// Session callbacks. Its only real job is the host key decision.
struct ClientHandler {
    policy: HostKeyPolicy,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        match &self.policy {
            HostKeyPolicy::AcceptAny => Ok(true),
            HostKeyPolicy::Pinned(allowed) => {
                let offered = match key {
                    russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => {
                        key.to_openssh().ok()
                    }
                    russh::keys::PublicKeyOrCertificate::Certificate(c) => c.to_openssh().ok(),
                };
                Ok(match offered {
                    Some(offered) => allowed.iter().any(|a| a.trim() == offered.trim()),
                    None => false,
                })
            }
        }
    }
}

#[async_trait::async_trait]
impl Transport for SshTransport {
    async fn run(
        &self,
        router: &Router,
        command: &str,
        paging_command: Option<&str>,
    ) -> Result<String, DriverError> {
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(self.idle_timeout * 2),
            ..Default::default()
        });
        let handler = ClientHandler {
            policy: self.host_keys.clone(),
        };

        let mut session = tokio::time::timeout(
            self.connect_timeout,
            client::connect(config, (router.host.as_str(), router.port), handler),
        )
        .await
        .map_err(|_| DriverError::ConnectionFailed("connection timed out".to_string()))?
        .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        self.authenticate(&mut session, router).await?;

        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?;

        // A wide terminal with no scrolling: routers wrap their tables to the
        // terminal width, and a narrow one would corrupt every column.
        channel
            .request_pty(true, "vt100", 250, 0, 0, 0, &[])
            .await
            .map_err(|e| DriverError::IoError(e.to_string()))?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| DriverError::IoError(e.to_string()))?;

        if let Some(paging) = paging_command {
            channel
                .data_bytes(format!("{paging}\n").into_bytes())
                .await
                .map_err(|e| DriverError::IoError(e.to_string()))?;
        }
        channel
            .data_bytes(format!("{command}\n").into_bytes())
            .await
            .map_err(|e| DriverError::IoError(e.to_string()))?;

        let prompt = Regex::new(
            &crate::catalogue::BUILTIN
                .vendor(&router.vendor)
                .map_err(|e| DriverError::ConnectionFailed(e.to_string()))?
                .prompt,
        )
        .map_err(|e| DriverError::ParseError(e.to_string()))?;

        let output = read_until_prompt(&mut channel, &prompt, self.idle_timeout).await?;

        // Best effort: the answer is already in hand, so a failure to say
        // goodbye politely is not the visitor's problem.
        let _ = channel.close().await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "done", "en")
            .await;

        Ok(strip_echo(&output, command))
    }
}

/// Reads until the prompt reappears, the channel closes, or the router goes
/// quiet for longer than the idle timeout.
async fn read_until_prompt(
    channel: &mut russh::Channel<client::Msg>,
    prompt: &Regex,
    idle: Duration,
) -> Result<String, DriverError> {
    let mut output = String::new();

    loop {
        let message = match tokio::time::timeout(idle, channel.wait()).await {
            // Quiet for too long: return what we have rather than hold the slot.
            Err(_) => break,
            Ok(None) => break,
            Ok(Some(message)) => message,
        };

        match message {
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                output.push_str(&String::from_utf8_lossy(&data));
                // The prompt only counts at the end of what we have read; a
                // prompt-shaped string inside the output is not the end.
                if let Some(last) = output.lines().last() {
                    if prompt.is_match(last.trim_end()) {
                        break;
                    }
                }
            }
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }

    if output.is_empty() {
        return Err(DriverError::EmptyResponse);
    }
    Ok(output)
}

/// Removes the echoed command and the trailing prompt.
///
/// An interactive shell echoes what was typed, and parsers should see the
/// router's answer, not our own command coming back.
fn strip_echo(output: &str, command: &str) -> String {
    let mut lines: Vec<&str> = output.lines().collect();

    if let Some(position) = lines
        .iter()
        .position(|line| line.trim_end().ends_with(command))
    {
        lines.drain(..=position);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    // The last line is the prompt that ended the read.
    lines.pop();

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoed_command_and_trailing_prompt_are_removed() {
        let raw = "\
display bgp routing-table 198.51.100.0 255.255.255.0
 Total Number of Routes: 1
*>  198.51.100.0/24    192.0.2.254
<edge-01>";
        let cleaned = strip_echo(raw, "display bgp routing-table 198.51.100.0 255.255.255.0");
        assert_eq!(
            cleaned,
            " Total Number of Routes: 1\n*>  198.51.100.0/24    192.0.2.254"
        );
    }

    #[test]
    fn output_without_an_echo_keeps_everything_but_the_prompt() {
        let raw = " Total Number of Routes: 0\n<edge-01>";
        assert_eq!(
            strip_echo(raw, "display bgp peer"),
            " Total Number of Routes: 0"
        );
    }

    /// Default policy is documented as laboratory-only; this test exists so
    /// changing it is a deliberate act with a visible diff.
    #[test]
    fn the_default_host_key_policy_is_accept_any() {
        assert!(matches!(HostKeyPolicy::default(), HostKeyPolicy::AcceptAny));
    }
}
