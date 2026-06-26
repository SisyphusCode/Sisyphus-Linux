use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process;

use forge_common::{
    decode_response, encode_request, ControlRequest, ControlResponse, DEFAULT_CONTROL_SOCKET,
};

fn socket_path() -> String {
    env::var("FORGE_CONTROL_SOCKET").unwrap_or_else(|_| DEFAULT_CONTROL_SOCKET.to_string())
}

fn usage() -> ! {
    eprintln!(
        "Usage: forgectl <status|start|stop|restart|reload|enable|disable|list|activate-target|shutdown> [name]\n\
               forgectl <name>                 # status for just that service\n\
               forgectl enable|disable <service> [runlevel]   (defaults to graphical)\n\
               forgectl list [runlevel]\n\
               forgectl rc-update <add|del|show> <service> [runlevel]\n\
         Environment: FORGE_CONTROL_SOCKET (default {DEFAULT_CONTROL_SOCKET})\n\
         Shutdown: FORGE_SHUTDOWN_ACTION=reboot|halt|poweroff (default reboot)"
    );
    process::exit(2);
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        usage();
    }

    let request = match args[0].as_str() {
        "status" => ControlRequest::Status,
        "boot-profile" | "profile" => ControlRequest::BootProfile,
        "shutdown" => ControlRequest::Shutdown,
        "logs" => {
            let name = args.get(1).cloned().unwrap_or_else(|| usage());
            let tail = args
                .iter()
                .position(|a| a == "--tail")
                .and_then(|idx| args.get(idx + 1))
                .and_then(|v| v.parse().ok());
            ControlRequest::Logs { name, tail }
        }
        "start" => ControlRequest::Start {
            name: args.get(1).cloned().unwrap_or_else(|| usage()),
        },
        "stop" => ControlRequest::Stop {
            name: args.get(1).cloned().unwrap_or_else(|| usage()),
        },
        "restart" => ControlRequest::Restart {
            name: args.get(1).cloned().unwrap_or_else(|| usage()),
        },
        "reload" => ControlRequest::Reload {
            name: args.get(1).cloned().unwrap_or_else(|| usage()),
        },
        "activate-target" => ControlRequest::ActivateTarget {
            name: args.get(1).cloned().unwrap_or_else(|| usage()),
        },
        "enable" => ControlRequest::Enable {
            service: args.get(1).cloned().unwrap_or_else(|| usage()),
            runlevel: args.get(2).cloned(),
        },
        "disable" => ControlRequest::Disable {
            service: args.get(1).cloned().unwrap_or_else(|| usage()),
            runlevel: args.get(2).cloned(),
        },
        "list" | "ls" => ControlRequest::RcUpdateShow {
            runlevel: args.get(1).cloned(),
        },
        "rc-update" => match args.get(1).map(String::as_str) {
            Some("add") => ControlRequest::RcUpdateAdd {
                service: args.get(2).cloned().unwrap_or_else(|| usage()),
                runlevel: args.get(3).cloned().unwrap_or_else(|| usage()),
            },
            Some("del") | Some("delete") | Some("remove") => ControlRequest::RcUpdateDel {
                service: args.get(2).cloned().unwrap_or_else(|| usage()),
                runlevel: args.get(3).cloned().unwrap_or_else(|| usage()),
            },
            Some("show") | Some("list") => ControlRequest::RcUpdateShow {
                runlevel: args.get(2).cloned(),
            },
            _ => usage(),
        },
        _ => {
            // bare name -> status for just that service
            let name = args[0].clone();
            if !name.starts_with('-') && args.len() == 1 {
                ControlRequest::Service { name }
            } else {
                usage()
            }
        }
    };

    let mut stream = UnixStream::connect(socket_path()).unwrap_or_else(|e| {
        eprintln!("forgectl: cannot connect to control socket: {e}");
        process::exit(1);
    });

    let payload = encode_request(&request).expect("encode request");
    writeln!(stream, "{payload}").expect("write request");

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");

    let response: ControlResponse = decode_response(&line).unwrap_or_else(|e| {
        eprintln!("forgectl: invalid response: {e}");
        process::exit(1);
    });

    match response {
        ControlResponse::Ok {
            message,
            services,
            profile,
            logs,
        } => {
            if let Some(message) = message {
                println!("{message}");
            }
            if let Some(services) = services {
                if services.is_empty() {
                    // This can happen for bare `forgectl <name>` when not found
                    eprintln!("service not found");
                } else {
                    for svc in services {
                        let pid = svc.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
                        println!(
                            "{:16} {:8} {:12} pid={}",
                            svc.name, svc.kind, svc.state, pid
                        );
                    }
                }
            }
            if let Some(profile) = profile {
                println!(
                    "Boot {} ms — target '{}'",
                    profile.total_boot_ms, profile.active_target
                );
                for wave in profile.waves {
                    println!(
                        "  wave {}: {} ms — {}",
                        wave.wave,
                        wave.duration_ms,
                        wave.services.join(", ")
                    );
                }
            }
            if let Some(logs) = logs {
                for entry in logs {
                    if let Some(ts) = entry.ts {
                        println!("[{}] {}: {}", entry.source, ts, entry.message);
                    } else {
                        println!("[{}] {}", entry.source, entry.message);
                    }
                }
            }
        }
        ControlResponse::Error { message } => {
            eprintln!("error: {message}");
            process::exit(1);
        }
    }
}
