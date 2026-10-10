#!/usr/bin/env bash
set -euo pipefail
printf 'process=%s\n' "$(awk '/^Cpus_allowed_list:/ {print $2}' "/proc/$$/status")"
printf 'child=%s\n' "$(bash -c 'awk '\''/^Cpus_allowed_list:/ {print $2}'\'' /proc/self/status')"
printf 'nproc=%s\n' "$(nproc)"
printf 'getconf=%s\n' "$(getconf _NPROCESSORS_ONLN)"

# membership is relative to the cgroup namespace; mount roots can be host paths.
cgroup_directory() {
  local controller="$1" membership root mount suffix
  membership="$(awk -F : -v controller="$controller" \
    'controller == "unified" && $1 == "0" {print $3; exit}
     index("," $2 ",", "," controller ",") {print $3; exit}' /proc/self/cgroup)"
  read -r root mount < <(awk -v controller="$controller" \
    '{for (i=7; i<=NF; i++) if ($i == "-") {
      if ((controller == "unified" && $(i+1) == "cgroup2") ||
          ($(i+1) == "cgroup" && index("," $(i+3) ",", "," controller ","))) {
        print $4, $5; exit
      }
    }}' /proc/self/mountinfo)
  test -n "$membership" && test -n "$root" && test -n "$mount"
  if [[ "$root$mount$membership" == *\\* ]]; then
    echo 'Unsupported escaped cgroup path; CPU readback incomplete.' >&2
    return 1
  fi
  suffix="$membership"
  if [ "$membership" = "$root" ]; then suffix=''
  elif [[ "$membership" == "$root/"* ]]; then suffix="${membership#"$root"}"; fi
  printf '%s\n' "${mount}${suffix}"
}

if awk -F : '$1 == "0" {found=1} END {exit !found}' /proc/self/cgroup; then
  directory="$(cgroup_directory unified)"
  printf 'layout=v2\ncpuset=%s\n' "$(cat "$directory/cpuset.cpus.effective")"
  read -r quota period < "$directory/cpu.max"
else
  directory="$(cgroup_directory cpuset)"
  effective="$(cat "$directory/cpuset.effective_cpus")"
  cpu="$(cgroup_directory cpu)"
  quota="$(cat "$cpu/cpu.cfs_quota_us")"
  period="$(cat "$cpu/cpu.cfs_period_us")"
  printf 'layout=v1\ncpuset=%s\n' "$effective"
fi
printf 'quota=%s\nperiod=%s\n' "$quota" "$period"
