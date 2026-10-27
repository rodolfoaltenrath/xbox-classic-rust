#!/usr/bin/env python3
"""Compara um quadro BGRA 640x480 com uma captura de referência (ADR-0089).

Uso: python3 scripts/comparar-referencia.py <quadro.bgra> <referencia.png> \
         <x0,y0,x1,y1 da área 640x480 na referência> [--png saída.png]

A referência pública do mesh é uma captura de janela do macOS: está em outra
escala e passou por gerenciamento de cor (o fundo 0x202020 aparece como ~43).
Cores absolutas não são comparáveis; a métrica usa o que sobrevive a isso:

- silhueta: pixels cuja luminância difere da do fundo de cada imagem por mais de
  LIMIAR, fora da região do texto (canto superior esquerdo). Mede IoU, caixa
  envolvente e centroide;
- brilho relativo: luminância média do objeto menos a do fundo, em cada imagem.

Requer Pillow. Imprime linhas `chave: valor` estáveis; nunca decide aprovação,
quem decide é o critério escrito no PLANO/ADR.
"""
import argparse
import statistics

from PIL import Image

LIMIAR = 10
TEXTO = (0, 0, 260, 100)  # x0, y0, x1, y1 excluídos (texto do pbkit)
MARGEM = 8  # borda excluída: cantos arredondados e sombra da janela da captura


def luminancia(rgb):
    r, g, b = rgb
    return 0.299 * r + 0.587 * g + 0.114 * b


def mascara(img):
    px = img.load()
    fundo = statistics.median(luminancia(px[x, y]) for y in range(440, 480) for x in range(0, 640, 4))
    pontos = set()
    soma = 0.0
    for y in range(480):
        for x in range(640):
            if TEXTO[0] <= x < TEXTO[2] and TEXTO[1] <= y < TEXTO[3]:
                continue
            if x < MARGEM or y < MARGEM or x >= 640 - MARGEM or y >= 480 - MARGEM:
                continue
            delta = luminancia(px[x, y]) - fundo
            if abs(delta) > LIMIAR:
                pontos.add((x, y))
                soma += delta
    return fundo, pontos, (soma / len(pontos) if pontos else 0.0)


def caixa(pontos):
    xs = [p[0] for p in pontos]
    ys = [p[1] for p in pontos]
    return (min(xs), min(ys), max(xs), max(ys)) if pontos else None


def centroide(pontos):
    if not pontos:
        return None
    return (sum(p[0] for p in pontos) / len(pontos), sum(p[1] for p in pontos) / len(pontos))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("quadro")
    ap.add_argument("referencia")
    ap.add_argument("area")
    ap.add_argument("--png")
    a = ap.parse_args()
    bruto = open(a.quadro, "rb").read()
    if len(bruto) != 640 * 480 * 4:
        raise SystemExit(f"quadro deve ter {640 * 480 * 4} bytes, tem {len(bruto)}")
    nosso = Image.frombytes("RGBA", (640, 480), bruto, "raw", "BGRA").convert("RGB")
    area = tuple(float(v) for v in a.area.split(","))
    ref = Image.open(a.referencia).convert("RGB").resize((640, 480), Image.Resampling.BOX, box=area)
    if a.png:
        lado = Image.new("RGB", (1280, 480))
        lado.paste(nosso, (0, 0))
        lado.paste(ref, (640, 0))
        lado.save(a.png)
    fundo_n, m_n, brilho_n = mascara(nosso)
    fundo_r, m_r, brilho_r = mascara(ref)
    uniao = len(m_n | m_r)
    iou = len(m_n & m_r) / uniao if uniao else 0.0
    print(f"fundo: nosso={fundo_n:.1f} referencia={fundo_r:.1f}")
    print(f"silhueta_pixels: nosso={len(m_n)} referencia={len(m_r)}")
    print(f"silhueta_iou: {iou:.3f}")
    print(f"caixa: nosso={caixa(m_n)} referencia={caixa(m_r)}")
    cn, cr = centroide(m_n), centroide(m_r)
    if cn and cr:
        print(f"centroide: nosso=({cn[0]:.1f},{cn[1]:.1f}) referencia=({cr[0]:.1f},{cr[1]:.1f})")
    print(f"brilho_relativo: nosso={brilho_n:.1f} referencia={brilho_r:.1f}")


if __name__ == "__main__":
    main()
