"""Check the notice inventory against the locked Windows build and Rust toolchain."""

import hashlib
import os
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8")


def main():
    notices = (ROOT / "THIRD-PARTY-NOTICES.txt").read_text(encoding="utf-8")
    table = notices.split("Package and version | License used\n", 1)[1].split("\n\n", 1)[0]
    recorded = {line.split(" | ")[0] for line in table.splitlines()}
    tree = run(
        os.environ.get("CARGO", "cargo"), "tree", "--frozen", "--target",
        "x86_64-pc-windows-msvc", "--edges", "normal,build",
        "--prefix", "none", "--format", "{p}",
    )
    expected = {
        f"{name} {version}" for name, version in
        re.findall(r"(?m)^([\w-]+) v([^\s]+)", tree)
        if name != "uac2-control"
    }
    if recorded != expected:
        raise SystemExit(
            f"Missing package notices: {sorted(expected - recorded)}\n"
            f"Stale package notices: {sorted(recorded - expected)}"
        )

    rustc = os.environ.get("RUSTC", "rustc")
    version = run(rustc, "-V").split()
    if f"rustc {version[1]} {version[2]})" not in notices:
        raise SystemExit("Review the standard library notices for this Rust toolchain.")
    sysroot = Path(run(rustc, "--print", "sysroot").strip())
    std_notice = sysroot / "share/doc/rust/COPYRIGHT-library.html"
    if not std_notice.is_file():
        raise SystemExit("Install rust-docs for this toolchain to verify its copyright inventory.")
    digest = hashlib.sha256(std_notice.read_bytes()).hexdigest()
    if f"Original file SHA-256: {digest}" not in notices:
        raise SystemExit("The Rust standard library copyright inventory has changed.")
    print(f"Verified {len(recorded)} Windows dependency versions and the Rust standard library notice inventory.")


if __name__ == "__main__":
    main()
