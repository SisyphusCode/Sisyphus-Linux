# ⚒️ The Forge

**An immortal, high-performance PID 1 init system forged in Rust.**

The Forge is designed to shatter standard boot timelines by replacing fragile shell scripts and bloated orchestration with a ruthless, memory-safe core. Engineered to propel highly customized environments and rolling-release systems like Sisyphus Arch, it saturates NVMe bandwidth and multi-core CPUs to achieve near-instantaneous desktop readiness.

## 🚀 Core Architecture

* **The Immortal Subreaper:** A zero-dependency, panic-resistant foundation that manages `SIGCHLD` signals directly from the kernel, guaranteeing zero leaked memory and zero defunct zombies.
* **Embedded Rhai Engine:** Bypasses the overhead of `/bin/sh` entirely. The Forge utilizes `.rhai` scripts to blend declarative dependency mapping with blazing-fast procedural execution.
* **Parallel DAG Orchestration:** Dynamically builds a Directed Acyclic Graph (DAG) during early boot to execute non-blocking user-space services simultaneously.
* **Socket Activation:** Pre-allocates file descriptors for core IPC buses (like D-Bus), allowing heavy desktop environments to initialize immediately without waiting on sequential dependencies.

## 🏗️ Workspace Structure

This repository utilizes a Cargo Workspace to separate the core init logic from downstream tooling.

* `forge-core/`: The PID 1 binary. Contains the subreaper, kernel API mounts (`/proc`, `/sys`, `/dev`), and the embedded Rhai scripting bridge.
* `forge-cli/`: User-space command-line tooling (`forgectl`).
* `forge-logind/`: Minimal session / runtime-dir manager.
* `forge-common/`: Shared IPC types.

## 🚀 Modes of Operation

The Forge supports two configuration modes:

* **Systemd Compatibility Mode** (default): Parses systemd-style `.service`, `.socket`, `.target` files
* **Native OpenRC-style Mode**: Uses simplified TOML configuration with OpenRC-inspired syntax

To enable Native Mode: Set `FORGE_NATIVE_MODE=1` environment variable.

See [NATIVE_MODE.md](NATIVE_MODE.md) for details on the native OpenRC-style configuration format.

## 🛠️ Runtime Service Management (`forgectl`)

`forgectl` is the control tool (talks to the init over a Unix socket).

Basic usage:

```bash
forgectl status                    # all services
forgectl <name>                    # status for just that service (e.g. forgectl dbus)
forgectl start <name>
forgectl stop <name>
forgectl restart <name>
forgectl reload <name>
forgectl enable <service> [runlevel]   # defaults to "graphical"
forgectl disable <service> [runlevel]
forgectl list [runlevel]
forgectl activate-target graphical
forgectl shutdown
```

Examples:

```bash
forgectl enable cosmic-session
forgectl start cosmic-greeter
forgectl status | grep cosmic
```

`rc-update` subcommand is still supported for compatibility:

```bash
forgectl rc-update add <service> <runlevel>
```

Environment: `FORGE_CONTROL_SOCKET` (defaults to `/run/forge/control.sock`).

## 🖥️ Booting to Desktop (Production)

The Forge is a **production-capable PID 1**. It is not a full distro — it manages services on top of whatever root filesystem and packages you provide.

### Recommended: Full root + Forge as init (real desktop)

1. Have a complete system (e.g. your Sisyphus Arch or other) with desktop packages installed:
   - dbus
   - udev / eudev / systemd
   - display manager (sddm / gdm / lightdm)
   - Your DE + drivers + fonts + polkit etc.
   - agetty or getty

2. Install Forge:
   ```bash
   make
   sudo ./scripts/install.sh /
   ```

### RHEL 10 notes
On RHEL 10 (and clones): `sudo dnf install rust cargo rustfmt clippy` then `make`.
The pinned 1.92.0 matches the EL10 rust package. Use mock-boot for safe testing without root.

To use stock systemd units (closer to "just like systemd"):
  FORGE_IMPORT_SYSTEM_UNITS=1 ./scripts/dev-boot.sh
Parser now handles many real .service (ExecStart, Type, After, User=, Wants, etc.).
Execution honors User=/Group=/WorkingDirectory= from units.
See also FORGE_SYSTEMD_UNIT_DIR for your .service files.

3. Install the production unit set (copy the realistic ones):
   ```bash
   sudo cp forge-core/examples/units/0{0,2,3,4,5}-*.forge.toml \
           forge-core/examples/units/5{0,1}-getty-*.forge.toml \
           forge-core/examples/units/60-display-manager.forge.toml \
           forge-core/examples/units/99-graphical.target.forge.toml \
           /etc/forge/units/
   ```

4. Edit the copied units to point `exec =` at the real binaries on your system.

5. Choose graphical boot:
   ```bash
   echo "graphical" > /etc/forge/default.target
   ```

6. Boot with:
   ```
   init=/usr/sbin/forge-core
   ```
   (in bootloader or dracut cmdline). The dracut module (`packaging/dracut/90forge`) can help.

Forge will:
- Mount essential VFS
- Start dbus → udev + trigger/settle → logind → gettys
- Start your display manager
- You get a graphical login / desktop

You can also drop real `.service` files into `/etc/forge/systemd/` — Forge imports a large subset.

### Fast iteration (no real PID 1)
```bash
./scripts/dev-boot.sh
FORGE_TARGET=graphical ./scripts/dev-boot.sh
```

### Full isolated sandbox mock boot (recommended for reproducing hangs/errors)
This runs the *complete* boot sequence (all waves, socket activation with real LISTEN_FDS fd passing, service spawns, dbus name waits, etc.) as PID 1 inside private namespaces + private /run tmpfs. No host /run pollution, pids, or devices are touched.

```bash
./scripts/mock-boot.sh
# Lighter run that still exercises the core problematic paths:
FORGE_TARGET=mock-desktop ./scripts/mock-boot.sh
# Or use the mock unit set:
FORGE_UNIT_DIR=forge-core/examples/units-mock ./scripts/mock-boot.sh
# Catch hangs:
timeout 20s ./scripts/mock-boot.sh
```

Logs and journal are captured under /tmp/forge-mock-$$/. See the script header for more options.

### QEMU test with more complete environment
Build the initramfs on a host that has the desktop packages, then:
```bash
./scripts/build-initramfs.sh
KERNEL=/boot/vmlinuz-... ./scripts/run-qemu.sh
```

See also the new production units under `forge-core/examples/units/` (dbus, udev, gettys, graphical, display-manager).

The sandbox will show restarts for daemons (normal — they expect real PID 1 + kernel). On a real boot they stay up.

### Most realistic test: bootc OCI container (CIQ RLC Pro AI official images)
CIQ ships RLC as bootc container images. This gives you the *exact* shipped userspace (packages, GDM, mutter, NetworkManager, NVIDIA bits, Plymouth, etc.) without building a custom initramfs.

```bash
# Build + run with Forge as PID 1 inside the official bootc image
./scripts/run-bootc-test.sh

# Or with more output / different timeout
DEBUG=1 TIMEOUT=60s ./scripts/run-bootc-test.sh
```

The script:
* Builds release `forge-core`
* Runs the bootc image (`depot.ciq.com/.../rlc-bootc:pro-ai-9` by default) 
* Overrides `/sbin/init` + injects your current `/etc/forge` + `/usr/libexec/forge` scripts via bind mounts
* Uses `--privileged` + realistic volumes so udev, cgroups, dbus, logind etc. behave like a real system

Capture the serial-like output and grep for `⏱️`, `WAVE`, `FAILED`, `dbus`, `display-manager`, `gdm`, `plymouth` etc.

This is currently the closest you can get to "my real machine" without a full bare-metal switch or QEMU disk image.

For permanent images:
* Use `bootc-image-builder` (see query) to turn the image into qcow2, then QEMU it (you can still bind-mount or layer your init).

## 🛡️ License

MIT License
