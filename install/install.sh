#!/usr/bin/env bash
set -euo pipefail

agent="auto"
scope="global"
skill_directory=""
bundle_root=""
binary_directory=""
force=0
no_path=0

usage() {
    cat <<'EOF'
Trace installer

Usage: ./install.sh [options]

  --agent auto|codex|claude|cursor|custom
  --scope global|project
  --skill-directory PATH     Required with --agent custom
  --bundle-root PATH         Override the release bundle directory
  --binary-directory PATH    Override the binary installation directory
  --force                    Replace an existing Trace skill after backing it up
  --no-path                  Do not add the Trace bin directory to shell startup
  --list-agents              Show detected supported agents and exit
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --agent) agent="$2"; shift 2 ;;
        --scope) scope="$2"; shift 2 ;;
        --skill-directory) skill_directory="$2"; shift 2 ;;
        --bundle-root) bundle_root="$2"; shift 2 ;;
        --binary-directory) binary_directory="$2"; shift 2 ;;
        --force) force=1; shift ;;
        --no-path) no_path=1; shift ;;
        --list-agents)
            list_agents=1
            shift
            ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

agent_root() {
    case "$1" in
        codex) echo "$HOME/.codex/skills" ;;
        claude) echo "$HOME/.claude/skills" ;;
        cursor) echo "$HOME/.cursor/skills" ;;
        *) return 1 ;;
    esac
}

agent_name() {
    case "$1" in
        codex) echo "Codex" ;;
        claude) echo "Claude Code" ;;
        cursor) echo "Cursor" ;;
        *) echo "$1" ;;
    esac
}

detected_agents=()
for candidate in codex claude cursor; do
    root="$(agent_root "$candidate")"
    if command_exists "$candidate" || [[ -d "$root" ]]; then
        detected_agents+=("$candidate")
    fi
done

if [[ "${list_agents:-0}" == "1" ]]; then
    for candidate in codex claude cursor; do
        status="not detected"
        for found in "${detected_agents[@]:-}"; do
            [[ "$found" == "$candidate" ]] && status="detected"
        done
        echo "$(agent_name "$candidate"): $status"
    done
    exit 0
fi

if [[ -z "$bundle_root" ]]; then
    bundle_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
else
    bundle_root="$(cd "$bundle_root" && pwd)"
fi

binary_source="$bundle_root/trace"
skill_source="$bundle_root/skill/trace-hardware"
[[ -f "$binary_source" ]] || { echo "trace was not found in $bundle_root. Run this installer from a Trace release bundle." >&2; exit 1; }
[[ -f "$skill_source/SKILL.md" ]] || { echo "skill/trace-hardware/SKILL.md was not found in $bundle_root." >&2; exit 1; }

if [[ "$agent" == "auto" ]]; then
    if [[ "${#detected_agents[@]}" -eq 0 ]]; then
        echo "No supported agent detected. Use --agent codex, --agent claude, --agent cursor, or --agent custom --skill-directory PATH." >&2
        exit 1
    elif [[ "${#detected_agents[@]}" -eq 1 ]]; then
        agent="${detected_agents[0]}"
    else
        echo "Detected agent tools:"
        for index in "${!detected_agents[@]}"; do
            candidate="${detected_agents[$index]}"
            echo "  $((index + 1)). $(agent_name "$candidate")"
        done
        read -r -p "Select the agent to install Trace into: " selection
        [[ "$selection" =~ ^[0-9]+$ ]] || { echo "Invalid agent selection." >&2; exit 1; }
        index=$((selection - 1))
        [[ "$index" -ge 0 && "$index" -lt "${#detected_agents[@]}" ]] || { echo "Invalid agent selection." >&2; exit 1; }
        agent="${detected_agents[$index]}"
    fi
fi

if [[ "$agent" == "custom" ]]; then
    [[ -n "$skill_directory" ]] || { echo "--skill-directory is required with --agent custom." >&2; exit 1; }
    skill_target="$skill_directory/trace-hardware"
elif [[ "$scope" == "project" ]]; then
    case "$agent" in
        codex|claude|cursor) skill_target="$PWD/.$agent/skills/trace-hardware" ;;
        *) echo "Unsupported agent: $agent" >&2; exit 1 ;;
    esac
else
    skill_target="$(agent_root "$agent")/trace-hardware"
fi

if [[ -e "$skill_target" ]]; then
    [[ "$force" == "1" ]] || { echo "$skill_target already exists. Re-run with --force to replace it." >&2; exit 1; }
    backup="$skill_target.backup-$(date +%Y%m%d%H%M%S)"
    mv "$skill_target" "$backup"
    echo "Existing skill backed up to $backup"
fi

if [[ -z "$binary_directory" ]]; then
    binary_target_root="$HOME/.local/bin"
else
    binary_target_root="$binary_directory"
fi
binary_target="$binary_target_root/trace"
mkdir -p "$binary_target_root" "$(dirname "$skill_target")"
cp "$binary_source" "$binary_target"
chmod 755 "$binary_target"
cp -R "$skill_source" "$skill_target"

if [[ "$no_path" == "0" ]]; then
    profile="$HOME/.profile"
    [[ "${SHELL:-}" == */zsh ]] && profile="$HOME/.zprofile"
    path_line='export PATH="$HOME/.local/bin:$PATH"'
    touch "$profile"
    if ! grep -Fqx "$path_line" "$profile"; then
        printf '\n# Trace CLI\n%s\n' "$path_line" >> "$profile"
    fi
fi

echo "Installed Trace CLI to $binary_target"
echo "Installed trace-hardware skill for $(agent_name "$agent") to $skill_target"
if command_exists kicad-cli; then
    echo "KiCad CLI detected."
else
    echo "KiCad CLI not detected; install KiCad or set TRACE_KICAD_CLI before using schematic commands."
fi
echo "Open a new terminal, then run: trace --version"
