#!/usr/bin/env bash
# deploy.sh — one-command release wrapper for wb300.
#
# Implements the CLAUDE.md "Deploy a new version" runbook in two explicit
# phases so it respects the PR gate (bump → merge → tag):
#
#   ./deploy.sh bump <patch|minor|major|X.Y.Z>
#       On the workbranch. Bumps [package] version in Cargo.toml (+ refreshes
#       Cargo.lock), enforces the lockstep-changelog guard, runs the local
#       gates, commits the specific release files, and pushes the branch.
#       YOU then merge the PR to main (the gate); crates-publish.yml runs.
#
#   ./deploy.sh tag
#       On main, post-merge. Verifies a clean tree, that main is at the bumped
#       version and CI is green, then creates & pushes the vX.Y.Z tag — which
#       fires release.yml → windows-installers.yml. Refuses to re-tag an
#       existing version; never force-pushes.
#
# The release model is tag-triggered and cargo-dist-native: nothing deploys
# until a vX.Y.Z tag is pushed, so a bad merge never ships by itself.
#
# Runs under Git Bash on the Windows dev box, and on macOS/Linux. Requires:
# git, cargo, and (for CI checks in `tag`) the GitHub CLI `gh`.

set -euo pipefail

# Always operate from the repo root (the script's own directory).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

CO_AUTHOR_TRAILER="Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"

# ── helpers ─────────────────────────────────────────────────────────

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '  \033[36m*\033[0m %s\n' "$*"; }
ok()   { printf '  \033[32m✔\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*" >&2; }
die()  { printf '  \033[31m✘ %s\033[0m\n' "$*" >&2; exit 1; }

usage() {
    cat >&2 <<'EOF'
usage:
  ./deploy.sh bump <patch|minor|major|X.Y.Z>   # phase 1: on the workbranch
  ./deploy.sh tag                              # phase 2: on main, post-merge
EOF
    exit 2
}

# The [package] version is the single source of truth. Parse only the version
# inside the [package] section (Cargo.toml has other version-like keys, e.g.
# cargo-dist-version under [workspace.metadata.dist]).
current_version() {
    awk '
        /^\[/ { inpkg = ($0 == "[package]") }
        inpkg && /^version[[:space:]]*=/ {
            gsub(/.*=[[:space:]]*"/, ""); gsub(/".*/, ""); print; exit
        }
    ' Cargo.toml
}

# Rewrite ONLY the first version line inside [package].
set_version() {
    local new="$1"
    awk -v new="$new" '
        /^\[/ { inpkg = ($0 == "[package]") }
        {
            if (inpkg && !done && $0 ~ /^version[[:space:]]*=/) {
                print "version = \"" new "\""
                done = 1
                next
            }
            print
        }
    ' Cargo.toml > Cargo.toml.deploy.tmp && mv Cargo.toml.deploy.tmp Cargo.toml
}

# Compute the next version. Accepts an explicit X.Y.Z or a semver bump kind.
compute_next() {
    local cur="$1" kind="$2"
    if printf '%s' "$kind" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        printf '%s' "$kind"
        return
    fi
    # Strip any prerelease/build suffix before arithmetic.
    local triple major minor patch
    triple="${cur%%-*}"; triple="${triple%%+*}"
    # `|| true`: `read` returns non-zero at EOF without a trailing newline; the
    # heredoc supplies one, but guard anyway so a malformed (<3-component)
    # version can't silently abort the script under `set -e`.
    IFS=. read -r major minor patch <<EOF || true
$triple
EOF
    # A well-formed version is three numeric components; reject anything else
    # rather than producing a garbage bump like ".0.0".
    if ! printf '%s' "$triple" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        die "current [package] version '$cur' is not a clean X.Y.Z — fix Cargo.toml before bumping."
    fi
    case "$kind" in
        major) printf '%s.0.0' "$((major + 1))" ;;
        minor) printf '%s.%s.0' "$major" "$((minor + 1))" ;;
        patch) printf '%s.%s.%s' "$major" "$minor" "$((patch + 1))" ;;
        *) die "bump kind must be one of: patch | minor | major | X.Y.Z (got '$kind')" ;;
    esac
}

# Refresh Cargo.lock's wb300 entry to match the new [package] version. Done
# before the --locked gates so they don't fail on a stale lockfile.
refresh_lockfile() {
    cargo update -p wb300 --offline >/dev/null 2>&1 \
        || cargo update -p wb300 >/dev/null 2>&1 \
        || cargo check --quiet >/dev/null 2>&1 \
        || true
}

run_gates() {
    info "cargo fmt --all -- --check"
    cargo fmt --all -- --check || die "rustfmt found unformatted code"
    info "cargo clippy --all-targets --workspace -- -D warnings"
    cargo clippy --all-targets --workspace -- -D warnings || die "clippy failed"
    info "cargo test --locked --workspace"
    cargo test --locked --workspace || die "tests failed"
    # --allow-dirty: this gate runs after the version bump but BEFORE the
    # release commit, so the working tree intentionally carries the bumped
    # Cargo.toml / Cargo.lock / changelogs. The dry run still verifies the
    # package builds from the to-be-committed tree.
    info "cargo publish --dry-run --locked --allow-dirty"
    cargo publish --dry-run --locked --allow-dirty || die "cargo publish --dry-run failed"
    ok "all gates green"
}

# ── phase 1: bump ───────────────────────────────────────────────────

cmd_bump() {
    local kind="${1:-}"
    [ -n "$kind" ] || usage

    local branch cur new
    branch="$(git rev-parse --abbrev-ref HEAD)"
    if [ "$branch" = "main" ]; then
        warn "You are on 'main'. The runbook bumps on a workbranch, then merges"
        warn "to main via a gated PR. Re-run on a workbranch unless you have a"
        warn "stated reason to bump directly on main."
    fi

    cur="$(current_version)"
    [ -n "$cur" ] || die "could not read [package] version from Cargo.toml"
    new="$(compute_next "$cur" "$kind")"

    bold "Bumping wb300: v$cur → v$new"

    # Lockstep changelog guard. CHANGELOG.md carries the versioned section;
    # HUMAN_CHANGELOG.md deliberately has no version numbers, so we instead
    # require it to have changed since the previous release tag — the reliable
    # signal that BOTH changelogs were updated in this cycle.
    local new_re
    new_re="$(printf '%s' "$new" | sed 's/\./\\./g')"
    grep -qE "^## \[$new_re\]" CHANGELOG.md \
        || die "CHANGELOG.md has no '## [$new]' section. Add the release notes (rename the Unreleased section) before bumping."

    local prev_tag
    prev_tag="$(git describe --tags --abbrev=0 2>/dev/null || true)"
    if [ -n "$prev_tag" ]; then
        if git diff --quiet "$prev_tag" -- HUMAN_CHANGELOG.md; then
            die "HUMAN_CHANGELOG.md is unchanged since $prev_tag — update it in lockstep with CHANGELOG.md before bumping."
        fi
    else
        warn "No previous tag found (first release) — skipping the HUMAN_CHANGELOG diff check. Make sure it has an entry for this release."
    fi
    ok "lockstep changelog guard passed"

    set_version "$new"
    refresh_lockfile
    # Sanity: Cargo.lock should now carry the new version for wb300.
    if ! grep -q "^version = \"$new\"" Cargo.lock 2>/dev/null; then
        warn "Cargo.lock may not reflect v$new yet; the --locked gates below will catch a stale lockfile."
    fi
    ok "Cargo.toml + Cargo.lock set to v$new"

    run_gates

    git add Cargo.toml Cargo.lock CHANGELOG.md HUMAN_CHANGELOG.md
    git commit -m "chore(release): wb300 v$new

$CO_AUTHOR_TRAILER"

    info "pushing $branch to origin"
    git push -u origin "$branch"

    bold "Bump committed and pushed."
    cat <<EOF

Next:
  1. Open / update the PR for '$branch' → main and let CI go green.
  2. Merge to main (the gate). crates-publish.yml publishes wb300 to crates.io.
  3. On main, run:  ./deploy.sh tag
EOF
}

# ── phase 2: tag ────────────────────────────────────────────────────

cmd_tag() {
    local branch cur tag
    branch="$(git rev-parse --abbrev-ref HEAD)"
    [ "$branch" = "main" ] || die "tag must be pushed from 'main' (you are on '$branch'). Check out main after the PR merges."

    git diff --quiet && git diff --cached --quiet \
        || die "working tree is dirty — commit or stash before tagging."

    info "fetching origin"
    git fetch origin --quiet --tags

    # main must match origin/main (the merged, CI-green commit).
    if ! git diff --quiet main origin/main; then
        die "local main differs from origin/main — pull --ff-only first."
    fi

    cur="$(current_version)"
    [ -n "$cur" ] || die "could not read [package] version from Cargo.toml"
    tag="v$cur"

    # Never re-tag an existing version.
    if git rev-parse "$tag" >/dev/null 2>&1; then
        die "$tag already exists locally. Never re-tag a released version — bump the patch and re-run."
    fi
    if git ls-remote --exit-code --tags origin "$tag" >/dev/null 2>&1; then
        die "$tag already exists on origin. Never re-tag a released version — bump the patch and re-run."
    fi

    # Best-effort CI-green gate. Set DEPLOY_SKIP_CI_CHECK=1 to override (e.g.
    # gh unavailable, or CI status is being checked another way).
    if [ "${DEPLOY_SKIP_CI_CHECK:-0}" != "1" ] && command -v gh >/dev/null 2>&1; then
        info "checking latest CI run on main"
        local concl
        concl="$(gh run list --branch main --workflow CI --limit 1 --json conclusion --jq '.[0].conclusion' 2>/dev/null || echo "")"
        case "$concl" in
            success) ok "CI is green on main" ;;
            "")      warn "could not determine CI status (continuing — verify manually)" ;;
            *)       die "latest CI run on main is '$concl', not 'success'. Fix CI before tagging (or set DEPLOY_SKIP_CI_CHECK=1 to override)." ;;
        esac
    else
        warn "skipping CI-green check (gh unavailable or DEPLOY_SKIP_CI_CHECK=1)"
    fi

    bold "Tagging $tag and pushing to fire release.yml"
    git tag -a "$tag" -m "wb300 $tag"
    git push origin "$tag"

    ok "$tag pushed."
    cat <<EOF

CI now runs:
  release.yml            builds the 6-target artifacts + GitHub Release (incl. Global MSI)
  windows-installers.yml attaches the 4 Windows installers + .sha256 sidecars

Watch (use Monitor / a separate terminal — don't foreground in an agent loop):
  gh run watch
Verify when done:
  git ls-remote --tags origin | grep $tag
  gh release view $tag
  cargo install wb300 --force && wb300 --version
EOF
}

# ── dispatch ────────────────────────────────────────────────────────

case "${1:-}" in
    bump) shift; cmd_bump "$@" ;;
    tag)  shift; cmd_tag "$@" ;;
    *)    usage ;;
esac
