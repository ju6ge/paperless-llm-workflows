#import "@preview/oxifmt:1.0.0": strfmt

#let data_file = "eval.json";

#if "benchmark_stats" in sys.inputs.keys() {
    data_file = sys.inputs.at("benchmark_stats")
}

#let data = json(data_file)

#set page(width: 25cm, height: 5.67cm + (0.68cm * data.keys().len()) , margin: 0.2em)

#let calc_rate(stats) = {
    let rate = stats.success / (stats.success + stats.failure + stats.error)
    let percent = rate * 100

    let rate_inv = 1 - rate

    table.cell(fill: color.hsv(120deg * rate, 60%, 80%))[ #set align(center); #strfmt("{:.2}", percent)~% ]
}

#let benchmark_types = data.at(data.keys().at(1)).at(0).keys()

#let ordered_data = data.pairs().sorted(key: it => { let v = it.at(1).at(1); return v.success / (v.success + v.failure + v.error) })

#table(
    columns: (auto,) + (1fr, ) * (benchmark_types.len() + 1),
    stroke: none,
    gutter: 0.2em,
    fill: (x, y) => {
        if calc.even(y) {
           rgb("#EEE8D5")
        } else {
           rgb("#FDF6E3")
        }
    },
    table.header([#set align(center+horizon); *Modelname*], ..benchmark_types.map(i => [#align(horizon+center, rotate(270deg, reflow: true)[*#i*])]), [#set align(horizon+center); *Overall*]),
    ..ordered_data.map(d => {
        let model = d.at(0)
        let bench_type_stats = d.at(1).at(0)
        let all_stats = d.at(1).at(1)

        let row = ([#model],)
        for btype in benchmark_types {
            row.push(calc_rate(bench_type_stats.at(btype)))
        }
        row.push(calc_rate(all_stats))
        return row
    }).flatten()
)
