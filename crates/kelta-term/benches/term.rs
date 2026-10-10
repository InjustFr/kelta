//! Criterion benches (BUILD_PLAN §4 L1): parse throughput (≥ 200 MB/s) and snapshot encode of
//! 200x60 + 1000 history lines (≤ 15 ms, ≤ 150 KB).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use kelta_term::model::TermModel;

#[path = "../tests/it/common/workload.rs"]
mod workload;

fn parse(c: &mut Criterion) {
    let log = workload::build_log(20_000);
    let mut g = c.benchmark_group("parse");
    g.throughput(Throughput::Bytes(log.len() as u64));
    g.bench_function("build_log_200x60", |b| {
        let mut m = TermModel::new(200, 60, 10_000);
        b.iter(|| m.feed(std::hint::black_box(&log)));
    });
    g.finish();
}

fn snapshot(c: &mut Criterion) {
    let mut m = TermModel::new(200, 60, 3000);
    m.feed(&workload::build_log(1100));
    let mut out = Vec::new();
    c.bench_function("snapshot_200x60_1000", |b| {
        b.iter(|| {
            out.clear();
            let _ = m.snapshot_into(1000, &kelta_term::palette::Palette::default(), &mut out);
        });
    });
}

criterion_group!(benches, parse, snapshot);
criterion_main!(benches);
