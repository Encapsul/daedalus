#!/usr/bin/env python3
"""Generate 'flamegraph-style' visualizations of SISR chunk reuse.

Each chunk is a rectangle (treemap) where:
  - WIDTH proportional to chunk size (bytes)
  - COLOR: green = reused from cache, red = must be downloaded
  - ORDER: chunk index (left-to-right, like a flamegraph stack)

Generates:
  - chunk_flamegraph.png     — per-model treemap, one row per model
  - chunk_waterfall.png      — waterfall: cumulative reuse progress
  - chunk_sunburst.png       — sunburst: model ring → chunk ring
  - chunk_heatmap.png        — heatmap: model vs chunk index, color = size/reuse

Usage:  python3 plot_flamegraphs.py [results.csv]
"""

import csv
import sys
import os

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.patches as patches
import numpy as np
import squarify

DATA_DIR = os.path.dirname(os.path.abspath(__file__))
CSV_PATH = os.path.join(DATA_DIR, sys.argv[1] if len(sys.argv) > 1 else "results.csv")

rows = []
with open(CSV_PATH) as f:
    reader = csv.DictReader(
        (ln for ln in f if not ln.startswith("#")),
        fieldnames=[
            "model", "quantization", "model_size_mb", "payload_size_mb",
            "chunk_target_mb", "total_chunks", "reused_chunks", "fetched_chunks",
            "reused_bytes_mb", "fetched_bytes_mb", "full_download_mb",
            "delta_download_mb", "bandwidth_saved_pct",
        ],
    )
    next(reader)
    for row in reader:
        rows.append({k: (float(v) if k not in ("model", "quantization") else v) for k, v in row.items()})

models = [r["model"] for r in rows]
total_chunks = [int(r["total_chunks"]) for r in rows]
reused_chunks = [int(r["reused_chunks"]) for r in rows]
fetched_chunks = [int(r["fetched_chunks"]) for r in rows]
saved_pct = [r["bandwidth_saved_pct"] for r in rows]
full_mb = [r["full_download_mb"] for r in rows]
delta_mb = [r["delta_download_mb"] for r in rows]
reused_mb = [r["reused_bytes_mb"] for r in rows]
fetched_mb = [r["fetched_bytes_mb"] for r in rows]

# --- Chart 1: Per-model chunk treemap (flamegraph-style) ---
# One subplot per model, showing chunk sizes with green/red coloring
fig, axes = plt.subplots(len(rows), 1, figsize=(14, 0.5 * len(rows) + 1))
if len(rows) == 1:
    axes = [axes]

COLOR_REUSED = "#2ecc71"
COLOR_FETCH = "#e74c3c"

for ax, row in zip(axes, rows):
    model = row["model"]
    n_total = int(row["total_chunks"])
    n_reused = int(row["reused_chunks"])
    n_fetch = int(row["fetched_chunks"])

    # Create chunk sizes: each chunk ~full_mb / total_chunks (approximate)
    # For visualization, use chunk_target_mb as the unit
    avg_chunk_mb = row["payload_size_mb"] / n_total
    chunk_sizes = [avg_chunk_mb] * n_total

    # Colors: first n_reused are green, rest are red
    colors = [COLOR_REUSED] * n_reused + [COLOR_FETCH] * n_fetch

    squarify.plot(sizes=chunk_sizes, color=colors, ax=ax, alpha=0.8,
                  text_kwargs={"fontsize": 6}, pad=0.5)
    ax.set_title(
        f"{model:12s}  {row['payload_size_mb']:.0f} MB  |  "
        f"{n_reused}/{n_total} chunks cached  |  {row['bandwidth_saved_pct']:.1f}% saved",
        fontsize=8, loc="left", fontweight="bold"
    )
    ax.set_xticks([])
    ax.set_yticks([])

plt.suptitle(
    "SISR Chunk Reuse Treemap - PleIAs/Baguettotron-GGUF\n"
    "Xeon 2-core, 7.8GB RAM, Ubuntu 22.04 | "
    "16MB FastCDC chunks | green=reused, red=downloaded",
    fontsize=9, fontweight="bold"
)
plt.tight_layout(rect=[0, 0, 1, 0.96])
plt.savefig(os.path.join(DATA_DIR, "chunk_flamegraph.png"), dpi=150)
plt.close()

# --- Chart 2: Cumulative reuse waterfall ---
fig, ax = plt.subplots(figsize=(12, 6))
x = np.arange(len(models))
cumulative_reused = np.array(reused_mb)
cumulative_fetched = np.array(fetched_mb)

# Stacked bar: reused (bottom) + fetched (top)
ax.bar(x, reused_mb, label="Reused (cached)", color=COLOR_REUSED, edgecolor="black", linewidth=0.5)
ax.bar(x, fetched_mb, bottom=reused_mb, label="Fetched (downloaded)", color=COLOR_FETCH,
       edgecolor="black", linewidth=0.5)

# Annotate
for i, row in enumerate(rows):
    ax.text(i, reused_mb[i] / 2, f"{row['reused_chunks']}ch", ha="center", va="center", fontsize=6,
            color="white", fontweight="bold")
    if fetched_mb[i] > 0.5:
        ax.text(i, reused_mb[i] + fetched_mb[i] / 2, f"{row['fetched_chunks']}ch\n{fetched_mb[i]:.0f}MB",
                ha="center", va="center", fontsize=6, color="white", fontweight="bold")

ax.set_xticks(x)
ax.set_xticklabels(models, rotation=45, ha="right", fontsize=8)
ax.set_ylabel("Data volume (MiB)")
ax.set_title(
    "SISR Waterfall: Cumulative Reused vs Fetched per Model\n"
    "Same app-code update (V1->V2), 16MB chunks"
)
ax.legend(fontsize=8)
plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "chunk_waterfall.png"), dpi=150)
plt.close()

# --- Chart 3: Radial chunk distribution (polar bar) ---
fig, ax = plt.subplots(figsize=(9, 9), subplot_kw=dict(projection="polar"))

total_chunks_all = sum(total_chunks)
reused_chunks_all = sum(reused_chunks)
fetched_chunks_all = total_chunks_all - reused_chunks_all
total_mb = sum(full_mb)
reused_mb_all = sum(reused_mb)
fetched_mb_all = sum(fetched_mb)

model_angles = 2 * np.pi / len(rows)

for i, row in enumerate(rows):
    angle = model_angles
    mid_theta = (2 * i + 1) * angle / 2 - np.pi / 2

    n_total = int(row["total_chunks"])
    n_reused = int(row["reused_chunks"])
    n_fetch = int(row["fetched_chunks"])

    if n_total > 0:
        frac_reused = n_reused / n_total

        if frac_reused > 0:
            ax.bar(mid_theta - angle * (1 - frac_reused) / 2,
                   height=1, width=angle * frac_reused, bottom=0,
                   color=COLOR_REUSED, alpha=0.7,
                   edgecolor="white", linewidth=0.5)

        if frac_reused < 1:
            ax.bar(mid_theta + angle * frac_reused / 2,
                   height=1, width=angle * (1 - frac_reused), bottom=0,
                   color=COLOR_FETCH, alpha=0.7,
                   edgecolor="white", linewidth=0.5)

    ax.text(mid_theta, 1.12, row["model"], ha="center", va="center", fontsize=8, fontweight="bold")

ax.set_ylim(0, 1.2)
ax.set_rlabel_position(0)
ax.set_yticks([])
ax.set_xticks([])

from matplotlib.patches import Patch
legend_handles = [
    Patch(color=COLOR_REUSED, alpha=0.7, label=f"Reused: {reused_chunks_all}/{total_chunks_all} chunks ({reused_mb_all:.0f} MB)"),
    Patch(color=COLOR_FETCH, alpha=0.7, label=f"Fetched: {fetched_chunks_all}/{total_chunks_all} chunks ({fetched_mb_all:.0f} MB)"),
]
ax.legend(handles=legend_handles, loc="upper left", bbox_to_anchor=(1.02, 1.0), fontsize=9)

ax.set_title(
    "SISR Chunk Distribution — PleIAs/Baguettotron-GGUF\n"
    "Xeon 2-core, 7.8GB RAM | 16MB chunks",
    fontsize=9, pad=20
)

plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "chunk_sunburst.png"), dpi=150, bbox_inches="tight")
plt.close()

# --- Chart 4: Heatmap (model × chunk index, color = reuse status) ---
max_chunks = max(total_chunks)
heatmap_data = np.zeros((len(rows), max_chunks))
for i, row in enumerate(rows):
    n_reused = int(row["reused_chunks"])
    heatmap_data[i, :n_reused] = 1  # 1 = reused
    heatmap_data[i, n_reused:] = 0  # 0 = fetched

fig, ax = plt.subplots(figsize=(14, 5))
im = ax.imshow(heatmap_data, aspect="auto", cmap=plt.cm.RdYlGn, vmin=0, vmax=1,
               interpolation="nearest")
ax.set_xticks(range(0, max_chunks, max(1, max_chunks // 10)))
ax.set_xticklabels([f"C{t}" for t in range(0, max_chunks, max(1, max_chunks // 10))], fontsize=6)
ax.set_yticks(range(len(models)))
ax.set_yticklabels(models, fontsize=8)
ax.set_xlabel("Chunk index (left→right = position in payload)")
ax.set_title(
    "SISR Chunk Reuse Heatmap - Green=reused, Red=fetched\n"
    "App-code update across 9 GGUF quantizations"
)
cbar = plt.colorbar(im, ax=ax, shrink=0.8)
cbar.set_ticks([0.25, 0.75])
cbar.set_ticklabels(["Fetched", "Reused"])
plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "chunk_heatmap.png"), dpi=150)
plt.close()

print(f"Generated 4 flamegraph-style charts for {len(rows)} models:")
print(f"  chunk_flamegraph.png  — per-model treemap (flamegraph-style)")
print(f"  chunk_waterfall.png   — stacked waterfall")
print(f"  chunk_sunburst.png    — sunburst distribution")
print(f"  chunk_heatmap.png     — model × chunk index heatmap")
