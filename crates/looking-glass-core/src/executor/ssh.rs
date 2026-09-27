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

use crate::catalogue::SessionMode;
use crate::driver::DriverError;
use crate::executor::Transport;
use crate::inventory::{Credentials, Router};
use regex::Regex;
use russh::client::{self, Handle};
use russh::kex;
use russh::keys::{load_secret_key, Algorithm, EcdsaCurve, HashAlg, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use std::borrow::Cow;
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
    ///
    /// It has to outlast a silence the router is entitled to. A traceroute
    /// across a hop that does not answer prints nothing while its probes time
    /// out — around five seconds each — so a five second idle cut the answer
    /// off after the first probe and returned a traceroute with no hops at
    /// all. The executor's own timeout is what bounds a genuinely stuck
    /// session; this only decides when to stop waiting for more.
    idle_timeout: Duration,
    host_keys: HostKeyPolicy,
}

impl Default for SshTransport {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(20),
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
                let password = std::env::var(variable).map_err(|_| {
                    tracing::error!(
                        router = %router.id,
                        variable = %variable,
                        "router credential error: environment variable is not set"
                    );
                    DriverError::ConnectionFailed(format!("{variable} is not set"))
                })?;
                if password.is_empty() {
                    tracing::error!(
                        router = %router.id,
                        variable = %variable,
                        "router credential error: environment variable is empty"
                    );
                    return Err(DriverError::ConnectionFailed(format!(
                        "{variable} is empty"
                    )));
                }
                session
                    .authenticate_password(user.clone(), password)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            router = %router.id,
                            error = %e,
                            "SSH authenticate_password network/protocol error"
                        );
                        DriverError::ConnectionFailed(e.to_string())
                    })?
            }
            Credentials::KeyFile {
                path,
                passphrase_env,
            } => {
                let passphrase = passphrase_env
                    .as_ref()
                    .and_then(|variable| std::env::var(variable).ok())
                    .filter(|value| !value.is_empty());
                let key = load_secret_key(path, passphrase.as_deref()).map_err(|e| {
                    tracing::error!(
                        router = %router.id,
                        path = %path,
                        error = %e,
                        "failed to load SSH private key file"
                    );
                    DriverError::ConnectionFailed(e.to_string())
                })?;
                session
                    .authenticate_publickey(
                        user.clone(),
                        PrivateKeyWithHashAlg::new(Arc::new(key), None),
                    )
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            router = %router.id,
                            error = %e,
                            "SSH authenticate_publickey error"
                        );
                        DriverError::ConnectionFailed(e.to_string())
                    })?
            }
            Credentials::None => {
                tracing::error!(
                    router = %router.id,
                    "no credentials configured for router"
                );
                return Err(DriverError::ConnectionFailed(
                    "no credentials configured".to_string(),
                ));
            }
        };

        if !result.success() {
            // Never says which of user, password or method was wrong: that is
            // an oracle for anyone who can read the visitor-facing error.
            tracing::error!(
                router = %router.id,
                username = %user,
                "SSH authentication was refused by the router (check username/password in environment)"
            );
            return Err(DriverError::ConnectionFailed(
                "authentication was refused".to_string(),
            ));
        }
        Ok(())
    }
}

/// Key exchange algorithms, in the order this client asks for them.
///
/// The library's default list is modern and short — curve25519, the SHA-2
/// Diffie-Hellman groups, a post-quantum hybrid — and contains **no**
/// `ecdh-sha2-nistp*`, although it implements all three. Cisco IOS-XE offers
/// exactly those three and `diffie-hellman-group14-sha1`, so the intersection
/// was empty and every Cisco answered "could not reach the router" twelve
/// milliseconds after being asked. OpenSSH negotiates nistp256 with the same
/// router without comment, which is why nothing looked wrong from a terminal.
///
/// The NIST curves go after the modern algorithms, and legacy SHA-1 algorithms
/// (`diffie-hellman-group14-sha1`, `diffie-hellman-group-exchange-sha1`,
/// `diffie-hellman-group1-sha1`) are placed strictly at the end as fallback options
/// for older network appliances.
///
/// A router that offers modern cryptography negotiates it first, but legacy
/// equipment with older firmware remains reachable.
pub fn kex_preference() -> Vec<kex::Name> {
    vec![
        kex::CURVE25519,
        kex::CURVE25519_PRE_RFC_8731,
        kex::DH_GEX_SHA256,
        kex::DH_G18_SHA512,
        kex::DH_G16_SHA512,
        kex::DH_G14_SHA256,
        // Not in the library's defaults, and all a great deal of vendor
        // equipment has.
        kex::ECDH_SHA2_NISTP256,
        kex::ECDH_SHA2_NISTP384,
        kex::ECDH_SHA2_NISTP521,
        // Legacy fallback key exchange algorithms for older equipment.
        // Placed strictly at the end of the client's preference list so
        // modern, secure algorithms are always negotiated first if offered.
        kex::DH_G14_SHA1,
        kex::DH_GEX_SHA1,
        kex::DH_G1_SHA1,
        // Advertised so a server that supports extensions says so; not a key
        // exchange in itself.
        kex::EXTENSION_SUPPORT_AS_CLIENT,
    ]
}

/// Host key algorithms, in the order this client asks for them.
///
/// The negotiated algorithm is the first entry the server also supports, so
/// the order decides what happens on a router that offers several.
///
/// `ecdsa-sha2-nistp521` sits at the end, behind RSA, and that is the whole
/// point of this list. A Huawei NE40E offers `ssh-dss, ssh-rsa,
/// ecdsa-sha2-nistp521`; the client's default order picks nistp521, and
/// decoding what VRP sends for it fails with `mpint encoding invalid` — so
/// every query to every Huawei answered "could not reach the router", with the
/// router reachable and the credentials right. OpenSSH accepts the same key,
/// which is why nothing looks wrong from a terminal.
///
/// Nothing weaker is added to make this work. `ssh-dss` is 1024-bit DSA, gone
/// from OpenSSH years ago, and a looking glass that authenticates a router with
/// it is worse than one that refuses.
pub fn host_key_preference() -> Vec<Algorithm> {
    vec![
        Algorithm::Ed25519,
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        },
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP384,
        },
        Algorithm::Rsa {
            hash: Some(HashAlg::Sha512),
        },
        Algorithm::Rsa {
            hash: Some(HashAlg::Sha256),
        },
        // SHA-1 signatures, which is all a VRP offers for RSA.
        Algorithm::Rsa { hash: None },
        // Last, for the reason above: a server that offers nothing else is
        // still tried, and fails the way it does today.
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP521,
        },
    ]
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
                let matched = match &offered {
                    Some(offered) => allowed.iter().any(|a| a.trim() == offered.trim()),
                    None => false,
                };
                if !matched {
                    tracing::error!(
                        offered = ?offered,
                        allowed = ?allowed,
                        "SSH host key verification failed: server host key does not match pinned keys"
                    );
                }
                Ok(matched)
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
            preferred: russh::Preferred {
                kex: Cow::Owned(kex_preference()),
                key: Cow::Owned(host_key_preference()),
                ..client::Config::default().preferred
            },
            ..Default::default()
        });
        let policy = match &router.host_key {
            Some(key) => HostKeyPolicy::Pinned(Arc::new(vec![key.clone()])),
            None => {
                if matches!(self.host_keys, HostKeyPolicy::AcceptAny) {
                    tracing::warn!(
                        router = %router.id,
                        "connecting to router via SSH without host key verification (AcceptAny)"
                    );
                }
                self.host_keys.clone()
            }
        };
        let handler = ClientHandler { policy };

        let mut session = tokio::time::timeout(
            self.connect_timeout,
            client::connect(config, (router.host.as_str(), router.port), handler),
        )
        .await
        .map_err(|_| {
            tracing::error!(
                router = %router.id,
                host = %router.host,
                port = router.port,
                timeout_secs = self.connect_timeout.as_secs(),
                "SSH connection timed out"
            );
            DriverError::ConnectionFailed("connection timed out".to_string())
        })?
        .map_err(|e| {
            tracing::error!(
                router = %router.id,
                host = %router.host,
                port = router.port,
                error = %e,
                "SSH connection failed (TCP or handshake)"
            );
            DriverError::ConnectionFailed(e.to_string())
        })?;

        self.authenticate(&mut session, router).await?;

        let vendor = crate::catalogue::BUILTIN
            .vendor(&router.vendor)
            .map_err(|e| {
                tracing::error!(router = %router.id, vendor = %router.vendor, error = %e, "unknown vendor in catalogue");
                DriverError::ConnectionFailed(e.to_string())
            })?;

        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| {
                tracing::error!(router = %router.id, error = %e, "SSH channel_open_session failed");
                DriverError::ConnectionFailed(e.to_string())
            })?;

        // One exec request, no terminal, read until the channel closes.
        //
        // RouterOS answers this immediately. Given a shell on a terminal it
        // probes what it has been given — `ESC [ 9999 B`, `ESC Z`,
        // `ESC [ 6 n` — and waits for the client to report its cursor
        // position before printing anything, so every query came back empty.
        if vendor.session == SessionMode::Exec {
            channel
                .exec(true, command)
                .await
                .map_err(|e| DriverError::IoError(e.to_string()))?;

            let output = read_until_close(&mut channel, self.idle_timeout).await?;
            let _ = channel.close().await;
            let _ = session
                .disconnect(Disconnect::ByApplication, "done", "en")
                .await;
            return Ok(output);
        }

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

        let prompt =
            Regex::new(&vendor.prompt).map_err(|e| DriverError::ParseError(e.to_string()))?;

        // Wait for the router to finish greeting before asking it anything.
        //
        // A shell opens with a banner — the VTY count, the last login, a
        // legal notice — and that banner ends with a prompt. Sending the
        // command first and then reading until a prompt matches the banner's
        // prompt, so the greeting comes back as the answer and the answer is
        // thrown away when the channel closes. A Huawei answered every query
        // with its own "last login" line.
        //
        // Nothing is sent to provoke this. An extra newline earns an extra
        // prompt, and then every later read stops one exchange early: the
        // paging command's echo was returned as the answer to the query.
        //
        // The wait is short because it only matters when the router says
        // nothing: with a banner the read ends at its prompt, and without one
        // there is no reason to hold the session open for the full idle
        // timeout.
        let _greeting = read_until_prompt(&mut channel, &prompt, GREETING_TIMEOUT, false).await?;

        // The paging command's own echo and prompt are consumed too: left in
        // the stream, the query's answer would start with the tail of this
        // exchange.
        if let Some(paging) = paging_command {
            channel
                .data_bytes(format!("{paging}\n").into_bytes())
                .await
                .map_err(|e| DriverError::IoError(e.to_string()))?;
            let _ = read_until_prompt(&mut channel, &prompt, self.idle_timeout, false).await?;
        }

        channel
            .data_bytes(format!("{command}\n").into_bytes())
            .await
            .map_err(|e| DriverError::IoError(e.to_string()))?;

        let mut output = read_until_prompt(&mut channel, &prompt, self.idle_timeout, true)
            .await
            .map_err(|e| {
                tracing::error!(
                    router = %router.id,
                    error = ?e,
                    "failed reading router output until prompt"
                );
                e
            })?;

        // If the output does not contain the command echo, but instead matches
        // the paging command or greeting banner that arrived late, read the
        // next prompt to capture the actual command answer.
        let has_command_echo = output
            .lines()
            .any(|line| line.trim_end().ends_with(command) || line.contains(command));

        if !has_command_echo {
            if let Some(paging) = paging_command {
                if output.contains(paging) {
                    tracing::warn!(
                        router = %router.id,
                        "ssh output appears to be delayed paging response; reading next prompt for command output"
                    );
                    if let Ok(next) =
                        read_until_prompt(&mut channel, &prompt, self.idle_timeout, false).await
                    {
                        if !next.is_empty() {
                            output.push('\n');
                            output.push_str(&next);
                        }
                    }
                }
            }
        }

        // Best effort: the answer is already in hand, so a failure to say
        // goodbye politely is not the visitor's problem.
        let _ = channel.close().await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "done", "en")
            .await;

        Ok(strip_echo(&output, command))
    }
}

/// How long to wait for a router's opening banner.
///
/// Routers over WAN or public route-servers may take several seconds to load
/// legal notices and present the initial shell prompt. When the router prints
/// a banner, this read returns immediately as soon as its prompt arrives.
const GREETING_TIMEOUT: Duration = Duration::from_secs(8);

/// Reads an exec channel until the router closes it.
///
/// There is no prompt to wait for here: the command was the request, and the
/// answer ends when the channel does.
async fn read_until_close(
    channel: &mut russh::Channel<client::Msg>,
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

/// Reads until the prompt reappears, the channel closes, or the router goes
/// quiet for longer than the idle timeout.
///
/// `require_output` is false for the reads whose content is discarded — the
/// greeting and the paging command — because a router that prints no banner is
/// perfectly normal and must not be reported as an empty response.
async fn read_until_prompt(
    channel: &mut russh::Channel<client::Msg>,
    prompt: &Regex,
    idle: Duration,
    require_output: bool,
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

    if require_output && output.is_empty() {
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
    // The last line is the prompt that ended the read. Do not strip if the line
    // is part of structured output ending with a closing brace/bracket.
    if lines.last().is_some_and(|line| {
        let t = line.trim();
        !t.ends_with('}') && !t.ends_with(']')
    }) {
        lines.pop();
    }

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

    #[test]
    fn echoed_command_strips_preceding_paging_artifacts() {
        let raw = "\
set cli screen-length 0
Screen length set to 0
rviews@route-server.ip.att.net> 
show route aspath-regex \".* 273556 .*\" detail | display json
{
    \"route-information\": []
}
rviews@route-server.ip.att.net> ";
        let cleaned = strip_echo(
            raw,
            "show route aspath-regex \".* 273556 .*\" detail | display json",
        );
        assert_eq!(cleaned, "{\n    \"route-information\": []\n}");
    }

    /// Default policy is documented as laboratory-only; this test exists so
    /// changing it is a deliberate act with a visible diff.
    #[test]
    fn the_default_host_key_policy_is_accept_any() {
        assert!(matches!(HostKeyPolicy::default(), HostKeyPolicy::AcceptAny));
    }
}

#[cfg(test)]
mod host_key_tests {
    use super::*;

    /// The order is the fix. A Huawei NE40E offers `ssh-dss, ssh-rsa,
    /// ecdsa-sha2-nistp521`, the negotiated algorithm is the first entry the
    /// server also supports, and decoding VRP's nistp521 signature fails — so
    /// RSA has to come first or every Huawei reports itself unreachable.
    #[test]
    fn rsa_is_offered_before_nistp521() {
        let preference = host_key_preference();

        let rsa = preference
            .iter()
            .position(|algorithm| matches!(algorithm, Algorithm::Rsa { .. }))
            .expect("RSA is offered");
        let nistp521 = preference
            .iter()
            .position(|algorithm| {
                matches!(
                    algorithm,
                    Algorithm::Ecdsa {
                        curve: EcdsaCurve::NistP521
                    }
                )
            })
            .expect("nistp521 is still offered, for a server that has nothing else");

        assert!(
            rsa < nistp521,
            "RSA must be preferred over nistp521: a Huawei offers both, and the client cannot read its nistp521 signature"
        );
    }

    /// The NIST curves are the whole point of the key exchange list. A Cisco
    /// IOS-XE offers `ecdh-sha2-nistp256/384/521` and
    /// `diffie-hellman-group14-sha1` and nothing else; the library's defaults
    /// contain none of them, so the handshake ended with "no common Kex
    /// algorithm" twelve milliseconds in, and every Cisco was unreachable.
    #[test]
    fn the_nist_curves_are_offered() {
        let preference = kex_preference();

        for curve in [
            kex::ECDH_SHA2_NISTP256,
            kex::ECDH_SHA2_NISTP384,
            kex::ECDH_SHA2_NISTP521,
        ] {
            assert!(
                preference.contains(&curve),
                "{curve:?} must be offered: a great deal of vendor equipment has nothing else"
            );
        }
    }

    /// Modern first. A router that has something better than a NIST curve
    /// still gets it; one that has not is merely reachable.
    #[test]
    fn curve25519_is_preferred_over_the_nist_curves() {
        let preference = kex_preference();
        let modern = preference
            .iter()
            .position(|name| *name == kex::CURVE25519)
            .expect("curve25519 is offered");
        let nist = preference
            .iter()
            .position(|name| *name == kex::ECDH_SHA2_NISTP256)
            .expect("nistp256 is offered");

        assert!(modern < nist);
    }

    /// SHA-1 key exchange algorithms are offered strictly as fallbacks after all
    /// modern algorithms and NIST curves, guaranteeing modern cryptography is
    /// preferred whenever the router supports it.
    #[test]
    fn sha1_key_exchange_is_fallback_only() {
        let preference = kex_preference();
        let nist = preference
            .iter()
            .position(|name| *name == kex::ECDH_SHA2_NISTP521)
            .expect("nistp521 is offered");

        for weak in [kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1] {
            let pos = preference
                .iter()
                .position(|name| *name == weak)
                .expect("legacy kex is offered as fallback");
            assert!(
                pos > nist,
                "{weak:?} must be positioned after modern algorithms as fallback only"
            );
        }
    }

    /// DSA is 1024-bit and gone from OpenSSH. A looking glass that
    /// authenticates a router with it is worse than one that refuses.
    #[test]
    fn dsa_is_not_offered() {
        assert!(
            !host_key_preference()
                .iter()
                .any(|algorithm| matches!(algorithm, Algorithm::Dsa)),
            "ssh-dss must not be added, however convenient it would be"
        );
    }
}


