#!/usr/bin/env python3
"""Create a deterministic V3 disposition inventory for the C# xUnit suite."""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path


ATTRIBUTE = re.compile(r"\[(Fact|Theory)(?P<args>[^\]]*)\]")
METHOD = re.compile(
    r"\b(?:public|internal)\s+(?:async\s+)?(?:void|Task(?:<[^>]+>)?|[A-Za-z_][A-Za-z0-9_<>?]*)\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\("
)
LEGACY = re.compile(
    r"(?:legacy|compat|nscripter|backward|migration|migrate|(?:^|_)v[12](?:_|$)|oldformat|old_format)",
    re.IGNORECASE,
)
SOURCE_STRING = re.compile(
    r"(?:ReadAllText|ReadAllLines)\s*\([^;\n]*(?:\.cs|Parser|VirtualMachine|Program)",
    re.IGNORECASE,
)


def body_after(source: str, start: int) -> str:
    opening = source.find("{", start)
    if opening < 0:
        return ""
    depth = 0
    quote = None
    escaped = False
    for index in range(opening, len(source)):
        char = source[index]
        if escaped:
            escaped = False
            continue
        if quote and char == "\\":
            escaped = True
            continue
        if char in {'"', "'"}:
            quote = None if quote == char else (char if quote is None else quote)
            continue
        if quote:
            continue
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[opening : index + 1]
    return source[opening:]


def classify(path: Path, name: str, attribute: str, body: str) -> tuple[str, str]:
    identity = f"{path.stem}.{name}"
    if "Skip" in attribute:
        return "fixture_gap", "replace_or_supply_fixture"
    if SOURCE_STRING.search(body):
        return "source_string_dependency", "replace_with_structural_test"
    if LEGACY.search(identity):
        return "legacy_specification", "migration_boundary"
    return "behavior_regression", "candidate_for_v3_corpus"


def inventory(repository: Path) -> dict:
    test_root = repository / "src" / "AriaEngine.Tests"
    records = []
    for path in sorted(test_root.rglob("*.cs")):
        source = path.read_text(encoding="utf-8-sig")
        for attribute_match in ATTRIBUTE.finditer(source):
            method_match = METHOD.search(source, attribute_match.end())
            if not method_match:
                continue
            between = source[attribute_match.end() : method_match.start()]
            if "[Fact" in between or "[Theory" in between:
                continue
            name = method_match.group("name")
            attribute = attribute_match.group(0)
            body = body_after(source, method_match.end())
            category, disposition = classify(path, name, attribute, body)
            line = source.count("\n", 0, attribute_match.start()) + 1
            records.append(
                {
                    "id": f"{path.stem}.{name}",
                    "file": path.relative_to(repository).as_posix(),
                    "line": line,
                    "kind": attribute_match.group(1).lower(),
                    "category": category,
                    "v3_disposition": disposition,
                }
            )
    records.sort(key=lambda record: (record["file"], record["line"], record["id"]))
    counts = Counter(record["category"] for record in records)
    return {
        "schema_version": 1,
        "purpose": "V3 compatibility-corpus candidate inventory; heuristic classifications require review",
        "test_definitions": len(records),
        "category_counts": dict(sorted(counts.items())),
        "tests": records,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("compatibility/v3/legacy-test-inventory.json"),
    )
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    repository = args.repository.resolve()
    output = args.output if args.output.is_absolute() else repository / args.output
    encoded = json.dumps(inventory(repository), ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if not output.exists() or output.read_text(encoding="utf-8") != encoded:
            print(f"stale legacy test inventory: {output}")
            return 1
        print(f"legacy test inventory is current: {output}")
        return 0
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(encoded, encoding="utf-8")
    print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
