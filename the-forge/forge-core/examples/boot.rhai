// Rhai boot hooks — runs before unit waves start.
// Try: ./scripts/dev-boot.sh

log_status("SCRIPT", "Forge boot script loaded");
log_status("SCRIPT", "Boot elapsed: " + boot_ms() + " ms");

// Optional: override target before waves (must exist as [target] unit).
// set_target("multi-user");
