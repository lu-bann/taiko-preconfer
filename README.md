[![CI](https://github.com/lu-bann/taiko-preconfer/actions/workflows/check.yml/badge.svg)](https://github.com/lu-bann/taiko-preconfer/actions/workflows/check.yml)
[![Dependabot Updates](https://github.com/lu-bann/taiko-preconfer/actions/workflows/dependabot/dependabot-updates/badge.svg)](https://github.com/lu-bann/taiko-preconfer/actions/workflows/dependabot/dependabot-updates)

# Taiko Preconfer
## Overview
The following diagram illustrates the interaction of a preconfer with the taiko network and the underlying L1.
```mermaid
sequenceDiagram
    participant User
    participant Taiko Network
    participant Preconfer
    participant L1 Contracts
    Preconfer->>L1 Contracts:(1) Register
    loop
        User->>Taiko Network:(2) tx
        User->>Taiko Network:(2) tx
        User->>Taiko Network:(2) tx
        Preconfer->>Taiko Network:(3) Fetch head
        Taiko Network->>Preconfer:(3) head
        Preconfer->>Taiko Network:(3) Fetch txs
        Taiko Network->>Preconfer:(3) txs
        Preconfer->>Preconfer:(3) Build L2 block
        Preconfer->>Preconfer:(3) Signs L2 block
        Preconfer->>Taiko Network:(3) Signed L2 block
        Taiko Network->>Taiko Network:(4) Execute L2 block
        Taiko Network->>User:(4) Latest preconfed state
    end
    Preconfer->>L1 Contracts:(5) Propose batch
    
```

## Components
In the preconfer operations get triggered by streams. For this we have two stream implementations:
* a slot stream (`preconfirmation/src/stream/slot_stream.rs`) that provides the L2 slot number as well as the corresponding L1 slot at the start of an L2 slot. This triggers the main computations.
* a header stream (`preconfirmation/src/stream/header_stream.rs`) that provides headers that are guaranteed to have increasing block numbers. This is used to track both the heads of both L1 and L2. It combines a websocket stream and a polling stream
  * websocket stream: typically faster, cheap, but unreliable, may provide blocks out of order or miss blocks
  * polling stream: reliable, slower, higher computational costs, provides blocks in order

The preconfirmation algorithm is split into different responsibilities
* ActiveOperatorModel: Responsible for determining if we are responsible for preconfirmation in a given slot
* SequencingMonitor: Responsible for waiting until we are in sync with the status endpoint
* Preconfer: Responsible for publishing preconfirmed transactions to the L2
* ConfirmationStrategy: Responsible for confirmation of preconfirmed blocks on the L1

## Development
#### git hooks
Enable git hooks through either
 - copying them from `<REPO_ROOT>/scripts/git` to `<REPO_ROOT>/.git/hooks` or
 - setting the hooks path to `<REPO_ROOT>/scripts/git`, i.e. `git config core.hooksPath <REPO_ROOT>/scripts/git`

The following hooks are available:
 - post-merge: Removes branches that have been deleted on the remote

#### code coverage
To compute test coverage install `cargo-llvm-cov`.
* Install: `cargo install cargo-llvm-cov`
* Run from repo root: `zsh scripts/coverage.sh`
* Display coverage in vscode: Install `Coverage Gutters` extension
