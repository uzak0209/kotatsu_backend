#!/usr/bin/env python3

from __future__ import annotations

import argparse
import sys
from pathlib import Path

try:
    from diagrams import Cluster, Diagram, Edge
    from diagrams.generic.network import Router
    from diagrams.onprem.ci import GithubActions
    from diagrams.onprem.client import Client, Users
    from diagrams.onprem.compute import Server
    from diagrams.onprem.container import Docker
    from diagrams.onprem.network import Internet
    from diagrams.onprem.vcs import Github
    from diagrams.programming.language import Bash, Rust
    from diagrams.saas.cdn import Cloudflare
except ModuleNotFoundError as exc:
    if exc.name == "diagrams":
        print(
            "Missing Python package 'diagrams'.\n"
            "Run via uv instead:\n"
            "  uv run --with-requirements docs/infra/requirements.txt "
            "python3 docs/infra/generate_diagrams.py",
            file=sys.stderr,
        )
        raise SystemExit(1) from exc
    raise


BASE_DIR = Path(__file__).resolve().parent
DEFAULT_OUTPUT_DIR = BASE_DIR / "out"
DEFAULT_FORMATS = ("png", "dot")

GRAPH_ATTR = {
    "bgcolor": "white",
    "pad": "0.4",
    "ranksep": "1.1",
    "nodesep": "0.7",
    "splines": "spline",
    "fontname": "Helvetica",
}

NODE_ATTR = {
    "fontname": "Helvetica",
}

EDGE_ATTR = {
    "fontname": "Helvetica",
    "fontsize": "10",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render Kotatsu backend infrastructure diagrams."
    )
    parser.add_argument(
        "--outdir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help=f"Output directory for rendered diagrams (default: {DEFAULT_OUTPUT_DIR})",
    )
    parser.add_argument(
        "--formats",
        default=",".join(DEFAULT_FORMATS),
        help="Comma-separated output formats supported by diagrams, such as png,dot.",
    )
    return parser.parse_args()


def normalize_formats(raw_formats: str) -> list[str]:
    formats = [fmt.strip() for fmt in raw_formats.split(",") if fmt.strip()]
    if not formats:
        raise ValueError("At least one output format is required.")
    return formats


def render_runtime_diagram(output_dir: Path, outformats: list[str]) -> None:
    with Diagram(
        name="Kotatsu Backend Runtime",
        filename=str(output_dir / "kotatsu_backend_runtime"),
        direction="LR",
        show=False,
        outformat=outformats,
        graph_attr=GRAPH_ATTR,
        node_attr=NODE_ATTR,
        edge_attr=EDGE_ATTR,
    ):
        players = Users("Players")
        cloudflare = Cloudflare("Cloudflare DNS")
        internet = Internet("Internet")
        router = Router("Home router")
        ddns = Bash("cloudflare-ddns-update.sh")

        with Cluster("Home server"):
            host = Server("Self-hosted machine")
            with Cluster("Container runtime"):
                runtime = Docker("Docker Compose\nor Podman")
                api = Rust("api-server\nHTTP matchmaking\nTCP 8080")
                realtime = Rust("realtime-server\nUDP realtime\nUDP 4433")

        players >> Edge(label="Resolve PUBLIC_HOSTNAME") >> cloudflare
        ddns >> Edge(label="Update A record") >> cloudflare

        players >> Edge(label="HTTP matchmaking\nTCP 8080") >> internet
        players >> Edge(label="Realtime gameplay\nUDP 4433") >> internet
        internet >> router

        router >> Edge(label="Port forward 8080/tcp") >> api
        router >> Edge(label="Port forward 4433/udp") >> realtime

        host >> runtime >> [api, realtime]
        api >> Edge(label="ControlPlane gRPC\nTCP 50051 (internal)") >> realtime


def render_delivery_diagram(output_dir: Path, outformats: list[str]) -> None:
    with Diagram(
        name="Kotatsu Backend Delivery",
        filename=str(output_dir / "kotatsu_backend_delivery"),
        direction="LR",
        show=False,
        outformat=outformats,
        graph_attr=GRAPH_ATTR,
        node_attr=NODE_ATTR,
        edge_attr=EDGE_ATTR,
    ):
        maintainer = Client("Maintainer")
        repo = Github("GitHub repo")
        actions = GithubActions("deploy-home.yml")
        manual_deploy = Bash("just deploy-home\nscripts/deploy-home.sh")
        ddns = Bash("cloudflare-ddns-update.sh")
        cloudflare = Cloudflare("Cloudflare DNS")

        with Cluster("Home server"):
            host = Server("Self-hosted machine")
            runtime = Docker("Docker Compose\nor Podman")
            api = Rust("api-server")
            realtime = Rust("realtime-server")

        maintainer >> Edge(label="git push to main") >> repo
        repo >> Edge(label="triggers workflow") >> actions
        actions >> Edge(label="SSH deploy\n.env.selfhost") >> host

        maintainer >> Edge(label="manual deploy") >> manual_deploy
        manual_deploy >> Edge(label="SSH deploy") >> host

        maintainer >> Edge(label="run periodically") >> ddns
        ddns >> Edge(label="Update DNS record") >> cloudflare

        host >> runtime >> [api, realtime]


def main() -> None:
    args = parse_args()
    outformats = normalize_formats(args.formats)
    args.outdir.mkdir(parents=True, exist_ok=True)

    render_runtime_diagram(args.outdir, outformats)
    render_delivery_diagram(args.outdir, outformats)


if __name__ == "__main__":
    main()
