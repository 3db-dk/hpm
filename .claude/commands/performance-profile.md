# Performance Profile Command

Executes performance analysis and profiling of the HPM codebase.

## Usage
```
/performance-profile [--bench] [--flamegraph] [--build-times]
```

## Options
- `--bench`: Run benchmarks and performance tests
- `--flamegraph`: Generate flamegraph for CPU profiling
- `--build-times`: Analyze build and compilation times

## Description
Performs comprehensive performance analysis including benchmarking, profiling, and build time optimization.

## Process
1. Execute cargo bench for performance benchmarks
2. Analyze build times with cargo build --timings
3. Generate CPU profiles for critical code paths
4. Identify memory allocation patterns
5. Review async/await performance characteristics
6. Compare against performance baselines

## Output
- Benchmark results with statistical analysis
- Build time breakdown and optimization opportunities
- Profiling data with hotspot identification
- Performance recommendations and optimization targets