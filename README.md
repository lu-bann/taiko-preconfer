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

## Docker image
If you want to run the preconfer through docker:
* `cp .env.sample .env` and add the missing information, i.e.
  * addresses for used contracts: 
    - TAIKO_ANCHOR_ADDRESS: anchor contract
    - TAIKO_PRECONF_ROUTER_ADDRESS: preconf router
    - TAIKO_INBOX_ADDRESS: taiko inbox
    - TAIKO_WHITELIST_ADDRESS: whitelist
    - GOLDEN_TOUCH_ADDRESS=0x0000777735367b36bC9B61C50022d9D0700dB4Ec
    - GOLDEN_TOUCH_PRIVATE_KEY=0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38
  * secrets:
    - PRIVATE_KEY: private key of preconfer
    - JWT_SECRET: jwt secret for submitting preconfirmed blocks and accessing mempool
  * urls:
    - L2_CLIENT_URL: geth rpc url
    - L2_PRECONFIRMATION_URL: taiko-client preconfirmation url
    - L2_AUTH_CLIENT_URL: geth rpc url for taikoAuth_txPoolContent requests
    - L2_WS_URL: geth websocket url (for streaming blocks/events)
    - L1_CLIENT_URL: L1 rpc client
    - L1_WS_URL: l1 websocket url (for streaming blocks/events)
* Build the image if necessary and start the preconfer: `docker-compose up`.

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
