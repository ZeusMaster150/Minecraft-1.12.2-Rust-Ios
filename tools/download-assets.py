#!/usr/bin/env python3
"""Download the official Minecraft Java Edition 1.12.2 asset tree.

The client does not ship Mojang assets. This script rebuilds the logical
`assets/minecraft/...` layout directly from Mojang's public download CDN, the
same files the official launcher fetches from its asset index:

    version manifest -> 1.12.2 version json -> asset index (1.12)
                     -> resources.download.minecraft.net/<xx>/<sha1>

You should own Minecraft: Java Edition. Nothing here bypasses authentication:
these are the public resource files, no account data is involved.

Usage (Termux / Linux / macOS):

    python3 tools/download-assets.py                     # -> runtime/assets
    python3 tools/download-assets.py --zip /sdcard/Download/assets.zip
    python3 tools/download-assets.py --threads 16 --verify-only

On iOS the result must end up in the app sandbox as
`Documents/assets/minecraft/...` (see docs/ios.md).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import threading
import time
import urllib.error
import urllib.request
import zipfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
RESOURCE_BASE = "https://resources.download.minecraft.net"
VERSION_ID = "1.12.2"
USER_AGENT = "mc112-rust-asset-fetcher/1.0"

_print_lock = threading.Lock()


def log(message: str) -> None:
    with _print_lock:
        print(message, flush=True)


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def fetch_bytes(url: str, attempts: int = 4) -> bytes:
    last_error: Exception | None = None
    for attempt in range(attempts):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=120) as response:
                return response.read()
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as error:
            last_error = error
            time.sleep(1.5 * (attempt + 1))
    raise RuntimeError(f"failed to download {url}: {last_error}")


def sha1_of(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def resolve_asset_index() -> dict:
    manifest = fetch_json(MANIFEST_URL)
    entry = next(
        (version for version in manifest["versions"] if version["id"] == VERSION_ID),
        None,
    )
    if entry is None:
        raise SystemExit(f"version {VERSION_ID} is not in the Mojang version manifest")
    version = fetch_json(entry["url"])
    index_meta = version["assetIndex"]
    log(f"asset index: {index_meta['id']} ({index_meta['totalSize']} bytes of objects)")
    return fetch_json(index_meta["url"])["objects"]


def download_object(name: str, digest: str, root: Path) -> tuple[str, int, bool]:
    """Returns (name, bytes, downloaded). Existing, verified files are skipped."""
    target = root / name
    if target.is_file() and sha1_of(target) == digest:
        return name, 0, False

    payload = fetch_bytes(f"{RESOURCE_BASE}/{digest[:2]}/{digest}")
    actual = hashlib.sha1(payload).hexdigest()
    if actual != digest:
        raise RuntimeError(f"hash mismatch for {name}: {actual} != {digest}")

    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_suffix(target.suffix + ".part")
    temporary.write_bytes(payload)
    os.replace(temporary, target)
    return name, len(payload), True


def human(size: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if size < 1024:
            return f"{size:.1f} {unit}"
        size /= 1024
    return f"{size:.1f} TB"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--destination",
        type=Path,
        default=Path("runtime/assets"),
        help="output directory (default: runtime/assets)",
    )
    parser.add_argument("--threads", type=int, default=12)
    parser.add_argument("--limit", type=int, default=0, help="only the first N objects (testing)")
    parser.add_argument("--verify-only", action="store_true", help="check existing files, download nothing")
    parser.add_argument("--zip", type=Path, default=None, help="also write a ZIP of the result")
    arguments = parser.parse_args()

    objects = resolve_asset_index()
    names = sorted(objects)
    if arguments.limit:
        names = names[: arguments.limit]
    log(f"objects: {len(names)} -> {arguments.destination}")

    destination = arguments.destination
    destination.mkdir(parents=True, exist_ok=True)

    downloaded = skipped = failed = 0
    total_bytes = 0
    started = time.monotonic()

    def report(done: int) -> None:
        elapsed = max(time.monotonic() - started, 0.001)
        log(
            f"  {done}/{len(names)}  {human(total_bytes)}  "
            f"{total_bytes / elapsed / 1024:.0f} KB/s"
        )

    if arguments.verify_only:
        for index, name in enumerate(names, 1):
            if sha1_of(destination / name) == objects[name]["hash"]:
                passed = True
            else:
                passed = False
            if not passed:
                log(f"  MISSING/BAD {name}")
                failed += 1
            if index % 2000 == 0:
                report(index)
        log(f"verify: {len(names) - failed} ok, {failed} bad")
        return 0 if failed == 0 else 1

    with ThreadPoolExecutor(max_workers=max(arguments.threads, 1)) as pool:
        futures = {
            pool.submit(download_object, name, objects[name]["hash"], destination): name
            for name in names
        }
        for index, future in enumerate(as_completed(futures), 1):
            name = futures[future]
            try:
                _, size, fetched = future.result()
            except Exception as error:  # noqa: BLE001 - report and continue
                failed += 1
                log(f"  FAILED {name}: {error}")
                continue
            if fetched:
                downloaded += 1
                total_bytes += size
            else:
                skipped += 1
            if index % 500 == 0 or index == len(names):
                report(index)

    log(
        f"done: {downloaded} downloaded, {skipped} already present, {failed} failed "
        f"in {time.monotonic() - started:.0f}s, {human(total_bytes)} transferred"
    )

    required = [
        "minecraft/lang/en_us.lang",
        "minecraft/textures/gui/title/minecraft.png",
        "minecraft/textures/gui/widgets.png",
        "minecraft/textures/font/ascii.png",
    ]
    missing = [name for name in required if not (destination / name).is_file()]
    if missing:
        log(f"WARNING: required client assets missing: {', '.join(missing)}")

    if arguments.zip is not None:
        arguments.zip.parent.mkdir(parents=True, exist_ok=True)
        log(f"writing {arguments.zip}")
        with zipfile.ZipFile(arguments.zip, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for path in sorted(destination.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(destination.parent))

    return 0 if failed == 0 and not missing else 1


if __name__ == "__main__":
    sys.exit(main())
