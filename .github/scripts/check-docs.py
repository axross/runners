"""Run the installed whole-root OKF checker without modifying its managed copy."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / ".claude/skills/living-project-documentation/scripts"
CACHE = ROOT / "node_modules/.cache/runners-docs"
REVISION = "527e81aff55992be21507e97004286789126f88c"
DIGEST = "7f92df7d6a2118a5b7e5c388aa97de48f6ad31b942abc9e2fc7d344830661f49"
URL = (
    "https://raw.githubusercontent.com/open-knowledge-format/open-knowledge-format/"
    f"{REVISION}/skills/okf-generate/scripts/validate.py"
)
REQUIREMENTS = Path(__file__).with_name("okf-requirements.txt").resolve()


def prepare():
    digest = hashlib.sha256()
    digest.update(REQUIREMENTS.read_bytes())
    for value in (DIGEST, sys.version, sys.executable):
        digest.update(value.encode() + b"\0")
    for name in ("package.json", "package-lock.json"):
        digest.update((SCRIPTS / name).read_bytes())
    cache = CACHE / digest.hexdigest()
    if cache.is_dir():
        return cache

    CACHE.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=CACHE) as temporary:
        stage = Path(temporary) / "tools"
        stage.mkdir()
        with urlopen(URL, timeout=60) as response:
            validator = response.read()
        if hashlib.sha256(validator).hexdigest() != DIGEST:
            raise ValueError("downloaded OKF validator has an unexpected SHA-256")
        (stage / "validate.py").write_bytes(validator)
        for name in ("package.json", "package-lock.json"):
            shutil.copyfile(SCRIPTS / name, stage / name)
        npm = shutil.which("npm.cmd" if os.name == "nt" else "npm")
        if npm is None:
            raise FileNotFoundError("npm is required to install the locked OKF parsers")
        subprocess.run(
            [npm, "ci", "--ignore-scripts", "--no-audit", "--no-fund"],
            cwd=stage,
            check=True,
        )
        subprocess.run([sys.executable, "-m", "venv", stage / "venv"], check=True)
        if os.name == "nt":
            python = stage / "venv/Scripts/python.exe"
            shutil.copyfile(python, python.with_name("python3.exe"))
        else:
            python = stage / "venv/bin/python3"
        subprocess.run(
            [python, "-m", "pip", "--disable-pip-version-check", "install",
             "--only-binary=:all:", "--no-deps", "--require-hashes", "-r", REQUIREMENTS],
            check=True,
        )
        try:
            stage.rename(cache)
        except OSError:
            if not cache.is_dir():
                raise
    return cache


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "export", type=Path, nargs="?",
        help="check this standalone bundle instead of the checkout",
    )
    args = parser.parse_args()
    cache = prepare()
    python_bin = cache / "venv" / ("Scripts" if os.name == "nt" else "bin")
    env = dict(os.environ, PATH=str(python_bin) + os.pathsep + os.environ["PATH"])
    # ESM resolves the locked parsers in the parent directory of this private copy.
    with tempfile.TemporaryDirectory(dir=cache) as scripts:
        for source in SCRIPTS.glob("*.mjs"):
            shutil.copyfile(source, Path(scripts) / source.name)
        cmd = ["node", Path(scripts) / "check-docs.mjs", "--validator", cache / "validate.py"]
        if args.export is None:
            cmd.extend(["--source-root", ROOT, ROOT / "docs"])
        else:
            cmd.append(args.export.resolve())
        return subprocess.run(cmd, env=env).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Could not prepare documentation validation: {error}", file=sys.stderr)
        sys.exit(2)
