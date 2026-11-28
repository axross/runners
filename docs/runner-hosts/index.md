# Runner hosts

Choose the machine that runs the pools. A Windows host runs each job in a Docker
container. An Apple silicon Mac runs each job in a fresh Tart VM.

## Windows with Docker Desktop

- [Prepare Windows, Docker Desktop and Marshal and check which releases can run on axpc.](./windows-setup.md)
- [Set GitHub repository approval requirements and issue a token for a repository or organisation pool.](./registration.md)
- [Choose pool resources, cache volumes and routing labels, set up an on-demand pool, delete an unwanted scale set, or migrate an existing configuration.](./windows-pools.md)
- [Build the runner image or decide whether a warning calls for an OS refresh or a new release.](./windows-image.md)
- [Start or stop the runners and check their state on the machine and on GitHub.](./windows-running.md)
- [Follow a slot through registration and disposal of its job container.](./windows-job.md)
- [Export private diagnostics, including retained containers, before disabling collection.](./windows-diagnostics.md)
- [Recover a failed pool or roll back to a compatible release.](./windows-recovery.md)

## macOS with Tart

- [Install Tart and Softnet, restore Softnet's privilege and run in the required signed-in session.](./mac-setup.md)
- [Determine which resources a Linux job VM receives and what happens to it after a job.](./linux-vm.md)
- [Build or replace the golden Linux VM and recover from an interrupted build.](./golden-build.md)
- [Configure pools, check workflow routing, monitor runners and remove a retired pool's VMs.](./mac-pools.md)
- [Interpret a stopped instance after sleep or a network change and decide when to resume.](./mac-resume.md)
- [Size macOS guests, update their base and diagnose the two-guest limit.](./macos-vm.md)
- [Verify behavior on Mac hardware and find out which claims have native evidence.](./mac-verification.md)
