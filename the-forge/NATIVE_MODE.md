# Native OpenRC-style Mode for The Forge

This document describes the new native OpenRC-style mode for The Forge init system. This mode provides OpenRC-like functionality but implemented entirely in Rust, replacing the systemd compatibility layer.

## Overview

The Forge now supports two modes of operation:

1. **Systemd Compatibility Mode** (default): Parses systemd-style `.service`, `.socket`, `.target`, etc. files
2. **Native Mode** (new): Uses a simplified, OpenRC-inspired configuration format with pure Rust implementation

## Enabling Native Mode

To enable native mode, set the environment variable:

```bash
export FORGE_NATIVE_MODE=1
```

Or pass it to the init process in your bootloader:

```
init=/usr/sbin/forge-core FORGE_NATIVE_MODE=1
```

## Native Unit Format

Native units use TOML files with a simplified, OpenRC-inspired syntax. Unlike systemd units which have complex INI-style formats, native units are pure TOML and easier to write and parse.

### Service Units

A basic service unit:

```toml
name = "sshd"
description = "OpenSSH server"
command = "/usr/sbin/sshd"
args = ["-D"]

# OpenRC-style dependencies
need = ["localmount", "devfs"]
use = ["logger", "dns"]
after = ["network"]
before = ["login"]

# Runlevels (similar to OpenRC)
runlevels = ["default", "multi-user"]

# Process management
user = "root"
group = "root"
working_directory = "/"

# Supervision (OpenRC-style)
supervise = true
crash_on_start = true

# Environment
[environment]
SSHD_OPTS = "-4"
LANG = "en_US.UTF-8"
```

### Target Units

Targets are similar to OpenRC runlevels:

```toml
name = "multi-user"
description = "Multi-user target"
requires = ["dbus", "localmount", "devfs", "network"]
wants = ["sshd", "cronie"]
```

## Key Differences from Systemd Mode

### 1. Simplified Configuration
- **Systemd**: Complex INI-style with many sections (`[Unit]`, `[Service]`, `[Install]`)
- **Native**: Single flat TOML structure with clear OpenRC-style keys

### 2. Dependency Keywords
- **Systemd**: `After=`, `Requires=`, `Wants=`, `BindsTo=`, etc.
- **Native**: `need`, `use`, `after`, `before` (OpenRC-style)

### 3. Dependencies Semantics
| Native Keyword | Systemd Equivalent | Meaning |
|--------------|-------------------|--------|
| `need` | `Requires=` | Hard dependency - must start before this service |
| `use` | `Wants=` | Soft dependency - nice to have started |
| `after` | `After=` | Ordering - start after these services |
| `before` | `Before=` | Ordering - start before these services |

### 4. Runlevels vs Targets
- **Systemd**: Uses targets as the main boot goal
- **Native**: Uses runlevels (OpenRC-style) but still supports targets

Runlevel to target mapping:
- `boot` → `sysinit`
- `sysinit` → `sysinit`
- `single` → `rescue`
- `multi-user` → `multi-user`
- `graphical` → `graphical`

### 5. Service Types
Supported service types:
- `simple` - Default, process runs in foreground
- `forking` - Process forks into background
- `oneshot` - Runs once and exits
- `dbus` - D-Bus service with bus name

## Configuration Fields

### Service Configuration

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Service name |
| `description` | string | No | Human-readable description |
| `command` | string | Yes | Path to executable |
| `args` | array[string] | No | Command line arguments |
| `need` | array[string] | No | Hard dependencies |
| `use` | array[string] | No | Soft dependencies |
| `after` | array[string] | No | Start after these |
| `before` | array[string] | No | Start before these |
| `runlevels` | array[string] | No | Runlevels this service belongs to |
| `user` | string | No | User to run as |
| `group` | string | No | Group to run as |
| `working_directory` | string | No | Working directory |
| `environment` | map | No | Environment variables |
| `supervise` | bool | No | Enable supervision (auto-restart) |
| `crash_on_start` | bool | No | Fail if service crashes during startup |
| `timeout_secs` | integer | No | Startup timeout in seconds |
| `pidfile` | string | No | PID file to write |

### Target Configuration

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Target name |
| `description` | string | No | Human-readable description |
| `requires` | array[string] | No | Required dependencies |
| `wants` | array[string] | No | Wanted dependencies |

## Example Units

See `forge-core/examples/native/` for example units:

- `00-sysinit.target.toml` - System initialization target
- `01-localmount.service.toml` - Local filesystem mounts
- `02-devfs.service.toml` - Device filesystem
- `50-dbus.service.toml` - D-Bus system bus
- `99-multi-user.target.toml` - Multi-user target

## Using Native Mode

### 1. Enable Native Mode

Set the environment variable:

```bash
export FORGE_NATIVE_MODE=1
```

### 2. Create Unit Files

Create your service and target files in `/etc/forge/units/` using the native TOML format.

### 3. Set Default Target

Create `/etc/forge/default.target` with your desired target:

```
echo "multi-user" > /etc/forge/default.target
```

### 4. Boot with Native Mode

Add to your bootloader:

```
init=/usr/sbin/forge-core FORGE_NATIVE_MODE=1
```

## Benefits of Native Mode

### 1. Pure Rust Implementation
- No shell scripts involved
- All logic in Rust for safety and performance
- No external dependencies beyond what The Forge already needs

### 2. Simplified Configuration
- TOML is easier to read and write than INI
- Less boilerplate than systemd units
- More intuitive dependency keywords

### 3. Better Performance
- No parsing of complex INI-style formats
- Direct TOML parsing is faster
- Simpler dependency resolution

### 4. OpenRC Compatibility
- Familiar syntax for OpenRC users
- Similar concepts (runlevels, need/use dependencies)
- Easier migration from OpenRC

## Migration from Systemd Units

To migrate from systemd units to native units:

### Systemd Example
```ini
[Unit]
Description=OpenSSH server
After=network.target auditd.service
Requires=sshd-keygen.service

[Service]
Type=simple
ExecStart=/usr/sbin/sshd -D
User=root
Environment=SSHD_OPTS=-4

[Install]
WantedBy=multi-user.target
```

### Native Equivalent
```toml
name = "sshd"
description = "OpenSSH server"
command = "/usr/sbin/sshd"
args = ["-D"]
after = ["network", "sshd-keygen"]
need = ["sshd-keygen"]
runlevels = ["multi-user"]
user = "root"

[environment]
SSHD_OPTS = "-4"
```

## Migration from OpenRC

To migrate from OpenRC runscripts to native units:

### OpenRC Runscript Example
```bash
#!/sbin/openrc-run

description="SSH Daemon"

need="localmount devfs"
use="logger"

start() {
    ebegin "Starting sshd"
    start-stop-daemon --start --exec /usr/sbin/sshd -- -D
    eend $?
}

stop() {
    ebegin "Stopping sshd"
    start-stop-daemon --stop --exec /usr/sbin/sshd
    eend $?
}
```

### Native Equivalent
```toml
name = "sshd"
description = "SSH Daemon"
command = "/usr/sbin/sshd"
args = ["-D"]
need = ["localmount", "devfs"]
use = ["logger"]
runlevels = ["default"]
```

The Forge's native mode handles all the process management (start, stop, supervision) automatically - no need for shell scripts!

## Building and Testing

To test native mode:

```bash
# Build
make

# Test with native mode
FORGE_NATIVE_MODE=1 FORGE_UNIT_DIR=forge-core/examples/native ./scripts/dev-boot.sh
```

## Troubleshooting

### "Service not found" Errors
- Check that your unit files are in the correct directory (`/etc/forge/units/` by default)
- Verify the unit name matches exactly (case-sensitive)
- Check for TOML syntax errors

### Dependency Resolution Issues
- Ensure all services referenced in `need`, `use`, `after`, `before` exist
- Check for circular dependencies
- Use `FORGE_BOOT_DEBUG=1` for detailed dependency resolution logs

### Service Fails to Start
- Check that the `command` path is correct and executable
- Verify the user/group has permission to execute the command
- Check environment variables and working directory settings

## Debugging Native Mode

Enable verbose logging:

```bash
FORGE_NATIVE_MODE=1 FORGE_BOOT_DEBUG=1 ./scripts/dev-boot.sh
```

Check the service logs:

```bash
# With native mode, logs are still in the same locations
ls /var/log/forge/
ls /run/forge/log/
```

## Future Enhancements

The native mode can be extended with:

1. **Socket Activation**: Add native socket activation support
2. **Device Units**: Native device unit support
3. **Mount Units**: Native mount unit support
4. **Timer Units**: Native timer support
5. **More Service Types**: Additional service types as needed
6. **Cgroup v2 Support**: Enhanced cgroup support
7. **Resource Limits**: Native resource limit configuration

## Conclusion

The native OpenRC-style mode provides a simpler, more direct way to configure The Forge services while maintaining all the performance and safety benefits of the Rust implementation. It's particularly suitable for users coming from OpenRC who want a familiar configuration style but with modern init system features.
