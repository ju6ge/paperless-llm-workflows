from pydantic import BaseModel, TypeAdapter
from typing import Optional, List, Dict, Any, Tuple
from enum import Enum

import matplotlib.pyplot as plt
import numpy as np

import argparse
import os


class BenchmarkType(str, Enum):
    CustomFieldExtraction = "CustomFieldExtraction"
    CorrespondentSuggest = "CorrespondentSuggest"
    DecideValidCorrespondent = "DecideValidCorrespondent"
    DecideInvalidCorrespondent = "DecideInvalidCorrespondent"


BENCHMARK_TYPES = [
    BenchmarkType.CustomFieldExtraction,
    BenchmarkType.CorrespondentSuggest,
    BenchmarkType.DecideValidCorrespondent,
    BenchmarkType.DecideInvalidCorrespondent,
]


class SingleResult(BaseModel):
    benchmark_type: BenchmarkType
    doc_id: int
    expected_result: Any
    benchmark_result: Any
    success: bool
    error: Optional[str]


class BenchmarkResult(BaseModel):
    model: str
    results: List[SingleResult]


class ResultStats(BaseModel):
    success: int
    failure: int
    error: int


def load_all_results(directory: str) -> Dict[str, BenchmarkResult]:
    results = {}
    for filename in os.listdir(directory):
        if filename.endswith(".json"):
            model_name = filename[:-5]
            with open(os.path.join(directory, filename)) as brf:
                result_data = BenchmarkResult.model_validate_json(brf.read())
                results[model_name] = result_data
    return results


def count_stats(
    results: Dict[str, BenchmarkResult],
) -> Dict[str, Tuple[Dict[BenchmarkType, ResultStats], ResultStats]]:
    stats = {}
    for model, benchmark in results.items():
        btype_stats = {}
        for benchtype in BENCHMARK_TYPES:
            btype_results = filter(lambda x: x.benchmark_type == benchtype, benchmark.results)
            btype_success = len(list(filter(lambda x: x.success, btype_results)))
            btype_results = filter(lambda x: x.benchmark_type == benchtype, benchmark.results)
            btype_failure = len(list(filter(
                lambda x: not x.success and x.error is None, btype_results
            )))
            btype_results = filter(lambda x: x.benchmark_type == benchtype, benchmark.results)
            btype_error = len(list(filter(
                lambda x: not x.success and x.error is not None, btype_results
            )))
            btype_stats[benchtype] = ResultStats(
                success=btype_success, failure=btype_failure, error=btype_error
            )

        all_success = len(list(filter(lambda x: x.success, benchmark.results)))
        all_failure = len(list(filter(
            lambda x: not x.success and x.error is None, benchmark.results
        )))
        all_error = len(list(filter(
            lambda x: not x.success and x.error is not None, benchmark.results
        )))
        stats[model] = (
            btype_stats,
            ResultStats(success=all_success, failure=all_failure, error=all_error),
        )
    return stats

def plot_histogram(stats: Dict[str, Tuple[Dict[BenchmarkType, ResultStats], ResultStats]], output_plot: str):
    plt.style.use("Solarize_Light2")
    fig, ax = plt.subplots(len(BENCHMARK_TYPES) + 1, 1, sharex=True)
    models = stats.keys()
    x = np.arange(len(models))
    width = 0.25
    for i, bentype in enumerate(BENCHMARK_TYPES):
        btype_stats = {
            "success": list( stats[m][0][bentype].success for m in models ),
            "failure": list( stats[m][0][bentype].failure for m in models ),
            "error": list( stats[m][0][bentype].error for m in models ),
        }
        
        multiplier = 0
        for stat, count in btype_stats.items():
            offset = width * multiplier
            rects = ax[i].bar(x + offset, count, width, label=stat)
            ax[i].bar_label(rects, padding=3)
            multiplier += 1

        # Add some text for labels, title and custom x-axis tick labels, etc.
        ax[i].set_ylabel(str(bentype).split(".")[1], fontsize="x-small")
        ax[i].set_ylim(0, 600)
        ax[i].set_xticks(x + width, [""]*len(models))

    all_stats = {
        "success": list( stats[m][1].success for m in models ),
        "failure": list( stats[m][1].failure for m in models ),
        "error": list( stats[m][1].error for m in models ),
    }
    multiplier = 0
    i = len(BENCHMARK_TYPES)
    for stat, count in all_stats.items():
        offset = width * multiplier
        rects = ax[i].bar(x + offset, count, width, label=stat)
        ax[i].bar_label(rects, padding=3)
        multiplier += 1

    # Add some text for labels, title and custom x-axis tick labels, etc.
    #ax.set_ylabel('Length (mm)')
    ax[i].set_ylabel("Overall", fontsize="small")
    ax[i].set_ylim(0, 1500)
    ax[i].set_xticks(x + width, models)

    #plt.tight_layout()
    handles, labels = ax[i].get_legend_handles_labels()
    fig.legend(handles, labels, loc='lower center', ncols=3)
    plt.xticks(rotation=45, ha="right")
    plt.gcf().set_size_inches(16, 10)
    plt.savefig(output_plot, orientation="landscape", dpi=600, bbox_inches='tight')


def main():
    args = argparse.ArgumentParser()
    args.add_argument("ben_results_path", type=str)
    args.add_argument("--output_plot", type=str, required=False)
    args.add_argument("--output_json", type=str, required=False)
    parsed_args = args.parse_args()
    results = load_all_results(parsed_args.ben_results_path)

    stats = count_stats(results)
    # maybe write the results as a summary json?
    if parsed_args.output_json:
        stats_results_json = TypeAdapter(Dict[str, Tuple[Dict[BenchmarkType, ResultStats], ResultStats]]).dump_json(stats, indent=4).decode()
        with open(parsed_args.output_json, "w") as f:
            f.write(stats_results_json)

    if parsed_args.output_plot:
        plot_histogram(stats, parsed_args.output_plot)


if __name__ == "__main__":
    main()
