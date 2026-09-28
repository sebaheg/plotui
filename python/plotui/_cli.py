"""The `plotui` console script: exec the bundled native CLI binary.

Prebuilt wheels carry the compiled CLI at ``plotui/_bin/plotui`` (``.exe``
on Windows); this
entry point replaces the Python process with it, so ``plotui`` on the
command line *is* the native binary. Source builds don't bundle it.
"""

import os
import sys


def main() -> None:
    name = "plotui.exe" if sys.platform == "win32" else "plotui"
    exe = os.path.join(os.path.dirname(__file__), "_bin", name)
    if not os.path.exists(exe):
        sys.stderr.write(
            "plotui: this install does not bundle the CLI (source builds don't).\n"
            "Use a prebuilt wheel, or another install method: https://plotui.xyz/#install\n"
        )
        raise SystemExit(1)
    argv = [exe, *sys.argv[1:]]
    if sys.platform == "win32":
        # Windows has no exec: os.execv would spawn and detach, leaving the
        # console to the shell while the CLI still reads it
        import subprocess

        raise SystemExit(subprocess.call(argv))
    try:
        os.execv(exe, argv)
    except PermissionError:
        os.chmod(exe, 0o755)
        os.execv(exe, argv)
