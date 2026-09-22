#!/usr/bin/env bash
# Deploy the primary Autheo Dev Hub only.  It never selects hive-node or
# hive-ui, which are separate services (including hive-ui's loopback :3002).
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ANSIBLE_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_DIR="$(cd -- "$ANSIBLE_DIR/.." && pwd)"
PLAYBOOK="$ANSIBLE_DIR/playbooks/parallel-deploy.yml"
ROLE_TASKS="$ANSIBLE_DIR/roles/autheo_devhub/tasks/main.yml"
VAULT_FILE="$ANSIBLE_DIR/inventory/group_vars/all/vault.yml"
DEFAULT_VAULT_PASSWORD_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/autheo/ansible/vault_pass"

inventory="${AUTHEO_DEVHUB_INVENTORY:-$ANSIBLE_DIR/inventory/hosts.ini}"
repo="${AUTHEO_DEVHUB_REPO:-https://github.com/ThothDivision/L0_devhub_deploy.git}"
version="${AUTHEO_DEVHUB_VERSION:-main}"
source_dir="${AUTHEO_DEVHUB_SOURCE_DIR:-$REPO_DIR}"
repo_explicit=0
source_dir_explicit=0
[[ -n "${AUTHEO_DEVHUB_REPO:-}" ]] && repo_explicit=1
[[ -n "${AUTHEO_DEVHUB_SOURCE_DIR:-}" ]] && source_dir_explicit=1
limit="${AUTHEO_DEVHUB_LIMIT:-}"
vault_password_file="${AUTHEO_DEVHUB_VAULT_PASSWORD_FILE:-$DEFAULT_VAULT_PASSWORD_FILE}"
vault_password_file_explicit=0
assume_yes="${AUTHEO_DEVHUB_ASSUME_YES:-0}"
update="${AUTHEO_DEVHUB_UPDATE:-0}"

usage() {
  cat <<'EOF'
Usage: ansible/scripts/deploy-autheo-devhub.sh [options]

Deploy only the primary Autheo Dev Hub (autheo-devhub.service, :3001).
This does not deploy hive-node or hive-ui (:3002).

Options:
  --inventory PATH             Inventory file (default: ansible/inventory/hosts.ini)
  --repo URL                   Dev Hub source repository (or pair with --source-dir)
  --version REF                Dev Hub source revision (default: main)
  --source-dir PATH            Local Dev Hub checkout to package (default: this checkout)
  --limit PATTERN              Limit Ansible to the elected Dev Hub host
  --vault-password-file PATH   Use an existing vault password file
  --update                     Clean-fast-forward this checkout before deployment
  --yes                        Skip the interactive deployment confirmation
  -h, --help                   Show this help

Environment equivalents:
  AUTHEO_DEVHUB_INVENTORY, AUTHEO_DEVHUB_REPO, AUTHEO_DEVHUB_VERSION,
  AUTHEO_DEVHUB_SOURCE_DIR,
  AUTHEO_DEVHUB_LIMIT, AUTHEO_DEVHUB_VAULT_PASSWORD_FILE,
  AUTHEO_DEVHUB_UPDATE=1, AUTHEO_DEVHUB_ASSUME_YES=1.

Before changing a host, this command decrypts the existing local Ansible
vault. It refuses to deploy when the vault or its password source is missing
or cannot decrypt. No vault values are printed.

The default password source is
$XDG_CONFIG_HOME/autheo/ansible/vault_pass, or
$HOME/.config/autheo/ansible/vault_pass when XDG_CONFIG_HOME is unset.
EOF
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

while (($#)); do
  case "$1" in
    --inventory)
      (($# >= 2)) || die "--inventory requires a path"
      inventory="$2"
      shift 2
      ;;
    --repo)
      (($# >= 2)) || die "--repo requires a URL"
      repo="$2"
      repo_explicit=1
      shift 2
      ;;
    --version)
      (($# >= 2)) || die "--version requires a ref"
      version="$2"
      shift 2
      ;;
    --source-dir)
      (($# >= 2)) || die "--source-dir requires a path"
      source_dir="$2"
      source_dir_explicit=1
      shift 2
      ;;
    --limit)
      (($# >= 2)) || die "--limit requires an Ansible pattern"
      limit="$2"
      shift 2
      ;;
    --vault-password-file)
      (($# >= 2)) || die "--vault-password-file requires a path"
      vault_password_file="$2"
      vault_password_file_explicit=1
      shift 2
      ;;
    --update)
      update=1
      shift
      ;;
    --yes)
      assume_yes=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1 (use --help)"
      ;;
  esac
done

if [[ "$repo_explicit" == "1" && "$source_dir_explicit" != "1" ]]; then
  # An explicitly selected repository must retain the direct-role behavior
  # unless its corresponding local checkout was explicitly selected too.
  source_dir=""
fi

[[ -f "$ROLE_TASKS" ]] || die "required role tasks are missing: $ROLE_TASKS"
[[ -f "$PLAYBOOK" ]] || die "required playbook is missing: $PLAYBOOK"
[[ -f "$inventory" ]] || die "inventory does not exist: $inventory"
[[ -f "$VAULT_FILE" ]] || die \
  "vault file is missing: $VAULT_FILE. Recover the pre-existing encrypted vault; do not create a replacement."

[[ -f "$vault_password_file" ]] ||
  die "vault password source is missing: $vault_password_file. Restore the exact pre-existing password from approved secret storage; do not create a replacement."
[[ -r "$vault_password_file" ]] ||
  die "vault password source is not readable: $vault_password_file. Correct access without changing its contents."
if [[ -n "$source_dir" ]]; then
  [[ -d "$source_dir" ]] ||
    die "Dev Hub source directory does not exist: $source_dir"
  git -C "$source_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
    die "Dev Hub source directory is not a Git worktree: $source_dir"
fi

vault_error="$(mktemp)"
source_archive=""
trap 'rm -f "$vault_error" "${deploy_log:-}" "${source_archive:-}"' EXIT

vault_cmd=(ansible-vault view)
if [[ "$vault_password_file_explicit" == "1" || -n "${AUTHEO_DEVHUB_VAULT_PASSWORD_FILE:-}" ]]; then
  vault_cmd+=(--vault-password-file "$vault_password_file")
fi
vault_cmd+=("$VAULT_FILE")

if ! (cd "$ANSIBLE_DIR" &&
  ANSIBLE_CONFIG="$ANSIBLE_DIR/ansible.cfg" "${vault_cmd[@]}" >/dev/null 2>"$vault_error"); then
  if grep -Eq 'password file.*(not found|does not exist)' "$vault_error"; then
    die "vault password source is missing. Restore the configured pre-existing password file or pass --vault-password-file PATH."
  fi
  if grep -Eq 'password file.*(not readable|Permission denied)' "$vault_error"; then
    die "vault password source is not readable. Correct its ownership or permissions without changing its contents."
  fi
  die "vault preflight could not decrypt $VAULT_FILE. Its 1.1 header has no vault ID, so this is not a vault-ID mismatch; use the correct pre-existing password source."
fi
rm -f "$vault_error"

if [[ "$update" == "1" ]]; then
  git -C "$REPO_DIR" diff --quiet ||
    die "--update refuses a working tree with tracked changes; commit, stash, or inspect them first"
  [[ -z "$(git -C "$REPO_DIR" status --porcelain)" ]] ||
    die "--update refuses an unclean working tree; it will not merge, rebase, or reset local work"
  branch="$(git -C "$REPO_DIR" branch --show-current)"
  [[ -n "$branch" ]] || die "--update requires a checked-out branch"
  git -C "$REPO_DIR" fetch origin "$branch"
  if git -C "$REPO_DIR" merge-base --is-ancestor "origin/$branch" HEAD; then
    :
  elif git -C "$REPO_DIR" merge-base --is-ancestor HEAD "origin/$branch"; then
    # --ff-only can only advance the current branch pointer and update a clean
    # worktree; it never creates a merge commit or resolves divergent history.
    git -C "$REPO_DIR" merge --ff-only "origin/$branch"
  else
    die "--update refuses a diverged worktree; it will not merge, rebase, or reset history"
  fi
fi

if [[ -n "$source_dir" ]]; then
  source_revision="$(git -C "$source_dir" rev-parse "${version}^{commit}" 2>/dev/null)" ||
    die "Dev Hub source directory does not contain the requested revision: $version"
  source_archive="$(mktemp --suffix=.tar.gz)"
  git -C "$source_dir" archive --format=tar.gz --output="$source_archive" "$source_revision"
else
  source_revision=""
fi

if [[ "$assume_yes" != "1" ]]; then
  printf 'Deploy only autheo-devhub (:3001) from %s at %s? [y/N] ' "$repo" "$version"
  read -r reply
  [[ "$reply" == "y" || "$reply" == "Y" ]] || die "deployment cancelled"
fi

playbook_cmd=(
  ansible-playbook
  -i "$inventory"
  "$PLAYBOOK"
  --tags autheo_devhub
  -e "autheo_devhub_enabled=true"
  -e "autheo_devhub_repo=$repo"
  -e "autheo_devhub_version=$version"
  -e "autheo_devhub_source_archive=$source_archive"
  -e "autheo_devhub_source_revision=$source_revision"
)
if [[ -n "$limit" ]]; then
  playbook_cmd+=(--limit "$limit")
fi
if [[ -n "$vault_password_file" ]]; then
  playbook_cmd+=(--vault-password-file "$vault_password_file")
fi

deploy_log="$(mktemp)"
if ! (cd "$ANSIBLE_DIR" &&
  ANSIBLE_CONFIG="$ANSIBLE_DIR/ansible.cfg" "${playbook_cmd[@]}") 2>&1 | tee "$deploy_log"; then
  die "Autheo Dev Hub deployment failed; no success verification was accepted"
fi
grep -Fq 'AUTHEO DEV HUB VERIFIED (:3001)' "$deploy_log" ||
  die "deployment ended without the required AUTHEO DEV HUB VERIFIED (:3001) result"

cat <<'EOF'
Autheo Dev Hub deployment verified. On the elected Dev Hub host, follow up with:
  systemctl status autheo-devhub
  cat /opt/autheo-devhub/.autheo-devhub-release
  curl http://127.0.0.1:3001/
EOF
