use std::fs;
use std::os::unix::io::{AsRawFd, OwnedFd};
use std::path::Path;

use nix::sys::socket::{
    bind, listen, socket, AddressFamily, Backlog, SockFlag, SockType, UnixAddr,
};

#[derive(Debug, Clone)]
pub enum ListenSpec {
    UnixStream(String),
    UnixSeqpacket(String),
    Netlink { family: u32 },
}

#[derive(Debug)]
pub struct SocketEntry {
    pub path: String,
    pub fd: OwnedFd,
}

#[derive(Debug)]
pub struct SocketActivation {
    pub entries: Vec<SocketEntry>,
}

pub fn parse_listen(line: &str) -> ListenSpec {
    if let Some(rest) = line.strip_prefix("netlink:") {
        let mut parts = rest.split(':');
        let family = parts.next().unwrap_or("kobject-uevent");
        let group = parts.next().and_then(|g| g.parse().ok()).unwrap_or(1);
        let _ = family;
        return ListenSpec::Netlink { family: group };
    }
    if line.contains("seqpacket") || line == "/run/udev/control" {
        let path = line.strip_prefix("seqpacket:").unwrap_or(line).to_string();
        return ListenSpec::UnixSeqpacket(path);
    }
    ListenSpec::UnixStream(line.to_string())
}

pub fn activate_sockets(name: &str, listen: &[String]) -> Result<SocketActivation, String> {
    let mut entries = Vec::new();
    for line in listen {
        let spec = parse_listen(line);
        let fd = activate_listen(&spec)?;
        let label = match &spec {
            ListenSpec::UnixStream(p) | ListenSpec::UnixSeqpacket(p) => p.clone(),
            ListenSpec::Netlink { family } => format!("netlink:{family}"),
        };
        entries.push(SocketEntry { path: label, fd });
    }
    if entries.is_empty() {
        return Err(format!("socket unit '{name}' has no listen paths"));
    }
    Ok(SocketActivation { entries })
}

fn activate_listen(spec: &ListenSpec) -> Result<OwnedFd, String> {
    match spec {
        ListenSpec::UnixStream(path) => activate_unix_stream(path, SockType::Stream),
        ListenSpec::UnixSeqpacket(path) => activate_unix_stream(path, SockType::SeqPacket),
        ListenSpec::Netlink { family } => activate_netlink(*family),
    }
}

fn activate_unix_stream(path: &str, sock_type: SockType) -> Result<OwnedFd, String> {
    let path_obj = Path::new(path);
    if let Some(parent) = path_obj.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(path);

    let fd = socket(AddressFamily::Unix, sock_type, SockFlag::empty(), None)
        .map_err(|e| e.to_string())?;

    let addr = UnixAddr::new(path).map_err(|e| e.to_string())?;
    bind(fd.as_raw_fd(), &addr).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if sock_type == SockType::Stream {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o666));
        } else {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }

    listen(&fd, Backlog::new(128).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    if path == "/run/dbus/system_bus_socket" || path == "/run/udev/control" {
        crate::selinux::relabel_paths(&[
            path_obj
                .parent()
                .unwrap_or(path_obj)
                .to_str()
                .unwrap_or(path),
            path,
        ]);
    }

    Ok(fd)
}

fn activate_netlink(group: u32) -> Result<OwnedFd, String> {
    let fd = socket(
        AddressFamily::Netlink,
        SockType::Raw,
        SockFlag::empty(),
        None,
    )
    .map_err(|e| e.to_string())?;

    #[repr(C)]
    struct SockAddrNl {
        nl_family: u16,
        nl_pad: u16,
        nl_pid: u32,
        nl_groups: u32,
    }

    let addr = SockAddrNl {
        nl_family: libc::AF_NETLINK as u16,
        nl_pad: 0,
        nl_pid: 0,
        nl_groups: group,
    };

    let rc = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &addr as *const SockAddrNl as *const libc::sockaddr,
            std::mem::size_of::<SockAddrNl>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(format!("netlink bind: {}", std::io::Error::last_os_error()));
    }

    Ok(fd)
}
