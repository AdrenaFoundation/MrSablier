# MrSablier

OpenSource GRPC rust client (Keeper) handling Liquidation, SL and TP for the adrena program.

See MrSablierStaking for the staking related counterpart.

## Build

`$> cargo build`
`$> cargo build --release`

## Run

`$> RUST_LOG=debug ./target/debug/mrsablier --endpoint https://adrena-solanam-6f0c.mainnet.rpcpool.com/ --x-token <> --commitment processed`
`$> RUST_LOG=info ./target/debug/mrsablier --endpoint https://adrena-solanam-6f0c.mainnet.rpcpool.com/ --x-token <> --commitment processed`

## Run as a service using (Daemon)[https://www.libslack.org/daemon/manual/daemon.1.html]

`daemon --name=mrsablier --output=/home/ubuntu/MrSablier/logfile.log -- /home/ubuntu/MrSablier/target/release/mrsablier --payer-keypair /home/ubuntu/MrSablier/mr_sablier.json --endpoint https://adrena-solanam-6f0c.mainnet.rpcpool.com/<> --x-token <> --commitment processed`

### Monitor Daemon logs

`tail -f -n 100 ~/MrSablier/logfile.log | tspin`

### Stop Daemon

`daemon --name=mrsablier --stop`