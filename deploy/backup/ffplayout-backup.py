#!/usr/bin/env python3
"""Backup do ffplayout: banco, programações (playlists) e pastas escolhidas do storage.

O banco é copiado com a API de backup do SQLite, que é segura com o
ffplayout rodando. O resultado é um .tar.gz por execução, e os mais
antigos além de --keep são apagados.

Uso:
  ffplayout-backup.py --db /var/lib/ffplayout/ffplayout.db \\
      --playlists /var/lib/ffplayout/playlists \\
      --storage /var/lib/ffplayout/storage --storage-dirs commercials,marca \\
      --dest /var/backups/ffplayout --keep 14

Só usa a biblioteca padrão do Python.
"""

import argparse
import datetime
import sqlite3
import sys
import tarfile
import tempfile
from pathlib import Path

PREFIX = "ffplayout-"
SUFFIX = ".tar.gz"


def copy_database(source: Path, target: Path) -> None:
    """Copy a live database consistently and check the copy."""
    with sqlite3.connect(f"file:{source}?mode=ro", uri=True) as origin:
        with sqlite3.connect(target) as copy:
            origin.backup(copy)

    with sqlite3.connect(target) as copy:
        result = copy.execute("PRAGMA integrity_check").fetchone()[0]

    if result != "ok":
        raise RuntimeError(f"a copia do banco esta corrompida: {result}")


def make_backup(args: argparse.Namespace) -> Path:
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    destination = Path(args.dest)
    destination.mkdir(parents=True, exist_ok=True)
    archive = destination / f"{PREFIX}{stamp}{SUFFIX}"
    partial = archive.with_suffix(archive.suffix + ".partial")

    with tempfile.TemporaryDirectory() as work:
        database_copy = Path(work) / "ffplayout.db"

        copy_database(Path(args.db), database_copy)

        with tarfile.open(partial, "w:gz") as tar:
            tar.add(database_copy, arcname="ffplayout.db")

            if args.playlists and Path(args.playlists).is_dir():
                tar.add(args.playlists, arcname="playlists")

            for name in filter(None, (args.storage_dirs or "").split(",")):
                folder = Path(args.storage) / name.strip()

                if folder.is_dir():
                    tar.add(folder, arcname=f"storage/{name.strip()}")

    # A backup is only kept when the archive reads back completely.
    with tarfile.open(partial, "r:gz") as tar:
        names = tar.getnames()

    if "ffplayout.db" not in names:
        raise RuntimeError("o arquivo de backup ficou sem o banco")

    partial.rename(archive)

    return archive


def prune(destination: Path, keep: int) -> list:
    backups = sorted(destination.glob(f"{PREFIX}*{SUFFIX}"))
    old = backups[:-keep] if keep > 0 else []

    for path in old:
        path.unlink()

    return old


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--db", required=True, help="caminho do banco SQLite")
    parser.add_argument("--playlists", help="pasta das programacoes")
    parser.add_argument("--storage", help="pasta de storage do canal")
    parser.add_argument("--storage-dirs", help="subpastas do storage a incluir, separadas por virgula")
    parser.add_argument("--dest", required=True, help="pasta dos backups")
    parser.add_argument("--keep", type=int, default=14, help="quantos backups manter")
    args = parser.parse_args()

    if not Path(args.db).is_file():
        print(f"banco nao encontrado: {args.db}", file=sys.stderr)

        return 2

    if args.storage_dirs and not args.storage:
        print("--storage-dirs precisa de --storage", file=sys.stderr)

        return 2

    archive = make_backup(args)
    removed = prune(Path(args.dest), args.keep)

    print(f"backup criado: {archive} ({archive.stat().st_size} bytes)")

    for path in removed:
        print(f"backup antigo removido: {path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
