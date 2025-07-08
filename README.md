[![CI](https://github.com/lu-bann/taiko-preconfer/actions/workflows/check.yml/badge.svg)](https://github.com/lu-bann/taiko-preconfer/actions/workflows/check.yml)
[![Dependabot Updates](https://github.com/lu-bann/taiko-preconfer/actions/workflows/dependabot/dependabot-updates/badge.svg)](https://github.com/lu-bann/taiko-preconfer/actions/workflows/dependabot/dependabot-updates)

# Taiko Preconfer

## Components
The preconfirmation algorithm is split into different responsibilities
* Streams:
  * l2 head (`preconfirmation/src/stream/l2_head_info_stream.rs`)
  * l1 headers (`preconfirmation/src/stream/header_stream.rs`)
* SlotModel (`preconfirmation/src/preconf/slot_model.rs`): Responsible for determining if we are responsible for preconfirmation in a given slot
* SequencingMonitor (`preconfirmation/src/preconf/sequencing_monitor.rs`): Responsible for waiting until we are in sync with the status endpoint
* WhitelistMonitor (`taiko-preconfer/src/util.rs`): Responsible for monitoring selected preconfers for current and next epoch
* BlockBuilder (`preconfirmation/src/preconf/block_builder.rs`): Responsible for publishing preconfirmed transactions to the L2
* ConfirmationStrategy (`preconfirmation/src/preconf/confirmation_strategy.rs`): Responsible for confirmation of preconfirmed blocks on the L1

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
