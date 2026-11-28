---
type: Procedure
title: "Observe a Linux job VM whose network overlaps Docker's bridge"
description: "Collect the approved read-only evidence from a stuck Linux job VM on axmc and its runner container before anything restarts or deletes the VM."
---

# Observe a Linux job VM whose network overlaps Docker's bridge

[#124](https://github.com/axross/runners/issues/124) tracks a Linux job VM on axmc
whose Softnet address and gateway, which is also its DNS server, fell inside Docker's
default bridge range, so the runner container in it could not reach DNS. The cause is a
hypothesis: the container holds only the bridge's route for that range, and so tries to
reach the gateway directly on the bridge. Softnet picks each VM's subnet at random from
private ranges and avoids only the Mac's own interfaces, which
[#124's handoff comment](https://github.com/axross/runners/issues/124#issuecomment-7019718140)
records and no other page states.

This procedure makes the limited, read-only observation the maintainer approved there
(option B) at the next recurrence. Every command reads or looks something up, and none
changes a setting, a rule or a counter. The only file the procedure creates is your
private notes file on the Mac. A matching result confirms the
mechanism for that case only, and it does not prove the cause of the first incident.
It is not continuous monitoring and it grants no access to axmc.

**This procedure has not been run on a host.** [Verify a runner on Mac
hardware](./mac-verification.md) lists the claims it relies on that no run has
confirmed. A trial of its commands against a Linux job VM that works normally is
planned on #124. That trial cannot exercise the failure, so this page stays unrun on a
recurrence after it.

## Before You Start

All of these must hold. If one does not, do not start.

- **A recurrence report.** A Linux pool's runner on axmc keeps retrying its connection
  to GitHub, or jobs on it fail to resolve names.
- **The affected job VM still exists.** `tart list --source local` shows it, as
  [Health checks](./mac-pools.md#health-checks) describes.
- **A usable access route to axmc.** You have a terminal as the macOS user that runs
  Marshal, because Tart lists and runs only that user's VMs. How to reach axmc from
  elsewhere is open on #124, and this page does not choose a route.
- **The maintainer's approval at the time.** #124's decisions require it for this
  observation and for each operation on axmc.

## What Is Forbidden

Option B forbids these. Nothing on this page does any of them.

- Restarting anything: the instance (`restart`, `stop` or `start`), the VM, the
  container or Docker.
- Changing a route, a network setting or any host configuration, in the VM, in the
  container or on the Mac. That includes installing a tool, starting a helper
  container, changing a firewall rule, resetting a counter and editing a file.
- Causing the fault on purpose, or reproducing the overlap.
- Dumping environment variables or the JIT runner configuration.
- A full `docker inspect`.
- Capturing credentials or authenticated payloads.

The JIT configuration reaches the runner through an environment variable of the
`docker run` client and of the container, and the runner-specific GitHub host appears
only inside it. So these commands are never run, even though each is read-only:

- `env`, `printenv`, `set`, `ps e`, and any read of a process's `environ` file;
- `docker top`, which prints process command lines that option B does not cover;
- `docker inspect` without a format that names only address fields, `docker logs`
  and a `docker ps` listing that prints more than names;
- `tcpdump` with `-w`, `-X` or `-A`.

The only privileged commands are the fixed `sudo -n` commands in steps 3, 8 and 9:
the check `sudo -n true`, the listing of firewall and NAT rules with their counters,
and a header capture of ARP and port 53 limited in time and packet count. Each step
skips them when `sudo -n true` fails.

## What Deletes the VM

A job VM does not wait for you. Its slot deletes it on every path out of the cycle,
so while you observe, nobody may do any of the following, and you stop at once if one
happens:

- Close the lid, sleep the Mac or change its primary network. Either stops every
  runner and deletes every VM; see
  [Sleep, Wake, and Network Changes](./mac-resume.md#sleep-wake-and-network-changes).
- Stop or restart the instance, update or quit the Mac app, or kill or start the
  command line. A stop ends every VM within 30 s, and a `restart` waits up to 35 s
  for the instance to run its container steps, delete the VMs and read the
  configuration again. A dying CLI has each VM's shim delete its VM within 15 s, and
  the next `start` deletes leftovers. See
  [Starting, watching and stopping](./mac-pools.md#starting-watching-and-stopping) and
  [The Clients](../marshal-cli/foreground-commands.md#the-clients).
- Let the runner container exit. The slot then deletes the VM when the job ends; see
  the slot's cycle in [The Golden VM Contract](./linux-vm.md#the-golden-vm-contract).

An app update's drain does not delete the VM at once. On axmc's Tart backend it
waits for every runner as if it were busy, because that backend reads no runner output,
so a stuck VM survives until the drain's 6-hour bound; see
[The Update Drain](../marshal-cli/agent-lifecycle.md#the-update-drain). In the first
incident the VM stayed until a `restart`, but nothing guarantees that. The steps
therefore collect the items most likely to vanish first.

## Collecting the Evidence

Run each command in the terminal of the macOS user that runs Marshal. Replace the
`<placeholders>` with your values, and keep them out of anything you post; see
[Post the Evidence](#post-the-evidence). Each command goes through `tart exec`, which
runs it in the VM as `admin` directly and without a shell. A command that waits on the
network carries its time limit inside, as `timeout <seconds>` in the executed command,
so nothing lingers if you stop waiting. macOS has no `timeout` to wrap `tart exec`
itself.

**Stop conditions.** They apply to every step.

- The job VM is gone before or during the observation: stop, and report on #124 what
  you collected and when the VM went.
- `tart exec` itself cannot reach the guest, or the guest agent does not answer: stop.
  End a hung `tart exec` client with Ctrl+C, and report what you collected. A command
  that runs in the guest and exits non-zero, such as a missing file or status 127 for
  a missing tool, is not a stop: its exit status is in the record, so continue.
- A step would need a forbidden action, a `sudo` that `sudo -n true` refused, or a tool
  that is missing: skip that step and record why.

### 1. Prepare the shell

Run the whole procedure in one `bash` session. macOS's default shell is zsh, so start
`bash` first, before you define anything below, because a new shell drops the helpers
and the variables. Then create a notes file that only you can read, which is the
evidence file that [Post the Evidence](#post-the-evidence) scans. Give it an absolute
path, because `~` does not expand inside the single quotes:

```bash
umask 077
EVIDENCE='<absolute-path-of-evidence-file>'
: >>"$EVIDENCE"
```

The helpers print a UTC time and the command, quoted so that you can paste it back,
before its output and the exit status after it, and append all of it to that file as
well as the terminal. Exit status 124 from `timeout` means the limit ended the command.

```bash
obs() {
  {
    printf '== %s UTC:' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf ' %q' "$@"
    printf '\n'
    "$@"
    printf 'exit status: %s\n' "$?"
  } 2>&1 | tee -a "$EVIDENCE"
}
vm() { obs tart exec "$VM" "$@"; }
ctr() { obs tart exec "$VM" docker exec "$CTR" "$@"; }
```

### 2. Record the state and identify the VM and the container

Record the status that
[Starting, watching and stopping](./mac-pools.md#starting-watching-and-stopping)
describes, the VM list and the log lines for that slot. The status names the
configuration file and every pool, and the VM list names the VMs of other pools, so the
scrub in [Post the Evidence](#post-the-evidence) covers both. The log's time stamps are
UTC, in the same form as the VM and container names, so convert only times that come
from elsewhere, such as GitHub's pages.

```bash
obs marshal status
obs tart list --source local
obs sh -c 'grep -hF -e "<pool> slot <slot>: booting job VM" -e "<pool> slot <slot>: starting container" ~/Library/Logs/marshal/*.log | tail -n 2'
```

The affected slot's status line, such as `running container <name>`, and the two log
lines name the VM the slot booted and the container it started. If the log lookup
prints no lines, identify the VM from that status line and the VM list. If the VM is
not in the list, it is gone: stop, and report on #124 what you collected and when the
VM went. Otherwise set `VM`, and list the container names in it. The filter prints
names only.

```bash
VM='<vm>'
vm docker ps --filter 'name=<pool>-' --format '{{.Names}}'
CTR='<container>'
```

### 3. Record the tools and the `sudo` right

Network tools in the VM and the container are optional. Nothing here installs one, so
a step whose tool is missing is skipped, and a missing `timeout` skips every step that
waits on the network in that VM or container. The guest agent's `PATH` is not
recorded, so a tool reported missing may only be off that path; skip it all the same.
`nft` and `iptables` sit in `/usr/sbin`, which that path may lack, and `sudo` uses its
own path. If both are reported missing, step 8 still tries the `nft` command and then the
`iptables` commands once, and their "command not found" answers are the record.

```bash
vm sh -c 'for t in ip tcpdump dig curl timeout getent grep bash nft iptables; do command -v "$t" >/dev/null || echo "missing: $t"; done'
ctr sh -c 'for t in ip dig curl timeout getent bash; do command -v "$t" >/dev/null || echo "missing: $t"; done'
vm sudo -n true
```

Exit status 0 from `sudo -n true` means steps 8 and 9 may run. Any other status skips
them, and you record that.

### 4. Collect the container's network

The container goes first, because its exit ends the VM's life. If the container is
gone, the slot is deleting the VM: record the time, and do step 5 only while the VM is
still listed. The first command prints only the container's address, prefix and
gateway, and no configuration or environment field.

```bash
vm docker inspect "$CTR" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}/{{.IPPrefixLen}} gateway {{.Gateway}}{{"\n"}}{{end}}'
ctr cat /proc/net/route
ctr cat /proc/net/arp
ctr cat /etc/resolv.conf
ctr sh -c 'for f in /sys/class/net/*/iflink; do printf "%s %s\n" "$f" "$(cat "$f")"; done'
```

Set `DNS` to the non-loopback `nameserver` address in the resolver file. The route
lookup for that address shows which route the container uses to reach it. With `ip` in
the container, ask for it, and for its policy rules. Otherwise work it out from the
route table with the helper below, which you paste into the same `bash` session. The
helper's result follows the `obs` record of the `ctr` call that feeds it, whose time
stamp covers it.

```bash
DNS='<dns-address>'
ctr ip route get "$DNS"
ctr ip -4 rule show
```

```bash
route_lookup() {
  local want=$1 a b c d line iface dest gw metric mask len m matches=''
  IFS=. read -r a b c d <<<"$want"
  want=$(( (d << 24) | (c << 16) | (b << 8) | a ))
  hex_ip() { printf '%d.%d.%d.%d' "0x${1:6:2}" "0x${1:4:2}" "0x${1:2:2}" "0x${1:0:2}"; }
  while read -r line; do
    read -r iface dest gw _ _ _ metric mask _ <<<"$line"
    [[ $dest =~ ^[0-9A-F]{8}$ && $mask =~ ^[0-9A-F]{8}$ ]] || continue
    (( (want & 16#$mask) == 16#$dest )) || continue
    len=0; m=$(( 16#$mask ))
    while (( m )); do len=$(( len + (m & 1) )); m=$(( m >> 1 )); done
    matches+="$len $metric $iface $(hex_ip "$dest")/$len via $(hex_ip "$gw")"$'\n'
  done
  echo '-- routes that match; the longest prefix wins, then the lowest metric --'
  printf '%s' "$matches" | sort -k1,1nr -k2,2n
}
ctr cat /proc/net/route | route_lookup "$DNS" | tee -a "$EVIDENCE"
```

The kernel prints each address in `/proc/net/route` as four bytes in hexadecimal,
lowest byte first, and the helper decodes them. A `via 0.0.0.0` route is
directly connected. If the best match is the bridge's connected route, the container
treats the DNS address as a neighbour on the bridge, which is the hypothesis.

### 5. Collect the VM's network and resolver files

Run the VM list again first. If the VM is gone, stop and report. The first `/proc` reads
need no tool beyond the base system.

```bash
obs tart list --source local
vm sh -c 'grep -B1 "host LOCAL" /proc/net/fib_trie'
vm cat /proc/net/route
vm cat /proc/net/arp
vm sh -c 'for f in /sys/class/net/*/ifindex; do printf "%s %s\n" "$f" "$(cat "$f")"; done'
vm sh -c 'for f in /proc/sys/net/ipv4/ip_forward /proc/sys/net/ipv4/conf/*/proxy_arp /proc/sys/net/ipv4/conf/*/forwarding; do printf "%s %s\n" "$f" "$(cat "$f")"; done'
vm cat /etc/resolv.conf
vm cat /run/systemd/resolve/resolv.conf
vm docker network inspect bridge --format '{{range .IPAM.Config}}{{.Subnet}} gateway {{.Gateway}}{{"\n"}}{{end}}'
```

The first command prints the VM's own addresses, and the route file shows their
prefixes. The resolver files are both read because the VM runs stock Ubuntu, so its
`/etc/resolv.conf` may name a local stub, and the upstream servers are in
systemd-resolved's own file, which may not exist; record that. The last command prints
only the default bridge's subnet and gateway. Where the VM has `ip`, add its policy
rules, routes and neighbour table:

```bash
vm ip -4 rule show
vm ip -4 route show table all
vm ip -4 neigh show
```

### 6. Test name lookups

Look up public GitHub hosts that GitHub documents for self-hosted runners, in the VM
and in the container. The runner-specific host cannot be read without reading the JIT
configuration, so it is not looked up, and the evidence says so. A `getent` lookup uses
UDP, and TCP only for a truncated answer, so a working lookup does not show that TCP
works; the `dig` steps below separate the two.

```bash
for h in github.com api.github.com results-receiver.actions.githubusercontent.com; do
  vm timeout 10 getent hosts "$h"
  ctr timeout 10 getent hosts "$h"
done
```

Where `dig` is present, query the DNS address with separate UDP and TCP queries. Where
it is not, test only a TCP connection to port 53 and record the UDP query as skipped.

```bash
vm timeout 10 dig +time=3 +tries=1 "@$DNS" github.com A
vm timeout 10 dig +tcp +time=3 +tries=1 "@$DNS" github.com A
ctr timeout 10 dig +time=3 +tries=1 "@$DNS" github.com A
ctr timeout 10 dig +tcp +time=3 +tries=1 "@$DNS" github.com A
```

```bash
vm timeout 5 bash -c "exec 3<>/dev/tcp/$DNS/53"
ctr timeout 5 bash -c "exec 3<>/dev/tcp/$DNS/53"
```

Compare the VM's answers with the container's. A lookup that works in the VM and times
out in the container fits the pattern that [Reading the Result](#reading-the-result)
describes.

### 7. Test an HTTPS request

Send one request without credentials and discard the body. The second form skips DNS
by pinning a GitHub address from step 6, with the right TLS host name. Run the
container's requests only when `curl` is present there.

```bash
vm timeout 20 curl -sS -m 15 -o /dev/null -w '%{http_code} %{remote_ip}\n' https://github.com/
vm timeout 20 curl -sS -m 15 -o /dev/null -w '%{http_code} %{remote_ip}\n' --resolve 'github.com:443:<github-address>' https://github.com/
ctr timeout 20 curl -sS -m 15 -o /dev/null -w '%{http_code} %{remote_ip}\n' https://github.com/
ctr timeout 20 curl -sS -m 15 -o /dev/null -w '%{http_code} %{remote_ip}\n' --resolve 'github.com:443:<github-address>' https://github.com/
```

If the lookups answer but the connection fails, DNS is not the fault. Take only the
counters of step 8, when `sudo -n true` succeeded, skip step 9, and go to
[Post the Evidence](#post-the-evidence).

### 8. Read the firewall and NAT counters

Only when `sudo -n true` succeeded. Use `nft` when step 3 found it, and otherwise
`iptables`. Read the rules twice, the second time after `sleep 10`, so the rules whose
counters move stand out. Never change a rule or reset a counter.

```bash
vm sudo -n timeout 10 nft list ruleset
sleep 10
vm sudo -n timeout 10 nft list ruleset
```

```bash
vm sudo -n timeout 10 iptables -w 5 -t nat -L -n -v -x
vm sudo -n timeout 10 iptables -w 5 -t filter -L -n -v -x
sleep 10
vm sudo -n timeout 10 iptables -w 5 -t nat -L -n -v -x
vm sudo -n timeout 10 iptables -w 5 -t filter -L -n -v -x
```

Softnet's flow table lives on the Mac, and no Mac-side way to read it is defined.
Record it as skipped. When DNS works but the connection fails, the counters above are
all you collect: record them and stop.

### 9. Capture ARP and port-53 headers

Only when `sudo -n true` succeeded and `tcpdump` is already in the VM. Capture three
interfaces, one capture each, and never with `-w`, `-X` or `-A`:

- the bridge, the interface of the connected route for the bridge's subnet in step 5's
  route table, which is `docker0` by default;
- the container's host-side interface, the one in step 5's index list whose index
  equals the container's `eth0` link index (`iflink`) from step 4;
- the VM's uplink, the interface of the default route, whose destination is
  `00000000` in the route table.

```bash
vm sudo -n timeout 15 tcpdump -p -nn -c 20 -i '<interface>' 'arp or port 53'
```

Each capture ends after 15 s or 20 packets. While it runs, repeat the container's
lookup from step 6 in a second `bash` session, so that packets appear. Define step 1's
`obs`, `vm` and `ctr` there, set the same values, and run the lookup through `ctr`, so
that it lands in the same notes file:

```bash
VM='<vm>'
CTR='<container>'
EVIDENCE='<absolute-path-of-evidence-file>'
ctr timeout 10 getent hosts github.com
```

That is a lookup, not a fault. The capture prints every name queried on that
interface, including the runner-specific GitHub host and any name a job looks up, such
as a private registry. All of it falls under the scrub in
[Post the Evidence](#post-the-evidence).

## Reading the Result

The hypothesis predicts this pattern. Each point is evidence for or against it in this
case, and none is proof.

- The VM's address, its gateway and the DNS address fall inside the bridge's subnet:
  the ranges overlap.
- The container's route lookup for the DNS address names the bridge's connected route,
  and its neighbour table holds no complete entry for that address: the container asks
  the bridge for the gateway.
- Lookups work in the VM and fail in the container. Softnet's DNS rule then is not the
  cause; see
  [Softnet Confines a Job VM's Network](../security/vm-isolation.md#softnet-confines-a-job-vms-network).

Report what you saw, and do not claim a cause for the first incident.

## Post the Evidence

Post one comment on #124 with these:

- the notes file: each command with its UTC time and output, as `obs` recorded it,
  which holds the status lines, the VM list, the VM and container names, the two log
  lines of step 2 and every later command;
- the commands you skipped and why, including Softnet's flow table, and that no
  command here reads the runner-specific GitHub host, which a packet capture may still
  show.

Post nothing else from the log or the machine. Before you post, read the whole text
and check that no token, environment variable or JIT configuration appears. A search over
the text you saved finds only the obvious cases:

```bash
grep -n -i -E 'token|secret|jit|ghp_|ghs_|github_pat_|authorization|bearer|password|environ|AWS_|ACTIONS_RUNNER_INPUT' "$EVIDENCE"
```

Because #124 is public, replace these with placeholders, as
[the disclosure rule](../security/disclosure.md#nothing-identifying-or-secret-is-committed)
does for committed files:

- the pool segment of the VM and container names, and the VMs of other pools that
  `tart list` printed;
- owner, repository, pool, image, job, workflow, volume and network names, and the
  configuration file's path in the status;
- resolver search domains, and the VM's host name;
- MAC addresses, home-directory paths and the Mac's account name;
- any host name or address other than the VM's, Docker's and public GitHub hosts.

The procedure is done when the evidence is posted, or when you record an abort on #124
with its reason.

## Recovery Is a Separate Operation

Do not restart to recover. A `restart` is forbidden here and stops every slot's jobs. It
needs the maintainer's go after the evidence or the abort is posted. Then follow
[Starting, watching and stopping](./mac-pools.md#starting-watching-and-stopping) for the
`restart`, and [Resuming](./mac-resume.md#resuming) if a stop came first.
