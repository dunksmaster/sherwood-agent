#!/usr/bin/env python3
"""Check that every relative Markdown link in the repo resolves to a real file.

Run from the repo root. Exit non-zero on the first broken link found.
Skips external links (http/https/mailto) and in-page anchors.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LINK = re.compile(r"\[[^\]]*\]\(([^)#]+?)(#[^)]*)?\)")
SKIP_DIRS = {".git", "target", "node_modules"}


def markdown_files() -> list[Path]:
    out: list[Path] = []
    for p in ROOT.rglob("*.md"):
        if any(part in SKIP_DIRS for part in p.relative_to(ROOT).parts):
            continue
        out.append(p)
    return sorted(out)


def main() -> int:
    broken: list[str] = []
    checked = 0
    for md in markdown_files():
        text = md.read_text(encoding="utf-8")
        for m in LINK.finditer(text):
            target = m.group(1).strip()
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            checked += 1
            resolved = (md.parent / target).resolve()
            if not resolved.exists():
                rel = md.relative_to(ROOT).as_posix()
                broken.append(f"{rel} -> {target}")

    if broken:
        print(f"FAIL: {len(broken)} broken relative link(s):")
        for b in broken:
            print(f"  - {b}")
        return 1

    print(f"OK: {checked} relative links across {len(markdown_files())} files resolve.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
