#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt: every crate that MusicCraft's packages contain, with its
licence and the licence texts that come with it, after the licences of the bundled fonts.

Run from the repository root: python3 tools/licences.py
"""
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGETS = ["x86_64-pc-windows-msvc", "aarch64-apple-darwin", "x86_64-unknown-linux-gnu"]
NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")


def shipped():
    """Crates reachable from the app through normal dependencies, on any platform."""
    out = {}
    for target in TARGETS:
        cmd = ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target]
        meta = json.loads(subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True).stdout)
        packages = {p["id"]: p for p in meta["packages"]}
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        todo = [i for i, p in packages.items() if p["name"] == "musiccraft"]
        seen = set()
        while todo:
            i = todo.pop()
            if i in seen:
                continue
            seen.add(i)
            for d in nodes[i]["deps"]:
                if any(k["kind"] is None for k in d["dep_kinds"]):
                    todo.append(d["pkg"])
        for i in seen:
            p = packages[i]
            if p["source"]:
                out[(p["name"], p["version"])] = p
    return [out[k] for k in sorted(out)]


def main():
    parts = [(ROOT / "packaging" / "font-licenses.txt").read_text(encoding="utf-8").rstrip(), ""]
    line = "=" * 64
    parts += [line, "Rust crates", line, ""]
    texts = {}
    for p in shipped():
        where = p["repository"] or f"https://crates.io/crates/{p['name']}"
        parts.append(f"{p['name']} {p['version']} - {p['license']} - {where}")
        folder = Path(p["manifest_path"]).parent
        for f in sorted(folder.iterdir()):
            if f.is_file() and f.name.upper().startswith(NAMES):
                text = f.read_text(encoding="utf-8", errors="replace").strip().replace("\r\n", "\n")
                key = hashlib.sha256(" ".join(text.split()).encode()).hexdigest()
                texts.setdefault(key, (text, []))[1].append(f"{p['name']} {p['version']} ({f.name})")
    parts += ["", "Crates under MPL-2.0 are used unmodified. Their source is at the addresses above and on",
              "https://crates.io, in the version listed.", ""]
    for text, users in texts.values():
        parts += [line, ", ".join(users), line, "", text, ""]
    (ROOT / "THIRD-PARTY-LICENSES.txt").write_text("\n".join(parts), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
