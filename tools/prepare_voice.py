"""Extract only the runtime voice files from the supplied Speech2Go installer.

Usage: python3 tools/prepare_voice.py [path/to/247_S2G_Maxim.exe]
Requires innoextract. Existing voice files are never overwritten.
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNTIME = (
    Path("app/vox/lang_ru_ru.dat"),
    Path("app/vox/vox_ru_maxim22i"),
    Path("app/x64/voice_ru_maxim.dll"),
)


def main() -> None:
    if len(sys.argv) > 2:
        raise SystemExit(__doc__)
    installer = Path(sys.argv[1] if len(sys.argv) == 2 else ROOT / "sources/247_S2G_Maxim.exe")
    if not installer.is_file():
        raise SystemExit(f"installer not found: {installer}")
    destination = ROOT / "extracted"
    present = [destination / path for path in RUNTIME if (destination / path).exists()]
    if len(present) == len(RUNTIME):
        print(f"voice files already present in {destination}")
        return
    if present:
        names = ", ".join(str(path) for path in present)
        raise SystemExit(f"partial voice installation; refusing to overwrite: {names}")
    extractor = shutil.which("innoextract")
    if extractor is None:
        raise SystemExit("innoextract is required (brew install innoextract / apt install innoextract)")
    with tempfile.TemporaryDirectory(prefix="maxim-voice-") as temp:
        command = [extractor, "--silent", "--output-dir", temp]
        for path in RUNTIME:
            command.extend(("--include", str(path)))
        subprocess.run([*command, str(installer.resolve())], check=True)
        for path in RUNTIME:
            source = Path(temp) / path
            if not source.is_file() or source.stat().st_size == 0:
                raise SystemExit(f"installer did not contain {path}")
        for path in RUNTIME:
            target = destination / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(Path(temp) / path), target)
    print(f"voice files ready in {destination}")


if __name__ == "__main__":
    main()
