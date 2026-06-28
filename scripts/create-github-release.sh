#!/usr/bin/env bash
# Create GitHub release for a tag (requires: gh auth login).
#
# Usage: ./scripts/create-github-release.sh <tag>
#   e.g. ./scripts/create-github-release.sh v0.1.0-alpha.2
#
# The tag MUST already exist locally AND on the remote. Push first:
#   git push origin <tag>
set -euo pipefail

REPO="kenjiroe/ozr"

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <tag>   (e.g. v0.1.0-alpha.2)" >&2
  exit 2
fi
TAG="$1"
NOTES=".github/RELEASE_${TAG}.md"

if ! command -v gh >/dev/null 2>&1; then
  echo "Install GitHub CLI: https://cli.github.com/" >&2
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "Run: gh auth login" >&2
  exit 1
fi

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Tag $TAG does not exist locally. Create it first: git tag $TAG" >&2
  exit 1
fi

# Tag must be pushed to the remote, else the GitHub release points at a
# non-existent ref and release-check CI never runs.
if ! git ls-remote --tags "$REPO" "refs/tags/$TAG" | grep -q .; then
  echo "Tag $TAG is not pushed to $REPO. Run: git push origin $TAG" >&2
  exit 1
fi

if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  echo "Release $TAG already exists: https://github.com/$REPO/releases/tag/$TAG" >&2
  exit 1
fi

if [[ ! -f "$NOTES" ]]; then
  echo "Missing release notes: $NOTES" >&2
  exit 1
fi

gh release create "$TAG" \
  --repo "$REPO" \
  --verify-tag \
  --title "$TAG" \
  --notes-file "$NOTES"

echo "Published: https://github.com/$REPO/releases/tag/$TAG"
