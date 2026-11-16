#!/usr/bin/env python3
"""Report source files that need responsibility/size review; not a build gate."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
findings = []
for path in (ROOT / "crates").rglob("*"):
    if any(part in {"target", "vendor", "node_modules", ".cache", ".git"} for part in path.relative_to(ROOT).parts):
        continue
    if path.suffix not in {".rs", ".slint"} or not path.is_file():
        continue
    lines = len(path.read_text().splitlines())
    entry = path.name in {"lib.rs", "main.rs"}
    if lines >= 750:
        action = "SEGMENT / justify cohesion"
    elif lines >= 500:
        action = "REVIEW responsibilities"
    elif entry and lines > 150:
        action = "REVIEW entry-point scope"
    else:
        continue
    findings.append((lines, str(path.relative_to(ROOT)), action))

print("Rust/Slint sources under crates/ (generated/build directories excluded).")
print("Lines  Source file                                      Review")
for lines, name, action in sorted(findings, reverse=True):
    print(f"{lines:5}  {name:<47}  {action}")
print(f"\n{len(findings)} files to evaluate. See docs/code-structure.md for exceptions and scope.")
