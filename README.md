# Sisyphus Linux

Forge-init Linux distribution with COSMIC desktop, Kiwi OEM images, and Calamares installer.

## Layout

| Directory | Purpose |
|-----------|---------|
| `the-forge/` | Rust PID 1 init system (DAG engine, forgectl, COPR spec) |
| `session-layer/` | Wayland/D-Bus session glue (systemd1-stub, COSMIC greeter) |
| `iso-builder/` | Kiwi OEM image definition and root overlay |
| `installer/` | Calamares settings, branding, and COPR spec |
| `packages/` | Custom RPMs (systemd-compat dummy provider) |

## Build image

```bash
sudo ./iso-builder/build.sh
```

## COPR packages

- `sisyphuscode/the-forge` — Forge init RPM
- `sisyphuscode/sisyphus-linux` — `sisyphus-installer-config` Calamares branding
