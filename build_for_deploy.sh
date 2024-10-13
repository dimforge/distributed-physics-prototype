#!/bin/sh

set -v

cargo build --release -p steadyum-partitionner --features dim3
strip target/release/steadyum-partitionner

cargo build --release -p steadyum-updater --features dim3
strip target/release/steadyum-updater

cargo build --release -p steadyum-runner --features dim3
strip target/release/steadyum-runner


