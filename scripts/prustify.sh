#!/usr/bin/env bash

function subst() {
    find site -type f -name "*.rs" | while read -r SRC_FILE; do
        sed -i "" "$1" "$SRC_FILE"
    done
}
subst "s/rust-lang\/rustc-perf/prusti-dev\/prusti-perf/g"
subst "s/viperproject\/prusti-dev/zgrannan\/prusti-dev/g"
subst "s/rust-lang\/rust/zgrannan\/prusti-dev/g"
subst "s/rust-lang-ci\/rust/zgrannan\/prusti-dev/g"
subst "s/https:\/\/perf.rust-lang.org/http:\/\/3.94.193.1:2346/g"
