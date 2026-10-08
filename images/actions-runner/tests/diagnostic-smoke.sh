#!/usr/bin/env bash
set -euo pipefail
image="${1:?runner image required}"
work="$(mktemp -d)"
suffix="$(basename "$work" | tr '[:upper:]' '[:lower:]')"
candidate="actions-runner:diagnostic-$suffix"
containers=()
cleanup() {
  for container in "${containers[@]}"; do docker rm -f "$container" > /dev/null 2>&1 || true; done
  docker image rm "$candidate" > /dev/null 2>&1 || true
  rm -rf -- "$work"
}
trap cleanup EXIT

tar -cf - images/actions-runner/runner-diagnostics.cpp images/actions-runner/tests/diagnostics-test.cpp \
  images/actions-runner/tests/diagnostics-test.sh |
  docker run --rm --network none -i "$image" bash -c \
    'set -euo pipefail; work=$(mktemp -d); trap '\''rm -rf -- "$work"'\'' EXIT
     tar -xf - -C "$work"; cd "$work"; bash images/actions-runner/tests/diagnostics-test.sh'

cat > "$work/run.sh" <<'RUNNER'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /home/runner/.gradle/daemon/fixture
printf '%s\n' "$DIAGNOSTIC_TEST_MARKER" > /home/runner/.gradle/daemon/fixture/daemon-99.out.log
printf '%s\n' "$DIAGNOSTIC_TEST_MARKER" > /tmp/hs_err_pid99.log
ln -s /tmp/hs_err_pid99.log /tmp/hs_err_pid100.log
if [ "$DIAGNOSTIC_TEST_MODE" = wait ]; then
  trap 'exit 23' TERM INT
  while true; do sleep 0.1; done
fi
exit "$DIAGNOSTIC_TEST_MODE"
RUNNER
docker build --network none --tag "$candidate" --file - "$work" <<DOCKERFILE
FROM $image
COPY --chown=runner:docker --chmod=0755 run.sh /home/runner/run.sh
DOCKERFILE

for mode in 0 7 wait abrupt observer-failure raw; do
  name="diagnostic-$suffix-$mode"
  containers+=("$name")
  runner_mode="$mode"
  args=()
  if [ "$mode" = abrupt ] || [ "$mode" = observer-failure ]; then runner_mode='wait'; fi
  if [ "$mode" = raw ]; then runner_mode=0; args=(raw-run); fi
  docker run --detach --network none --name "$name" \
    -e "DIAGNOSTIC_TEST_MARKER=$suffix" -e "DIAGNOSTIC_TEST_MODE=$runner_mode" \
    "$candidate" /usr/local/bin/runner-diagnostics "${args[@]}" > /dev/null
  expected="$runner_mode"
  if [ "$runner_mode" = wait ]; then
    for ((attempt = 0; attempt < 100; attempt++)); do
      if docker exec "$name" test -f /tmp/hs_err_pid99.log; then break; fi
      sleep 0.1
    done
    if [ "$mode" = observer-failure ]; then
      docker exec "$name" bash -c 'read -r observer runner < /proc/1/task/1/children; test -n "$runner"; kill -KILL "$observer"'
    fi
    if [ "$mode" = abrupt ]; then docker kill "$name" > /dev/null; expected=137
    else docker stop --time 10 "$name" > /dev/null; expected=23; fi
  fi
  test "$(docker wait "$name")" = "$expected"
  test "$(docker inspect --format '{{.State.Running}}' "$name")" = false
  test "$(docker inspect --format '{{.Image}}' "$name")" = "$(docker image inspect --format '{{.Id}}' "$candidate")"
  mkdir -m 700 "$work/$mode"
  export_args=()
  if [ "$mode" = raw ]; then export_args+=(-RawRecords); fi
  if [ "$mode" = abrupt ] || [ "$mode" = observer-failure ]; then export_args+=(-FinalUnavailable); fi
  pwsh -NoProfile -NonInteractive -File hosts/windows-docker-desktop/tests/export-smoke.ps1 \
    -Name "$name" -Sink "$work/$mode" -Marker "$suffix" "${export_args[@]}"
  docker rm "$name" > /dev/null
done
echo 'Actual image diagnostic lifecycle, private records and exit checks passed.'
