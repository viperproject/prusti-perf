#!/usr/bin/env python3

from os import path, makedirs
import shutil
import subprocess

def make_cargo(package_name):
    return f"""
[package]
name = "{package_name}"
version = "0.0.1"
authors = [ "Prusti Developers" ]
edition = "2018"

[workspace]
"""

perf_config = """{
  "category": "primary"
}"""

with open("../prusti-dev/benchmarked-files.csv") as f:
    filenames = f.read().splitlines()

for filename in filenames:
    cargo_name = filename.split("/")[-1].replace("_", "-").split(".rs")[0].lower()
    dir_name = f"collector/benchmarks/{cargo_name}"
    if path.isdir(dir_name):
        pass
        # continue
    else:
        makedirs(dir_name)
    cargo_toml_filename = f"{dir_name}/Cargo.toml"
    with open(cargo_toml_filename, 'w') as f:
        f.write(make_cargo(cargo_name))
    perf_config_filename = f"{dir_name}/perf-config.json"
    with open(perf_config_filename, 'w') as f:
        f.write(perf_config)
    src_dir = f"{dir_name}/src"
    if not path.isdir(src_dir):
        makedirs(src_dir)
    shutil.copy(f"../prusti-dev/{filename}", f"{src_dir}/lib.rs")
    p = subprocess.Popen(["cargo", "generate-lockfile"], cwd = dir_name)
    p.wait()
