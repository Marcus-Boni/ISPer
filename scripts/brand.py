#!/usr/bin/env python3
"""
A marca do ISPer, desenhada uma vez só.

Até 01/10/2026 o projeto tinha três logos: barras brancas em losango sobre
vermelho tijolo no desktop, três barras terracota apoiadas embaixo no portal, e
cinco barras marfim centradas com o ponto de gravação no Android. A do Android
é a certa, e não por gosto: é o gesto do próprio indicador do desktop — a
forma de onda que aparece enquanto o ISPer escuta, e o ponto que diz que está
gravando. As outras duas eram ícones genéricos de áudio.

Este script guarda a geometria dessa marca e gera dela tudo o que as três
plataformas consomem. Nenhum ícone se edita à mão; edita-se aqui e roda:

    python scripts/brand.py            # regrava todos os arquivos
    python scripts/brand.py --check    # não grava; sai 1 se algum estiver defasado

Saídas
------
assets/brand/            os mestres em SVG e a página que explica a marca
website/src/app/         favicon (SVG) e apple-icon
website/public/icons/    ícones do manifesto, inclusive o maskable
apps/isper-app/src-tauri/icons/   o que o Tauri empacota, e a bandeja
apps/isper-android/.../drawable/  o primeiro plano do ícone adaptativo

Dois enquadramentos, a mesma marca
----------------------------------
O Android mostra a área central de 72 de um ícone de 108, e é nessa moldura que
a marca foi equilibrada: a caixa dela não fica no centro, a MASSA fica (as
barras pesam à esquerda, o ponto puxa à direita, e o centro de massa cai a
0,7 de unidade do centro da tela). Toda placa de 48 px para cima usa essa
mesma moldura, para o ícone do desktop ter a proporção exata do celular.

Abaixo disso a moldura de 72 dá barras de 0,9 px a 16 px, que viram cinza. A
moldura pequena (64) faz quatro unidades valerem exatamente um pixel a 16 px:
as cinco barras e os quatro vãos caem na grade. É tamanho óptico, não outra
marca — a geometria não muda, só o quanto dela cabe na placa.
"""

from __future__ import annotations

import argparse
import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent

# ── A marca ─────────────────────────────────────────────────────────────────
# Coordenadas no canvas de 108 do ícone adaptativo do Android, de onde ela veio.

CANVAS = 108
CENTER = 54

BAR_WIDTH = 4
BARS = [  # (x, altura) — a fala sobe até o meio e decai mais depressa do que subiu
    (34, 16),
    (42, 28),
    (50, 40),
    (58, 24),
    (66, 12),
]
DOT = (76, 54, 5)  # cx, cy, r — o ponto de gravação

# Paleta: as mesmas cores do DESIGN.md, e nenhuma outra.
WARM_VOID = "#161311"
IVORY = "#ECE7E1"
TERRACOTTA = "#F07E72"
MUTED = "#8D847C"  # o ponto apagado da bandeja ociosa

FRAME_FULL = (18, 72)   # origem, lado — a área visível do launcher do Android
FRAME_SMALL = (22, 64)  # quatro unidades = um pixel a 16 px
TILE_RADIUS = 0.225     # fração do lado da placa

SUPERSAMPLE = 8  # desenhar 8× maior e reduzir dá bordas limpas sem rasterizador de SVG


def hex_rgba(color: str, alpha: int = 255) -> tuple[int, int, int, int]:
    c = color.lstrip("#")
    return (int(c[0:2], 16), int(c[2:4], 16), int(c[4:6], 16), alpha)


def bar_rects() -> list[tuple[float, float, float, float]]:
    """Cada barra como (x0, y0, x1, y1), centrada na linha média."""
    return [(x, CENTER - h / 2, x + BAR_WIDTH, CENTER + h / 2) for x, h in BARS]


# ── SVG ─────────────────────────────────────────────────────────────────────

def _num(v: float) -> str:
    return f"{v:g}"


def svg_mark_shapes(dot_color: str = TERRACOTTA, bar_color: str = IVORY) -> str:
    bars = "".join(
        f'<rect x="{_num(x0)}" y="{_num(y0)}" width="{BAR_WIDTH}" height="{_num(y1 - y0)}"/>'
        for x0, y0, _, y1 in bar_rects()
    )
    cx, cy, r = DOT
    return (
        f'<g fill="{bar_color}">{bars}</g>'
        f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{dot_color}"/>'
    )


def svg_tile(frame: tuple[int, int], dot_color: str = TERRACOTTA) -> str:
    origin, side = frame
    radius = side * TILE_RADIUS
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{origin} {origin} {side} {side}">'
        f'<rect x="{origin}" y="{origin}" width="{side}" height="{side}" rx="{_num(round(radius, 2))}" fill="{WARM_VOID}"/>'
        f"{svg_mark_shapes(dot_color)}"
        "</svg>\n"
    )


def svg_mark() -> str:
    """Só a marca, sem placa, enquadrada pela própria caixa — para quem compõe com ela."""
    x0 = min(x for x, _ in BARS)
    x1 = DOT[0] + DOT[2]
    y0 = CENTER - max(h for _, h in BARS) / 2
    y1 = CENTER + max(h for _, h in BARS) / 2
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{_num(x0)} {_num(y0)} {_num(x1 - x0)} {_num(y1 - y0)}">'
        f"{svg_mark_shapes()}"
        "</svg>\n"
    )


def ts_geometry() -> str:
    """A geometria para o portal, que compõe com a marca em React e na imagem OG."""
    x0 = min(x for x, _ in BARS)
    x1 = DOT[0] + DOT[2]
    tallest = max(h for _, h in BARS)
    bars = ",\n    ".join(
        f"{{ x: {x0r:g}, y: {y0:g}, width: {BAR_WIDTH}, height: {y1 - y0:g} }}"
        for x0r, y0, _, y1 in bar_rects()
    )
    cx, cy, r = DOT
    return (
        "// Gerado por scripts/brand.py — não edite à mão. A marca do ISPer é uma só em\n"
        "// todas as plataformas; edite a geometria no script e rode-o de novo.\n"
        "//\n"
        "// Coordenadas no canvas de 108 do ícone adaptativo do Android, de onde a\n"
        "// marca veio: ondas de fala centradas numa linha média e o ponto de gravação.\n\n"
        "export const BRAND_MARK = {\n"
        f'  viewBox: "{x0:g} {CENTER - tallest / 2:g} {x1 - x0:g} {tallest:g}",\n'
        f"  midline: {CENTER},\n"
        f"  bars: [\n    {bars},\n  ],\n"
        f"  dot: {{ cx: {cx}, cy: {cy}, r: {r} }},\n"
        f'  colors: {{ bars: "{IVORY}", dot: "{TERRACOTTA}", tile: "{WARM_VOID}" }},\n'
        "} as const;\n"
    )


def android_foreground() -> str:
    """O primeiro plano do ícone adaptativo — o desenho de onde a marca saiu."""
    bars = " ".join(
        f"M{x},{_num(CENTER - h / 2)}h{BAR_WIDTH}v{h}h-{BAR_WIDTH}z" for x, h in BARS
    )
    cx, cy, r = DOT
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        "<!-- Gerado por scripts/brand.py: a marca do ISPer, a mesma em todas as\n"
        "     plataformas. Ondas de fala com o ponto de gravação — o gesto do indicador\n"
        "     do desktop. Não edite aqui; edite a geometria no script. -->\n"
        '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
        f'    android:width="{CANVAS}dp"\n'
        f'    android:height="{CANVAS}dp"\n'
        f'    android:viewportWidth="{CANVAS}"\n'
        f'    android:viewportHeight="{CANVAS}">\n'
        "    <path\n"
        f'        android:fillColor="{IVORY}"\n'
        f'        android:pathData="{bars}" />\n'
        "    <path\n"
        f'        android:fillColor="{TERRACOTTA}"\n'
        f'        android:pathData="M{cx},{cy}m-{r},0a{r},{r} 0,1 1,{2 * r} 0a{r},{r} 0,1 1,-{2 * r} 0" />\n'
        "</vector>\n"
    )


# ── Raster ──────────────────────────────────────────────────────────────────

def render_tile(
    size: int,
    frame: tuple[int, int],
    *,
    dot_color: str = TERRACOTTA,
    bleed: bool = False,
    safe_scale: float = 1.0,
) -> Image.Image:
    """
    A placa rasterizada. `bleed` enche o quadrado inteiro (ícone maskable: quem
    corta é o sistema); `safe_scale` encolhe a marca para dentro da zona segura
    que o sistema garante não cortar.
    """
    origin, side = frame
    big = size * SUPERSAMPLE
    scale = big / side

    def to_px(v: float) -> float:
        return (v - origin) * scale

    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    if bleed:
        draw.rectangle((0, 0, big, big), fill=hex_rgba(WARM_VOID))
    else:
        draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=big * TILE_RADIUS, fill=hex_rgba(WARM_VOID))

    # Encolher em torno do centro da tela, que é onde a massa da marca está.
    pivot = to_px(CENTER)

    def place(v: float) -> float:
        return pivot + (to_px(v) - pivot) * safe_scale

    for x0, y0, x1, y1 in bar_rects():
        draw.rectangle((place(x0), place(y0), place(x1) - 1, place(y1) - 1), fill=hex_rgba(IVORY))
    cx, cy, r = DOT
    rr = r * scale * safe_scale
    draw.ellipse((place(cx) - rr, place(cy) - rr, place(cx) + rr, place(cy) + rr), fill=hex_rgba(dot_color))

    # BOX numa redução inteira é cobertura de área exata — que é exatamente o
    # que antisserrilhamento é. LANCZOS afia, e sobre formas binárias deixa um
    # halo fino em volta das barras.
    return im.resize((size, size), Image.BOX)


def render_hinted(size: int, *, dot_color: str = TERRACOTTA) -> Image.Image:
    """
    A marca nos tamanhos em que ela não cabe na grade: hinting, como em fonte.

    A moldura pequena deixa 16 e 32 px perfeitos, mas a 24 px quatro unidades
    viram 1,5 px — cada barra um pixel cheio e meio cinza. E a 16 px o vão de uma
    unidade entre a última barra e o ponto vale 0,25 px, então o ponto se funde
    com a onda. Aqui as barras são dispostas direto em pixels inteiros (largura,
    vão e altura arredondados, com todas as alturas na mesma paridade para
    dividirem uma linha média), e o ponto ganha ao menos um pixel de folga. O
    ponto continua antisserrilhado: círculo nunca cai na grade, e forçar dá um
    quadrado.
    """
    s = size / FRAME_SMALL[1]
    bw = max(1, round(BAR_WIDTH * s))
    gap = max(1, round(2 * BAR_WIDTH * s) - bw)
    dot_d = max(2, round(2 * DOT[2] * s))
    dot_gap = max(1, round((DOT[0] - DOT[2] - (BARS[-1][0] + BAR_WIDTH)) * s))

    tallest = max(1, round(max(h for _, h in BARS) * s))

    def snap_height(h: int) -> int:
        v = max(1, round(h * s))
        if (v - tallest) % 2:
            v += 1 if v < h * s else -1
        return max(1, v)

    heights = [snap_height(h) for _, h in BARS]
    width = len(BARS) * bw + (len(BARS) - 1) * gap + dot_gap + dot_d
    # Inclinado à direita no resto ímpar: o centro de MASSA fica à esquerda da
    # caixa, e é ele que tem de ficar no meio da placa.
    x = -(-(size - width) // 2)
    mid = size / 2 if tallest % 2 == 0 else (size - 1) // 2 + 0.5

    k = SUPERSAMPLE
    big = size * k
    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=big * TILE_RADIUS, fill=hex_rgba(WARM_VOID))
    for h in heights:
        top = mid - h / 2
        draw.rectangle((x * k, top * k, (x + bw) * k - 1, (top + h) * k - 1), fill=hex_rgba(IVORY))
        x += bw + gap
    x += dot_gap - gap
    r = dot_d / 2
    draw.ellipse((x * k, (mid - r) * k, (x + dot_d) * k, (mid + r) * k), fill=hex_rgba(dot_color))
    return im.resize((size, size), Image.BOX)


def png_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def ico_bytes(frames: list[Image.Image]) -> bytes:
    """
    ICO com uma imagem própria por tamanho.

    O gravador de ICO do Pillow redimensiona UMA imagem para todos os tamanhos,
    o que mandaria a moldura grande para os 16 px — as barras de 0,9 px que
    este arquivo existe para evitar. Aqui cada entrada é um PNG desenhado para
    o seu tamanho; o Windows lê PNG dentro de ICO desde o Vista.
    """
    blobs = [png_bytes(f) for f in frames]
    header = struct.pack("<HHH", 0, 1, len(frames))
    offset = 6 + 16 * len(frames)
    entries = b""
    for im, blob in zip(frames, blobs):
        w, h = im.size
        entries += struct.pack(
            "<BBBBHHII",
            w if w < 256 else 0,
            h if h < 256 else 0,
            0, 0, 1, 32, len(blob), offset,
        )
        offset += len(blob)
    return header + entries + b"".join(blobs)


def icns_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, format="ICNS")
    return buf.getvalue()


# ── O que vai para onde ─────────────────────────────────────────────────────

TAURI = ROOT / "apps/isper-app/src-tauri/icons"
WEB_APP = ROOT / "website/src/app"
WEB_ICONS = ROOT / "website/public/icons"
ANDROID_DRAWABLE = ROOT / "apps/isper-android/app/src/main/res/drawable"
BRAND = ROOT / "assets/brand"


def tile_for(size: int, **kw) -> Image.Image:
    """A placa certa para cada tamanho: hinting abaixo de 32, moldura pequena a 32, a do launcher daí para cima."""
    if size < 32:
        return render_hinted(size, **kw)
    return render_tile(size, FRAME_SMALL if size == 32 else FRAME_FULL, **kw)


def outputs() -> dict[Path, bytes]:
    out: dict[Path, bytes] = {}

    # Os mestres
    out[BRAND / "isper-mark.svg"] = svg_mark().encode()
    out[BRAND / "isper-icon.svg"] = svg_tile(FRAME_FULL).encode()
    out[BRAND / "isper-icon-small.svg"] = svg_tile(FRAME_SMALL).encode()
    out[BRAND / "isper-icon-1024.png"] = png_bytes(tile_for(1024))

    # Android: o desenho original, agora derivado daqui
    out[ANDROID_DRAWABLE / "ic_launcher_foreground.xml"] = android_foreground().encode()

    # Portal: o favicon é vetor na moldura pequena, que é o tamanho em que ele vive.
    # SVG não tem hinting, então a 16 px o navegador desenha a versão macia; o
    # .ico ao lado leva os quadros ajustados à grade para telas de densidade 1,
    # favoritos e o que mais pedir /favicon.ico (que dava 404 até aqui).
    out[WEB_APP / "icon.svg"] = svg_tile(FRAME_SMALL).encode()
    out[WEB_APP / "favicon.ico"] = ico_bytes([tile_for(s) for s in (16, 24, 32, 48)])
    out[ROOT / "website/src/lib/brand-mark.ts"] = ts_geometry().encode()
    out[WEB_APP / "apple-icon.png"] = png_bytes(tile_for(180))
    out[WEB_ICONS / "icon-192.png"] = png_bytes(tile_for(192))
    out[WEB_ICONS / "icon-512.png"] = png_bytes(tile_for(512))
    # Maskable: o sistema recorta em círculo ou squircle e só garante os 80%
    # centrais, então o fundo vai até a borda e a marca encolhe para dentro.
    out[WEB_ICONS / "icon-maskable-512.png"] = png_bytes(
        render_tile(512, FRAME_FULL, bleed=True, safe_scale=0.8)
    )

    # Desktop: os quatro arquivos que o tauri.conf.json empacota…
    out[TAURI / "32x32.png"] = png_bytes(tile_for(32))
    out[TAURI / "128x128.png"] = png_bytes(tile_for(128))
    out[TAURI / "128x128@2x.png"] = png_bytes(tile_for(256))
    out[TAURI / "icon.ico"] = ico_bytes([tile_for(s) for s in (16, 24, 32, 48, 64, 256)])
    # …e os demais tamanhos que o `tauri icon` costumava deixar, para nenhum
    # arquivo desta pasta ainda mostrar a marca antiga.
    out[TAURI / "64x64.png"] = png_bytes(tile_for(64))
    out[TAURI / "icon.png"] = png_bytes(tile_for(512))
    out[TAURI / "icon.icns"] = icns_bytes(tile_for(1024))
    for s in (30, 44, 71, 89, 107, 142, 150, 284, 310):
        out[TAURI / f"Square{s}x{s}Logo.png"] = png_bytes(tile_for(s))
    out[TAURI / "StoreLogo.png"] = png_bytes(tile_for(50))

    # Bandeja: o ponto é a luz de gravação. Apagado enquanto o ISPer só espera,
    # aceso enquanto grava — o mesmo símbolo do ícone, agora dizendo o estado.
    # RGBA cru porque a bandeja monta a imagem com Image::new_owned, sem o
    # decodificador de PNG do Tauri; os .png ao lado existem para revisão.
    for name, dot in (("tray", MUTED), ("tray-recording", TERRACOTTA)):
        im = render_tile(32, FRAME_SMALL, dot_color=dot)
        out[TAURI / f"{name}.rgba"] = im.tobytes()
        out[TAURI / f"{name}.png"] = png_bytes(im)

    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="não grava; sai 1 se algo estiver defasado")
    args = parser.parse_args()

    stale = []
    for path, data in outputs().items():
        current = path.read_bytes() if path.exists() else None
        if current == data:
            continue
        stale.append(path)
        if not args.check:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

    rel = [str(p.relative_to(ROOT)).replace("\\", "/") for p in stale]
    if args.check:
        if rel:
            print("Marca defasada em:", *rel, sep="\n  ")
            print("Rode: python scripts/brand.py")
            return 1
        print("Marca em dia em todas as plataformas.")
        return 0

    print(f"{len(rel)} arquivo(s) regravado(s)" if rel else "Nada a regravar.")
    for r in rel:
        print("  " + r)
    return 0


if __name__ == "__main__":
    sys.exit(main())
