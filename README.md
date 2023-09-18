# Neutron Examples

A set of example contracts exploring [Neutron's](docs.neutron.org) interchain accounts/queries/txs.

## Usage

You need Rust, Cargo & Go (1.20) in your path or if you have [Nix with flakes](https://nixos.wiki/wiki/Flakes) setup, just run:
```
❯ : nix develop
```

This repo uses the [xtask](https://github.com/matklad/cargo-xtask) pattern.

Your point of entry:

```
❯ : cargo x
Usage: xtask <COMMAND>

Commands:
  dist               compile contracts for distribution
  start-local        start a local neutron network instance (neutron + gaia + hermes + icq relayer)
  clean-local-state  clean local network state, resetting the chains
  clean-local-all    clean local network artifacts including built binaries and source file
  test               testing tasks
  help               Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

You have two options to deploy and run the contracts locally:

1. Start a local network that stays up between deploy & execute runs:

```
// in one terminal
❯ : cargo x start-local

// in another terminal
❯ : cargo t -- --no-capture
```

2. Start a local network, deploy the contracts and run them all once

```
❯ : cargo x test e2e
```

> WARNING: The first time you start a local network it'll take some time to fetch and build the binaries, especially [Hermes](https://github.com/informalsystems/hermes) as it has far too many dependencies (>470).
