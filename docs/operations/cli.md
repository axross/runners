---
type: Guide
title: "marshal-cli documentation routes"
description: "Published section names and their current pages in the marshal-cli documentation."
---

# CLI

[marshal-cli documentation](../marshal-cli/index.md)

## Running It

[marshal-cli entry points](../marshal-cli/entry-points.md#running-it)

## Profiles

[Host profiles and JSON validation](../marshal-cli/json-validation.md#profiles)

### New Rules

[Host profiles and JSON validation](../marshal-cli/json-validation.md#new-rules)

### Migrations

[Host profiles and JSON validation](../marshal-cli/json-validation.md#migrations)

### Labels

[Host profiles and JSON validation](../marshal-cli/json-validation.md#labels)

### The Checkout Rule

[Host profiles and JSON validation](../marshal-cli/json-validation.md#the-checkout-rule)

## Validation Rules

[Host profiles and JSON validation](../marshal-cli/json-validation.md#validation-rules)

### Error Order

[Host profiles and JSON validation](../marshal-cli/json-validation.md#error-order)

### Behaviour That Is Easy to Miss

[Host profiles and JSON validation](../marshal-cli/json-validation.md#behaviour-that-is-easy-to-miss)

### Inputs the Fixtures Pin

[Host profiles and JSON validation](../marshal-cli/json-validation.md#inputs-the-fixtures-pin)

## The TOML Configuration

[TOML configuration and edits](../marshal-cli/toml-configuration.md#the-toml-configuration)

### Schema Version 1

[TOML configuration and edits](../marshal-cli/toml-configuration.md#schema-version-1)

### Extending the Schema

[TOML configuration and edits](../marshal-cli/toml-configuration.md#extending-the-schema)

### TOML Problems

[TOML configuration and edits](../marshal-cli/toml-configuration.md#toml-problems)

### One Rule Set, Two Readers

[TOML configuration and edits](../marshal-cli/toml-configuration.md#one-rule-set-two-readers)

### Converting a JSON Configuration

[TOML configuration and edits](../marshal-cli/toml-configuration.md#converting-a-json-configuration)

### Changing the File

[TOML configuration and edits](../marshal-cli/toml-configuration.md#changing-the-file)

## The Runtime Layer

[Pool runtime and slot cycle](../marshal-cli/pool-runtime.md#the-runtime-layer)

### Backend

[Docker backend contract](../marshal-cli/docker-backend.md#backend)

### Runtime Seam

[Pool runtime and slot cycle](../marshal-cli/pool-runtime.md#runtime-seam)

### Tart Backend

[Tart runtime and golden VM provisioning](../marshal-cli/tart-backend.md#tart-backend)

### The VM Shim

[Tart runtime and golden VM provisioning](../marshal-cli/tart-backend.md#the-vm-shim)

### Host Prerequisites

[Tart runtime and golden VM provisioning](../marshal-cli/tart-backend.md#host-prerequisites)

### Golden VM Provisioning

[Tart runtime and golden VM provisioning](../marshal-cli/tart-backend.md#golden-vm-provisioning)

### CPU Placement

[Pool runtime and slot cycle](../marshal-cli/pool-runtime.md#cpu-placement)

### Registration

[Pool runtime and slot cycle](../marshal-cli/pool-runtime.md#registration)

### Names and the Slot Loop

[Pool runtime and slot cycle](../marshal-cli/pool-runtime.md#names-and-the-slot-loop)

### What the Tests Cover

[Pool runtime and slot cycle](../marshal-cli/pool-runtime.md#what-the-tests-cover)

### Signal Source

[Mac sleep and network signals](../marshal-cli/mac-signals.md#signal-source)

#### Wiring the Signals into the Instance

[Mac sleep and network signals](../marshal-cli/mac-signals.md#wiring-the-signals-into-the-instance)

## The Instance Layer

[Foreground instance ownership](../marshal-cli/foreground-instance.md#the-instance-layer)

### Instance State

[Foreground instance ownership](../marshal-cli/foreground-instance.md#instance-state)

### Single-Instance Lock

[Foreground instance ownership](../marshal-cli/foreground-instance.md#single-instance-lock)

### Control Channel

[Foreground instance ownership](../marshal-cli/foreground-instance.md#control-channel)

### Roots and Private Files

[Foreground instance ownership](../marshal-cli/foreground-instance.md#roots-and-private-files)

### What the Instance Tests Cover

[Foreground instance ownership](../marshal-cli/foreground-instance.md#what-the-instance-tests-cover)

## The Agent

[Resident agent and state transitions](../marshal-cli/resident-agent.md#the-agent)

### Agent State

[Resident agent and state transitions](../marshal-cli/resident-agent.md#agent-state)

#### The Agent's Configuration

[Resident agent and state transitions](../marshal-cli/resident-agent.md#the-agents-configuration)

### The Agent's Signals

[Agent lifecycle and update drain](../marshal-cli/agent-lifecycle.md#the-agents-signals)

### The Update Drain

[Agent lifecycle and update drain](../marshal-cli/agent-lifecycle.md#the-update-drain)

### The Token Store

[Host token store](../marshal-cli/token-store.md#the-token-store)

### Protocol Version 2

[Agent protocol version 2 and snapshots](../marshal-cli/agent-protocol.md#protocol-version-2)

### Version Rules

[Agent protocol version 2 and snapshots](../marshal-cli/agent-protocol.md#version-rules)

### Editing the Configuration

[Configuration editing protocol](../marshal-cli/config-protocol.md#editing-the-configuration)

### The Snapshot

[Agent protocol version 2 and snapshots](../marshal-cli/agent-protocol.md#the-snapshot)

### GitHub Status

[GitHub status polling](../marshal-cli/github-status.md#github-status)

### What the Agent Tests Cover

[Resident agent and state transitions](../marshal-cli/resident-agent.md#what-the-agent-tests-cover)

## The Image Contract

[Runner image compatibility and freshness](../marshal-cli/runner-images.md#the-image-contract)

### Labels

[Runner image compatibility and freshness](../marshal-cli/runner-images.md#labels)

### The Image Check

[Runner image compatibility and freshness](../marshal-cli/runner-images.md#the-image-check)

### Freshness

[Runner image compatibility and freshness](../marshal-cli/runner-images.md#freshness)

## macOS VM Pools

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#macos-vm-pools)

### Defaults and Minimums

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#defaults-and-minimums)

### Migration

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#migration)

### The Runner Record

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#the-runner-record)

### The Base Record

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#the-base-record)

### VM Names

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#vm-names)

### The Two-VM Count

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#the-two-vm-count)

### The Golden macOS VM Build

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#the-golden-macos-vm-build)

### The macOS Job VM

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#the-macos-job-vm)

### macOS Status

[macOS VM pools and capacity](../marshal-cli/macos-pools.md#macos-status)

## Running a Host

[Foreground host entry points](../marshal-cli/foreground-commands.md#running-a-host)

### Start

[Foreground host entry points](../marshal-cli/foreground-commands.md#start)

### The Clients

[Foreground host entry points](../marshal-cli/foreground-commands.md#the-clients)

### Stopping

[Foreground host entry points](../marshal-cli/foreground-commands.md#stopping)

### Diagnostics

[Diagnostic jobs and retained evidence](../marshal-cli/diagnostics.md#diagnostics)

### Build

[Building the host image or golden VM](../marshal-cli/build-images.md#build)

#### The Golden VM Build

[Building the host image or golden VM](../marshal-cli/build-images.md#the-golden-vm-build)

### The Log

[Foreground host entry points](../marshal-cli/foreground-commands.md#the-log)

### Containment

[Foreground host entry points](../marshal-cli/foreground-commands.md#containment)

### Console Close on Windows

[Foreground host entry points](../marshal-cli/foreground-commands.md#console-close-on-windows)

### What the Command Tests Cover

[Foreground host entry points](../marshal-cli/foreground-commands.md#what-the-command-tests-cover)

## Fixtures and Tests

[Configuration fixtures and expectations](../marshal-cli/configuration-fixtures.md#fixtures-and-tests)

### TOML Fixtures

[Configuration fixtures and expectations](../marshal-cli/configuration-fixtures.md#toml-fixtures)

## Toolchain and Gates

[Checking the Rust toolchain and workspaces](../marshal-cli/toolchain.md#toolchain-and-gates)

## The App Workspace

[Checking the Rust toolchain and workspaces](../marshal-cli/toolchain.md#the-app-workspace)

## Releases

[Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#releases)

### Installers

[Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#installers)

### The Dry Run and the Release Workflow

[Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#the-dry-run-and-the-release-workflow)

### The Last Command-Line Release

[Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#the-last-command-line-release)

### Signing, Held for M2 and W4

[Release signing identities](../marshal-cli/signing.md#signing-held-for-m2-and-w4)

### The Signing Contracts

[Release signing identities](../marshal-cli/signing.md#the-signing-contracts)

### Reading a Signature

[Release signing identities](../marshal-cli/signing.md#reading-a-signature)

### Extending the Release Workflows

[Release workflow extension contracts](../marshal-cli/release-extensions.md#extending-the-release-workflows)

#### Release Sets and Job Dependencies

[Release workflow extension contracts](../marshal-cli/release-extensions.md#release-sets-and-job-dependencies)

#### W1's Installer Pull Request

[Release workflow extension contracts](../marshal-cli/release-extensions.md#w1s-installer-pull-request)

#### The Restoration Point and Documents Through the Gap

[Release workflow extension contracts](../marshal-cli/release-extensions.md#the-restoration-point-and-documents-through-the-gap)

#### M2's Disk-Image Pull Request

[Release workflow extension contracts](../marshal-cli/release-extensions.md#m2s-disk-image-pull-request)

#### W4's Feed Pull Request and M4's Archive

[Release workflow extension contracts](../marshal-cli/release-extensions.md#w4s-feed-pull-request-and-m4s-archive)

#### Extending the Shape Check

[Release workflow extension contracts](../marshal-cli/release-extensions.md#extending-the-shape-check)

### What the Release Tests Cover

[Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#what-the-release-tests-cover)

## Installing, Updating and Rolling Back

[Checking and installing the Windows app MSI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back)

### Installing

[Checking and installing the Windows app MSI](../marshal-cli/install-on-windows.md#installing)

### Checking the Download

[Checking and installing the Windows app MSI](../marshal-cli/install-on-windows.md#checking-the-download)

### Updating, Repairing, Uninstalling and Rolling Back

[Checking and installing the Windows app MSI](../marshal-cli/install-on-windows.md#updating-repairing-uninstalling-and-rolling-back)

### What the Install Tests Cover

[Checking and installing the Windows app MSI](../marshal-cli/install-on-windows.md#what-the-install-tests-cover)

## Dependencies

[Rust dependency selection](../marshal-cli/dependencies.md#dependencies)

## Renaming the CLI

[Marshal host names](../marshal-cli/host-names.md#renaming-the-cli)
