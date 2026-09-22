//! Profile the solver on pre-parsed inputs, independently of quality checking.
//! See docs/performance.md for reproducible baseline/candidate commands.
#![cfg(feature = "parse")]

#[path = "support/corpus.rs"]
#[allow(dead_code)] // Quality checking is deliberately outside this profiler.
mod corpus;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs::File,
    hint::black_box,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

use rumba_core::{parser::parse_expr, simplify::simplify_mba};

#[derive(Clone, Copy, Default)]
struct Allocations {
    calls: usize,
    bytes: usize,
}

thread_local! {
    static ALLOCATIONS: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn record(size: usize) {
    ALLOCATIONS.with(|counter| {
        if let Some(mut counts) = counter.get() {
            counts.calls += 1;
            counts.bytes += size;
            counter.set(Some(counts));
        }
    });
}

// SAFETY: all operations preserve System's allocation/layout contracts. The
// counter is allocation-free thread-local storage and never touches the blocks.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let mut repeat = 1;
    let mut allocations = false;
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repeat" => repeat = args.next().expect("--repeat COUNT").parse().unwrap(),
            "--allocations" => allocations = true,
            "--outputs" => {
                output = Some(BufWriter::new(
                    File::create(args.next().expect("--outputs PATH")).unwrap(),
                ));
            }
            _ => panic!("unknown argument: {arg}"),
        }
    }
    assert!(repeat > 0);

    let mut total = Duration::ZERO;
    let mut total_allocations = Allocations::default();
    for dataset in corpus::DATASETS {
        let rows = corpus::rows(dataset.name, dataset.contents);
        let cases: Vec<_> = rows
            .iter()
            .map(|row| parse_expr(row.mba).unwrap())
            .collect();
        let mut elapsed = Duration::ZERO;
        let mut counts = Allocations::default();
        for iteration in 0..repeat {
            // Clone outside both the timed region and allocation counting.
            let inputs = cases.clone();
            for (row, expression) in rows.iter().zip(inputs) {
                if allocations {
                    ALLOCATIONS.set(Some(Allocations::default()));
                }
                let start = Instant::now();
                let result = black_box(simplify_mba(black_box(expression), 64));
                elapsed += start.elapsed();
                if let Some(measured) = ALLOCATIONS.replace(None) {
                    counts.calls += measured.calls;
                    counts.bytes += measured.bytes;
                }
                if iteration == 0
                    && let Some(output) = &mut output
                {
                    writeln!(output, "{}\t{result:?}", row.source).unwrap();
                }
            }
        }
        eprintln!(
            "{} count={} ns={} allocations={} bytes={}",
            dataset.name,
            cases.len() * repeat,
            elapsed.as_nanos(),
            counts.calls,
            counts.bytes,
        );
        total += elapsed;
        total_allocations.calls += counts.calls;
        total_allocations.bytes += counts.bytes;
    }
    eprintln!(
        "total ns={} allocations={} bytes={}",
        total.as_nanos(),
        total_allocations.calls,
        total_allocations.bytes,
    );
    if let Some(mut output) = output {
        output.flush().unwrap();
    }
}
