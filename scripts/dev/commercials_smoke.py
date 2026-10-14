#!/usr/bin/env python3
"""Smoke test for the commercials module: upload, conversion and settings.

It talks to a running ffplayout through its REST API, uploads a sample file
(for example a real MXF commercial), registers it as a spot, waits for the
background conversion and checks the result. When --storage points at the
channel storage folder on this machine, it also inspects the converted file
(format, audio channels, loudness, left/right content).

Only the Python 3 standard library is needed; ffprobe and ffmpeg (already
required by ffplayout) are used for the analysis when available.

Example:
    scripts/dev/commercials_smoke.py --url http://127.0.0.1:8787 \
        --user admin --password '...' --sample ~/comercial.mxf \
        --storage /var/lib/ffplayout/tv-media
"""

import argparse
import array
import json
import math
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

CHUNK_SIZE = 8 * 1024 * 1024
RESULTS = []


def check(name, ok, detail=""):
    RESULTS.append({"check": name, "ok": bool(ok), "detail": detail})
    mark = "PASSOU" if ok else "FALHOU"
    suffix = f" ({detail})" if detail else ""
    print(f"  [{mark}] {name}{suffix}")

    return bool(ok)


def info(text):
    print(f"  [info]   {text}")


class Api:
    def __init__(self, base):
        self.base = base.rstrip("/")
        self.token = None

    def request(self, method, path, body=None, raw=None, content_type=None):
        data = raw
        headers = {}

        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"

        if content_type:
            headers["Content-Type"] = content_type

        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"

        for attempt in range(6):
            request = urllib.request.Request(
                self.base + path, data=data, method=method, headers=headers
            )

            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    status, text = response.status, response.read().decode()
            except urllib.error.HTTPError as error:
                status, text = error.code, error.read().decode()
            except urllib.error.URLError as error:
                sys.exit(f"Nao foi possivel falar com {self.base}: {error.reason}")

            # The server limits the request rate; wait a moment and try again.
            if status != 429 or path.startswith("/auth/") or attempt == 5:
                break

            time.sleep(1 + attempt)

        try:
            return status, json.loads(text) if text else None
        except ValueError:
            return status, text

    def login(self, user, password):
        # The server accepts only 3 requests per minute on /auth/ from one
        # address, so a quick second run has to wait for the limit to reset.
        for attempt in range(5):
            status, body = self.request(
                "POST", "/auth/login", {"username": user, "password": password}
            )

            if status == 200 and isinstance(body, dict) and "access" in body:
                self.token = body["access"]

                return True

            if status != 429:
                break

            info("o servidor limita logins (3 por minuto); aguardando 21 s para tentar de novo...")
            time.sleep(21)

        info(f"login respondeu {status}: {body}")

        return False


def multipart(fields, file_field, file_name, content):
    boundary = uuid.uuid4().hex
    lines = []

    for name, value in fields.items():
        lines.append(f"--{boundary}\r\n".encode())
        lines.append(
            f'Content-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
        )

    lines.append(f"--{boundary}\r\n".encode())
    lines.append(
        (
            f'Content-Disposition: form-data; name="{file_field}"; '
            f'filename="{file_name}"\r\nContent-Type: application/octet-stream\r\n\r\n'
        ).encode()
    )
    lines.append(content)
    lines.append(f"\r\n--{boundary}--\r\n".encode())

    return b"".join(lines), f"multipart/form-data; boundary={boundary}"


def upload(api, channel, folder, local_file, remote_name):
    """Send a file through the chunked upload API, like the web interface."""
    size = os.path.getsize(local_file)
    batch = uuid.uuid4().hex
    query = urllib.parse.urlencode({"path": folder})
    offset = 0

    with open(local_file, "rb") as handle:
        while offset < size:
            chunk = handle.read(min(CHUNK_SIZE, size - offset))
            body, content_type = multipart(
                {
                    "fileName": remote_name,
                    "start": offset,
                    "end": offset + len(chunk),
                    "size": size,
                    "batch_id": batch,
                },
                "chunk",
                remote_name,
                chunk,
            )
            status, answer = api.request(
                "PUT",
                f"/api/file/{channel}/upload?{query}",
                raw=body,
                content_type=content_type,
            )

            if status != 200:
                return False, f"HTTP {status}: {answer}"

            offset += len(chunk)

    return True, ""


def tool(name):
    return shutil.which(name)


def ffprobe(path):
    """Streams and duration of a file, or None when ffprobe is unavailable."""
    if not tool("ffprobe"):
        return None

    result = subprocess.run(
        [
            "ffprobe", "-v", "error", "-show_entries",
            "stream=index,codec_type,codec_name,channels,channel_layout,"
            "sample_rate,width,height,r_frame_rate:format=duration",
            "-of", "json", path,
        ],
        capture_output=True,
        text=True,
    )

    if result.returncode != 0:
        return None

    try:
        return json.loads(result.stdout)
    except ValueError:
        return None


def describe_streams(probe):
    streams = []

    for stream in (probe or {}).get("streams", []):
        if stream.get("codec_type") == "video":
            streams.append(
                f"video {stream.get('codec_name')} "
                f"{stream.get('width')}x{stream.get('height')} "
                f"{stream.get('r_frame_rate')}"
            )
        elif stream.get("codec_type") == "audio":
            streams.append(
                f"audio {stream.get('codec_name')} "
                f"{stream.get('channels')}ch "
                f"{stream.get('channel_layout', '?')} "
                f"{stream.get('sample_rate')}Hz"
            )

    return streams


def measure_loudness(path):
    """Integrated loudness (LUFS) of a file, using ffmpeg's loudnorm."""
    if not tool("ffmpeg"):
        return None

    result = subprocess.run(
        [
            "ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-vn", "-af",
            "loudnorm=I=-24:TP=-2:LRA=11:print_format=json", "-f", "null", "-",
        ],
        capture_output=True,
        text=True,
    )
    start, end = result.stderr.rfind("{"), result.stderr.rfind("}")

    if start < 0 or end < start:
        return None

    try:
        return float(json.loads(result.stderr[start:end + 1])["input_i"])
    except (ValueError, KeyError):
        return None


def channel_samples(path, channel):
    result = subprocess.run(
        [
            "ffmpeg", "-nostdin", "-v", "error", "-i", path, "-af",
            f"pan=mono|c0=c{channel}", "-f", "s16le", "-ac", "1", "-",
        ],
        capture_output=True,
    )
    samples = array.array("h")
    samples.frombytes(result.stdout[: len(result.stdout) // 2 * 2])

    return samples


def channel_difference(path):
    """0 when both channels carry the same signal; about 1.4 when unrelated."""
    left, right = channel_samples(path, 0), channel_samples(path, 1)
    length = min(len(left), len(right))

    if length == 0:
        return None

    energy_left = sum(float(value) ** 2 for value in left[:length])
    energy_diff = sum(
        (float(left[i]) - float(right[i])) ** 2 for i in range(length)
    )

    return math.sqrt(energy_diff / max(energy_left, 1.0))


def wait_for_spot(api, channel, spot_id, timeout):
    started = time.time()
    last = None

    while time.time() - started < timeout:
        _, spots = api.request(
            "GET", f"/api/commercials/{channel}/spots?include_archived=true"
        )
        spot = next((s for s in spots if s["id"] == spot_id), None)

        if spot is None:
            return None

        line = f"{spot['status']} {spot['progress']}%"

        if line != last:
            info(f"conversao: {line}")
            last = line

        if spot["status"] != "processing":
            return spot

        time.sleep(1)

    return None


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--url", default="http://127.0.0.1:8787")
    parser.add_argument("--user", required=True)
    parser.add_argument("--password", required=True)
    parser.add_argument("--channel", type=int, default=1)
    parser.add_argument("--sample", required=True, help="arquivo a enviar (ex.: um MXF)")
    parser.add_argument(
        "--storage",
        help="pasta de storage do canal nesta maquina; ativa a analise do arquivo convertido",
    )
    parser.add_argument("--folder", default="upload", help="pasta de destino do envio")
    parser.add_argument(
        "--mapping",
        choices=["auto", "first_stream", "first_two_mono", "mix_all"],
        help="altera o mapeamento de audio durante o teste",
    )
    parser.add_argument("--loudness-target", type=float, help="alvo de volume em LUFS")
    parser.add_argument("--timeout", type=int, default=900, help="segundos de espera")
    parser.add_argument("--keep", action="store_true", help="nao apagar o que foi criado")

    return parser.parse_args()


def main():
    args = parse_args()
    api = Api(args.url)
    channel = args.channel
    created = {}
    original_settings = None
    started = time.time()
    stamp = time.strftime("%Y%m%d-%H%M%S")
    remote_name = f"smoke-{stamp}-{os.path.basename(args.sample)}"

    if not os.path.isfile(args.sample):
        sys.exit(f"Arquivo nao encontrado: {args.sample}")

    print("\n== 1. Acesso e configuracoes ==")

    if not check("login", api.login(args.user, args.password)):
        sys.exit("Sem login nao ha como continuar.")

    status, settings = api.request("GET", f"/api/commercials/{channel}/settings")

    if not check("le as configuracoes de conversao", status == 200, f"HTTP {status}"):
        sys.exit("A rota de configuracoes nao respondeu; este servidor tem a M2?")

    original_settings = dict(settings)
    info(f"configuracoes atuais: {json.dumps(settings)}")

    if args.mapping or args.loudness_target is not None:
        changed = dict(settings)

        if args.mapping:
            changed["audio_mapping"] = args.mapping

        if args.loudness_target is not None:
            changed["loudness_target_lufs"] = args.loudness_target

        status, answer = api.request("PUT", f"/api/commercials/{channel}/settings", changed)
        check("aplica as configuracoes do teste", status == 200, f"HTTP {status}")
        settings = answer if status == 200 else settings

    sample_probe = ffprobe(args.sample)
    source_streams = describe_streams(sample_probe)
    source_duration = None

    if sample_probe:
        try:
            source_duration = float(sample_probe["format"]["duration"])
        except (KeyError, ValueError):
            pass

    print("\n== 2. Arquivo de amostra ==")
    info(f"arquivo: {args.sample}")

    for line in source_streams or ["(ffprobe indisponivel; nao foi possivel listar as trilhas)"]:
        info(line)

    print("\n== 3. Cadastro e envio ==")
    suffix = stamp
    status, advertiser = api.request(
        "POST", f"/api/commercials/{channel}/advertisers", {"name": f"Teste {suffix}"}
    )

    if not check("cria o anunciante de teste", status == 201, f"HTTP {status}"):
        sys.exit("Sem anunciante nao ha como continuar (precisa ser administrador).")

    created["advertiser"] = advertiser["id"]
    status, campaign = api.request(
        "POST",
        f"/api/commercials/{channel}/campaigns",
        {"advertiser_id": advertiser["id"], "name": f"Avulso {suffix}", "kind": "standalone"},
    )

    if not check("cria a campanha de teste", status == 201, f"HTTP {status}"):
        sys.exit("Nao foi possivel criar a campanha.")

    created["campaign"] = campaign["id"]
    api.request("POST", f"/api/file/{channel}/create-folder", {"source": args.folder})

    sent, reason = upload(api, channel, args.folder, args.sample, remote_name)
    check("envia o arquivo pela API de upload (aceita o tipo do arquivo)", sent, reason)

    if not sent:
        sys.exit("O envio falhou; veja a mensagem acima.")

    remote_path = f"{args.folder}/{remote_name}"
    status, spot = api.request(
        "POST",
        f"/api/commercials/{channel}/spots",
        {"campaign_id": campaign["id"], "title": f"Teste {suffix}", "file_path": remote_path},
    )

    if not check("registra o spot (resposta 202, em processamento)", status == 202, f"HTTP {status}" + (f" {spot}" if status != 202 else "")):
        sys.exit("O cadastro do spot falhou.")

    created["spot"] = spot["id"]

    print("\n== 4. Conversao ==")
    spot = wait_for_spot(api, channel, spot["id"], args.timeout)

    if not check("a conversao terminou no prazo", spot is not None):
        sys.exit("A conversao nao terminou; veja o log do ffplayout.")

    ready = check(
        "o spot ficou pronto",
        spot["status"] == "ready",
        spot["error"] or f"status {spot['status']}",
    )

    if ready:
        info(f"duracao: {spot['duration_sec']:.2f} s; arquivo: {spot['file_path']}")

        if source_duration is not None:
            check(
                "a duracao bate com a do arquivo original (margem de 1 s)",
                abs(spot["duration_sec"] - source_duration) <= 1.0,
                f"original {source_duration:.2f} s, convertido {spot['duration_sec']:.2f} s",
            )

    analysis = {}

    if ready and args.storage:
        print("\n== 5. Analise do arquivo convertido ==")
        converted = os.path.join(args.storage, spot["file_path"])

        if check("o arquivo convertido existe na pasta de storage", os.path.isfile(converted), converted):
            probe = ffprobe(converted)
            streams = describe_streams(probe)
            analysis["converted_streams"] = streams

            for line in streams:
                info(line)

            video = [s for s in (probe or {}).get("streams", []) if s.get("codec_type") == "video"]
            audio = [s for s in (probe or {}).get("streams", []) if s.get("codec_type") == "audio"]

            check("um video H.264", len(video) == 1 and video[0].get("codec_name") == "h264")
            check(
                "um audio AAC estereo a 48 kHz",
                len(audio) == 1
                and audio[0].get("codec_name") == "aac"
                and audio[0].get("channels") == 2
                and audio[0].get("sample_rate") == "48000",
            )

            _, config = api.request("GET", f"/api/playout/config/{channel}")
            output = (config or {}).get("output") if isinstance(config, dict) else None

            if output and video:
                wanted = (output.get("width"), output.get("height"))
                got = (video[0].get("width"), video[0].get("height"))
                check(
                    "o tamanho do video e o da saida do canal",
                    wanted == got,
                    f"canal {wanted[0]}x{wanted[1]}, arquivo {got[0]}x{got[1]}",
                )
                info(f"taxa de quadros do canal: {output.get('fps')}; do arquivo: {video[0].get('r_frame_rate')}")

            if settings.get("loudness_enabled"):
                target = settings.get("loudness_target_lufs", -24.0)
                loudness = measure_loudness(converted)
                analysis["loudness_lufs"] = loudness

                if loudness is None:
                    info("nao foi possivel medir o volume (ffmpeg indisponivel?)")
                else:
                    check(
                        f"o volume esta perto do alvo de {target} LUFS (margem de 1,5)",
                        abs(loudness - target) <= 1.5,
                        f"medido {loudness:.1f} LUFS",
                    )

            difference = channel_difference(converted) if tool("ffmpeg") else None
            analysis["channel_difference"] = difference

            if difference is not None:
                if difference < 0.2:
                    verdict = "os dois canais levam o MESMO sinal (mono duplicado)"
                elif difference > 0.5:
                    verdict = "esquerdo e direito sao DIFERENTES (trilhas separadas)"
                else:
                    verdict = "esquerdo e direito parecidos, mas nao iguais"

                info(f"esquerdo x direito: diferenca {difference:.2f} -> {verdict}")

            partials = [
                name
                for name in os.listdir(os.path.dirname(converted))
                if name.endswith(".part")
            ]
            check("nenhum arquivo parcial ficou para tras", not partials, ", ".join(partials))
            info(f"OUVIR E CONFERIR: {converted}")
    elif ready:
        info("sem --storage: a analise do arquivo convertido foi pulada")

    if not args.keep:
        print("\n== 6. Limpeza ==")

        if original_settings is not None and (args.mapping or args.loudness_target is not None):
            status, _ = api.request("PUT", f"/api/commercials/{channel}/settings", original_settings)
            info(f"configuracoes restauradas (HTTP {status})")

        api.request("DELETE", f"/api/commercials/{channel}/spots/{created['spot']}")
        api.request("DELETE", f"/api/commercials/{channel}/campaigns/{created['campaign']}")
        api.request("DELETE", f"/api/commercials/{channel}/advertisers/{created['advertiser']}")

        for source in (remote_path, spot["file_path"] if ready else None):
            if source:
                api.request("POST", f"/api/file/{channel}/remove", {"source": source})

        info("anunciante, campanha, spot e arquivos de teste removidos")

    failed = [r for r in RESULTS if not r["ok"]]
    report = {
        "sample_streams": source_streams,
        "spot": {k: spot[k] for k in ("status", "error", "progress", "duration_sec")} if spot else None,
        "settings": settings,
        "analysis": analysis,
        "total_seconds": round(time.time() - started, 1),
        "checks": RESULTS,
    }

    print("\n== RESULTADO (cole este bloco na conversa) ==")
    print(json.dumps(report, indent=2, ensure_ascii=False))
    print(
        f"\n{len(RESULTS) - len(failed)} verificacoes passaram, {len(failed)} falharam "
        f"(tempo total {time.time() - started:.0f} s)."
    )

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
