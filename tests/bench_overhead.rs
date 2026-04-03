//! DevPulse self-overhead benchmark
//! Measures CPU time and memory usage of core operations

use devpulse::app::App;
use devpulse::config::Config;
use std::time::{Duration, Instant};

#[test]
fn bench_tick_overhead() {
    let mut app = App::new(Config::default());

    // Warm up (first tick establishes sysinfo baseline)
    app.tick();
    std::thread::sleep(Duration::from_millis(100));

    // Measure 10 consecutive ticks
    let iterations = 10;
    let start = Instant::now();
    for _ in 0..iterations {
        app.tick();
    }
    let elapsed = start.elapsed();
    let per_tick = elapsed / iterations;

    println!("\n=== DevPulse Overhead Benchmark ===");
    println!("tick() x {}: total {:?}", iterations, elapsed);
    println!("tick() avg: {:?} per call", per_tick);
    println!("Ports found: {}", app.port_entries.len());
    println!("Processes found: {}", app.process_list.len());

    // tick() should complete well under 500ms (the startup target)
    // For a 2-second refresh rate, tick must be << 2s
    assert!(
        per_tick < Duration::from_millis(500),
        "tick() took {:?}, exceeds 500ms budget",
        per_tick
    );

    // Should be under 100ms ideally
    if per_tick < Duration::from_millis(100) {
        println!("PASS: tick() is fast ({:?} < 100ms)", per_tick);
    } else {
        println!(
            "WARN: tick() is slow ({:?} >= 100ms), investigate",
            per_tick
        );
    }
}

#[test]
fn bench_memory_usage() {
    use sysinfo::{Pid, System};

    let mut sys = System::new();
    let my_pid = Pid::from_u32(std::process::id());

    // Measure before App creation
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mem_before = sys
        .process(my_pid)
        .map(|p| p.memory())
        .unwrap_or(0);

    // Create app and run several ticks
    let mut app = App::new(Config::default());
    app.tick();
    std::thread::sleep(Duration::from_millis(200));
    app.tick();
    app.tick();

    // Measure after
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mem_after = sys
        .process(my_pid)
        .map(|p| p.memory())
        .unwrap_or(0);

    let mem_delta_mb = (mem_after.saturating_sub(mem_before)) as f64 / 1_000_000.0;

    println!("\n=== Memory Usage ===");
    println!("Before App: {:.1} MB", mem_before as f64 / 1_000_000.0);
    println!("After 3 ticks: {:.1} MB", mem_after as f64 / 1_000_000.0);
    println!("Delta: {:.1} MB", mem_delta_mb);
    println!("Processes tracked: {}", app.process_list.len());
    println!("Ports tracked: {}", app.port_entries.len());

    // App memory should be well under 50MB target
    assert!(
        mem_after < 50_000_000,
        "Memory usage {:.1}MB exceeds 50MB target",
        mem_after as f64 / 1_000_000.0
    );
}

#[test]
fn bench_render_overhead() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = App::new(Config::default());
    app.tick();

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    // Measure 100 render cycles
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        terminal
            .draw(|f| devpulse::ui::draw(f, &app))
            .unwrap();
    }
    let elapsed = start.elapsed();
    let per_render = elapsed / iterations;

    println!("\n=== Render Benchmark ===");
    println!("draw() x {}: total {:?}", iterations, elapsed);
    println!("draw() avg: {:?} per frame", per_render);

    // At 30fps target, each frame budget is ~33ms
    assert!(
        per_render < Duration::from_millis(33),
        "draw() took {:?}, exceeds 33ms frame budget (30fps)",
        per_render
    );

    if per_render < Duration::from_millis(5) {
        println!("PASS: render is fast ({:?} < 5ms)", per_render);
    } else {
        println!(
            "WARN: render is moderate ({:?} >= 5ms)",
            per_render
        );
    }
}

#[test]
fn bench_filter_overhead() {
    let mut app = App::new(Config::default());
    app.tick();

    let process_count = app.process_list.len();

    // Measure filter application on real data
    app.global_filter.set_query("node");

    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _filtered: Vec<_> = app
            .process_list
            .iter()
            .filter(|p| app.global_filter.matches(&p.name) || app.global_filter.matches(&p.command))
            .collect();
    }
    let elapsed = start.elapsed();
    let per_filter = elapsed / iterations;

    println!("\n=== Filter Benchmark ===");
    println!(
        "Filter {} processes x {}: total {:?}",
        process_count, iterations, elapsed
    );
    println!("Filter avg: {:?} per pass", per_filter);

    // Filter should be sub-millisecond
    assert!(
        per_filter < Duration::from_millis(10),
        "Filter took {:?}, too slow for interactive use",
        per_filter
    );
}
