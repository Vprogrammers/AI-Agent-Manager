#!/usr/bin/env python3
"""Build a platform-specific AAM engine sidecar for Tauri packaging."""
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TAURI = ROOT / "desktop" / "src-tauri"
ENTRY = ROOT / "src" / "ai_agent_manager.py"
BIN = TAURI / "binaries"

def main():
    triple = subprocess.check_output(["rustc", "-vV"], text=True).split("host: ", 1)[1].splitlines()[0].strip()
    if not triple or "/" in triple or "\\" in triple:
        raise SystemExit("Invalid Rust target triple")
    try:
        import PyInstaller  # noqa: F401
    except ImportError:
        raise SystemExit("PyInstaller is required: python3 -m pip install pyinstaller")
    name = "aam-engine"
    build_dir = ROOT / "desktop" / ".engine-build"
    dist = build_dir / "dist"
    cmd = [sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean",
           "--onefile", "--name", name, "--distpath", str(dist),
           "--workpath", str(build_dir / "work"), "--specpath", str(build_dir),
           "--paths", str(ROOT / "src"), str(ENTRY)]
    subprocess.run(cmd, check=True, cwd=ROOT)
    extension = ".exe" if sys.platform == "win32" else ""
    source = dist / (name + extension)
    BIN.mkdir(parents=True, exist_ok=True)
    destination = BIN / (name + "-" + triple + extension)
    shutil.copy2(source, destination)
    if sys.platform != "win32":
        destination.chmod(destination.stat().st_mode | 0o111)
    print("Bundled engine:", destination)

if __name__ == "__main__":
    main()
