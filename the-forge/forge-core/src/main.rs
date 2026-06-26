mod boot;
mod boot_debug;
mod cgroup;
mod dbus_launch;
mod dbus_srv;
mod dbus_wait;
mod device;
mod early_boot;
mod engine;
mod environment;
mod ipc;
mod jobs;
mod journal;
mod launch;
mod mount_unit;
mod network;
mod notify;
mod plymouth;
mod privdrop;
mod rc_update;
mod reactor;
mod reaper;
mod recovery;
mod sandbox;
mod selinux;
mod service;
mod system_bus;
mod timer;
mod udev;
mod user_sessions;
mod vfs;

use parking_lot::Mutex;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::Duration;

use engine::ScriptEngine;
use service::forge::{lock_state, ForgeState};
use service::ghosttype_log;
use service::unit::{load_all_units, read_default_target};

fn unit_dir() -> PathBuf {
    std::env::var("FORGE_UNIT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/forge/units"))
}

fn systemd_unit_dir() -> PathBuf {
    std::env::var("FORGE_SYSTEMD_UNIT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/forge/systemd"))
}

fn network_config_path() -> PathBuf {
    std::env::var("FORGE_NETWORK_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/forge/network.toml"))
}

fn default_target_path() -> PathBuf {
    std::env::var("FORGE_DEFAULT_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/forge/default.target"))
}

fn boot_script_path() -> String {
    std::env::var("FORGE_BOOT_SCRIPT").unwrap_or_else(|_| "/etc/forge/boot.rhai".to_string())
}

fn log_dir() -> PathBuf {
    std::env::var("FORGE_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| recovery::resolve_log_dir())
}

fn resolve_default_target() -> String {
    if let Ok(name) = std::env::var("FORGE_TARGET") {
        return name;
    }
    if default_target_path().exists() {
        if let Ok(name) = read_default_target(&default_target_path()) {
            return name;
        }
    }
    "multi-user".to_string()
}

fn main() {
    // Parse CLI args very early (before any mounts or boot) so --help/--version work
    // even if the binary is invoked manually. Kernel usually passes no flags to init.
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let arg = args[1].as_str();
        if arg == "--help" || arg == "-h" {
            println!(
                "The Forge (PID 1 Init System) v{}",
                env!("CARGO_PKG_VERSION")
            );
            println!("Usage: forge-core [OPTIONS]");
            println!();
            println!("Options:");
            println!("  -h, --help     Print this help message");
            println!("  -v, --version  Print version information");
            println!();
            println!("Environment variables for customization:");
            println!("  FORGE_TARGET, FORGE_UNIT_DIR, FORGE_DEFAULT_TARGET,");
            println!("  FORGE_NETWORK_CONFIG, FORGE_BOOT_SCRIPT, FORGE_LOG_DIR, etc.");
            std::process::exit(0);
        }
        if arg == "--version" || arg == "-v" {
            println!("forge-core {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        // Ignore other early args (e.g. from kernel cmdline) and proceed.
    }

    boot::setup_environment();
    let is_pid1 = process::id() == 1 || std::env::var("FORGE_FORCE_PID1").is_ok();

    if !is_pid1 {
        ghosttype_log(
            "SANDBOX",
            "Not PID 1 — dry-run only; display-manager/getty/network/udev-trigger are skipped",
        );
        eprintln!(
            "NOTE: Running forge-core on a live desktop is a sandbox test only.\n\
             It will NOT start GDM, getty, or network-setup (those would log you out).\n\
             For a real boot test use: init=/usr/sbin/forge-core"
        );
    }

    if is_pid1 {
        let boot_stamp = format!(
            "forge-core {} pid={}",
            env!("CARGO_PKG_VERSION"),
            process::id(),
        );

        // Subreaper steals orphaned GDM/Xorg children from logind session leaders (breaks VT access).
        let desktop_boot = std::fs::read_to_string("/etc/forge/default.target")
            .map(|s| s.trim().eq_ignore_ascii_case("graphical"))
            .unwrap_or(false);
        let want_subreaper = std::env::var("FORGE_SUBREAPER")
            .map(|v| v == "1")
            .unwrap_or(!desktop_boot);
        if want_subreaper {
            ghosttype_log("INIT", "Claiming child subreaper status...");
            if let Err(e) = reaper::claim_subreaper_status() {
                ghosttype_log("WARN", &format!("Subreaper claim failed (continuing): {e}"));
            }
        } else {
            ghosttype_log(
                "INIT",
                "Skipping subreaper (graphical desktop — logind needs session process trees)",
            );
        }

        ghosttype_log("VFS", "Mounting kernel API virtual filesystems...");
        if let Err(e) = vfs::mount_essential_filesystems() {
            ghosttype_log("WARN", &format!("VFS mount incomplete (continuing): {e}"));
        }

        recovery::init_pid1_recovery();

        let _ = std::fs::create_dir_all("/var/log/forge");
        let _ = std::fs::create_dir_all("/var/lib/forge");
        let boot_line = format!("{boot_stamp}\n");
        let _ = std::fs::write("/var/log/forge/boot.log", &boot_line);
        let _ = std::fs::write("/var/lib/forge/boot.log", &boot_line);
        boot_debug::log(&format!("PID 1 boot begin {boot_stamp}"));

        // Ensure all directories a real desktop stack expects
        let _ = std::fs::create_dir_all("/run/forge");
        let _ = std::fs::create_dir_all("/run/forge/log");
        let _ = std::fs::create_dir_all("/var/log/forge");
        let _ = std::fs::create_dir_all("/run/dbus");

        let _ = std::fs::create_dir_all("/run/lock");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions("/run/lock", std::fs::Permissions::from_mode(0o1777));
        }

        let _ = std::fs::create_dir_all("/run/user");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions("/run/user", std::fs::Permissions::from_mode(0o755));
        }

        let _ = std::fs::create_dir_all("/var/tmp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions("/var/tmp", std::fs::Permissions::from_mode(0o1777));
        }

        let _ = std::fs::create_dir_all("/tmp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions("/tmp", std::fs::Permissions::from_mode(0o1777));
        }

        // Make /dev/pts/ptmx accessible if not already
        let _ = std::fs::create_dir_all("/dev/pts");
    }

    if let Err(e) = journal::init() {
        ghosttype_log("WARN", &format!("Journal init: {e}"));
    }

    if is_pid1 || std::env::var("FORGE_NETWORK").is_ok() {
        if let Err(e) = network::configure_from_file(&network_config_path()) {
            ghosttype_log("WARN", &format!("Network setup: {e}"));
        }
    }

    let active_target = resolve_default_target();
    let state = Arc::new(Mutex::new(ForgeState::new(active_target, log_dir())));
    {
        let mut locked = lock_state(&state);
        locked.set_boot_lenient(!is_pid1);
        locked.set_strict_boot(is_pid1);
    }

    // Check if native mode is enabled (pure Rust OpenRC-style units)
    let native_mode = std::env::var("FORGE_NATIVE_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    ghosttype_log(
        "UNITS",
        &format!("Loading units from {}", unit_dir().display()),
    );

    if native_mode {
        ghosttype_log("NATIVE", "Using native OpenRC-style mode");
        if let Err(err) =
            service::native::load_native_units(&mut lock_state(&state), &unit_dir(), is_pid1)
        {
            ghosttype_log("WARN", &format!("Native unit load issues: {err}"));
            if !is_pid1 {
                eprintln!("Native unit load failure: {err}");
                process::exit(1);
            }
        }
    } else {
        ghosttype_log(
            "UNITS",
            &format!(
                "Importing systemd units from {}",
                systemd_unit_dir().display()
            ),
        );
        if let Err(err) = load_all_units(
            &mut lock_state(&state),
            &unit_dir(),
            &systemd_unit_dir(),
            is_pid1,
        ) {
            ghosttype_log("WARN", &format!("Unit load issues: {err}"));
            if !is_pid1 {
                eprintln!("Unit load failure: {err}");
                process::exit(1);
            }
        }
    }

    ghosttype_log("ENGINE", "Spinning up embedded Rhai script interpreter...");
    let script_engine = ScriptEngine::new(Arc::clone(&state));

    let boot_script = boot_script_path();
    ghosttype_log(
        "PARSING",
        &format!("Loading script context: {}", boot_script),
    );
    if let Err(err) = script_engine.execute_script(&boot_script) {
        ghosttype_log("WARN", &format!("Script orchestration bypass: {}", err));
    }

    // Rust system bus names (systemd1, hostname1, …) must register before logind starts.
    system_bus::ensure_running(Arc::clone(&state));

    ghosttype_log(
        "BOOT",
        "Starting dependency-aware parallel service waves...",
    );
    {
        let mut locked = lock_state(&state);
        let boot_ok = if is_pid1 {
            locked.boot_with_fallback(&["multi-user", "rescue"])
        } else if let Err(err) = locked.boot_parallel() {
            eprintln!("Boot sequence failed: {err}");
            process::exit(1);
        } else {
            true
        };
        if is_pid1 && !boot_ok {
            ghosttype_log(
                "CRITICAL",
                "All boot targets failed — entering minimal steady state (rescue)",
            );
            locked.active_target = "rescue".into();
            let _ = locked.boot_parallel();
        }

        if is_pid1 {
            locked.persist_softlevel(&locked.active_target);
            let (dbus_ok, getty_ok, critical_failed) = locked.boot_health_signals();
            recovery::post_boot_recovery(recovery::assess_boot_health(
                dbus_ok,
                getty_ok,
                critical_failed,
            ));
        }
    }

    {
        let locked = lock_state(&state);
        if let Err(e) = udev::generate_rules(&locked) {
            ghosttype_log("WARN", &format!("udev rules: {e}"));
        }
        let dbus_info_dir = std::env::var("FORGE_DBUS_INFO_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/run/forge/dbus"));
        let _ = dbus_srv::write_bus_info(&dbus_info_dir);
    }

    dbus_srv::spawn_compat_server(Arc::clone(&state));
    if is_pid1 {
        plymouth::spawn_quit_worker();
    }
    timer::spawn_scheduler(Arc::clone(&state));

    if !is_pid1 {
        std::thread::sleep(Duration::from_millis(500));
        lock_state(&state).reap_all_pending();
        ghosttype_log("SUCCESS", "Sandbox execution complete. Exiting.");
        process::exit(0);
    }

    device::spawn_watchers(Arc::clone(&state));
    reactor::run(state);
}
