# Distributed physics experiments

Experiments on distributed physics with rapier. Note that this repository is **not** production-ready
and involves many manual steps in order to deploy the rapier distributed simulation cluster. The main
goal of this repository is to experiment with designs for building a distributed physics systems aiming
to simulate 1 million objects simultaneously.

Several executable are involved:

- **steadyum-partitionner**: responsible for partitioning the 3D simulation domain, assigning work to runners, and
  (virtual) time synchronization between runners.
- **steadyum-runner**: responsible for running a Rapier physics engine instance. It receives objects to simulate,
  simulates them, and automatically detects if an object needs to be transferred to a different runner as it moves.
- **steadyum-updater**: this is not a mandatory component, but it convenient for fast iterations. When deployed on a
  new node, it will communicate with the master `partitionner` instance to automatically download the latest versions
  of the partitionnar and runner executables, and deploys them locally.

## Building

Build each executable with cargo, just like any other rust project. We recommend stripping the executables to
reduce their sizes:

```shell
cargo build --release -p steadyum-partitionner --features dim3
strip target/release/steadyum-partitionner

cargo build --release -p steadyum-updater --features dim3
strip target/release/steadyum-updater

cargo build --release -p steadyum-runner --features dim3
strip target/release/steadyum-runner
```

## Deploying

Once all the executables are built you will need at least two nodes: one for running the master partitionner, one
for running the runners.

### Master partitionner node

On the master partitionner node:

1. Add a configuration file `.env` with the following content:

```.env
REDIS_ADDR="redis://127.0.0.1"
PARTITIONNER_ADDR="http://localhost"
PARTITIONNER_PORT="3535"
RUNNER_EXE="./steadyum-runner"
ZENOH_ROUTER=""
```

1. Run `steadyum-partitionner`.

### Runners nodes

There can be one or many runner nodes. They will all contribute to the same physic simulation.
On each of the runner node:

1. Add a configuration file `.env` with the following content. Be sure to replace the ip by the IP of
   the node running the master partitionner. Replace `ens4` by the name of the private network interface
   of the runner node. This is used to automatically retrieve its ip address on the private network.

```shell
PARTITIONNER_ADDR="http://10.0.2.153"
PARTITIONNER_PORT="3535"
RUNNER_EXE="./runner"
PRIV_NET_INT="ens4"
ZENOH_ROUTER="tcp/10.0.2.153:7447"
```

2. Upload the `steadyum-updater` and run it: `./steadyum-updater`. Based on the env file, it will automatically
   communicate with the master partitionner and download the necessary executables locally (runner and partitionner).
