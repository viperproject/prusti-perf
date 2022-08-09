#!/usr/bin/env bash

function subst() {
    find site -type f -name "*.rs" | while read -r SRC_FILE; do
        if [ "$(uname)" == "Darwin" ]; then
            sed -i "" "$1" "$SRC_FILE"
        else
            sed -i "$1" "$SRC_FILE"
        fi
    done
}
subst "s/rust-lang\/rustc-perf/prusti-dev\/prusti-perf/g"
subst "s/rust-lang\/rust/viperproject\/prusti-dev/g"
subst "s/rust-lang-ci\/rust/viperproject\/prusti-dev/g"
subst "s/https:\/\/perf.rust-lang.org/http:\/\/3.94.193.1:2346/g"

# subst "s/viperproject/zgrannan/g"
# subst "s/3.94.193.1/compute.zackg.me/g"
