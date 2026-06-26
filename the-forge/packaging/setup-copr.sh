#!/usr/bin/env bash
# Create/update the the-forge COPR project and submit a build.
#
# Prerequisites:
#   1. COPR API token from https://copr.fedorainfracloud.org/api/
#   2. Either export COPR_LOGIN + COPR_TOKEN, or create ~/.config/copr:
#
#        [copr-cli]
#        login = your-fas-username
#        username = your-fas-username
#        token = your-api-token
#        copr_url = https://copr.fedorainfracloud.org
#
# Usage:
#   ./packaging/setup-copr.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OWNER="${COPR_OWNER:-sisyphuscode}"
PROJECT="the-forge"
PACKAGE="the-forge"
CLONE_URL="https://github.com/SisyphusCode/the-forge.git"
SPEC="packaging/the-forge.spec"
COMMITTISH="${COPR_COMMIT:-main}"

COPR_CHROOTS=(
    epel-9-x86_64
    epel-10-x86_64
    centos-stream-10-x86_64
    fedora-44-x86_64
    fedora-rawhide-x86_64
)

for arg in "$@"; do
    case "$arg" in
        -h|--help)
            sed -n '2,20p' "$0"
            exit 0
            ;;
        *) echo "Unknown option: $arg" >&2; exit 2 ;;
    esac
done

if ! command -v copr-cli >/dev/null 2>&1; then
    echo "copr-cli not found. Install with: pip3 install copr-cli rich" >&2
    exit 1
fi

if [[ -n "${COPR_LOGIN:-}" && -n "${COPR_TOKEN:-}" ]]; then
    mkdir -p ~/.config
    cat > ~/.config/copr <<EOF
[copr-cli]
login = ${COPR_LOGIN}
username = ${COPR_LOGIN}
token = ${COPR_TOKEN}
copr_url = https://copr.fedorainfracloud.org
EOF
    chmod 600 ~/.config/copr
fi

if [[ ! -f ~/.config/copr ]]; then
    echo "Missing ~/.config/copr. Set COPR_LOGIN and COPR_TOKEN or create the file first." >&2
    exit 1
fi

echo "==> Authenticated as: $(copr-cli whoami)"

CHROOT_ARGS=()
for chroot in "${COPR_CHROOTS[@]}"; do
    CHROOT_ARGS+=(--chroot "$chroot")
done

if copr-cli list "${OWNER}" 2>/dev/null | grep -q "^Name: ${PROJECT}$"; then
    echo "==> COPR project ${OWNER}/${PROJECT} already exists"
    echo "==> Syncing chroots: ${COPR_CHROOTS[*]}"
    copr-cli modify "${PROJECT}" \
        "${CHROOT_ARGS[@]}" \
        --description "The Forge — Rust PID 1 init system with parallel DAG boot and forgectl"
else
    echo "==> Creating COPR project ${OWNER}/${PROJECT}..."
    copr-cli create "${PROJECT}" \
        "${CHROOT_ARGS[@]}" \
        --description "The Forge — Rust PID 1 init system with parallel DAG boot and forgectl" \
        --enable-net on
fi

if copr-cli list-packages "${OWNER}/${PROJECT}" --output-format json \
    | grep -q "\"name\": \"${PACKAGE}\""; then
    echo "==> Updating SCM package ${PACKAGE}..."
    copr-cli edit-package-scm "${OWNER}/${PROJECT}" \
        --name "${PACKAGE}" \
        --clone-url "${CLONE_URL}" \
        --commit "${COMMITTISH}" \
        --spec "${SPEC}" \
        --method rpkg \
        --webhook-rebuild on
else
    echo "==> Adding SCM package ${PACKAGE}..."
    copr-cli add-package-scm "${OWNER}/${PROJECT}" \
        --name "${PACKAGE}" \
        --clone-url "${CLONE_URL}" \
        --commit "${COMMITTISH}" \
        --spec "${SPEC}" \
        --method rpkg \
        --webhook-rebuild on
fi

echo "==> Submitting SCM build..."
BUILD_ID="$(copr-cli buildscm "${OWNER}/${PROJECT}" \
    --clone-url "${CLONE_URL}" \
    --commit "${COMMITTISH}" \
    --spec "${SPEC}" \
    --method rpkg \
    "${CHROOT_ARGS[@]}" \
    --nowait \
    | awk '/Created Build/{print $3}' | tr -d '[:space:]')"

if [[ -n "${BUILD_ID}" ]]; then
    echo "==> Build submitted: https://copr.fedorainfracloud.org/coprs/${OWNER}/${PROJECT}/build/${BUILD_ID}/"
    copr-cli watch-build "${BUILD_ID}" || true
else
    echo "==> Build submitted. Check: https://copr.fedorainfracloud.org/coprs/${OWNER}/${PROJECT}/"
fi

echo "==> Done."
echo "    COPR:  https://copr.fedorainfracloud.org/coprs/${OWNER}/${PROJECT}/"
echo "    Install:"
echo "      sudo dnf copr enable ${OWNER}/${PROJECT}"
echo "      sudo dnf install the-forge"