#!/usr/bin/env bash

cargo build --release --package ansivil
cargo run --release --package ansivil -- -c ./rickroll.sh &
PID=$!
sleep 30
kill $PID
