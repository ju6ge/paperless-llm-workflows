The scripts directory contains code to evaluate benchmarking results to make the results easier to reason about

# Histogram Model Comparision

``` sh
python eval-bench-results.py ../benchmark_results --output_plot "out.png"
```

Produces `out.png` with a histogram overview over the `success`, `failure` and `error` counts for each benchmark type and overall for all benchmarked models.

# Model comparision table

``` sh
python eval-bench-results.py ../benchmark_results --output_json "eval.json"
typst compile model-comparision-table.typ --input benchmark_stats=eval.json table.svg
```

Produces `table.svg` displaying and table of the models success rate ordered by the overall metric. 


