//! Runs a driver's parser over a file and prints what came out.
//!
//! This is the other half of `lab/capture.sh`. The script produces what a
//! router actually printed; this shows what the driver makes of it, which is
//! the only way to see the difference between "the parser ran" and "the parser
//! understood". Everything it prints is what the API would return.
//!
//!   cargo run -p looking-glass-core --example parse_capture -- \
//!       mikrotik_routeros ping lab/captured/mikrotik_routeros/ping_v4.txt
//!
//! Queries: ping, traceroute, bgp_route, bgp_summary.

use std::env;
use std::fs;
use std::process::ExitCode;

use looking_glass_core::driver::VendorDriver;
use looking_glass_core::vendors::{
    AristaDriver, BirdDriver, CiscoDriver, DatacomDriver, FrrDriver, HuaweiVrpDriver,
    JuniperDriver, MikrotikDriver, NokiaSrosDriver,
};

fn driver_for(vendor: &str) -> Option<Box<dyn VendorDriver>> {
    // The catalogue's own names, so what is typed here is what is configured
    // for a router.
    match vendor {
        "huawei_vrp" => Some(Box::new(HuaweiVrpDriver)),
        "cisco_iosxe" => Some(Box::new(CiscoDriver::new(false))),
        "cisco_iosxr" => Some(Box::new(CiscoDriver::new(true))),
        "juniper_junos" => Some(Box::new(JuniperDriver)),
        "nokia_sros" => Some(Box::new(NokiaSrosDriver)),
        "mikrotik_routeros" => Some(Box::new(MikrotikDriver)),
        "datacom_dmos" => Some(Box::new(DatacomDriver)),
        "bird_routing_daemon" => Some(Box::new(BirdDriver)),
        "arista_eos" => Some(Box::new(AristaDriver)),
        "frr" => Some(Box::new(FrrDriver)),
        _ => None,
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let [vendor, query, path] = arguments.as_slice() else {
        eprintln!("usage: parse_capture <vendor> <ping|traceroute|bgp_route|bgp_summary> <file>");
        return ExitCode::from(2);
    };

    let Some(driver) = driver_for(vendor) else {
        eprintln!("unknown vendor: {vendor}");
        return ExitCode::from(2);
    };

    let raw = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(2);
        }
    };

    // The capture files carry a header of comment lines naming the command.
    // Strip it: a router never sends those, and leaving them in would be
    // testing the parser against something it will not meet.
    let body: String = raw
        .lines()
        .skip_while(|line| line.starts_with('!'))
        .collect::<Vec<_>>()
        .join("\n");

    let outcome = match query.as_str() {
        "ping" => driver.parse_ping(&body).map(|r| format!("{r:#?}")),
        "traceroute" => driver.parse_traceroute(&body).map(|r| format!("{r:#?}")),
        "bgp_route" => driver.parse_bgp_route(&body).map(|r| format!("{r:#?}")),
        "bgp_summary" => driver.parse_bgp_summary(&body).map(|r| format!("{r:#?}")),
        other => {
            eprintln!("unknown query: {other}");
            return ExitCode::from(2);
        }
    };

    match outcome {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            // A parser that refuses is doing its job (fail closed). Say so
            // plainly rather than printing an empty structure.
            eprintln!("the driver refused this output: {error}");
            ExitCode::FAILURE
        }
    }
}
