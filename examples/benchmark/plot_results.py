#!/usr/bin/env python3
"""Plot SISR delta OTA benchmark results from results.csv.

Generates:
  - bandwidth_savings.png  — bar chart of bandwidth saved (%) by model
  - reuse_vs_fetch.png     — dual-bar: reused vs fetched data (MiB)
  - delta_vs_full.png      — scatter: delta download vs full download
  - summary_panel.png      — 2x2 summary: savings, reuse, chunk counts, model sizes

Usage:  python3 plot_results.py [results.csv]
"""

import csv
import sys
import os

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

DATA_DIR = os.path.dirname(os.path.abspath(__file__))
CSV_PATH = os.path.join(DATA_DIR, sys.argv[1] if len(sys.argv) > 1 else "results.csv")

# Machine + environment metadata (embedded so the graph is self-describing)
MACHINE = {
    "cpu": "Intel Xeon (Skylake), 2 cores",
    "ram_gb": 7.8,
    "os": "Ubuntu 22.04.2 LTS",
    "rust": "1.97.1 (musl)",
    "chunk_target_mb": 16,
}

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
        rows.append({k: float(v) if k not in ("model", "quantization") else v for k, v in row.items()})

models = [r["model"] for r in rows]
saved = [r["bandwidth_saved_pct"] for r in rows]
reused_mb = [r["reused_bytes_mb"] for r in rows]
fetched_mb = [r["fetched_bytes_mb"] for r in rows]
delta_mb = [r["delta_download_mb"] for r in rows]
full_mb = [r["full_download_mb"] for r in rows]
total_chunks = [r["total_chunks"] for r in rows]

# --- Chart 1: Bandwidth savings bar chart ---
fig, ax = plt.subplots(figsize=(10, 5))
colors = ["#2ecc71" if s >= 95 else "#3498db" if s >= 85 else "#e74c3c" for s in saved]
bars = ax.bar(range(len(models)), saved, color=colors, edgecolor="black", linewidth=0.5)
ax.set_xticks(range(len(models)))
ax.set_xticklabels(models, rotation=45, ha="right", fontsize=8)
ax.set_ylabel("Bandwidth Saved (%)")
ax.set_title(
    "SISR Delta OTA: Bandwidth Savings by GGUF Quantization\n"
    f"{MACHINE['cpu']} | {MACHINE['ram_gb']}GB RAM | {MACHINE['os']} | "
    f"FastCDC {MACHINE['chunk_target_mb']}MB chunks"
)
ax.set_ylim(70, 105)
for bar, val in zip(bars, saved):
    ax.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 0.5,
            f"{val:.1f}%", ha="center", va="bottom", fontsize=8)
plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "bandwidth_savings.png"), dpi=150)
plt.close()

# --- Chart 2: Dual-axis reused vs fetched ---
fig, ax1 = plt.subplots(figsize=(10, 5))
x = np.arange(len(models))
w = 0.35
ax1.bar(x - w/2, reused_mb, w, label="Reused (cached)", color="#2ecc71", edgecolor="black", linewidth=0.5)
ax1.bar(x + w/2, fetched_mb, w, label="Fetched (downloaded)", color="#e74c3c", edgecolor="black", linewidth=0.5)
ax1.set_xticks(x)
ax1.set_xticklabels(models, rotation=45, ha="right", fontsize=8)
ax1.set_ylabel("Data (MiB)")
ax1.set_title(
    "SISR Delta: Reused vs Fetched per Model (single-line app edit)\n"
    f"{MACHINE['cpu']} | {MACHINE['ram_gb']}GB RAM | chunk={MACHINE['chunk_target_mb']}MB"
)
ax1.legend()
plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "reuse_vs_fetch.png"), dpi=150)
plt.close()

# --- Chart 3: Scatter delta vs full ---
fig, ax = plt.subplots(figsize=(7, 6))
ax.scatter(full_mb, delta_mb, s=120, c=saved, cmap="RdYlGn", edgecolors="black", linewidth=0.5)
for i, m in enumerate(models):
    ax.annotate(m, (full_mb[i], delta_mb[i]), fontsize=7, xytext=(5, 3), textcoords="offset points")
ax.plot([0, max(full_mb) * 1.1], [0, max(full_mb) * 1.1], "k--", alpha=0.3, label="y=x (no savings)")
ax.set_xlabel("Full download (MiB)")
ax.set_ylabel("Delta download (MiB)")
ax.set_title(
    "SISR vs Full Download: 9 GGUF Quantizations\n"
    f"{MACHINE['cpu']} | {MACHINE['os']}"
)
ax.legend()
plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "delta_vs_full.png"), dpi=150)
plt.close()

# --- Chart 4: 2x2 summary panel ---
fig, ((ax1, ax2), (ax3, ax4)) = plt.subplots(2, 2, figsize=(12, 9))

fig.suptitle(
    "SISR Delta OTA Benchmark — PleIAs/Baguettotron-GGUF on "
    f"{MACHINE['cpu']}, {MACHINE['ram_gb']}GB RAM, {MACHINE['os']}",
    fontsize=11, fontweight="bold"
)

# Savings %
bars = ax1.bar(range(len(models)), saved, color=colors, edgecolor="black", linewidth=0.5)
ax1.set_xticks(range(len(models)))
ax1.set_xticklabels(models, rotation=45, ha="right", fontsize=7)
ax1.set_ylabel("Saved (%)")
ax1.set_title("Bandwidth Saved (%)")
ax1.set_ylim(70, 105)

# Reused vs fetched
ax2.bar(x - w/2, reused_mb, w, label="Reused", color="#2ecc71")
ax2.bar(x + w/2, fetched_mb, w, label="Fetched", color="#e74c3c")
ax2.set_xticks(x)
ax2.set_xticklabels(models, rotation=45, ha="right", fontsize=7)
ax2.set_ylabel("Data (MiB)")
ax2.set_title("Reused vs Fetched (MiB)")
ax2.legend(fontsize=7)

# Chunk counts
ax3.bar(range(len(models)), total_chunks, color="#9b59b6", edgecolor="black", linewidth=0.5)
ax3.set_xticks(range(len(models)))
ax3.set_xticklabels(models, rotation=45, ha="right", fontsize=7)
ax3.set_ylabel("Chunks")
ax3.set_title(f"Total Chunks (target={MACHINE['chunk_target_mb']}MB/chunk)")

# Model sizes
ax4.bar(range(len(models)), [r["model_size_mb"] for r in rows], color="#34495e", edgecolor="black", linewidth=0.5)
ax4.set_xticks(range(len(models)))
ax4.set_xticklabels(models, rotation=45, ha="right", fontsize=7)
ax4.set_ylabel("GGUF size (MB)")
ax4.set_title("GGUF Model Sizes")

plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "summary_panel.png"), dpi=150)
plt.close()

print(f"Plotted {len(rows)} models → bandwidth_savings.png, reuse_vs_fetch.png, "
      f"delta_vs_full.png, summary_panel.png")

