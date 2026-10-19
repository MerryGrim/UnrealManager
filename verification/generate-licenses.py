"""Complete the locked dependency notices without changing any dependency.

Python 3.11+ standard library only. Crate archives are checked against Cargo.lock.
Missing monorepo license files are read at the crate's recorded upstream commit.
This is release tooling, not an application runtime dependency.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import io
import json
import pathlib
import re
import shutil
import tarfile
import tomllib
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
LICENSE_NAME = re.compile(r"^(?:licen[cs]e|copying|copyright|notice|unlicense|authors)(?:[-_.].*)?$", re.I)
MPL_NAMES = {"cssparser", "cssparser-macros", "dtoa-short", "option-ext", "selectors"}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-home", required=True, type=pathlib.Path)
    parser.add_argument("--evidence", required=True, type=pathlib.Path)
    parser.add_argument("--source-archive", required=True, type=pathlib.Path)
    parser.add_argument("--windows-dependencies", type=pathlib.Path,
                        help="Output of cargo tree --locked --offline --target x86_64-pc-windows-msvc --prefix none --format '{p}'")
    parser.add_argument("--notices-only", action="store_true",
                        help="Preserve the existing JSON and source ZIP; finish standalone nested notices only")
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    upstream_cache = evidence / "upstream"
    upstream_cache.mkdir(exist_ok=True)
    snapshot_path = ROOT / "source/src/licenses.json"
    initial = snapshot_path.read_bytes()
    backup = evidence / "licenses.before.json"
    if not backup.exists():
        backup.write_bytes(initial)
    records = json.loads(initial)
    cargo = tomllib.loads((ROOT / "source/src-tauri/Cargo.lock").read_text("utf-8"))
    locked = {(p["name"], p["version"]): p for p in cargo["package"] if "source" in p}
    actual = {(p["name"], p["version"]) for p in records if p["source"] == "Cargo"}
    if actual != set(locked):
        raise ValueError("Cargo notice inventory differs from Cargo.lock; review explicitly")
    npm = json.loads((ROOT / "source/package-lock.json").read_text("utf-8"))["packages"]
    for record in records:
        if record["source"] == "npm":
            if npm["node_modules/" + record["name"]]["version"] != record["version"]:
                raise ValueError("npm notice version differs from package-lock.json")

    src_roots = list((args.cargo_home / "registry/src").glob("*"))
    cache_roots = list((args.cargo_home / "registry/cache").glob("*"))

    def crate_folder(name: str, version: str) -> pathlib.Path:
        return next(p / f"{name}-{version}" for p in src_roots if (p / f"{name}-{version}").is_dir())

    def archive_bytes(name: str, version: str) -> bytes:
        filename = f"{name}-{version}.crate"
        path = next(p / filename for p in cache_roots if (p / filename).is_file())
        data = path.read_bytes()
        if sha256(data) != locked[(name, version)]["checksum"]:
            raise ValueError("Crate archive checksum mismatch: " + filename)
        return data

    def crate_info(name: str, version: str) -> tuple[dict, dict]:
        data = archive_bytes(name, version)
        prefix = f"{name}-{version}/"
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            metadata = tomllib.loads(archive.extractfile(prefix + "Cargo.toml").read().decode("utf-8"))["package"]
            try:
                vcs = json.loads(archive.extractfile(prefix + ".cargo_vcs_info.json").read())
            except KeyError:
                vcs = {}
        return metadata, vcs

    def crate_file(name: str, version: str, filename: str) -> str:
        with tarfile.open(fileobj=io.BytesIO(archive_bytes(name, version)), mode="r:gz") as archive:
            return archive.extractfile(f"{name}-{version}/{filename}").read().decode("utf-8-sig")

    def fetch(url: str) -> bytes:
        if not url.startswith(("https://api.github.com/", "https://raw.githubusercontent.com/")):
            raise ValueError("Only primary upstream GitHub license fetches are allowed: " + url)
        cached = upstream_cache / (sha256(url.encode("utf-8")) + ".bin")
        if cached.exists():
            return cached.read_bytes()
        request = urllib.request.Request(url, headers={"User-Agent": "UnrealManager-license-notices", "Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(request, timeout=30) as response:
            data = response.read(4 * 1024 * 1024 + 1)
        if len(data) > 4 * 1024 * 1024:
            raise ValueError("License response exceeds limit: " + url)
        cached.write_bytes(data)
        (upstream_cache / (sha256(url.encode("utf-8")) + ".url.txt")).write_text(url + "\n", "utf-8")
        return data

    # Published subcrates omit workspace licenses. These related packages identify
    # the original workspace's immutable revision when the subcrate has no VCS file.
    fallback = {
        "libappindicator-sys": ("tauri-apps/libappindicator-rs", "libappindicator", "0.9.0"),
        "rustls-platform-verifier-android": ("rustls/rustls-platform-verifier", "rustls-platform-verifier", "0.7.0"),
        "winapi-i686-pc-windows-gnu": ("retep998/winapi-rs", "winapi", "0.3.9"),
        "winapi-x86_64-pc-windows-gnu": ("retep998/winapi-rs", "winapi", "0.3.9"),
    }
    missing = []
    groups = {}
    original_missing = sum(not r["texts"] for r in records)
    for record in records:
        name, version = record["name"], record["version"]
        if record["source"] == "npm":
            record["sourceUrl"] = f"https://www.npmjs.com/package/{name}/v/{version}"
            continue
        metadata, vcs = crate_info(name, version)
        record["sourceUrl"] = f"https://crates.io/crates/{name}/{version}"
        record["sourceArchiveUrl"] = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
        record["sourceArchiveSha256"] = locked[(name, version)]["checksum"]
        if metadata.get("authors"):
            record["authors"] = metadata["authors"]
        if metadata.get("repository"):
            record["repository"] = metadata["repository"]
        if record["texts"]:
            continue
        if name == "r-efi":
            # This upstream intentionally puts the complete MIT grant and all
            # author names in AUTHORS rather than a separate LICENSE file.
            record["selectedLicense"] = "MIT"
            record["texts"] = [{"name": "AUTHORS", "text": crate_file(name, version, "AUTHORS"),
                                "sourceUrl": f"https://docs.rs/crate/{name}/{version}/source/AUTHORS"}]
            continue
        if name == "selectors":
            # The crate has MPL headers but omits the license body. MPL-2.0 is
            # invariant; use the full unmodified text also shipped by option-ext.
            record["texts"] = [
                {"name": "MPL-2.0.txt", "text": crate_file("option-ext", "0.2.0", "LICENSE.txt"),
                 "sourceUrl": "https://www.mozilla.org/MPL/2.0/"},
                {"name": "Source-license-notice.txt", "text": "\n".join(crate_file(name, version, "lib.rs").splitlines()[:3]) + "\n",
                 "sourceUrl": f"https://docs.rs/crate/{name}/{version}/source/lib.rs"},
            ]
            continue
        repository = metadata.get("repository", "")
        revision = vcs.get("git", {}).get("sha1")
        if name in fallback:
            repo, sibling_name, sibling_version = fallback[name]
            _, sibling_vcs = crate_info(sibling_name, sibling_version)
            repository = "https://github.com/" + repo
            revision = revision or sibling_vcs["git"]["sha1"]
            record["licenseWorkspacePackage"] = f"{sibling_name}@{sibling_version}"
        match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+?)(?:\.git)?/?", repository)
        if not match or not revision or not re.fullmatch("[a-f0-9]{40}", revision):
            raise ValueError("No immutable primary license source for " + name)
        key = (match[1], revision)
        record["licenseRevision"] = revision
        groups[key] = None
        missing.append((record, key))

    def upstream_licenses(key: tuple[str, str]) -> tuple[tuple[str, str], list[dict]]:
        repository, revision = key
        tree = json.loads(fetch(f"https://api.github.com/repos/{repository}/git/trees/{revision}"))
        licenses = []
        for item in tree["tree"]:
            if item["type"] != "blob" or not LICENSE_NAME.fullmatch(item["path"]):
                continue
            url = f"https://raw.githubusercontent.com/{repository}/{revision}/{item['path']}"
            text = fetch(url).decode("utf-8-sig")
            licenses.append({"name": item["path"], "text": text, "sourceUrl": url})
        if not licenses or not any(len(item["text"].strip()) >= 500 for item in licenses):
            raise ValueError("No root license files in " + str(key))
        return key, licenses

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
        for key, texts in executor.map(upstream_licenses, groups):
            groups[key] = texts
            print("Fetched upstream licenses:", key[0], key[1], flush=True)
    for record, key in missing:
        record["texts"] = groups[key]
    if any(not r["texts"] or any(not t["text"].strip() for t in r["texts"]) for r in records):
        raise ValueError("Unresolved notice texts remain")

    mpl = [r for r in records if r["name"] in MPL_NAMES and r["source"] == "Cargo"]
    if len(mpl) != 5:
        raise ValueError("Unexpected MPL inventory")
    version = json.loads((ROOT / "source/package.json").read_text("utf-8"))["version"]
    archive_path = args.source_archive.resolve()
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    if archive_path.exists() and not args.notices_only:
        raise FileExistsError("Do not overwrite release source evidence: " + str(archive_path))
    archive_url = f"https://github.com/MerryGrim/UnrealManager/releases/download/v{version}/{archive_path.name}"
    source_instructions = [
        f"Unreal Manager {version} - exact third-party MPL source archives",
        "",
        "These are unchanged original crates.io source packages. A .crate is a gzip-compressed tar archive.",
        "Extract a package with: tar -xzf crates/NAME-VERSION.crate",
        "The SHA-256 values below are pinned in the application's Cargo.lock.",
        "Each package retains its upstream copyrights and Mozilla Public License 2.0.",
        "Additional upstream license files are included where a subcrate omitted them.",
        "",
    ]
    if args.notices_only:
        with zipfile.ZipFile(archive_path) as package:
            for record in mpl:
                content = package.read(f"crates/{record['name']}-{record['version']}.crate")
                if sha256(content) != record["sourceArchiveSha256"]:
                    raise ValueError("Existing MPL source archive checksum mismatch")
    else:
        with zipfile.ZipFile(archive_path, "x", compression=zipfile.ZIP_DEFLATED) as package:
            for record in mpl:
                filename = f"{record['name']}-{record['version']}.crate"
                package.writestr("crates/" + filename, archive_bytes(record["name"], record["version"]))
                for text in record["texts"]:
                    package.writestr("licenses/" + filename[:-6] + "/" + text["name"], text["text"])
                source_instructions += [filename, "Source: " + record["sourceArchiveUrl"], "SHA-256: " + record["sourceArchiveSha256"], ""]
            package.writestr("README.txt", "\n".join(source_instructions))

    # Cargo packages can embed separately attributed code below their root.
    # Preserve those original notices too, rather than relying only on SPDX metadata.
    target_keys = set(locked)
    if args.windows_dependencies:
        target_keys = set()
        for line in args.windows_dependencies.read_text("utf-8-sig").splitlines():
            match = re.match(r"^(\S+) v([^\s]+)", line)
            if match and (match[1], match[2]) in locked:
                target_keys.add((match[1], match[2]))
        if not target_keys:
            raise ValueError("No locked packages identified in Windows dependency tree")
    already_included = re.sub(r"\s+", " ", "\n".join(t["text"] for r in records for t in r["texts"])).strip()
    supplemental = []
    supplemental_seen = set()
    nested_files_checked = 0
    for name, version in sorted(target_keys):
        prefix = f"{name}-{version}/"
        with tarfile.open(fileobj=io.BytesIO(archive_bytes(name, version)), mode="r:gz") as archive:
            for member in archive.getmembers():
                filename = pathlib.PurePosixPath(member.name).name
                if not member.isfile() or not LICENSE_NAME.fullmatch(filename):
                    continue
                if pathlib.PurePosixPath(filename).suffix.lower() in {".rs", ".h", ".c", ".py", ".toml"}:
                    continue
                nested_files_checked += 1
                try:
                    text = archive.extractfile(member).read().decode("utf-8-sig")
                except UnicodeError:
                    continue
                normalized = re.sub(r"\s+", " ", text).strip()
                if not normalized or normalized in already_included or normalized in supplemental_seen:
                    continue
                supplemental_seen.add(normalized)
                supplemental.append({"name": name, "version": version,
                                     "path": member.name[len(prefix):], "text": text,
                                     "sourceUrl": f"https://static.crates.io/crates/{name}/{name}-{version}.crate"})

    lines = [
        f"Unreal Manager {version} - Third-Party Licenses and Source Notices",
        "",
        "Original Unreal Manager code is MIT licensed; the following components retain their own licenses.",
        "This overinclusive inventory includes build-time and non-Windows packages from the locked dependency graph.",
        "Entries do not imply that every listed component is included in the Windows executable.",
        "License texts and copyright notices below are preserved from original package or upstream workspace files.",
        "",
        "MPL-2.0 source availability",
        "Unmodified source packages for cssparser, cssparser-macros, dtoa-short, option-ext and selectors",
        "are provided in the accompanying release asset: " + archive_path.name,
        archive_url,
        "Each component entry also identifies its exact source version and download URL.",
        "These source packages remain under MPL-2.0; no rights under that license are restricted by the application license.",
        "",
    ]
    for record in records:
        lines += ["=" * 80, f"{record['name']} {record['version']} ({record['source']})", "License: " + record["license"], "Source: " + record["sourceUrl"]]
        if record.get("repository"):
            lines.append("Upstream: " + record["repository"])
        if record.get("sourceArchiveUrl"):
            lines += ["Exact source archive: " + record["sourceArchiveUrl"], "Source SHA-256: " + record["sourceArchiveSha256"]]
        if record.get("authors"):
            lines.append("Package authors: " + "; ".join(record["authors"]))
        if record.get("licenseWorkspacePackage"):
            lines.append("Workspace license source package: " + record["licenseWorkspacePackage"])
        if record.get("selectedLicense"):
            lines.append("Distributed under this available license option: " + record["selectedLicense"])
        for text in record["texts"]:
            lines += ["", "--- " + text["name"] + " ---"]
            if text.get("sourceUrl"):
                lines.append("Original license source: " + text["sourceUrl"])
            lines += [text["text"].rstrip(), ""]
    lines += ["=" * 80, "Additional original notices embedded inside dependency source packages", "",
              "The following verbatim notices supplement the package-root license inventory above.",
              "Repeated identical license bodies are included once.", ""]
    for item in supplemental:
        lines += ["=" * 80, f"{item['name']} {item['version']} - {item['path']}",
                  "Exact original source archive: " + item["sourceUrl"], "", item["text"].rstrip(), ""]
    notices = ROOT / "source/installer/THIRD-PARTY-LICENSES.txt"
    notices.write_text("\n".join(lines) + "\n", "utf-8")
    snapshot_bytes = (json.dumps(records, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    if args.notices_only and records != json.loads(initial):
        raise ValueError("Notices-only mode must not change embedded JSON")
    if not args.notices_only:
        snapshot_path.write_bytes(snapshot_bytes)
    report = {"components": len(records), "cargoComponents": len(locked), "completedMissingEntries": original_missing, "missingTexts": 0,
              "snapshotSha256Before": sha256(initial), "snapshotSha256After": sha256(snapshot_path.read_bytes()),
              "notices": str(notices), "noticesSha256": sha256(notices.read_bytes()),
              "mplSourceArchive": str(archive_path), "mplSourceArchiveSha256": sha256(archive_path.read_bytes()),
              "mplPackages": [{k: r[k] for k in ("name", "version", "sourceArchiveUrl", "sourceArchiveSha256")} for r in mpl],
              "nestedAuditPackages": len(target_keys), "nestedNoticeFilesChecked": nested_files_checked,
              "supplementalNotices": [{k: item[k] for k in ("name", "version", "path", "sourceUrl")} for item in supplemental]}
    report_name = "license-notices-final-report.json" if args.notices_only else "license-generation-report.json"
    with (evidence / report_name).open("x", encoding="utf-8") as report_file:
        report_file.write(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2), flush=True)


if __name__ == "__main__":
    main()
