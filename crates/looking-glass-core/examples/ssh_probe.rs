//! Opens an SSH session to a router and reports what the handshake did.
//!
//! Written for one question: which host key algorithms a device actually
//! works with. A vendor that offers three and breaks on the one the client
//! prefers is indistinguishable, from the outside, from a device that is
//! unreachable — the connection simply fails.
//!
//!   cargo run -p looking-glass-core --example ssh_probe -- 172.20.20.6 admin admin
//!
//! The password is an argument because this is a lab tool; the product reads
//! credentials from the environment and never from a command line.

use std::borrow::Cow;
use std::env;
use std::process::ExitCode;
use std::sync::Arc;

use russh::client;
use russh::keys::{Algorithm, HashAlg, PublicKeyOrCertificate};

struct AcceptAny;

impl client::Handler for AcceptAny {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        if let PublicKeyOrCertificate::PublicKey { key, .. } = key {
            println!("  host key offered: {}", key.algorithm());
        }
        Ok(true)
    }
}

async fn try_with(
    host: &str,
    user: &str,
    password: &str,
    label: &str,
    keys: Vec<Algorithm>,
) -> bool {
    println!("{label}");
    let mut preferred = client::Config::default().preferred;
    preferred.key = Cow::Owned(keys);
    // The product's key exchange list, always: a device that shares no key
    // exchange with the client never gets as far as its host key, and the
    // error then says nothing about the algorithms being tried here.
    preferred.kex = Cow::Owned(looking_glass_core::executor::ssh::kex_preference());

    let config = Arc::new(client::Config {
        preferred,
        ..Default::default()
    });

    match client::connect(config, (host, 22), AcceptAny).await {
        Ok(mut session) => match session.authenticate_password(user, password).await {
            Ok(result) if result.success() => {
                println!("  connected and authenticated\n");
                true
            }
            Ok(_) => {
                println!("  connected, authentication refused\n");
                false
            }
            Err(error) => {
                println!("  connected, authentication error: {error}\n");
                false
            }
        },
        Err(error) => {
            println!("  handshake failed: {error}\n");
            false
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let [host, user, password] = arguments.as_slice() else {
        eprintln!("usage: ssh_probe <host> <user> <password>");
        return ExitCode::from(2);
    };

    let rsa_sha512 = Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    };
    let rsa_sha256 = Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    };
    let rsa_sha1 = Algorithm::Rsa { hash: None };

    let defaults = client::Config::default().preferred.key.to_vec();
    println!("client default host key preference:");
    for algorithm in &defaults {
        println!("  {algorithm}");
    }
    println!();

    let mut worked = Vec::new();
    for (label, keys) in [
        ("the library's defaults", defaults.clone()),
        (
            "what this product asks for",
            looking_glass_core::executor::ssh::host_key_preference(),
        ),
        (
            "RSA only, newest hash first",
            vec![rsa_sha512.clone(), rsa_sha256.clone(), rsa_sha1.clone()],
        ),
        ("ssh-rsa only (SHA-1, what a VRP offers)", vec![rsa_sha1]),
        ("ed25519 only", vec![Algorithm::Ed25519]),
    ] {
        if try_with(host, user, password, label, keys).await {
            worked.push(label);
        }
    }

    println!("worked: {worked:?}");
    if worked.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
