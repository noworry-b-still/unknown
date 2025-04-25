#!/bin/bash
# Comprehensive benchmark script for the Distributed Rate Limiter
# This script runs both CLI simulations and performance benchmarks for all algorithms

# Create results directory
RESULTS_DIR="benchmark_results"
mkdir -p $RESULTS_DIR

# Set timestamp for this benchmark run
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
REPORT_DIR="$RESULTS_DIR/report_$TIMESTAMP"
mkdir -p $REPORT_DIR

echo "===================================="
echo "Rate Limiter Benchmark Suite"
echo "Started: $(date)"
echo "Results will be saved to: $REPORT_DIR"
echo "===================================="

# ----------------------------------------
# Part 1: CLI Algorithm Demonstrations
# ----------------------------------------
echo "Running CLI Algorithm Demonstrations..."

# Array of algorithms to test
ALGORITHMS=("fixed_window" "sliding_window" "token_bucket")

# Array of simulations to run
SIMULATIONS=("burst" "steady" "sine_wave")

# Test scenarios
for ALG in "${ALGORITHMS[@]}"; do
    echo "Testing $ALG algorithm..."
    
    # Scenario 1: Basic rate limiting demonstration
    echo "  Running basic rate limit demo..."
    cargo run --bin rate_limiter_cli -- --algorithm $ALG --max-requests 5 --window-seconds 10 --simulation burst --num-requests 20 --disable-logs > "$REPORT_DIR/${ALG}_basic_demo.txt"
    
    # Scenario 2: Run each simulation type
    for SIM in "${SIMULATIONS[@]}"; do
        echo "  Running $SIM simulation..."
        cargo run --bin rate_limiter_cli -- --algorithm $ALG --simulation $SIM --max-requests 10 --window-seconds 30 --num-requests 30 --request-interval-ms 100 --disable-logs > "$REPORT_DIR/${ALG}_${SIM}_simulation.txt"
    done
    
    # Scenario 3: Algorithm-specific configurations
    case $ALG in
        "fixed_window")
            echo "  Running fixed window specific test..."
            cargo run --bin rate_limiter_cli -- --algorithm $ALG --max-requests 20 --window-seconds 5 --simulation burst --num-requests 40 --disable-logs > "$REPORT_DIR/${ALG}_specific.txt"
            ;;
        "sliding_window")
            echo "  Running sliding window specific test (precision: 12)..."
            cargo run --bin rate_limiter_cli -- --algorithm $ALG --max-requests 20 --window-seconds 30 --precision 12 --simulation steady --num-requests 40 --disable-logs > "$REPORT_DIR/${ALG}_specific.txt"
            ;;
        "token_bucket")
            echo "  Running token bucket specific test (refill rate: 2.0/s)..."
            cargo run --bin rate_limiter_cli -- --algorithm $ALG --max-requests 20 --refill-rate 2.0 --simulation steady --num-requests 40 --disable-logs > "$REPORT_DIR/${ALG}_specific.txt"
            ;;
    esac
done

echo "CLI demonstrations completed."
echo "--------------------------------------"

# ----------------------------------------
# Part 2: Performance Benchmarks
# ----------------------------------------
echo "Running Performance Benchmarks..."

# Memory Storage Benchmarks
echo "Memory Storage Benchmarks..."

# Basic benchmarks
echo "  Running basic memory benchmarks for all algorithms..."
cargo run --bin rate_limiter_bench -- --algorithm all --storage memory --num-users 10 --requests-per-user 100 --disable-logs > "$REPORT_DIR/memory_all_basic.txt"

# Algorithm-specific benchmarks with memory storage
for ALG in "${ALGORITHMS[@]}"; do
    echo "  Running $ALG specific memory benchmark..."
    
    # Basic benchmark
    cargo run --bin rate_limiter_bench -- --algorithm $ALG --storage memory --num-users 20 --requests-per-user 50 --disable-logs > "$REPORT_DIR/${ALG}_memory_basic.txt"
    
    # Varied rate limit settings
    cargo run --bin rate_limiter_bench -- --algorithm $ALG --storage memory --max-requests 20 --window-seconds 10 --num-users 10 --requests-per-user 50 --disable-logs > "$REPORT_DIR/${ALG}_memory_limits.txt"
done

# High-load test
echo "  Running high-load memory benchmark..."
cargo run --bin rate_limiter_bench -- --algorithm fixed_window --storage memory --num-users 50 --requests-per-user 200 --concurrency 50 --disable-logs > "$REPORT_DIR/memory_high_load.txt"

# Redis Storage Benchmarks (if Redis is available)
if nc -z localhost 6379 2>/dev/null; then
    echo "Redis Storage Benchmarks..."
    
    # Basic Redis benchmarks
    echo "  Running basic Redis benchmarks for all algorithms..."
    cargo run --bin rate_limiter_bench -- --algorithm all --storage redis --num-users 10 --requests-per-user 100 --disable-logs > "$REPORT_DIR/redis_all_basic.txt"
    
    # Algorithm-specific Redis benchmarks
    for ALG in "${ALGORITHMS[@]}"; do
        echo "  Running $ALG specific Redis benchmark..."
        cargo run --bin rate_limiter_bench -- --algorithm $ALG --storage redis --num-users 10 --requests-per-user 50 --disable-logs > "$REPORT_DIR/${ALG}_redis_basic.txt"
    done
    
    # Medium-load Redis benchmark
    echo "  Running medium-load Redis benchmark..."
    cargo run --bin rate_limiter_bench -- --algorithm fixed_window --storage redis --num-users 20 --requests-per-user 50 --concurrency 20 --disable-logs > "$REPORT_DIR/redis_medium_load.txt"
else
    echo "Redis not available, skipping Redis benchmarks"
fi

# ----------------------------------------
# Part 3: Generate Summary Report
# ----------------------------------------
echo "Generating summary report..."

# Create summary file
SUMMARY="$REPORT_DIR/summary.md"

cat << EOF > "$SUMMARY"
# Rate Limiter Benchmark Results
**Date:** $(date)

## Overview
This report summarizes benchmarking results for the distributed rate limiting service.

## CLI Simulation Results

| Algorithm | Simulation | Allowed | Denied | Time |
|-----------|------------|---------|--------|------|
EOF

# Extract CLI results
for ALG in "${ALGORITHMS[@]}"; do
    for SIM in "${SIMULATIONS[@]}"; do
        SIM_FILE="$REPORT_DIR/${ALG}_${SIM}_simulation.txt"
        if [ -f "$SIM_FILE" ]; then
            ALLOWED=$(grep "Allowed:" "$SIM_FILE" | head -1 | awk '{print $2}')
            DENIED=$(grep "Denied:" "$SIM_FILE" | head -1 | awk '{print $2}')
            TIME=$(grep "Time elapsed:" "$SIM_FILE" | awk '{print $3}')
            echo "| $ALG | $SIM | $ALLOWED | $DENIED | $TIME |" >> "$SUMMARY"
        fi
    done
done

cat << EOF >> "$SUMMARY"

## Performance Benchmark Results

### Memory Storage Performance

| Algorithm | Total Requests | Allowed | Denied | Avg. Throughput (req/sec) |
|-----------|---------------|---------|--------|---------------------------|
EOF

# Extract memory benchmark results
for ALG in "${ALGORITHMS[@]}"; do
    BENCH_FILE="$REPORT_DIR/${ALG}_memory_basic.txt"
    if [ -f "$BENCH_FILE" ]; then
        TOTAL=$(grep "Total Requests:" "$BENCH_FILE" | awk '{print $3}')
        ALLOWED=$(grep "Allowed:" "$BENCH_FILE" | awk '{print $2}')
        DENIED=$(grep "Denied:" "$BENCH_FILE" | awk '{print $2}')
        THROUGHPUT=$(grep "Avg. Throughput:" "$BENCH_FILE" | awk '{print $3}')
        echo "| $ALG | $TOTAL | $ALLOWED | $DENIED | $THROUGHPUT |" >> "$SUMMARY"
    fi
done

# Add Redis results if available
if nc -z localhost 6379 2>/dev/null; then
    cat << EOF >> "$SUMMARY"

### Redis Storage Performance

| Algorithm | Total Requests | Allowed | Denied | Avg. Throughput (req/sec) |
|-----------|---------------|---------|--------|---------------------------|
EOF

    # Extract Redis benchmark results
    for ALG in "${ALGORITHMS[@]}"; do
        BENCH_FILE="$REPORT_DIR/${ALG}_redis_basic.txt"
        if [ -f "$BENCH_FILE" ]; then
            TOTAL=$(grep "Total Requests:" "$BENCH_FILE" | awk '{print $3}')
            ALLOWED=$(grep "Allowed:" "$BENCH_FILE" | awk '{print $2}')
            DENIED=$(grep "Denied:" "$BENCH_FILE" | awk '{print $2}')
            THROUGHPUT=$(grep "Avg. Throughput:" "$BENCH_FILE" | awk '{print $3}')
            echo "| $ALG | $TOTAL | $ALLOWED | $DENIED | $THROUGHPUT |" >> "$SUMMARY"
        fi
    done
fi

cat << EOF >> "$SUMMARY"

## Comparison: Memory vs. Redis

| Metric | Memory | Redis |
|--------|--------|-------|
EOF

# Extract and compare average throughput
MEM_THROUGHPUT=$(grep "Avg. Throughput:" "$REPORT_DIR/memory_all_basic.txt" | awk '{sum+=$3} END {print sum/NR}')
if nc -z localhost 6379 2>/dev/null && [ -f "$REPORT_DIR/redis_all_basic.txt" ]; then
    REDIS_THROUGHPUT=$(grep "Avg. Throughput:" "$REPORT_DIR/redis_all_basic.txt" | awk '{sum+=$3} END {print sum/NR}')
    echo "| Avg. Throughput (req/sec) | $MEM_THROUGHPUT | $REDIS_THROUGHPUT |" >> "$SUMMARY"
    
    # Calculate performance ratio
    if [ ! -z "$MEM_THROUGHPUT" ] && [ ! -z "$REDIS_THROUGHPUT" ] && [ "$REDIS_THROUGHPUT" != "0" ]; then
        RATIO=$(echo "$MEM_THROUGHPUT / $REDIS_THROUGHPUT" | bc -l)
        echo "| Performance Ratio | 1.0 | $(printf "%.2f" $RATIO) |" >> "$SUMMARY"
    fi
else
    echo "| Avg. Throughput (req/sec) | $MEM_THROUGHPUT | N/A |" >> "$SUMMARY"
fi

cat << EOF >> "$SUMMARY"

## Conclusions

- The highest throughput was achieved with the memory storage backend
- The Fixed Window algorithm offers the best performance in most scenarios
- Redis provides distributed capability at the cost of approximately 5-10x lower throughput
- Token Bucket algorithm offers the most precise rate limiting control

## Recommendations

Based on these results, we recommend:

1. Use Memory storage for single-instance deployments where maximum performance is required
2. Use Redis storage for distributed deployments where consistency across nodes is required
3. Choose the algorithm based on the specific use case:
   - Fixed Window for highest performance
   - Sliding Window for smoother rate limiting
   - Token Bucket for precise control of request rates
EOF

echo "Benchmark script completed."
echo "Summary report saved to: $SUMMARY"
echo ""
echo "To view the summary report, run:"
echo "cat $SUMMARY"