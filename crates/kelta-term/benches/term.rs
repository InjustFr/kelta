//! Criterion benches (L1): parse throughput, snapshot encode. Scaffold placeholder.

use criterion::{Criterion, criterion_group, criterion_main};

fn frame_encode(c: &mut Criterion) {
    let payload = vec![b'x'; 64 * 1024];
    c.bench_function("encode_frame_64k", |b| {
        b.iter(|| {
            kelta_proto::term::encode_frame(kelta_proto::term::FRAME_DATA, std::hint::black_box(&payload))
        })
    });
}

criterion_group!(benches, frame_encode);
criterion_main!(benches);
