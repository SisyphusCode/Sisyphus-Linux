use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::service::ghosttype_log;

#[derive(Debug, Deserialize)]
struct NetworkConfig {
    #[serde(default)]
    interface: Vec<InterfaceConfig>,
}

#[derive(Debug, Deserialize)]
struct InterfaceConfig {
    ifname: String,
    #[serde(default)]
    dhcp: bool,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    gateway: Option<String>,
}

pub fn configure_from_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let cfg: NetworkConfig = toml::from_str(&raw).map_err(|e| e.to_string())?;

    for iface in cfg.interface {
        if let Err(e) = configure_interface(&iface) {
            ghosttype_log(
                "WARN",
                &format!("Network interface '{}': {}", iface.ifname, e),
            );
            // Continue; network is best-effort during early boot
        }
    }
    Ok(())
}

fn configure_interface(iface: &InterfaceConfig) -> Result<(), String> {
    ghosttype_log("NET", &format!("Bringing up interface '{}'", iface.ifname));

    let ip_bin = find_ip_bin();
    run_cmd(&ip_bin, &["link", "set", &iface.ifname, "up"])?;

    if iface.dhcp {
        if command_exists("udhcpc") {
            let _ = Command::new("udhcpc")
                .args([
                    "-i",
                    &iface.ifname,
                    "-q",
                    "-b",
                    "-s",
                    "/usr/share/udhcpc/default.script",
                ])
                .spawn();
            ghosttype_log("NET", &format!("Started DHCP client on '{}'", iface.ifname));
        } else if command_exists("dhclient") {
            let _ = Command::new("dhclient").arg(&iface.ifname).spawn();
            ghosttype_log("NET", &format!("Started dhclient on '{}'", iface.ifname));
        } else {
            ghosttype_log("NET", "No DHCP client found (udhcpc/dhclient)");
        }
        return Ok(());
    }

    if let Some(addr) = &iface.address {
        run_cmd(&ip_bin, &["addr", "add", addr, "dev", &iface.ifname])?;
    }
    if let Some(gw) = &iface.gateway {
        run_cmd(&ip_bin, &["route", "add", "default", "via", gw])?;
    }

    Ok(())
}

fn run_cmd(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} exited with {status}", args.join(" ")))
    }
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn find_ip_bin() -> String {
    for candidate in ["/sbin/ip", "/usr/sbin/ip", "/bin/ip", "/usr/bin/ip"] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    "ip".to_string()
}
