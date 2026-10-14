#!/usr/bin/env python3
"""Show how a date would be planned, in a form that is easy to read.

It asks a running ffplayout for the plan of a date (weekly template, break
windows and running campaigns) and prints the timeline with the clock time of
every program, spot and break, followed by the warnings. Nothing is changed
unless --apply is given.

Example:
    scripts/dev/commercials_plan_preview.py --user admin --password '...' \
        --date 2026-12-10
"""

import argparse
import json
import sys

from commercials_smoke import Api

KIND_NAMES = {
    "program": "PROGRAMA",
    "spot": "  spot",
    "break_filler": "  (enchimento do intervalo)",
    "day_filler": "(enchimento do fim do dia)",
}


def clock_length(seconds):
    total = int(round(seconds))

    return f"{total // 3600:02d}:{total % 3600 // 60:02d}:{total % 60:02d}"


def print_plan(preview):
    plan = preview["plan"]

    print(f"\nData: {preview['date']} (dia da semana {preview['weekday']}, 1 = segunda)")
    print(f"Ja existe playlist para esta data: {'SIM' if preview['playlist_exists'] else 'nao'}")
    print(
        f"Programas: {clock_length(plan['programs_sec'])}   "
        f"Intervalos: {clock_length(plan['breaks_sec'])}   "
        f"Total: {clock_length(plan['total_sec'])}   "
        f"Tamanho do dia: {clock_length(plan['day_length_sec'])}"
    )

    print("\n  HORA      DURACAO   ITEM")

    for item in plan["items"]:
        length = clock_length(item["out"] - item["seek"])
        label = KIND_NAMES.get(item["kind"], item["kind"])
        title = item["title"] or item["source"] or "(vazio)"

        print(f"  {item['start_time']}  {length}  {label}: {title}")

    if plan["breaks"]:
        print("\nINTERVALOS")

        for planned in plan["breaks"]:
            distance = planned["distance_sec"]
            note = "" if distance < 1 else f"  ({distance:.0f} s do horario pedido)"

            print(
                f"  pedido {planned['nominal_time']} -> comeca {planned['start_time']}{note}; "
                f"{planned['spots']} spot(s), {planned['spots_sec']:.0f} s de "
                f"{planned['duration_sec']} s"
            )

    if plan["warnings"]:
        print("\nAVISOS")

        for warning in plan["warnings"]:
            mark = "ERRO " if warning["severity"] == "error" else "aviso"

            print(f"  [{mark}] {warning['code']}: {warning['message']}")
    else:
        print("\nSem avisos.")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--url", default="http://127.0.0.1:8787")
    parser.add_argument("--user", required=True)
    parser.add_argument("--password", required=True)
    parser.add_argument("--channel", type=int, default=1)
    parser.add_argument("--date", required=True, help="AAAA-MM-DD")
    parser.add_argument("--apply", action="store_true", help="gravar a playlist da data")
    parser.add_argument(
        "--overwrite", action="store_true", help="com --apply, substituir uma playlist existente"
    )
    parser.add_argument("--json", action="store_true", help="mostrar a resposta crua")
    args = parser.parse_args()
    api = Api(args.url)

    if not api.login(args.user, args.password):
        sys.exit("Login falhou.")

    status, preview = api.request("GET", f"/api/commercials/{args.channel}/plan/{args.date}")

    if status != 200:
        sys.exit(f"A previa falhou (HTTP {status}): {preview}")

    if args.json:
        print(json.dumps(preview, indent=2, ensure_ascii=False))
    else:
        print_plan(preview)

    if not args.apply:
        print("\n(nada foi gravado; use --apply para gravar a playlist)")

        return 0

    query = "?overwrite=true" if args.overwrite else ""
    status, answer = api.request(
        "POST", f"/api/commercials/{args.channel}/plan/{args.date}{query}"
    )

    if status == 200:
        print(f"\nGravado: {answer['message']}")

        return 0

    print(f"\nNao foi gravado (HTTP {status}): {answer}")

    return 1


if __name__ == "__main__":
    sys.exit(main())
