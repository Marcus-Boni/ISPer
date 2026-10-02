#!/usr/bin/env python3
"""
As fontes da imagem de compartilhamento, tiradas das do próprio site.

A imagem OG (website/src/app/opengraph-image.tsx) é desenhada pelo Satori, que
não lê WOFF2 nem eixos variáveis. O site só tem a Fraunces e a Hanken Grotesk
assim — um WOFF2 variável cada —, então a imagem saía numa serifa genérica.

Este script gera, das mesmas fontes do site, instâncias estáticas em TTF nas
espessuras e nos tamanhos ópticos em que o site as desenha:

    python scripts/og-fonts.py            # regrava as fontes
    python scripts/og-fonts.py --check    # não grava; sai 1 se alguma estiver defasada

Precisa do fontTools com suporte a WOFF2: pip install "fonttools[woff]".

O tamanho óptico importa
------------------------
A Fraunces tem o eixo `opsz` (9 a 144), e o navegador o ajusta sozinho ao
tamanho do texto (`font-optical-sizing: auto`): o título de 78 px usa o desenho
de 78, de contraste alto e serifas finas, e não o de 9, feito para corpo de
texto. Cada instância fixa o `opsz` no tamanho em que aparece na imagem.

O itálico é do navegador
------------------------
O arquivo da Fraunces é só o romano. No site, "No seu computador." é o romano
inclinado pelo próprio navegador, e a imagem repete essa inclinação com
`skewX` — o Satori não inclina fonte nenhuma.

Licença: as duas fontes são OFL 1.1, sem nome reservado
(website/src/assets/fonts/OFL-*.txt), então a versão derivada mantém o nome.
"""

from __future__ import annotations

import argparse
import sys
from io import BytesIO
from pathlib import Path

try:
    from fontTools.subset import Options, Subsetter
    from fontTools.ttLib import TTFont
    from fontTools.varLib.instancer import instantiateVariableFont
except ImportError:  # pragma: no cover - mensagem para quem roda sem o fontTools
    sys.exit('Falta o fontTools: pip install "fonttools[woff]"')

ROOT = Path(__file__).resolve().parent.parent
FONTS = ROOT / "website/src/assets/fonts"
OUT = FONTS / "og"

# Latin básico e Latin-1 (os acentos do português), mais a pontuação que o
# texto da imagem usa ou pode vir a usar.
UNICODES = [*range(0x20, 0x7F), *range(0xA0, 0x100), 0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026]

# (arquivo de saída, fonte do site, eixos) — os valores que o globals.css usa.
INSTANCES = [
    # .brand: 720, e na imagem o "ISPer." tem 42 px.
    ("fraunces-720-opsz42.ttf", "fraunces-latin.woff2", {"wght": 720, "opsz": 42}),
    # .hero-copy h1: 760, e o título da imagem tem 78 px.
    ("fraunces-760-opsz78.ttf", "fraunces-latin.woff2", {"wght": 760, "opsz": 78}),
    # .hero-copy h1 em: 520, a linha "No seu computador.".
    ("fraunces-520-opsz78.ttf", "fraunces-latin.woff2", {"wght": 520, "opsz": 78}),
    # O texto corrido do site é Hanken 400.
    ("hanken-grotesk-400.ttf", "hanken-grotesk-latin.woff2", {"wght": 400}),
]


def build(source: str, axes: dict[str, float]) -> bytes:
    font = TTFont(FONTS / source)
    static = instantiateVariableFont(font, axes)

    options = Options()
    options.hinting = False
    options.layout_features = ["kern", "liga", "clig", "calt", "ccmp", "locl", "mark", "mkmk"]
    subsetter = Subsetter(options)
    subsetter.populate(unicodes=UNICODES)
    subsetter.subset(static)

    # TTF puro. A fonte aberta de um WOFF2 guarda o formato de origem e o
    # usaria de novo ao salvar — e o Satori recusa WOFF2 ("wOF2").
    static.flavor = None
    # Sem carimbo de hora: rodar de novo dá os mesmos bytes, e o --check funciona.
    static.recalcTimestamp = False
    buffer = BytesIO()
    static.save(buffer)
    return buffer.getvalue()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="não grava; sai 1 se alguma fonte estiver defasada")
    args = parser.parse_args()

    stale = []
    for name, source, axes in INSTANCES:
        data = build(source, axes)
        path = OUT / name
        if path.exists() and path.read_bytes() == data:
            continue
        stale.append(name)
        if not args.check:
            OUT.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

    if args.check:
        if stale:
            print("Fontes da imagem OG defasadas:", *stale, sep="\n  ")
            print("Rode: python scripts/og-fonts.py")
            return 1
        print("Fontes da imagem OG em dia.")
        return 0
    print(f"{len(stale)} fonte(s) regravada(s)" if stale else "Nada a regravar.")
    for name in stale:
        print("  " + name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
