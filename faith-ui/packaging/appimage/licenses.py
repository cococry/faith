#!/usr/bin/env python3
import json
import pathlib
import shutil
import subprocess
import sys

output = pathlib.Path(sys.argv[1]) / "rust"
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--format-version", "1",
    "--no-default-features", "--features", "appimage", "--filter-platform", "x86_64-unknown-linux-gnu",
]))
packages = []

for package in metadata["packages"]:
    name = f'{package["name"]}-{package["version"]}'
    root = pathlib.Path(package["manifest_path"]).parent
    notices = set()

    for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "UNLICENSE*"):
        notices.update(path for path in root.glob(pattern) if path.is_file())

    if package.get("license_file"):
        notices.add(root / package["license_file"])

    if package["name"] == "freetype-sys":
        for path in ("freetype2/LICENSE.TXT", "freetype2/docs/FTL.TXT", "freetype2/src/gzip/zlib/README", "libpng/LICENSE"):
            notice = root / path

            if notice.is_file():
                notices.add(notice)

    destination = output / name
    destination.mkdir(parents=True, exist_ok=True)

    for notice in notices:
        relative = notice.relative_to(root)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(notice, target)

    packages.append({"package": name, "license": package.get("license")})

output.mkdir(parents=True, exist_ok=True)
(output / "packages.json").write_text(json.dumps(packages, indent=2) + "\n")
