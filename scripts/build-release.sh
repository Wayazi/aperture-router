#!/bin/bash
# Build release binaries for aperture-router
# Usage: ./scripts/build-release.sh [version]

set -e

VERSION=${1:-$(git describe --tags --abbrev=0 2>/dev/null || echo "dev")}
RELEASE_DIR="release"

echo "Building aperture-router ${VERSION}"

# Create release directory
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

# Build for current platform
echo "Building for $(uname -m)..."
cargo build --release

# Create archive for current platform
ARCH=$(uname -m)
if [ "$ARCH" = "x86_64" ]; then
    ARCHIVE_NAME="aperture-router-x86_64-linux"
elif [ "$ARCH" = "aarch64" ]; then
    ARCHIVE_NAME="aperture-router-aarch64-linux"
else
    ARCHIVE_NAME="aperture-router-${ARCH}-linux"
fi

cd target/release
tar czf "../../${RELEASE_DIR}/${ARCHIVE_NAME}.tar.gz" aperture-router
cd ../..

# Generate checksums
cd "$RELEASE_DIR"
sha256sum "${ARCHIVE_NAME}.tar.gz" > "${ARCHIVE_NAME}.sha256"
cd ..

echo ""
echo "Release files created in ${RELEASE_DIR}/:"
ls -lh "$RELEASE_DIR"

echo ""
echo "Checksums:"
cat "${RELEASE_DIR}/${ARCHIVE_NAME}.sha256"

echo ""
echo "To create a GitHub release:"
echo "  git tag -a v${VERSION} -m 'Release v${VERSION}'"
echo "  git push origin v${VERSION}"
echo ""
echo "Then create a release on GitHub with the artifacts from ${RELEASE_DIR}/"
