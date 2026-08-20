#!/usr/bin/env python3
"""Plot SISR delta OTA bandwidth savings from results.csv.

Generates:
  - bandwidth_savings.png  — bar chart of bandwidth saved (%) by model
  - reuse_matrix.png       — dual-axis: reused vs fetched bytes
  - delta_vs_full.png       — scatter: delta download vs full download size

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
    next(reader)  # skip the header line (first non-comment row is fieldnames)
    for row in reader:
        rows.append({k: float(v) if k != "model" and k != "quantization" else v for k, v in row.items()})

models = [r["model"] for r in rows]
saved = [r["bandwidth_saved_pct"] for r in rows]
reused_mb = [r["reused_bytes_mb"] for r in rows]
fetched_mb = [r["fetched_bytes_mb"] for r in rows]
delta_mb = [r["delta_download_mb"] for r in rows]
full_mb = [r["full_download_mb"] for r in rows]

# --- Chart 1: Bandwidth savings bar chart ---
fig, ax = plt.subplots(figsize=(10, 5))
colors = ["#2ecc71" if s > 90 else "#3498db" if s > 80 else "#e74c3c" for s in saved]
bars = ax.bar(range(len(models)), saved, color=colors, edgecolor="black", linewidth=0.5)
ax.set_xticks(range(len(models)))
ax.set_xticklabels(models, rotation=45, ha="right", fontsize=8)
ax.set_ylabel("Bandwidth Saved (%)")
ax.set_title("SISR Delta OTA: Bandwidth Savings by GGUF Quantization")
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
ax1.set_title("SISR Delta: Reused vs Fetched per Model (app code update)")
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
ax.set_title("SISR vs Full Download: 9 GGUF Quantizations")
ax.legend()
plt.tight_layout()
plt.savefig(os.path.join(DATA_DIR, "delta_vs_full.png"), dpi=150)
plt.close()

print(f"Plotted {len(rows)} models → bandwidth_savings.png, reuse_vs_fetch.png, delta_vs_full.png")
