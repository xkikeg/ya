use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use winnow::{error::ContextError, Parser};
use ya::parse::{context::FlowIn, flow::seq::flow_sequence, input::Input, spaces::IndentLevel};

// Actually this benchmark is now not needed,
// however, these metrics are quite stable and good to tell about the noise.

fn parse_benchmark(c: &mut Criterion) {
    let mut input: String = "[".to_string();
    for _i in 0..1000 {
        // TODO: plain string unsupported yet.
        // input.push_str("12345,\n");
        // input.push_str("abcde,\n");
        input.push_str("'single\n quoted', ");
        input.push_str("\"double quoted\", ");
    }
    input.push(']');
    c.bench_function("parse", |b| {
        b.iter(|| -> Vec<ya::value::Value<'_>> {
            black_box(
                flow_sequence::<_, _, ContextError>(FlowIn, IndentLevel::initial())
                    .parse(Input::new(&input))
                    .unwrap(),
            )
        })
    });
}

// #[ctor::ctor]
// fn init() {
//     let _ = env_logger::builder().is_test(true).try_init();
// }

criterion_group!(benches, parse_benchmark);
criterion_main!(benches);
