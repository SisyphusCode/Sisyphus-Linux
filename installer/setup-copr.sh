#!/usr/bin/env bash
# Create/update the sisyphus-linux COPR project and submit installer SCM build.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OWNER="${COPR_OWNER:-sisyphuscode}"
PROJECT="sisyphus-linux"
PACKAGE="sisyphus-installer-config"
CLONE_URL="${COPR_CLONE_URL:-https://github.com/SisyphusCode/Sisyphus-Linux.git}"
SPEC="installer/sisyphus-installer-config.spec"
COMMITTISH="${COPR_COMMIT:-main}"

COPR_CHROOTS=(
    fedora-rawhide-x86_64
    fedora-44-x86_64
)

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

echo "==> Authenticated as: $(copr-cli whoami)"

CHROOT_ARGS=()
for chroot in "${COPR_CHROOTS[@]}"; do
    CHROOT_ARGS+=(--chroot "$chroot")
done

if copr-cli list "${OWNER}" 2>/dev/null | grep -q "^Name: ${PROJECT}$"; then
    echo "==> COPR project ${OWNER}/${PROJECT} already exists"
    copr-cli modify "${PROJECT}" \
        "${CHROOT_ARGS[@]}" \
        --description "Sisyphus Linux — Calamares installer config and branding"
else
    echo "==> Creating COPR project ${OWNER}/${PROJECT}..."
    copr-cli create "${PROJECT}" \
        "${CHROOT_ARGS[@]}" \
        --description "Sisyphus Linux — Calamares installer config and branding" \
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
        --method make_srpm \
        --subdir installer \
        --webhook-rebuild on
else
    echo "==> Adding SCM package ${PACKAGE}..."
    copr-cli add-package-scm "${OWNER}/${PROJECT}" \
        --name "${PACKAGE}" \
        --clone-url "${CLONE_URL}" \
        --commit "${COMMITTISH}" \
        --spec "${SPEC}" \
        --method make_srpm \
        --subdir installer \
        --webhook-rebuild on
fi

echo "==> Submitting SCM build (requires GitHub repo pushed first)..."
BUILD_OUT="$(copr-cli buildscm "${OWNER}/${PROJECT}" \
    --clone-url "${CLONE_URL}" \
    --commit "${COMMITTISH}" \
    --spec "${SPEC}" \
    --method make_srpm \
    --subdir installer \
    "${CHROOT_ARGS[@]}" \
    --nowait 2>&1)" || true
echo "${BUILD_OUT}"

BUILD_ID="$(echo "${BUILD_OUT}" | awk '/Created Build/{print $3}' | tr -d '[:space:]')"
if [[ -n "${BUILD_ID}" ]]; then
    echo "==> Build submitted: https://copr.fedorainfracloud.org/coprs/${OWNER}/${PROJECT}/build/${BUILD_ID}/"
    copr-cli watch-build "${BUILD_ID}" || true
else
    echo "==> Check builds: https://copr.fedorainfracloud.org/coprs/${OWNER}/${PROJECT}/"
fi

echo "==> Done."
echo "    COPR: https://copr.fedorainfracloud.org/coprs/${OWNER}/${PROJECT}/"