# Infrastructure Diagrams

This directory contains `python-diagrams` sources for the current Kotatsu backend setup.

Generated diagrams:
- `kotatsu_backend_runtime`: runtime traffic, router exposure, and internal gRPC control flow
- `kotatsu_backend_delivery`: deploy and DDNS update flow

## Prerequisites
- Python 3.7+
- Graphviz available on `PATH`
- Python package: `diagrams`

Quick sanity check with `uv`:
```bash
uv run --with-requirements docs/infra/requirements.txt python3 --version
```

Render both diagrams:
```bash
uv run --with-requirements docs/infra/requirements.txt python3 docs/infra/generate_diagrams.py
```

Render to a custom directory or format:
```bash
uv run --with-requirements docs/infra/requirements.txt python3 docs/infra/generate_diagrams.py --outdir docs/infra/out --formats png,dot
```

If you prefer a persistent local environment instead of `uv`, create a venv first:
```bash
python3 -m venv .venv
source .venv/bin/activate
python3 -m pip install -r docs/infra/requirements.txt
python3 docs/infra/generate_diagrams.py
```
