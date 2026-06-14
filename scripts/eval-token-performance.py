from pydantic import BaseModel
from typing import Optional, List, Dict, Any

import matplotlib.pyplot as plt
import numpy as np

import argparse
import os
import sys

from model import BenchmarkResult, TokenGenerationStats, BENCHMARK_TYPES

PHASES = ["prompt", "inject", "sample", "overall"]
PHASE_LABELS = {
    "prompt": "Prompt",
    "inject": "Inject",
    "sample": "Sample",
    "overall": "Overall",
}
PHASE_COLORS = {
    "prompt": "#1f77b4",  # blue
    "inject": "#2ca02c",  # green
    "sample": "#ff7f0e",  # orange
    "overall": "#9467bd",  # purple
}


def load_all_results(directory: str) -> Dict[str, BenchmarkResult]:
    results = {}
    for filename in os.listdir(directory):
        if filename.endswith(".json"):
            model_name = filename[:-5]
            filepath = os.path.join(directory, filename)
            with open(filepath) as f:
                result_data = BenchmarkResult.model_validate_json(f.read())
                results[model_name] = result_data
    return results


def compute_tps(stats: TokenGenerationStats, phase: str) -> float:
    """Compute TPS for a given phase from token stats."""
    if phase == "prompt":
        if stats.prompt_elapsed_ms <= 0:
            return 0.0
        return stats.prompt_tokens / (stats.prompt_elapsed_ms / 1000.0)
    elif phase == "inject":
        if stats.injected_elapsed_ms <= 0:
            return 0.0
        return stats.injected_tokens / (stats.injected_elapsed_ms / 1000.0)
    elif phase == "sample":
        if stats.sampled_elapsed_ms <= 0:
            return 0.0
        return stats.sampled_tokens / (stats.sampled_elapsed_ms / 1000.0)
    elif phase == "overall":
        total_ms = stats.injected_elapsed_ms + stats.sampled_elapsed_ms
        if total_ms <= 0:
            return 0.0
        return (stats.injected_tokens + stats.sampled_tokens) / (total_ms / 1000.0)
    return 0.0


def aggregate_stats(
    results: Dict[str, BenchmarkResult],
) -> Dict[str, Dict[str, Dict[str, Dict[str, float]]]]:
    """
    Aggregate TPS stats grouped by (benchmark_type, model, phase).
    Returns: {benchmark_type: {model: {phase: {mean, std, min, max, count}}}}
    """
    aggregated = {}
    for bench_type in BENCHMARK_TYPES + ["overall"]:
        aggregated[bench_type] = {}

    for model_name, benchmark in results.items():
        for bench_type in BENCHMARK_TYPES + ["overall"]:
            aggregated[bench_type][model_name] = {}
            for phase in PHASES:
                tps_values: List[float] = []
                for r in benchmark.results:
                    if bench_type != "overall" and r.benchmark_type != bench_type:
                        continue
                    if r.token_stats is None:
                        continue
                    tps = compute_tps(r.token_stats, phase)
                    if tps > 0:
                        tps_values.append(tps)
                if tps_values:
                    arr = np.array(tps_values)
                    aggregated[bench_type][model_name][phase] = {
                        "mean": float(np.mean(arr)),
                        "std": float(np.std(arr)),
                        "min": float(np.min(arr)),
                        "max": float(np.max(arr)),
                        "count": len(tps_values),
                    }
                else:
                    aggregated[bench_type][model_name][phase] = {
                        "mean": 0.0,
                        "std": 0.0,
                        "min": 0.0,
                        "max": 0.0,
                        "count": 0,
                    }

    return aggregated


def print_stats_table(
    stats: Dict[str, Dict[str, Dict[str, Dict[str, float]]]],
    models: List[str],
):
    """Print terminal table with mean, std, min, max per model x task x phase."""
    bench_types = BENCHMARK_TYPES + ["overall"]
    col_width = 16

    for bench_type in bench_types:
        print(f"\n{'=' * 110}")
        label = bench_type if bench_type != "overall" else "Overall"
        print(f"  {label}")
        print(f"{'=' * 110}")
        header = f"{'Model':<{col_width}}"
        for phase in PHASES:
            header += f"  {PHASE_LABELS[phase]:^{col_width - 2}}"
        print(header)
        print(f"{'-' * 110}")

        for model in models:
            if model not in stats[bench_type]:
                continue
            row = f"{model:<{col_width}}"
            for phase in PHASES:
                s = stats[bench_type][model].get(phase, {})
                if s.get("count", 0) == 0:
                    row += f"  {'N/A':^{col_width - 2}}"
                else:
                    row += f"  {s['mean']:8.1f} +/-{s['std']:5.1f} [{s['min']:5.0f}-{s['max']:5.0f} s"
            print(row)


def extract_raw_tps(
    results: Dict[str, BenchmarkResult],
) -> Dict[str, Dict[str, Dict[str, List[float]]]]:
    """
    Extract raw per-document TPS grouped by (benchmark_type, model, phase).
    Returns: {benchmark_type: {model: {phase: [tps_values]}}}
    """
    raw = {}
    for bench_type in BENCHMARK_TYPES + ["overall"]:
        raw[bench_type] = {}

    for model_name, benchmark in results.items():
        for bench_type in BENCHMARK_TYPES + ["overall"]:
            raw[bench_type][model_name] = {phase: [] for phase in PHASES}
            for r in benchmark.results:
                if bench_type != "overall" and r.benchmark_type != bench_type:
                    continue
                if r.token_stats is None:
                    continue
                for phase in PHASES:
                    tps = compute_tps(r.token_stats, phase)
                    if tps > 0:
                        raw[bench_type][model_name][phase].append(tps)

    return raw


def plot_boxplot(
    raw_tps: Dict[str, Dict[str, Dict[str, List[float]]]],
    models: List[str],
    output_plot: str,
    bench_types: Optional[List[str]] = None,
):
    """
    Horizontal box plot layout:
    - subplots for selected benchmark types (or all)
    - Models on y-axis, TPS on x-axis (log scale)
    - 4 box plots per model (phases), vertically offset
    """
    all_bench_types = bench_types if bench_types else BENCHMARK_TYPES + ["overall"]
    nrows = len(all_bench_types)
    n_models = len(models)

    # Each model gets 4 sub-slots (one per phase)
    plt.style.use("Solarize_Light2")

    y_per_model = 4
    phase_sub_offset = 0.55
    model_height = y_per_model * phase_sub_offset
    total_height = n_models * model_height * nrows

    fig, axes = plt.subplots(
        nrows,
        1,
        sharex=True,
        sharey=False,
        figsize=(max(16, 2.5 * n_models), max(10, total_height * 0.5)),
    )
    if nrows == 1:
        axes = np.array([axes])

    for row, bench_type in enumerate(all_bench_types):
        ax = axes[row]
        ax.set_xscale("log")
        label = bench_type if bench_type != "overall" else "Overall"

        all_parts = []
        for mi, model in enumerate(models):
            base_y = mi * model_height + y_per_model / 2
            for phase_idx, phase in enumerate(PHASES):
                data = raw_tps[bench_type].get(model, {}).get(phase, [])
                if not data:
                    data = [np.nan]
                pos = base_y + (phase_idx - (y_per_model - 1) / 2) * phase_sub_offset
                bp = ax.boxplot(
                    [data],
                    positions=[pos],
                    widths=phase_sub_offset * 0.85,
                    patch_artist=True,
                    showfliers=False,
                    vert=False,
                )
                # Color box interior
                for patch in bp["boxes"]:
                    patch.set_facecolor(PHASE_COLORS[phase])
                    patch.set_alpha(0.6)
                for element in ["medians", "whiskers", "caps"]:
                    for line in bp[element]:
                        line.set_color(PHASE_COLORS[phase])
                        line.set_linewidth(1.2)
                for line in bp["boxes"]:
                    line.set_color(PHASE_COLORS[phase])
                    line.set_linewidth(1.0)
                all_parts.append(bp["boxes"][0])

        # Y-axis: model names at group centers
        tick_positions = []
        tick_labels = []
        for mi, model in enumerate(models):
            center = mi * model_height + y_per_model / 2
            tick_positions.append(center)
            tick_labels.append(model)
        ax.set_yticks(tick_positions)
        ax.set_yticklabels(tick_labels, fontsize="medium")
        ax.xaxis.grid(True, which="major", linestyle="--", alpha=0.5)
        ax.set_ylim(
            -model_height * 0.4, (n_models - 0.4) * model_height + y_per_model / 2
        )
        ax.set_ylabel(label, fontsize="medium", fontweight="bold", color="darkblue")

    ax.set_xlabel("Tokens / sec (log scale)", fontsize="medium")

    # Legend: fake patches for each phase
    from matplotlib.patches import Patch

    legend_handles = [
        Patch(facecolor=PHASE_COLORS[ph], alpha=0.6, label=PHASE_LABELS[ph])
        for ph in PHASES
    ]
    fig.legend(
        legend_handles,
        [PHASE_LABELS[ph] for ph in PHASES],
        loc="upper left",
        fontsize="medium",
        ncol=len(PHASES),
    )

    plt.tight_layout()
    title_parts = [(bt if bt != "overall" else "Overall") for bt in all_bench_types]
    fig.suptitle(
        f"Token Generation Performance — {', '.join(title_parts)}",
        fontsize="large",
        y=1.01,
    )
    fig.savefig(output_plot, orientation="landscape", dpi=150, bbox_inches="tight")
    print(f"Plot saved to {output_plot}")


def main():
    args = argparse.ArgumentParser(
        description="Analyze token generation performance from benchmark results"
    )
    args.add_argument(
        "ben_results_path", type=str, help="Directory containing benchmark JSON files"
    )
    args.add_argument(
        "--output_plot", type=str, required=False, help="Output PNG file for box plot"
    )
    args.add_argument(
        "--bench-type",
        type=str,
        action="append",
        default=None,
        help="Benchmark type to plot (repeatable; default=all). "
        f"Valid: {BENCHMARK_TYPES}, overall",
    )
    parsed_args = args.parse_args()

    results = load_all_results(parsed_args.ben_results_path)
    if not results:
        print("No benchmark results found.")
        sys.exit(1)

    models = sorted(results.keys())
    stats = aggregate_stats(results)

    print_stats_table(stats, models)

    if parsed_args.output_plot:
        raw_tps = extract_raw_tps(results)
        plot_boxplot(raw_tps, models, parsed_args.output_plot, parsed_args.bench_type)


if __name__ == "__main__":
    main()
