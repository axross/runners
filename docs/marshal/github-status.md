---
type: Specification
title: "GitHub status in the window"
description: "When does the window subscribe to GitHub and what can its indicators establish?"
---

# GitHub status in the window

While the main window is shown and not minimized, the session opens a second
connection to the agent and holds the `github` subscription on it, renewing it every
30 s and subscribing again when the agent answers `not_subscribed`. Hiding or
minimizing the window closes that connection, which ends the subscription, so the
agent asks GitHub only while someone can see the answer; [CLI](../marshal-cli/github-status.md#github-status)
owns the topic and what the agent does with it. The app tells the session when the
window is shown, hidden, minimized or restored, from the window's events and from
every place it shows or hides it.

Each runner with a registration gets a GitHub column beside its local state, in words
that begin "GitHub:": checking…, offline, idle, running a job, not listed, or not shown
when the agent's value left the runner out for size. A runner reads as checking until
its repository was checked after both the window opened and the runner entered its
current state, so a list read before then never makes it not listed. While checks are
paused, or GitHub cannot answer for the repository, the last values read since the
runner entered its state stay, marked with their time, even when they were read before
the window opened. A busy runner
whose job is known shows the workflow and job names, cleaned again and cut to 120 code
points, and an Open run link. Open run is an element with the link role that takes Tab
in list order; its accessible name names the job and the runner. It passes the pool
and the runner's name to `open_job`; the session builds the address from the pool's
configured repository and the job's two numbers, and when the runner or its job is
gone nothing opens and the pool says "That job has ended." for a minute. If a focused
Open run goes away with its runner, focus moves to the pool's heading. The agent's
value does not say which runners it left out for size, so while it left any out,
every registered runner it does not list reads "not shown" rather than "not listed".
A word of the topic's open sets this reader does not know reads as checking, or says
nothing on the card.

An organisation pool uses its organisation and runner group to select GitHub
status, then matches the host's runner by name. Its busy runner reads
"Job not shown for organisation runners" and has no Open run link. The session
also refuses a job-page request for an organisation pool, rather than using a
repository entry with the same runner name. Organisation status keeps the same
freshness and held-value rules as repository status.

A pool's card says when job names are hidden or could not be read, and when GitHub
cannot answer for its repository, with the last check's time and when Marshal tries
again; a token cause W1's card already gives is not said twice, and a repository with
no stored token shows nothing from GitHub. The footer's first line is the request
budget, "not known yet" before any answer, and says when checks are slowed; a pause is
a banner. The summary line counts running jobs; the menu, the icon and its tooltip do
not. An agent without the topic, or one with no room for the subscription, is said in
the footer, and the latter is asked again every 60 s.

The live region says the running-jobs count with the summary, at most once per 5 s,
and each one-time GitHub announcement once, queued in order and written at least 5 s
after the last write: checks slowing or back to every 15 s, hidden or unreadable job
names, a repository GitHub cannot answer for, GitHub status unavailable, and a job that
has ended. A pause is said at once by its banner.

`app/crates/view/tests/view/github.rs` covers the states from a recorded value in
`tests/fixtures/github/`, `app/crates/session/tests/session/github.rs` the
subscription's lifecycle and Open run's address, `tests/frontend/github.test.mjs` the
column, the link's Tab stop, name and focus move, and `tests/ipc.rs` that showing and
hiding the window opens and ends the subscription and that the opener plugin's own
commands are refused.
