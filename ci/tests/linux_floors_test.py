#!/usr/bin/env python3
"""Properties of ci/linux-floors.sh, the check that hpm's Linux binary needs
no newer glibc or libstdc++ than Houdini 22.

Each case builds text shaped like `readelf -V` output from random symbol
versions (with names that must not count mixed in), runs the script on it
with --text, and compares what it reports and decides with an independent
model: versions compared as integer tuples, so 2.9 < 2.28 and 3.4.9 < 3.4.22,
which a string comparison gets wrong.

    python3 ci/tests/linux_floors_test.py [cases]

A failure prints the seed and the case; rerun with TEST_SEED=<seed>.
"""

import os
import random
import subprocess
import sys
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "linux-floors.sh"
FLOORS = {"GLIBC": (2, 28), "GLIBCXX": (3, 4, 22)}
NOISE = ["GLIBC_PRIVATE", "CXXABI_1.3.9", "GCC_3.0", "Name: libc.so.6", "OPENSSL_3.0.0"]


def dotted(version):
    return ".".join(str(p) for p in version)


def gen_version(rng, lib):
    if rng.random() < 0.3:
        # Near the floor: equal, one step either side in some part.
        v = list(FLOORS[lib])
        i = rng.randrange(len(v))
        v[i] = max(0, v[i] + rng.choice([-1, 0, 1]))
        return tuple(v)
    return tuple(rng.randint(0, 40) for _ in range(rng.randint(1, 4)))


def gen_case(rng):
    needs = [(lib, gen_version(rng, lib))
             for lib in rng.choices(list(FLOORS), k=rng.randint(0, 10))]
    noise = rng.sample(NOISE, rng.randint(0, len(NOISE)))
    return needs, noise


def readelf(needs, noise):
    lines = ["Version needs section '.gnu.version_r' contains 2 entries:"]
    for i, (lib, version) in enumerate(needs):
        lines.append(f"  0x{i:04x}:   Name: {lib}_{dotted(version)}  Flags: none  Version: {i + 2}")
        if i < len(noise):
            lines.append(f"  0x{i:04x}:   Name: {noise[i]}  Flags: none  Version: 9")
    lines += [f"  Name: {n}" for n in noise[len(needs):]]
    return "\n".join(lines) + "\n"


def model(needs):
    """(GLIBC shown, GLIBCXX shown, refused) as the script should report."""
    best = {}
    for lib, version in needs:
        # sort -V orders 2.28 and 2.28.0 apart; the model keys on the tuple
        # with trailing zeros kept, as the text is.
        if lib not in best or version > best[lib]:
            best[lib] = version
    shown = {lib: dotted(best[lib]) if lib in best else "none" for lib in FLOORS}
    refused = any(lib in best and pad_gt(best[lib], FLOORS[lib]) for lib in FLOORS)
    return shown["GLIBC"], shown["GLIBCXX"], refused


def pad_gt(a, b):
    n = max(len(a), len(b))
    return a + (0,) * (n - len(a)) > b + (0,) * (n - len(b))


def run(text):
    p = subprocess.run(["bash", str(SCRIPT), "--text"], input=text,
                       capture_output=True, text=True)
    return p.returncode, p.stdout


def prop_reports_and_decides(rng):
    needs, noise = gen_case(rng)
    # Two versions equal but for trailing zeros (2.28 / 2.28.0) leave which
    # one is "highest" to sort -V's tie order: the model cannot name it.
    seen = {}
    for lib, v in needs:
        key = (lib, tuple(v) + (0,) * (4 - len(v)))
        if seen.setdefault(key, v) != v:
            return True, None
    glibc, glibcxx, refused = model(needs)
    code, out = run(readelf(needs, noise))
    ok = (out == f"stdin needs GLIBC_{glibc}, GLIBCXX_{glibcxx}\n"
          and code == (1 if refused else 0))
    return ok, (needs, noise, code, out)


def prop_floors_inclusive(rng):
    del rng
    at = [(lib, v) for lib, v in FLOORS.items()]
    code_at, _ = run(readelf(at, []))
    bumped = [run(readelf([(lib, v[:-1] + (v[-1] + 1,))], []))[0] for lib, v in FLOORS.items()]
    return code_at == 0 and bumped == [1, 1], (code_at, bumped)


PROPS = [(prop_reports_and_decides, None), (prop_floors_inclusive, 1)]


def main():
    cases = int(sys.argv[1]) if len(sys.argv) > 1 else 300
    seed = int(os.environ.get("TEST_SEED") or random.SystemRandom().randrange(2**32))
    failed = 0
    for prop, fixed in PROPS:
        rng = random.Random(seed)
        n = fixed or cases
        for _ in range(n):
            ok, case = prop(rng)
            if not ok:
                print(f"FAIL {prop.__name__} (TEST_SEED={seed}): {case!r}")
                failed += 1
                break
        else:
            print(f"ok   {prop.__name__} ({n} cases)")
    if failed:
        print(f"{failed} FAILED (TEST_SEED={seed})")
        return 1
    print(f"all linux-floors properties passed (TEST_SEED={seed})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
