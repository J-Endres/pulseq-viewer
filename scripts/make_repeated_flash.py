"""Generate web/public/examples/flash_repeated.seq from flash_je.seq.

Repeats the 96 phase-encoding TRs of flash_je.seq 200 times, separated by a
10 ms delay block, giving ~96k blocks with two nested loops. Used to test
loop detection and drawing on long sequences.

Usage: python3 scripts/make_repeated_flash.py
"""

from pathlib import Path

REPEATS = 200
DELAY_TICKS = 1000  # 10 ms at the 10 us block raster

examples = Path(__file__).resolve().parent.parent / "web" / "public" / "examples"
src = (examples / "flash_je.seq").read_text()

head, rest = src.split("[BLOCKS]\n", 1)
lines = rest.split("\n")
blocks = []
for line in lines:
    if not line.strip():
        break
    blocks.append(line.split()[1:])
# Everything after the block table, without the (now invalid) signature.
tail = "\n".join(lines[len(blocks):]).split("[SIGNATURE]")[0]

initial_delay, body = blocks[0], blocks[1:]
out = [initial_delay]
for _ in range(REPEATS):
    out += body
    out.append([str(DELAY_TICKS)] + ["0"] * 6)

table = "\n".join(f"{i + 1} " + " ".join(row) for i, row in enumerate(out))
(examples / "flash_repeated.seq").write_text(f"{head}[BLOCKS]\n{table}\n{tail}")
print(f"{len(out)} blocks")
