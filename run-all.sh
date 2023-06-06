#!/bin/sh

years=$(ls data/ | grep "^[0-9]\+$")

for year in $years; do
    cargo run --release -- preprocess $year || exit 1
    cargo run --release -- $year > data/$year/output/stdout.log || exit 1
done