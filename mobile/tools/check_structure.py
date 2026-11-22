#!/usr/bin/env python3
"""Checks the structure rules of the mobile apps and prints one line per violation.

    check_structure.py [--root MOBILE_DIR]

The rules (the README explains them and their limits):
  1. A feature uses no other feature.
  2. A screen names no store, state holder or `Construct` type.
  3. A screen has no colour or size literal.

Exit status is 1 when a rule is broken, with `file:line: message` for each, and also when there is
nothing to scan (no feature, no Swift feature, a feature without a screen), so a moved or renamed
file cannot turn the check into a pass.
"""

import argparse
import re
import sys
from pathlib import Path

GRADLE_FEATURE_DEP = [
    re.compile(r'\bproject\(\s*(?:path\s*=\s*)?"?:feature:([\w-]+)'),
    re.compile(r"\bprojects\.feature\.(\w+)"),
]
# Any mention of another feature's package, in an import or written out in full.
KOTLIN_FEATURE_PACKAGE = re.compile(r"(?<!package )\bcf\.construct\.mobile\.feature\.(\w+)")

ATTRIBUTE = r"@\w+(?:\([^)\n]*\))?"
# Modifier words and attributes on the declaration line, in any order (`expect`, `final`, `private`,
# `@MainActor`, ...); the access levels that hide the type are looked for in them afterwards.
KOTLIN_TYPE = (
    rf"(?P<mods>(?:(?:{ATTRIBUTE}|(?!(?:class|interface|object|typealias)\b)\w+)[ \t]+)*)"
    r"(?:class|interface|object|typealias)\s+(?P<name>\w+)"
)
SWIFT_TYPE = (
    rf"(?P<mods>(?:(?:{ATTRIBUTE}|(?!(?:struct|class|enum|protocol|actor|typealias)\b)\w+)[ \t]+)*)"
    r"(?:struct|class|enum|protocol|actor|typealias)\s+(?P<name>\w+)"
)
# Access levels that hide a type from Swift. `internal` is Swift's own default, so it stays visible there.
KOTLIN_HIDDEN = {"private", "internal", "protected"}
SWIFT_HIDDEN = {"private", "fileprivate"}
# Declarations start their line: at column 0 for a top-level type, indented for a nested one.
KOTLIN_DECLARATION = re.compile(rf"^(?P<indent>[ \t]*){KOTLIN_TYPE}", re.M)
SWIFT_DECLARATION = re.compile(rf"^(?P<indent>[ \t]*){SWIFT_TYPE}", re.M)


def declared(pattern: re.Pattern, hidden: set[str], text: str) -> tuple[set[str], set[str], set[str]]:
    """The names in a file of its public top-level types, its nested types and its hidden top-level types."""
    public, nested, private = set(), set(), set()
    for match in pattern.finditer(text):
        if match.group("indent"):
            nested.add(match.group("name"))
        elif hidden & set(match.group("mods").split()):
            private.add(match.group("name"))
        else:
            public.add(match.group("name"))
    return public, nested, private


# Names a screen may not use: the interface and its implementations, holders, stores and models.
STORE_NAME = re.compile(
    r"\b(?!Experimental\w*Api\b)\w*(?:Construct|Holder|Api|Repository|Store|Dao|Database|Model)\b|\bUiScope\b"
)
STORE_IMPORT = re.compile(r"^\s*import\s+cf\.construct\.mobile\.core\.(?:construct|database)\b", re.M)

NUMBER = r"-?\d[\w.]*"
SWIFT_COLOURS = "red|orange|yellow|green|mint|teal|cyan|blue|indigo|purple|pink|brown|white|gray|black"
SWIFT_COLOUR_MODIFIERS = "foregroundColor|foregroundStyle|background|tint|fill|stroke"
SWIFT_SIZE_NAMES = "size|width|height|minWidth|maxWidth|minHeight|maxHeight|spacing|radius|cornerRadius|lineWidth"
KOTLIN_LITERALS = [
    (re.compile(r"\b\d[\d_]*(?:\.\d+)?f?\s*\.\s*(?:dp|sp)\b"), "size literal"),
    (re.compile(r"\b(?:Color|Dp|TextUnit)\s*\("), "colour or size built in the screen"),
    (re.compile(r"\b0x[0-9A-Fa-f]{6,8}\b"), "colour literal"),
    (re.compile(r"\bColor\.(?!Companion)[A-Z]\w*"), "colour literal"),
]
SWIFT_LITERALS = [
    (re.compile(r"\b(?:Color|UIColor|NSColor)\s*\("), "colour built in the screen"),
    (re.compile(r"#colorLiteral"), "colour literal"),
    (re.compile(r"\b(?:Color|UIColor)\.(?!clear\b)[a-z]\w*"), "colour literal"),
    (re.compile(rf"\.(?:{SWIFT_COLOUR_MODIFIERS})\(\s*\.(?:{SWIFT_COLOURS})\b"), "colour literal"),
    (re.compile(rf"\b(?:{SWIFT_SIZE_NAMES})\s*:\s*{NUMBER}"), "size literal"),
    (re.compile(rf"\b(?:CGFloat|CGSize|EdgeInsets)\s*\(\s*(?:\w+:\s*)?{NUMBER}"), "size literal"),
    (re.compile(rf"\.(?:padding|cornerRadius)\(\s*(?:\.\w+\s*,\s*)?{NUMBER}"), "size literal"),
    (re.compile(rf"\.offset\(\s*(?:x|y):\s*{NUMBER}"), "size literal"),
]


def blank(text: str) -> str:
    return "".join("\n" if c == "\n" else " " for c in text)


def skip_block_comment(text: str, i: int) -> int:
    depth = 0
    while i < len(text):
        if text[i : i + 2] == "/*":
            depth, i = depth + 1, i + 2
        elif text[i : i + 2] == "*/":
            depth, i = depth - 1, i + 2
            if depth == 0:
                break
        else:
            i += 1
    return i


def skip_string(text: str, i: int) -> int:
    """The index after the string that starts at `i`: plain, triple-quoted or Swift raw (`#"…"#`)."""
    hashes = len(text[i:]) - len(text[i:].lstrip("#"))
    quote = '"""' if text[i + hashes : i + hashes + 3] == '"""' else '"'
    end = quote + "#" * hashes
    j = i + hashes + len(quote)
    while j < len(text) and text[j : j + len(end)] != end:
        if text[j] == "\n" and len(quote) == 1:
            return j
        j += 2 if text[j] == "\\" and not hashes else 1
    return min(len(text), j + len(end))


CHAR_LITERAL = re.compile(r"'(?:\\.|[^'\\\n])'")
RAW_STRING_START = re.compile(r'#+"')


def strip_comments(text: str, keep_strings: bool = False) -> str:
    """Blanks comments, character literals and, unless `keep_strings`, the inside of strings; line
    breaks stay, so line numbers do."""
    out = []
    i, n = 0, len(text)
    while i < n:
        two = text[i : i + 2]
        char = CHAR_LITERAL.match(text, i)
        if two == "//":
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(blank(text[i:j]))
            i = j
        elif two == "/*":
            j = skip_block_comment(text, i)
            out.append(blank(text[i:j]))
            i = j
        elif char:
            out.append(blank(char.group(0)))
            i = char.end()
        elif text[i] == '"' or RAW_STRING_START.match(text, i):
            j = skip_string(text, i)
            body = text[i:j]
            if keep_strings:
                out.append(body)
            else:
                out.append(blank(body))
            i = j
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def key(name: str) -> str:
    """A feature's name as Kotlin (`shopping_cart`) and Swift (`ShoppingCart`) both write it."""
    return re.sub(r"[_-]", "", name).lower()


class Report:
    def __init__(self, root: Path):
        self.root = root
        self.lines: list[str] = []

    def violation(self, path: Path, line: int, message: str) -> None:
        self.lines.append(f"{path.relative_to(self.root)}:{line}: {message}")

    def required(self, what: str) -> None:
        self.lines.append(f"required: {what}")

    def empty_scan(self, what: str) -> None:
        self.lines.append(f"nothing to scan: {what}")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def kotlin_features(root: Path) -> list[Path]:
    base = root / "feature"
    return sorted(p for p in base.iterdir() if (p / "build.gradle.kts").is_file()) if base.is_dir() else []


def swift_features(root: Path) -> dict[str, list[Path]]:
    base = root / "iosApp" / "Features"
    if not base.is_dir():
        return {}
    return {d.name: sorted(d.rglob("*.swift")) for d in sorted(base.iterdir()) if d.is_dir()}


def check_kotlin_features(root: Path, report: Report) -> None:
    features = kotlin_features(root)
    if not features:
        report.empty_scan("no feature/<name>/build.gradle.kts")
    for feature in features:
        build = feature / "build.gradle.kts"
        text = strip_comments(read(build), keep_strings=True)
        for pattern in GRADLE_FEATURE_DEP:
            for match in pattern.finditer(text):
                if key(match.group(1)) != key(feature.name):
                    message = f"feature '{feature.name}' depends on feature '{match.group(1)}'"
                    report.violation(build, line_of(text, match.start()), message)
        for path in sorted(feature.rglob("*.kt")):
            text = strip_comments(read(path))
            for match in KOTLIN_FEATURE_PACKAGE.finditer(text):
                if key(match.group(1)) != key(feature.name):
                    message = f"feature '{feature.name}' uses feature '{match.group(1)}'"
                    report.violation(path, line_of(text, match.start()), message)


def check_swift_features(root: Path, report: Report) -> None:
    """A Swift feature may not name a public top-level type that another feature declares, in Swift
    or, as ConstructShared shows it, in Kotlin. Matching is by name, because all Swift is one
    module; a nested or hidden type the file declares itself is its own, as bare names go."""
    swift = swift_features(root)
    if not swift:
        report.empty_scan("no iosApp/Features/<Name>")
    texts = {path: strip_comments(read(path)) for files in swift.values() for path in files}
    owners: dict[str, set[str]] = {}
    own_file: dict[Path, set[str]] = {}
    for name, files in swift.items():
        for path in files:
            public, nested, private = declared(SWIFT_DECLARATION, SWIFT_HIDDEN, texts[path])
            own_file[path] = nested | private
            for type_name in public:
                owners.setdefault(type_name, set()).add(key(name))
    kotlin_own: dict[str, set[str]] = {}
    for feature in kotlin_features(root):
        kotlin_own[key(feature.name)] = set()
        for path in feature.rglob("*.kt"):
            public, nested, _ = declared(KOTLIN_DECLARATION, KOTLIN_HIDDEN, strip_comments(read(path)))
            for type_name in public:
                owners.setdefault(type_name, set()).add(key(feature.name))
            # Kotlin/Native shows a nested class to Swift qualified (`Outer.MemoryState`) and this check
            # cannot tell a qualified use from a bare one, so a feature's own nested Kotlin names pass.
            kotlin_own[key(feature.name)] |= nested
    for name, files in swift.items():
        for path in files:
            for type_name, type_owners in sorted(owners.items()):
                if key(name) in type_owners or type_name in own_file[path] | kotlin_own.get(key(name), set()):
                    continue
                match = re.search(rf"\b{re.escape(type_name)}\b", texts[path])
                if match:
                    message = f"feature '{name}' uses '{type_name}' of feature '{sorted(type_owners)[0]}'"
                    report.violation(path, line_of(texts[path], match.start()), message)


def find_screens(root: Path) -> tuple[list[tuple[Path, bool]], list[str]]:
    """Every file with `Screen` in its name (Kotlin: True), and the folders that have none: each feature
    folder that exists on a side needs one there."""
    found, missing = [], []
    for feature in kotlin_features(root):
        files = sorted(feature.glob("src/*Main/**/*Screen*.kt"))
        found += [(p, True) for p in files]
        if not files:
            missing.append(f"feature/{feature.name}/src/*Main needs a *Screen*.kt")
    for name, files in swift_features(root).items():
        screens = [p for p in files if "Screen" in p.name]
        found += [(p, False) for p in screens]
        if not screens:
            missing.append(f"iosApp/Features/{name} needs a *Screen*.swift")
    return found, missing


def check_screens(root: Path, report: Report) -> None:
    found, missing = find_screens(root)
    for line in missing:
        report.required(line)
    for path, kotlin in found:
        text = strip_comments(read(path))
        for match in STORE_NAME.finditer(text):
            message = f"a screen names the store or Construct type '{match.group(0)}'"
            report.violation(path, line_of(text, match.start()), message)
        if kotlin:
            for match in STORE_IMPORT.finditer(text):
                report.violation(path, line_of(text, match.start()), "a screen imports a store or Construct type")
        for pattern, label in KOTLIN_LITERALS if kotlin else SWIFT_LITERALS:
            for match in pattern.finditer(text):
                message = f"a screen has a {label}: '{match.group(0).strip()}'"
                report.violation(path, line_of(text, match.start()), message)


def run(root: Path) -> list[str]:
    report = Report(root)
    check_kotlin_features(root, report)
    check_swift_features(root, report)
    check_screens(root, report)
    return report.lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    lines = run(args.root.resolve())
    for line in lines:
        print(line)
    if lines:
        print(f"{len(lines)} structure problem(s)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
