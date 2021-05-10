#!/usr/bin/env bash

set -euo pipefail

# to run the bot locally
RUST_LOG=INFO cargo run -- --channels "#gougoutest" --nickname "rustytest"
