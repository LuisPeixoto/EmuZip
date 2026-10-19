#!/usr/bin/env python3
"""Validate EmuZip translation catalogs.

Checks that:
  * every string used in the code (@tr / tr!) exists in the .pot template;
  * every catalog only contains msgids that exist in the template;
  * placeholders ({}) match between msgid and msgstr.

Untranslated entries (empty msgstr) are allowed: they fall back to English.
"""
import glob
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(__file__), "..", "..", "crates", "emuzip-app")
Q = r'"((?:[^"\\]|\\.)*)"'


def parse(path):
    """Return {(msgctxt, msgid): msgstr} for a .po/.pot file."""
    entries, ctx, mid, mst, cur = {}, None, "", "", None

    def flush():
        nonlocal ctx, mid, mst
        if mid:
            entries[(ctx, mid)] = mst
        ctx, mid, mst = None, "", ""

    for line in open(path, encoding="utf-8").read().splitlines() + [""]:
        line = line.strip()
        if line.startswith("msgctxt "):
            flush()
            ctx, cur = re.match(r"msgctxt " + Q, line)[1], "ctx"
        elif line.startswith("msgid "):
            if cur != "ctx":
                flush()
            mid, cur = re.match(r"msgid " + Q, line)[1], "id"
        elif line.startswith("msgstr "):
            mst, cur = re.match(r"msgstr " + Q, line)[1], "str"
        elif line.startswith('"'):
            part = re.match(Q, line)[1]
            if cur == "ctx":
                ctx += part
            elif cur == "id":
                mid += part
            elif cur == "str":
                mst += part
        elif not line:
            flush()
            cur = None
    return entries


def used_in_code():
    keys = set()
    for path in glob.glob(f"{ROOT}/ui/*.slint"):
        for m in re.finditer(r"@tr\((?:" + Q + r" => )?" + Q, open(path, encoding="utf-8").read()):
            keys.add((m[1], m[2]))
    for path in glob.glob(f"{ROOT}/src/*.rs"):
        if path.endswith("i18n.rs"):  # runtime itself: only docs and tests
            continue
        for m in re.finditer(r"(?<![A-Za-z_])tr!\(\s*(?:" + Q + r" => )?" + Q, open(path, encoding="utf-8").read()):
            keys.add((m[1], m[2]))
    return keys


def main():
    errors = []
    template = set(parse(f"{ROOT}/i18n/emuzip-app.pot"))
    for key in sorted(used_in_code() - template, key=str):
        errors.append(f"used in code but missing from emuzip-app.pot: {key}")

    for po in sorted(glob.glob(f"{ROOT}/translations/*/LC_MESSAGES/emuzip-app.po")):
        lang = po.split(os.sep)[-3]
        entries = parse(po)
        for (ctx, mid), mst in entries.items():
            if (ctx, mid) not in template:
                errors.append(f"[{lang}] unknown msgid (not in template): {mid!r}")
            if mst and mid.count("{}") != mst.count("{}"):
                errors.append(f"[{lang}] placeholder mismatch: {mid!r} -> {mst!r}")
        done = sum(1 for v in entries.values() if v)
        print(f"{lang:>6}: {done}/{len(template)} translated")

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("translations OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
