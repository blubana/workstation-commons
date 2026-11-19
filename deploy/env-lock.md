# Shared conda environments

The CUDA 12.8 PyTorch stack is pinned in [install-ds-stack.sh](install-ds-stack.sh).
Conda packages and the remaining pip packages are resolved at installation time.
Record each deployed environment with `conda list --explicit` and
`python -m pip freeze` for change tracking. Reproducing the complete stack
requires an explicit conda lock and hash-pinned wheel requirements for the target platform.
