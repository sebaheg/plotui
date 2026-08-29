"""The `plotui` console script: exec the bundled native CLI binary.

Prebuilt wheels carry the compiled CLI at ``plotui/_bin/plotui``; this
entry point replaces the Python process with it, so ``plotui`` on the
command line *is* the native binary. Source builds don't bundle it.
"""

import os
import sys


def main() -> None:
    exe = os.path.join(os.path.dirname(__file__), "_bin", "plotui")
    if not os.path.exists(exe):
        sys.stderr.write(
            "plotui: this install does not bundle the CLI (source builds don't).\n"
            "Use a prebuilt wheel, or another install method: https://plotui.xyz/#install\n"
        )
        raise SystemExit(1)
    argv = [exe, *sys.argv[1:]]
    try:
        os.execv(exe, argv)
    except PermissionError:
        os.chmod(exe, 0o755)
        os.execv(exe, argv)
