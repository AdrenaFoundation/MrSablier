# MrSablier

OpenSource GRPC rust client (Keeper) handling Liquidation, SL and TP for the adrena program.

See MrSablierStaking for the staking related counterpart.

## Build

`$> cargo build`
`$> cargo build --release`

## Run

`$> RUST_LOG=debug ./target/debug/mrsablier --payer-keypair <path> --fumarole-endpoint https://adrena-solanam-f3a8.mainnet.rpcpool.com/<fumarole-xtoken> --endpoint https://adrena.rpcpool.com/<xtoken> --x-token <xtoken> --fumarole-x-token <fumarole-xtoken> --consumer-group <EXISTING_GROUP_NAME> --member-id <0-n>`
`$> RUST_LOG=info ./target/debug/mrsablier --payer-keypair <path> --fumarole-endpoint https://adrena-solanam-f3a8.mainnet.rpcpool.com/<fumarole-xtoken> --endpoint https://adrena.rpcpool.com/<xtoken> --x-token <xtoken> --fumarole-x-token <fumarole-xtoken> --consumer-group <EXISTING_GROUP_NAME> --member-id <0-n>`

## Run as a service using (Daemon)[https://www.libslack.org/daemon/manual/daemon.1.html]

`daemon --name=mrsablier --output=/home/ubuntu/MrSablier/logfile.log -- /home/ubuntu/MrSablier/target/release/mrsablier --payer-keypair /home/ubuntu/MrSablier/mr_sablier.json --fumarole-endpoint https://adrena-solanam-f3a8.mainnet.rpcpool.com/<fumarole-xtoken> --endpoint https://adrena-solanam-6f0c.mainnet.rpcpool.com/<> --fumarole-x-token <fumarole-xtoken> --x-token <> --consumer-group <EXISTING_GROUP_NAME> --member-id <0-n>`

### Monitor Daemon logs

`tail -f -n 100 ~/MrSablier/logfile.log | tspin`

### Stop Daemon

`daemon --name=mrsablier --stop`
