Hash-locked packages installed *inside* each `/opt/compute/kernels/sk-<minor>` venv
(everything else comes from `sci-legacy` via `--system-site-packages`).

Regenerate after changing a version (Python 3.11, glibc 2.39):

    uv pip compile -q --no-deps --generate-hashes --only-binary :all: \
      --python-version 3.11 --python-platform x86_64-manylinux_2_39 in.txt -o sk-<minor>.txt

`in.txt` = `pip list --path <venv>/lib/python3.11/site-packages --format=freeze` minus pip/setuptools.
