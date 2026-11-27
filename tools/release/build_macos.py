#!/usr/bin/env python3
"""Build an offline, relocatable Apple Silicon AISLE Studio disk image."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.request
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
AISLE_REV = "dbd34a71b6f82a011541b433306ca7e6a705e823"
FFMPEG_SHA = "6136812ea6d4e68bdba27e33c2a94382711cdf4f8602ffef056ff792bd6f9818"
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def run(*args, **kw):
    subprocess.run([str(x) for x in args], check=True, **kw)


def output(*args):
    return subprocess.check_output([str(x) for x in args], text=True).strip()


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def snapshot(repo, revision, target, *paths):
    target.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryFile() as archive:
        run("git", "-C", repo, "archive", revision, *paths, stdout=archive)
        archive.seek(0)
        with tarfile.open(fileobj=archive) as files:
            files.extractall(target, filter="data")


def ffmpeg(build):
    archive = build / "ffmpeg-8.0.3.tar.xz"
    if not archive.is_file() or sha(archive) != FFMPEG_SHA:
        urllib.request.urlretrieve(
            "https://ffmpeg.org/releases/ffmpeg-8.0.3.tar.xz", archive
        )
    if sha(archive) != FFMPEG_SHA:
        raise ValueError("Incorrect FFmpeg source archive")
    prefix = build / "ffmpeg"
    if not (prefix / "bin/ffmpeg").exists():
        with tarfile.open(archive) as files:
            files.extractall(build, filter="data")
        run("sh", HERE / "build_ffmpeg.sh", prefix, cwd=build / "ffmpeg-8.0.3")
    return prefix, archive


def copy_media(exe, res, libs, prefix):
    """Follow each FFmpeg load command, rewriting it relative to its consumer."""
    tools = res / "runtime/bin"
    tools.mkdir(parents=True)
    pending = [exe]
    for name in ("ffmpeg", "ffprobe"):
        pending.append(Path(shutil.copy2(prefix / "bin" / name, tools / name)))
    while pending:
        item = pending.pop()
        flags = []
        for line in output("otool", "-L", item).splitlines()[1:]:
            dep = line.strip().split(" (compatibility")[0]
            if dep.startswith(("/System/Library/", "/usr/lib/", "@")):
                continue
            src = Path(dep)
            if src.parent.resolve() != (prefix / "lib").resolve():
                raise ValueError(
                    f"Unexpected external native dependency {item.name}: {dep}"
                )
            dest = libs / src.name
            if dest == item:
                flags.extend(["-id", "@rpath/" + item.name])
                continue
            if not dest.exists():
                shutil.copy2(src, dest)
                dest.chmod(0o755)
                pending.append(dest)
            flags.extend(
                ["-change", dep, "@loader_path/" + os.path.relpath(dest, item.parent)]
            )
        if flags:
            run("install_name_tool", *flags, item)


def python_runtime(res, aisle):
    # uv's managed interpreter is relocatable. A venv points back to its builder.
    interpreter = Path(output("uv", "python", "find", "--managed-python", "3.13.5"))
    base = Path(output(interpreter, "-c", "import sys; print(sys.base_prefix)"))
    dest = res / "runtime/python"
    ignore = shutil.ignore_patterns(
        "__pycache__", "*.pyc", "site-packages", ".DS_Store"
    )
    shutil.copytree(base / "lib", dest / "lib", ignore=ignore, symlinks=True)
    (dest / "bin").mkdir()
    shutil.copy2(interpreter.resolve(), dest / "bin/python3.13")
    (dest / "bin/python3").symlink_to("python3.13")
    site = dest / "lib/python3.13/site-packages"
    run(
        "uv",
        "pip",
        "install",
        "--python",
        dest / "bin/python3",
        "--target",
        site,
        "--only-binary",
        ":all:",
        "--require-hashes",
        "--link-mode",
        "copy",
        "-r",
        HERE / "requirements.lock",
    )
    # The pinned source tree contains the harness configuration and the simulator assets.
    (site / "aisle").symlink_to(
        os.path.relpath(aisle / "src/aisle", site), target_is_directory=True
    )
    # Install AISLE distribution metadata from its upstream wheel.
    with tempfile.TemporaryDirectory() as tmp:
        run("uv", "build", "--wheel", "--out-dir", tmp, aisle)
        import zipfile

        with zipfile.ZipFile(next(Path(tmp).glob("aisle-*.whl"))) as wheel:
            for name in wheel.namelist():
                if ".dist-info/" in name:
                    wheel.extract(name, site)
    # These generated console scripts have staging-path shebangs. Studio uses modules.
    if (site / "bin").exists():
        shutil.rmtree(site / "bin")
    harness = res / "runtime/bin/harness"
    harness.write_text(
        '#!/bin/sh\nexec "$(dirname "$0")/../python/bin/python3" -B -s -m aisle.harness.cli "$@"\n'
    )
    harness.chmod(0o755)


def resources(res, meta):
    notices = res / "ThirdPartyNotices"
    notices.mkdir()
    inventory = []
    for p in meta["packages"]:
        name = p["name"].replace("-", "_")
        crate = Path(p["manifest_path"]).parent
        if (crate / "resources").is_dir() and (
            name.startswith("makepad") or crate.is_relative_to(ROOT)
        ):
            shutil.copytree(
                crate / "resources",
                res / "makepad" / name / "resources",
                dirs_exist_ok=True,
                ignore=shutil.ignore_patterns(".DS_Store"),
            )
        inventory.append(
            {k: p.get(k) for k in ("name", "version", "source", "license")}
        )
        for f in list(crate.glob("LICENSE*")) + list(crate.glob("COPYING*")):
            if f.is_file():
                folder = notices / "rust" / (p["name"] + "-" + p["version"])
                folder.mkdir(parents=True, exist_ok=True)
                shutil.copy2(f, folder / f.name)
    (notices / "rust-packages.json").write_text(json.dumps(inventory, indent=2))
    for theme in ("ios", "ios_dark"):
        for phase in ("theme", "roles", "widgets"):
            (res / "makepad" / f"desktop_style_{theme}_{phase}").symlink_to(
                "makepad_widgets"
            )


def native_files(app):
    magic = {bytes.fromhex(s) for s in ("cffaedfe", "feedfacf", "cafebabe", "cafebabf")}
    for f in app.rglob("*"):
        if f.is_symlink():
            if not f.resolve(strict=True).is_relative_to(app):
                raise ValueError(f"External bundle symlink: {f}")
        elif f.is_file():
            with f.open("rb") as stream:
                if stream.read(4) in magic:
                    yield f


def build_app(args, media, archive, repo, image):
    app = image / "AISLE Studio.app"
    contents = app / "Contents"
    res = contents / "Resources"
    exe = contents / "MacOS/aisle-studio"
    libs = contents / "Frameworks"
    for directory in (res, exe.parent, libs):
        directory.mkdir(parents=True, exist_ok=True)
    target = args.build / "target"
    env = os.environ | dict(
        CARGO_TARGET_DIR=str(target),
        MAKEPAD_PACKAGE_DIR="../Resources/makepad",
        MACOSX_DEPLOYMENT_TARGET="14.0",
        PKG_CONFIG_PATH=str(media / "lib/pkgconfig"),
        RUSTFLAGS="-C link-arg=-Wl,-headerpad_max_install_names",
    )
    run(
        "cargo",
        "build",
        "--locked",
        "--release",
        "-p",
        "aisle-studio",
        cwd=ROOT,
        env=env,
    )
    shutil.copy2(target / "release/aisle-studio", exe)
    run("strip", "-x", exe)
    copy_media(exe, res, libs, media)
    studio = res / "studio"
    snapshot(
        ROOT,
        "HEAD",
        studio,
        "data",
        "robots",
        "tools/aisle_workbench",
        "tools/aisle-validation",
        "tools/skill_qualification",
        "tools/nexus_workflow",
        "tools/nano-three-cubes",
    )
    for urdf in (studio / "data").glob("*/*.urdf"):
        for mesh in ET.parse(urdf).iter("mesh"):
            path = urdf.parent / mesh.attrib["filename"]
            if not path.exists():
                cached = ROOT / path.relative_to(studio)
                path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(cached, path)
    aisle = res / "AISLE"
    snapshot(repo, AISLE_REV, aisle)
    (aisle / "REVISION").write_text(AISLE_REV + "\n")
    python_runtime(res, aisle)
    meta = json.loads(
        output(
            "cargo",
            "metadata",
            "--locked",
            "--manifest-path",
            ROOT / "Cargo.toml",
            "--filter-platform",
            "aarch64-apple-darwin",
            "--format-version",
            "1",
        )
    )
    resources(res, meta)
    notices = res / "ThirdPartyNotices"
    shutil.copy2(archive, notices)
    shutil.copy2(HERE / "build_ffmpeg.sh", notices)
    shutil.copy2(HERE / "requirements.lock", notices)
    # Elevate the minimum if any wheel or library was built for a newer macOS.
    minimum = (14, 0)
    binaries = list(native_files(app))
    # Standalone CPython and repaired wheels can retain a build-time LC_ID_DYLIB.
    # Their consumers already load these private libraries through relative paths.
    for binary in binaries:
        if binary.suffix == ".dylib":
            run(
                "install_name_tool",
                "-id",
                "@rpath/" + binary.name,
                binary,
                stderr=subprocess.DEVNULL,
            )
    for start in range(0, len(binaries), 80):
        batch = binaries[start : start + 80]
        info = output("otool", "-arch", "arm64", "-l", *batch)
        for ver in re.findall(r"^\s*minos ([\d.]+)", info, re.M):
            minimum = max(minimum, tuple(int(v) for v in ver.split(".")[:2]))
        for dep in re.findall(r"^\s*name (/[^\n]+?) \(offset ", info, re.M):
            if not dep.startswith(("/usr/lib/", "/System/Library/")):
                raise ValueError(f"External install name: {dep}")
    minos = ".".join(map(str, minimum))
    (res / "release.json").write_text(
        json.dumps(
            dict(
                version=VERSION,
                studio_revision=output("git", "-C", ROOT, "rev-parse", "HEAD"),
                aisle_revision=AISLE_REV,
                nexus="0.2.1",
                python="3.13.5",
                ffmpeg="8.0.3",
                minimum_macos=minos,
                architecture="arm64",
            ),
            indent=2,
        )
        + "\n"
    )
    with (contents / "Info.plist").open("wb") as out:
        plistlib.dump(
            dict(
                CFBundleName="AISLE Studio",
                CFBundleIdentifier="org.dorobot.aisle-studio",
                CFBundleExecutable=exe.name,
                CFBundlePackageType="APPL",
                CFBundleShortVersionString=VERSION.split("-")[0],
                CFBundleVersion=VERSION.split(".")[-1],
                LSMinimumSystemVersion=minos,
                NSHighResolutionCapable=True,
                NSCameraUsageDescription="Access a robot camera selected for calibration.",
            ),
            out,
        )
    for f in binaries:
        run("codesign", "--force", "--sign", "-", f, stderr=subprocess.DEVNULL)
    run("codesign", "--force", "--sign", "-", app)
    run("codesign", "--verify", "--deep", "--strict", app)
    (image / "Applications").symlink_to("/Applications", target_is_directory=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--aisle",
        type=Path,
        help="Existing checkout containing the pinned AISLE revision",
    )
    p.add_argument("--build", type=Path, required=True, help="Reusable build cache")
    p.add_argument(
        "--out", type=Path, required=True, help="Directory for the finished DMG"
    )
    args = p.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        p.error("An Apple Silicon build Mac is required")
    args.build = args.build.resolve()
    args.build.mkdir(parents=True, exist_ok=True)
    args.out.mkdir(parents=True, exist_ok=True)
    repo = args.aisle or args.build / "aisle-repo"
    if args.aisle is None and not repo.exists():
        run(
            "git",
            "clone",
            "--filter=blob:none",
            "https://github.com/moxin-org/AISLE.git",
            repo,
        )
    run("git", "-C", repo, "cat-file", "-e", AISLE_REV + "^{commit}")
    run("uv", "python", "install", "3.13.5")
    run("python3", ROOT / "tools/fetch_robot_meshes.py", "g1")
    media, archive = ffmpeg(args.build)
    target = args.out.resolve() / f"AISLE-Studio-{VERSION}-macOS-arm64.dmg"
    with tempfile.TemporaryDirectory(dir=args.build) as tmp:
        image = Path(tmp) / "image"
        build_app(args, media, archive, repo, image)
        dmg = Path(tmp) / target.name
        run(
            "hdiutil",
            "create",
            "-volname",
            "AISLE Studio",
            "-srcfolder",
            image,
            "-fs",
            "HFS+",
            "-format",
            "UDZO",
            dmg,
        )
        run("hdiutil", "verify", dmg)
        shutil.copy2(dmg, target)
    target.with_suffix(".dmg.sha256").write_text(f"{sha(target)}  {target.name}\n")
    print(target)


if __name__ == "__main__":
    main()
