#!/usr/bin/env bash
#
# Builds and verifies the Linux packages, via the stages in `Containerfile`.
#
# CI and local runs both go through this script so there is exactly one
# definition of how the packages are built. A verify workflow that assembled
# them its own way would drift from the release workflow and stop testing
# what actually ships — which is worse than having no check, because it
# still reads as coverage.
#
#   ./scripts/linux-packages.sh build    # artifacts into ./dist
#   ./scripts/linux-packages.sh verify   # install and start each one
#   ./scripts/linux-packages.sh all      # both, build first
#
# Works with either podman or docker; set ENGINE to force one.
set -euo pipefail

cd "$(dirname "$0")/.."

ENGINE="${ENGINE:-$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)}"
IMAGE_PREFIX="${IMAGE_PREFIX:-pain-linux}"
DIST_DIR="${DIST_DIR:-dist}"

# Every image the test stages install into, so a failure names the distro
# that broke rather than a stage number.
TEST_STAGES=(test-deb test-rpm test-appimage)

build() {
    echo "==> Building packages with ${ENGINE}"
    "${ENGINE}" build --target builder -t "${IMAGE_PREFIX}-builder" -f Containerfile .

    # `podman build --output` only exists from podman 4.x, and the version
    # on Ubuntu 22.04 is 3.4 — create/copy/remove works on every version of
    # both engines and needs no volume mount.
    echo "==> Extracting artifacts into ${DIST_DIR}/"
    rm -rf "${DIST_DIR}"
    mkdir -p "${DIST_DIR}"
    local container
    container="$("${ENGINE}" create "${IMAGE_PREFIX}-builder" /bin/true)"
    trap '"${ENGINE}" rm -f "${container}" >/dev/null 2>&1 || true' RETURN
    "${ENGINE}" cp "${container}:/out/." "${DIST_DIR}/"

    ls -la "${DIST_DIR}/"
}

verify() {
    local failed=()
    for stage in "${TEST_STAGES[@]}"; do
        echo "==> Verifying: ${stage}"
        if ! "${ENGINE}" build --target "${stage}" -t "${IMAGE_PREFIX}-${stage}" -f Containerfile .; then
            failed+=("${stage}")
        fi
    done

    if [ ${#failed[@]} -gt 0 ]; then
        echo "FAILED: ${failed[*]}" >&2
        return 1
    fi
    echo "==> All package checks passed"
}

case "${1:-all}" in
    build) build ;;
    verify) verify ;;
    all) build && verify ;;
    *)
        echo "usage: $0 [build|verify|all]" >&2
        exit 2
        ;;
esac
