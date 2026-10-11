# Windows Android Emulator Host

The initial surface under `hosts/windows-android-emulator/` provides a
prerequisite diagnostic only. Real-host feasibility is **unverified**. A
successful inventory does not establish accelerated boot, network isolation,
or readiness to serve clients.

This procedure is for a Windows operator familiar with PowerShell, Android
Software Development Kit (SDK) tools, Android Virtual Device (AVD) management,
and Docker Desktop who needs to assess the proposed native emulator host without
disturbing existing resources. You need a full checkout, Windows PowerShell 5.1
for host inventory, and separately scoped permission for each real-host
operation. The connection Action at `actions/android-emulator-connect`,
supervisor, lease authority, relay, and common client are planned, not
implemented or runnable. Consumer application compatibility, workflow adoption,
and the other execution-pool plans remain separate work.

## Distinguish evidence and permissions

Use the following evidence levels; success at one does not establish the next:

| Level        | Check                                                                               | Permission boundary                                                                  | What the evidence establishes                                            |
| ------------ | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| Offline      | Configuration validation and synthetic prerequisite CLI fixtures                    | Local source checks; no personal-host access                                         | Configuration rejection/acceptance and fixture behavior only             |
| Inventory    | Bounded read-only Windows prerequisite diagnostic                                   | Permission to read the intended host's prerequisites                                 | Observed metadata and availability, not acceleration or isolation        |
| Feasibility  | Dedicated SDK/AVD setup, accelerated boot, restricted transport, and network matrix | Explicit host setup, execution, credentials/settings, and controlled endpoint grants | Actual platform behavior for the recorded versions and routes            |
| Full runtime | Lease fencing, reset, recovery, and tool integration                                | Later implementation and separately authorized runtime checks                        | Required evidence beyond this initial surface; tools are not implemented |

The standing repository delivery grant does not authorize Windows setup or
execution. This document is not such a grant. Before a real-host check, record
the allowed resources, effects, time window, and cleanup scope privately. A
permission to inventory the host MUST NOT be treated as permission to install
an SDK, execute its binaries, launch a device, or change networking.

## Validate the prerequisite configuration offline

The initial configuration contains only `sdkRoot` and `avdRoot`. Start from
`hosts/windows-android-emulator/emulator-host.example.json`. Store the working
configuration outside the checkout with two distinct, dedicated absolute local
Windows paths. These paths are generic examples, not selected host locations:

```json
{
  "sdkRoot": "C:\\example-emulator-sdk",
  "avdRoot": "C:\\example-emulator-avds"
}
```

Do not reuse personal Android Studio storage, the checkout, or existing runner
storage. The example does not create directories or install anything.

From the full checkout's root in PowerShell, validate the file:

```powershell
.\hosts\windows-android-emulator\inspect-prerequisites.ps1 -ConfigPath C:\path\to\emulator-host.json -ValidateOnly
```

`-ValidateOnly` reads the configuration only. It rejects unsafe path syntax,
lexically overlapping roots, and unknown fields, and launches no SDK programs.
It does not resolve physical aliases such as junctions, SUBST drives, or alternate
names, or establish ownership. Passing validation does not establish that
directories exist, have safe effective access controls, or contain a usable
toolchain. Do not add future profiles, ports, credentials, or lease settings to
this initial configuration; their runtime schema does not exist yet.

The existing `mise run test:host` task covers synthetic prerequisite CLI fixtures
alongside the Docker runner host checks. Follow the root README's command and
quality-gate procedure. Synthetic checks and GitHub-hosted Windows checks do
not exercise the personal host's SDK, acceleration, Docker backend, or network
boundary.

## Collect read-only host inventory

Obtain permission for read-only inventory on the intended Windows host before
running the default mode. Retain the output privately, together with the source
revision, exit outcome, and measured elapsed time. Run from the full checkout's
root under the intended existing account:

```powershell
.\hosts\windows-android-emulator\inspect-prerequisites.ps1 -ConfigPath C:\path\to\emulator-host.json
```

The default mode is Windows-only. It collects non-identifying OS, architecture,
virtualization, and PowerShell facts. It checks configured-root existence,
fixed SDK version metadata and binary presence, and Node version metadata
without executing SDK or Node binaries. The diagnostic targets a 30-second
operation budget with a five-second cleanup reserve and a 64-KiB output cap.
Admitted worker observations are deadline-limited, but containment compilation
and Windows process creation are synchronous and cannot be interrupted by that
timer. This is not a hard whole-invocation wall-clock guarantee; retain actual
elapsed time and report overruns. It uses the existing Windows Job Object
lifetime helper from the Docker host surface; copying the entry script alone
is unsupported.

The fixed inventory worker permits no stderr payload. Any worker stderr makes
collection fail; it is not copied to the public report or drained without a
byte limit.

Read the reported gaps as unavailable observations, not as successful checks.
The diagnostic always leaves feasibility unverified. Binary presence and
version metadata do not prove binary integrity, a working hypervisor, compatible
system images, or enforceable networking. The initial inventory performs no
SDK installation, device boot, SSH setup, or allocation.

## Prepare a separately authorized feasibility window

The remaining sections describe required real-host checks, not a runnable host
setup. Automated setup, boot, transport, and network-probe commands are not
implemented here. Before execution, record the exact vendor-supported commands
and selected tool versions in an operator-private procedure under the approved
operation scope; do not infer flags or settings from this document.

Prepare the window in the following order:

1. Obtain scoped permission for dedicated SDK/AVD setup, license acceptance,
   accelerated boot, and the named network tests.
   Credential provisioning and any SSH/private-network/filter settings require
   their own covered effects. Use only synthetic test data and controlled
   services; do not run a consumer workflow or contact a production backend.
2. Record the existing personal devices, SDK/AVD resources, runner registrations,
   containers, caches, labels, scheduled tasks, and Docker/WSL configuration
   privately. Establish a quiet test window without reallocating runner slots.
   [Windows Runner Host](./windows-runner-host.md) owns those existing resources,
   not this diagnostic.
3. Select compatible native Windows SDK/emulator/system-image versions from
   current vendor documentation. Record actual downloads and checksums or
   signatures where available, and explicit license acceptance. No SDK or
   system-image pins are supplied by this initial surface. Node 24 is the
   planned runtime, not proof of compatibility from inventory metadata.
4. Prepare dedicated sacrificial storage and an AVD under the existing account.
   Record effective access controls and every owned process, listener, and
   writable image needed for scoped cleanup. Keep personal SDK/environment
   settings and unrelated device sessions unchanged.
5. Define private canary endpoints, expected synthetic responses, protocol-level
   deadlines, and positive controls before probing. Include each available
   address family and network route. An absent route or unsupported probe is a
   coverage gap, not a denial result.

For this window, you MUST NOT run a global `adb kill-server`, disconnect
unrelated devices, restart or prune Docker/WSL, or change machine-wide settings.
If a required prerequisite needs an unapproved driver or security product, stop
and return the proposal for authorization and plan revision rather than
installing it as a workaround.

## Verify native accelerated boot and coexistence

Use only the dedicated sacrificial AVD and approved toolchain. The diagnostic
does not perform these checks. Execute the following checks under the separate
host-operation grant:

1. Record the effective native acceleration backend using the selected vendor's
   supported diagnostic, then boot the AVD with the approved acceleration
   configuration. Record the exact invocation and acceleration readback. A
   virtualization flag, tool-presence check, or software-only boot does not
   satisfy this check.
2. Confirm Android reaches the intended booted state through the dedicated
   test connection. Record boot duration and observed host CPU/memory pressure,
   with unavailable counters named explicitly. Do not infer capacity or
   reservation from one successful boot.
3. Compare the private before/after resource inventory. Confirm personal device
   sessions and existing runner resources remain unchanged. Record unexpected
   effects and scoped cleanup outcomes before attempting another run.

A failed boot or unconfirmed coexistence keeps feasibility unverified. Do not
change Docker/WSL resource allocation, personal Android settings, or existing
runner configuration to turn this check into a pass.

## Verify restricted transport and the network boundary

The restricted SSH service, lease-aware relay, and client are not implemented.
[Security](../conventions/security.md) owns the endpoint-access constraints for
this authorized probe of Windows Android Debug Bridge (ADB) transport. Do not
substitute a raw SSH forward for the planned lease-aware service.

First prove the permitted restricted path works using separate test identities
and verified SSH host identity. Record the synthetic device's identity privately
and transfer a fixed binary payload through ADB, comparing its byte count and
digest at both ends. The permitted path must work from both the real Orb and the
actual Linux runner image on Docker's default bridge. Do not register that image
as a runner or execute a consumer GitHub job for this probe.

Then execute the following matrix against controlled canaries. Record each
attempt, deadline, protocol outcome, server-side observation, and matching
positive control as separate evidence:

| Origin                                     | Required negative check                                                                                                                      | Positive control                                                                  |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Restricted SSH identity                    | Deny shell, arbitrary exec, file transfer, agent/remote forwarding, and arbitrary destination forwarding                                     | Permit only the approved bounded management and allocated-device data operations  |
| Android guest                              | Deny Windows loopback and forbidden private/LAN targets; allow only necessary host-internal transport and explicitly approved infrastructure | Retrieve the expected synthetic response from an approved public test origin      |
| Orb and default-bridge runner-image client | Deny direct Windows 5037, raw emulator ADB, console/control ports, and another device's channel                                              | Exchange ADB data with only the allocated test device through the restricted path |

Test IPv4 and IPv6 separately with actual TCP/UDP probes for the protocols in
scope, not ping alone. Include the emulator's host alias, Docker host aliases,
literal addresses, and DNS-resolved names. Exercise emulator helper processes
as well as the main emulator, and every available Ethernet/Wi-Fi or other
alternate route that can bypass the private-network transport. Inspect the
effective filtering coverage outside guest/job configuration. Loopback binding
and a private-network policy alone do not prove denial through other routes.

Include both fresh and already-open connections when checking restriction
changes. Observe failed payload delivery and connection closure as appropriate;
a connection failure with a dead canary is not evidence of filtering. Establish
each canary's expected response from an authorized control origin before and
after the negative attempt. Keep canary addresses and raw network records
private.

Android-to-host/LAN denial and client raw/control-port bypass denial are
different results. The existing accepted general Docker runner-container
LAN-egress risk remains unchanged. Do not claim total client-container egress
isolation from this matrix.

## Verify ADB reverse to a synthetic client service

Use the same restricted device transport and a dedicated client-local ADB
environment, never the personal Windows ADB server. Client isolation/setup
commands are not implemented yet; stop if an existing ADB
environment conflicts rather than altering unrelated sessions.

Check reverse traffic in the following order:

1. Start a controlled synthetic service in the client that returns a unique test
   marker. Verify its response locally and retain the expected bytes privately.
   In an Orb, use the supported supervised-service mechanism for a long-lived
   test service; do not change project secrets or settings for this check.
2. Configure one owned ADB reverse mapping to that client service through the
   allocated device path. Request the marker from Android and compare the
   response with the independent expectation. Retain matching client service
   records to prove the request reached the client, not a Windows service.
3. Remove only the owned mapping and repeat the request as a negative control.
   Verify the service still responds locally, then stop only the owned test
   service and transport. Record that unrelated mappings/devices are unchanged.

This check establishes reverse traffic only. It does not prove a consumer APK,
Metro, Expo MCP, Maestro, or a GitHub Action works; those integration tools and
demonstrations belong to later implementation and verification.

## Stop, retain evidence, and report feasibility

Stop before full host/client/Action implementation if any required boundary is
unenforceable or has incomplete decisive evidence. Missing coverage keeps the
gate pending. An unenforceable boundary requires a revised plan; you MUST NOT
relax the policy, install an unapproved driver/product, or add a broad firewall
exception. Keep failed owned resources unavailable until scoped cleanup is
confirmed; never delete by a broad name/PID match.

Retain raw configuration, commands, logs, network records, and measurements in
operator-private storage outside the checkout. Before readiness, publish a
human-sanitized evidence comment on
[issue #30](https://github.com/axross/runners/issues/30), then read back its
locator. Include the following evidence without machine or consumer values:

- Source identity, actual tool versions, and the scope of authorized checks.
- Decisive positive/negative results for every tested origin, protocol, and route.
- Boot and bounded-operation timings, observed resource pressure, and gaps.
- Unexpected effects, scoped cleanup results, and remaining unverified behavior.

Replace private addresses, paths, identities, and endpoints with generic roles.
Remove credentials, registrations, personal screenshots, and private app data;
an inventory designed to omit identifying fields is not automatic permission to
publish its raw output.

Passing this feasibility stage permits consideration of the next implementation
stage, not a ready claim. Lease expiry/revocation fencing, fresh state across all
writable AVD media, and the full client/tool/Action checks are still planned and
required before ready under issue #30. Consumer workflow adoption remains a
subsequent separately approved change, not deferred evidence for this plan.
