//! The command catalogue: what NOGGlass may ask a router, per vendor.
//!
//! Commands are data, not code ([ADR-0006] section 2.2): they live in
//! `commands.toml`, embedded at compile time. Adding a vendor is editing that
//! file plus writing a parser — no `match` arm anywhere else changes.
//!
//! The catalogue is also the second half of the injection defence. The first
//! half is [`crate::target`], which turns visitor text into typed values; this
//! module only ever substitutes those typed values into a fixed template, and
//! refuses at load time any template carrying shell metacharacters or an
//! unknown placeholder. A typo therefore fails when the binary starts, not on
//! a production router.
//!
//! [ADR-0006]: https://github.com/andrediashexa/looking-glass/blob/main/docs/adr/0006-structured-bgp-model-and-data-sources.md

use crate::driver::QueryTarget;
use crate::target::QueryLimits;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::LazyLock;

/// The catalogue shipped with the binary.
static BUILTIN_TOML: &str = include_str!("commands.toml");

/// Placeholders a template may use. Anything else fails validation.
const KNOWN_PLACEHOLDERS: &[&str] = &["target", "network", "netmask", "prefix_len", "count", "asn"];

/// Characters that let one command become two, or run another program. Pipes
/// are allowed because Junos and IOS use them for output modifiers, and the
/// content on both sides is ours.
const FORBIDDEN_IN_TEMPLATES: &[&str] = &[";", "&&", "||", "`", "$(", "\n", "\r", ">>"];

/// What can go wrong with the catalogue, at load time or at render time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogueError {
    /// The embedded file did not parse.
    Malformed(String),
    /// A template used a placeholder the renderer does not know.
    UnknownPlaceholder {
        vendor: String,
        command: String,
        placeholder: String,
    },
    /// A template contained something that could chain commands.
    UnsafeTemplate {
        vendor: String,
        command: String,
        found: String,
    },
    /// No vendor with that identifier.
    UnknownVendor(String),
    /// The vendor has no command for this query, which is an honest answer:
    /// the feature is missing, not approximated.
    Unsupported { vendor: String, query: String },
}

impl fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "command catalogue is malformed: {why}"),
            Self::UnknownPlaceholder {
                vendor,
                command,
                placeholder,
            } => write!(
                f,
                "{vendor}.{command} uses unknown placeholder {{{placeholder}}}"
            ),
            Self::UnsafeTemplate {
                vendor,
                command,
                found,
            } => write!(
                f,
                "{vendor}.{command} contains {found:?}, which could chain commands"
            ),
            Self::UnknownVendor(vendor) => write!(f, "no vendor named {vendor:?} in the catalogue"),
            Self::Unsupported { vendor, query } => {
                write!(f, "{vendor} has no command for {query}")
            }
        }
    }
}

impl std::error::Error for CatalogueError {}

/// One vendor's commands, exactly as the TOML declares them.
#[derive(Debug, Clone, Deserialize)]
pub struct VendorCommands {
    pub display_name: String,
    /// Command that turns off output paging. Absent when the vendor does not
    /// page, such as RouterOS.
    #[serde(default)]
    pub disable_paging: Option<String>,
    /// Regex matching the router prompt, used to detect end of output.
    pub prompt: String,
    /// How to ask this vendor for an answer.
    #[serde(default)]
    pub session: SessionMode,

    pub ping_v4: Option<String>,
    pub ping_v6: Option<String>,
    pub traceroute_v4: Option<String>,
    pub traceroute_v6: Option<String>,
    pub bgp_route_v4: Option<String>,
    pub bgp_route_v6: Option<String>,
    /// Dedicated command when querying a single host IP (Longest Prefix Match)
    /// rather than a network prefix. When absent, falls back to `bgp_route_v4`.
    pub bgp_route_ip_v4: Option<String>,
    /// Dedicated command when querying a single IPv6 host (Longest Prefix Match).
    /// When absent, falls back to `bgp_route_v6`.
    pub bgp_route_ip_v6: Option<String>,
    pub bgp_route_asn: Option<String>,
    pub bgp_summary: Option<String>,
}

impl VendorCommands {
    fn templates(&self) -> impl Iterator<Item = (&'static str, &String)> {
        [
            ("ping_v4", self.ping_v4.as_ref()),
            ("ping_v6", self.ping_v6.as_ref()),
            ("traceroute_v4", self.traceroute_v4.as_ref()),
            ("traceroute_v6", self.traceroute_v6.as_ref()),
            ("bgp_route_v4", self.bgp_route_v4.as_ref()),
            ("bgp_route_v6", self.bgp_route_v6.as_ref()),
            ("bgp_route_ip_v4", self.bgp_route_ip_v4.as_ref()),
            ("bgp_route_ip_v6", self.bgp_route_ip_v6.as_ref()),
            ("bgp_route_asn", self.bgp_route_asn.as_ref()),
            ("bgp_summary", self.bgp_summary.as_ref()),
        ]
        .into_iter()
        .filter_map(|(name, tpl)| tpl.map(|t| (name, t)))
    }
}

/// How a command reaches a router.
///
/// Most platforms want an interactive shell on a terminal: they refuse an exec
/// request, or they need a paging command sent first, and their answer ends at
/// a prompt. Some want the opposite, and giving them a shell is worse than
/// useless — RouterOS probes the terminal it has been given and **waits for
/// the client to answer** a Device Status Report before printing anything, and
/// IOS-XR closes a session that is fed a paging command and a query together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// A shell on a pseudo-terminal, read until the prompt returns.
    #[default]
    Shell,
    /// One exec request per command, read until the channel closes. No
    /// terminal, so nothing to negotiate and no prompt to wait for.
    Exec,
}

/// Every vendor the binary knows how to talk to.
#[derive(Debug, Clone)]
pub struct Catalogue {
    vendors: BTreeMap<String, VendorCommands>,
}

/// The catalogue embedded in the binary, validated once.
///
/// Validation failure is a programming error in `commands.toml`, caught by the
/// test suite before it can reach a release.
pub static BUILTIN: LazyLock<Catalogue> = LazyLock::new(|| {
    Catalogue::from_toml(BUILTIN_TOML).expect("the embedded command catalogue must be valid")
});

impl Catalogue {
    /// Parses and validates a catalogue.
    pub fn from_toml(source: &str) -> Result<Self, CatalogueError> {
        let vendors: BTreeMap<String, VendorCommands> =
            toml::from_str(source).map_err(|e| CatalogueError::Malformed(e.to_string()))?;
        let catalogue = Self { vendors };
        catalogue.validate()?;
        Ok(catalogue)
    }

    /// Rejects templates that could chain commands or that use a placeholder
    /// the renderer would leave in place.
    fn validate(&self) -> Result<(), CatalogueError> {
        for (vendor, commands) in &self.vendors {
            for (name, template) in commands.templates() {
                for bad in FORBIDDEN_IN_TEMPLATES {
                    if template.contains(bad) {
                        return Err(CatalogueError::UnsafeTemplate {
                            vendor: vendor.clone(),
                            command: name.to_string(),
                            found: (*bad).to_string(),
                        });
                    }
                }
                for placeholder in placeholders(template) {
                    if !KNOWN_PLACEHOLDERS.contains(&placeholder.as_str()) {
                        return Err(CatalogueError::UnknownPlaceholder {
                            vendor: vendor.clone(),
                            command: name.to_string(),
                            placeholder,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Vendor identifiers, sorted.
    pub fn vendor_ids(&self) -> impl Iterator<Item = &str> {
        self.vendors.keys().map(String::as_str)
    }

    pub fn vendor(&self, id: &str) -> Result<&VendorCommands, CatalogueError> {
        self.vendors
            .get(id)
            .ok_or_else(|| CatalogueError::UnknownVendor(id.to_string()))
    }

    /// Builds the ping command for one vendor.
    pub fn ping(
        &self,
        vendor: &str,
        target: IpAddr,
        limits: &QueryLimits,
    ) -> Result<String, CatalogueError> {
        let commands = self.vendor(vendor)?;
        let template = pick(
            target.is_ipv4(),
            commands.ping_v4.as_ref(),
            commands.ping_v6.as_ref(),
        )
        .ok_or_else(|| CatalogueError::Unsupported {
            vendor: vendor.to_string(),
            query: "ping".to_string(),
        })?;

        Ok(render(
            template,
            &[
                ("target", target.to_string()),
                ("count", limits.ping_count.clamp(1, 20).to_string()),
            ],
        ))
    }

    /// Builds the traceroute command for one vendor.
    pub fn traceroute(&self, vendor: &str, target: IpAddr) -> Result<String, CatalogueError> {
        let commands = self.vendor(vendor)?;
        let template = pick(
            target.is_ipv4(),
            commands.traceroute_v4.as_ref(),
            commands.traceroute_v6.as_ref(),
        )
        .ok_or_else(|| CatalogueError::Unsupported {
            vendor: vendor.to_string(),
            query: "traceroute".to_string(),
        })?;

        Ok(render(template, &[("target", target.to_string())]))
    }

    /// Builds the BGP route lookup for one vendor.
    pub fn bgp_route(&self, vendor: &str, target: &QueryTarget) -> Result<String, CatalogueError> {
        let commands = self.vendor(vendor)?;
        let unsupported = || CatalogueError::Unsupported {
            vendor: vendor.to_string(),
            query: "bgp_route".to_string(),
        };

        match target {
            QueryTarget::Asn(asn) => {
                let template = commands.bgp_route_asn.as_ref().ok_or_else(unsupported)?;
                Ok(render(template, &[("asn", asn.to_string())]))
            }
            QueryTarget::Ip(ip) => {
                let template = if ip.is_ipv4() {
                    commands
                        .bgp_route_ip_v4
                        .as_ref()
                        .or(commands.bgp_route_v4.as_ref())
                } else {
                    commands
                        .bgp_route_ip_v6
                        .as_ref()
                        .or(commands.bgp_route_v6.as_ref())
                }
                .ok_or_else(unsupported)?;
                let netmask = if ip.is_ipv4() {
                    "255.255.255.255".to_string()
                } else {
                    String::new()
                };
                Ok(render(
                    template,
                    &[
                        ("target", ip.to_string()),
                        ("network", ip.to_string()),
                        ("netmask", netmask),
                        (
                            "prefix_len",
                            if ip.is_ipv4() { "32" } else { "128" }.to_string(),
                        ),
                    ],
                ))
            }
            QueryTarget::Prefix(net) => {
                let template = pick(
                    net.addr().is_ipv4(),
                    commands.bgp_route_v4.as_ref(),
                    commands.bgp_route_v6.as_ref(),
                )
                .ok_or_else(unsupported)?;
                Ok(render(
                    template,
                    &[
                        ("target", net.to_string()),
                        ("network", net.network().to_string()),
                        ("netmask", net.netmask().to_string()),
                        ("prefix_len", net.prefix_len().to_string()),
                    ],
                ))
            }
        }
    }

    /// Builds the BGP summary command for one vendor.
    pub fn bgp_summary(&self, vendor: &str) -> Result<String, CatalogueError> {
        let commands = self.vendor(vendor)?;
        commands
            .bgp_summary
            .clone()
            .ok_or_else(|| CatalogueError::Unsupported {
                vendor: vendor.to_string(),
                query: "bgp_summary".to_string(),
            })
    }
}

fn pick<'a>(is_v4: bool, v4: Option<&'a String>, v6: Option<&'a String>) -> Option<&'a String> {
    if is_v4 {
        v4
    } else {
        v6
    }
}

/// Substitutes `{name}` placeholders.
///
/// Values come from typed data — addresses, prefixes, AS numbers, a clamped
/// probe count — so nothing a visitor typed is ever inserted here.
fn render(template: &str, values: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// Placeholder names used by a template.
fn placeholders(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                found.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::parse_target;

    #[test]
    fn the_shipped_catalogue_loads_and_validates() {
        let catalogue = &*BUILTIN;
        let ids: Vec<&str> = catalogue.vendor_ids().collect();
        assert!(ids.contains(&"huawei_vrp"), "got {ids:?}");
        assert!(ids.contains(&"mikrotik_routeros"), "got {ids:?}");
        assert_eq!(ids.len(), 9, "got {ids:?}");
    }

    #[test]
    fn renders_huawei_commands_from_typed_values() {
        let catalogue = &*BUILTIN;
        let limits = QueryLimits::default();

        assert_eq!(
            catalogue
                .ping("huawei_vrp", "198.51.100.1".parse().unwrap(), &limits)
                .unwrap(),
            "ping -c 5 198.51.100.1"
        );
        assert_eq!(
            catalogue
                .traceroute("huawei_vrp", "2001:db8::1".parse().unwrap())
                .unwrap(),
            "tracert ipv6 2001:db8::1"
        );
        // VRP wants address and mask separately for IPv4 when querying a prefix.
        assert_eq!(
            catalogue
                .bgp_route("huawei_vrp", &parse_target("198.51.100.0/24").unwrap())
                .unwrap(),
            "display bgp routing-table 198.51.100.0 255.255.255.0"
        );
        // But for a single host IP (Longest Prefix Match), VRP takes the IP directly without a netmask.
        assert_eq!(
            catalogue
                .bgp_route("huawei_vrp", &parse_target("191.243.120.1").unwrap())
                .unwrap(),
            "display bgp routing-table 191.243.120.1"
        );
        // And separately for IPv6 too. Written with a slash, an NE40E answers
        // `Wrong parameter found at '^' position` and every IPv6 route query
        // on every Huawei comes back empty — which the interface shows as no
        // route to that prefix.
        assert_eq!(
            catalogue
                .bgp_route("huawei_vrp", &parse_target("2001:db8::/32").unwrap())
                .unwrap(),
            "display bgp ipv6 routing-table 2001:db8:: 32"
        );
        assert_eq!(
            catalogue
                .bgp_route("huawei_vrp", &parse_target("2001:db8::1").unwrap())
                .unwrap(),
            "display bgp ipv6 routing-table 2001:db8::1"
        );
        assert_eq!(
            catalogue
                .bgp_route("huawei_vrp", &parse_target("AS65500").unwrap())
                .unwrap(),
            "display bgp routing-table regular-expression _65500_"
        );
    }

    #[test]
    fn every_template_renders_without_leftover_placeholders() {
        let catalogue = &*BUILTIN;
        let limits = QueryLimits::default();
        let targets = [
            parse_target("198.51.100.0/24").unwrap(),
            parse_target("2001:db8::/32").unwrap(),
            parse_target("198.51.100.1").unwrap(),
            parse_target("2001:db8::1").unwrap(),
            parse_target("AS65500").unwrap(),
        ];

        for vendor in catalogue.vendor_ids() {
            for target in &targets {
                if let Ok(command) = catalogue.bgp_route(vendor, target) {
                    assert!(
                        !command.contains('{'),
                        "{vendor} left a placeholder in {command:?}"
                    );
                }
            }
            for ip in ["198.51.100.1", "2001:db8::1"] {
                let ip: IpAddr = ip.parse().unwrap();
                if let Ok(command) = catalogue.ping(vendor, ip, &limits) {
                    assert!(
                        !command.contains('{'),
                        "{vendor} left a placeholder in {command:?}"
                    );
                }
                if let Ok(command) = catalogue.traceroute(vendor, ip) {
                    assert!(
                        !command.contains('{'),
                        "{vendor} left a placeholder in {command:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_missing_command_is_reported_not_approximated() {
        // RouterOS has no AS-path regex lookup in the catalogue.
        let err = BUILTIN
            .bgp_route("mikrotik_routeros", &parse_target("AS65500").unwrap())
            .unwrap_err();
        assert!(matches!(err, CatalogueError::Unsupported { .. }), "{err:?}");
    }

    #[test]
    fn unknown_vendor_is_an_error() {
        let err = BUILTIN.bgp_summary("not_a_vendor").unwrap_err();
        assert_eq!(err, CatalogueError::UnknownVendor("not_a_vendor".into()));
    }

    #[test]
    fn templates_that_could_chain_commands_are_refused_at_load() {
        let hostile = r##"
[evil]
display_name = "Evil"
prompt = "#"
ping_v4 = "ping {target} ; reload"
"##;
        let err = Catalogue::from_toml(hostile).unwrap_err();
        assert!(
            matches!(err, CatalogueError::UnsafeTemplate { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_placeholders_are_refused_at_load() {
        let typo = r##"
[typo]
display_name = "Typo"
prompt = "#"
ping_v4 = "ping {targt}"
"##;
        let err = Catalogue::from_toml(typo).unwrap_err();
        assert!(
            matches!(
                err,
                CatalogueError::UnknownPlaceholder { ref placeholder, .. } if placeholder == "targt"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn ping_count_is_clamped_even_if_configuration_is_absurd() {
        let limits = QueryLimits {
            ping_count: 200,
            ..QueryLimits::default()
        };
        let command = BUILTIN
            .ping("huawei_vrp", "198.51.100.1".parse().unwrap(), &limits)
            .unwrap();
        assert_eq!(command, "ping -c 20 198.51.100.1");
    }
}
