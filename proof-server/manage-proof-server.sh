#!/usr/bin/env bash

################################################################################
# Midnight Proof Server Management Script
# Production-ready script for managing the midnight-proof-server binary
################################################################################

set -euo pipefail

# Script configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY_NAME="midnight-proof-server"
PID_FILE="${PID_FILE:-/tmp/${BINARY_NAME}.pid}"
LOG_FILE="${LOG_FILE:-/tmp/${BINARY_NAME}.log}"
CONFIG_FILE="${CONFIG_FILE:-$SCRIPT_DIR/proof-server.conf}"

# Default configuration values (can be overridden by config file or environment)
PORT="${MIDNIGHT_PROOF_SERVER_PORT:-6300}"
VERBOSE="${MIDNIGHT_PROOF_SERVER_VERBOSE:-false}"
JOB_CAPACITY="${MIDNIGHT_PROOF_SERVER_JOB_CAPACITY:-0}"
NUM_WORKERS="${MIDNIGHT_PROOF_SERVER_NUM_WORKERS:-2}"
JOB_TIMEOUT="${MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT:-600.0}"
NO_FETCH_PARAMS="${MIDNIGHT_PROOF_SERVER_NO_FETCH_PARAMS:-false}"

# Build configuration
CARGO_PROFILE="${CARGO_PROFILE:-release}"
FEATURES="${FEATURES:-}"
RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

# ICICLE library path (for GPU builds)
ICICLE_LIB_PATH="${ICICLE_LIB_PATH:-/opt/icicle/lib}"

# Monitoring configuration
HEALTH_CHECK_INTERVAL="${HEALTH_CHECK_INTERVAL:-30}"
MAX_RESTART_ATTEMPTS="${MAX_RESTART_ATTEMPTS:-3}"
RESTART_DELAY="${RESTART_DELAY:-5}"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

################################################################################
# Utility Functions
################################################################################

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

log_debug() {
    if [[ "${VERBOSE}" == "true" ]]; then
        echo -e "${CYAN}[DEBUG]${NC} $*"
    fi
}

print_header() {
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}$*${NC}"
    echo -e "${BLUE}================================================${NC}"
}

# Load configuration from file if it exists
load_config() {
    if [[ -f "$CONFIG_FILE" ]]; then
        log_info "Loading configuration from: $CONFIG_FILE"
        # shellcheck source=/dev/null
        source "$CONFIG_FILE"
    else
        log_debug "No configuration file found at: $CONFIG_FILE"
    fi
}

# Get the path to the binary
get_binary_path() {
    local binary_path="$PROJECT_ROOT/target/$CARGO_PROFILE/$BINARY_NAME"
    if [[ ! -f "$binary_path" ]]; then
        log_error "Binary not found at: $binary_path"
        log_info "Please build the project first using: ./manage-proof-server.sh build"
        return 1
    fi
    echo "$binary_path"
}

# Check if the process is running
is_running() {
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid=$(cat "$PID_FILE")
        if ps -p "$pid" > /dev/null 2>&1; then
            return 0
        else
            log_warn "PID file exists but process is not running"
            rm -f "$PID_FILE"
            return 1
        fi
    fi
    return 1
}

# Get process PID
get_pid() {
    if [[ -f "$PID_FILE" ]]; then
        cat "$PID_FILE"
    fi
}

################################################################################
# Build Functions
################################################################################

build_binary() {
    print_header "Building Midnight Proof Server"
    
    log_info "Build profile: $CARGO_PROFILE"
    log_info "Features: ${FEATURES:-none}"
    
    cd "$PROJECT_ROOT"
    
    local cargo_args=(
        "build"
        "--package" "midnight-proof-server"
    )
    
    if [[ "$CARGO_PROFILE" == "release" ]]; then
        cargo_args+=("--release")
    fi
    
    if [[ -n "$FEATURES" ]]; then
        cargo_args+=("--features" "$FEATURES")
        
        # If building with GPU features, ensure ICICLE libraries will be built from source
        # or use preinstalled ones via ICICLE_FRONTEND_INSTALL_DIR
        if [[ "$FEATURES" =~ "gpu" ]]; then
            log_warn "Building with GPU features"
            log_warn "ICICLE libraries will be built from source unless ICICLE_FRONTEND_INSTALL_DIR is set"
            log_warn "This may take significant time on first build"
            if [[ -n "${ICICLE_FRONTEND_INSTALL_DIR:-}" ]]; then
                log_info "Using ICICLE libraries from: $ICICLE_FRONTEND_INSTALL_DIR"
            fi
        fi
    fi
    
    log_info "Running: cargo ${cargo_args[*]}"
    cargo "${cargo_args[@]}"
    
    log_info "Build completed successfully"
    local binary_path
    binary_path=$(get_binary_path)
    log_info "Binary location: $binary_path"
}

################################################################################
# Server Management Functions
################################################################################

start_server() {
    print_header "Starting Midnight Proof Server"
    
    if is_running; then
        log_warn "Server is already running (PID: $(get_pid))"
        return 0
    fi
    
    local binary_path
    binary_path=$(get_binary_path) || return 1
    
    # Prepare command line arguments
    local args=()
    args+=("--port" "$PORT")
    
    if [[ "$VERBOSE" == "true" ]]; then
        args+=("--verbose")
    fi
    
    if [[ "$JOB_CAPACITY" -ne 0 ]]; then
        args+=("--job-capacity" "$JOB_CAPACITY")
    fi
    
    if [[ "$NUM_WORKERS" -ne 2 ]]; then
        args+=("--num-workers" "$NUM_WORKERS")
    fi
    
    if [[ "$JOB_TIMEOUT" != "600.0" ]]; then
        args+=("--job-timeout" "$JOB_TIMEOUT")
    fi
    
    if [[ "$NO_FETCH_PARAMS" == "true" ]]; then
        args+=("--no-fetch-params")
    fi
    
    log_info "Starting server with configuration:"
    log_info "  Port: $PORT"
    log_info "  Verbose: $VERBOSE"
    log_info "  Job Capacity: $JOB_CAPACITY"
    log_info "  Num Workers: $NUM_WORKERS"
    log_info "  Job Timeout: $JOB_TIMEOUT"
    log_info "  No Fetch Params: $NO_FETCH_PARAMS"
    log_info "  Log File: $LOG_FILE"
    
    # Build LD_LIBRARY_PATH for ICICLE if it exists (for GPU builds)
    local lib_path="${LD_LIBRARY_PATH:-}"
    
    # IMPORTANT: Unset ICICLE_BACKEND_INSTALL_DIR to use cargo-built libraries
    # The GitHub fork of midnight-zk builds ICICLE v4.0.0 from source
    unset ICICLE_BACKEND_INSTALL_DIR
    
    # First, check for cargo-built ICICLE libraries in target directory
    local cargo_icicle_lib="$PROJECT_ROOT/target/$CARGO_PROFILE/deps/icicle/lib"
    if [[ -d "$cargo_icicle_lib" ]]; then
        log_info "Using cargo-built ICICLE libraries from: $cargo_icicle_lib"
        lib_path="$cargo_icicle_lib:$lib_path"
    elif [[ -d "$ICICLE_LIB_PATH/backend" ]]; then
        log_debug "Using system ICICLE library paths from: $ICICLE_LIB_PATH"
        # Build library path from all backend directories
        for backend_dir in "$ICICLE_LIB_PATH"/backend/*/cuda; do
            if [[ -d "$backend_dir" ]]; then
                lib_path="$backend_dir:$lib_path"
            fi
        done
    fi
    
    # Add CUDA library path if it exists (needed for GPU)
    for cuda_path in /usr/local/cuda-*/lib64 /usr/local/cuda/lib64; do
        if [[ -d "$cuda_path" ]]; then
            lib_path="$lib_path:$cuda_path"
            log_debug "Added CUDA library path: $cuda_path"
            break
        fi
    done
    
    # Start the server in background with proper library path
    log_info "Launching: $binary_path ${args[*]}"
    LD_LIBRARY_PATH="$lib_path" RUST_BACKTRACE="$RUST_BACKTRACE" \
        nohup "$binary_path" "${args[@]}" > "$LOG_FILE" 2>&1 &
    local pid=$!
    echo "$pid" > "$PID_FILE"
    
    # Wait a moment and check if it's still running
    sleep 2
    if ps -p "$pid" > /dev/null 2>&1; then
        log_info "Server started successfully (PID: $pid)"
        log_info "Logs: tail -f $LOG_FILE"
    else
        log_error "Server failed to start"
        log_error "Check logs: tail $LOG_FILE"
        rm -f "$PID_FILE"
        return 1
    fi
}

stop_server() {
    print_header "Stopping Midnight Proof Server"
    
    if ! is_running; then
        log_warn "Server is not running"
        return 0
    fi
    
    local pid
    pid=$(get_pid)
    log_info "Stopping server (PID: $pid)"
    
    # Send SIGTERM
    kill -TERM "$pid" 2>/dev/null || true
    
    # Wait for graceful shutdown
    local timeout=30
    local count=0
    while ps -p "$pid" > /dev/null 2>&1 && [[ $count -lt $timeout ]]; do
        sleep 1
        ((count++))
    done
    
    # Force kill if still running
    if ps -p "$pid" > /dev/null 2>&1; then
        log_warn "Graceful shutdown failed, forcing kill"
        kill -KILL "$pid" 2>/dev/null || true
        sleep 1
    fi
    
    rm -f "$PID_FILE"
    log_info "Server stopped successfully"
}

restart_server() {
    print_header "Restarting Midnight Proof Server"
    stop_server
    sleep 2
    start_server
}

status_server() {
    print_header "Midnight Proof Server Status"
    
    if is_running; then
        local pid
        pid=$(get_pid)
        log_info "Status: ${GREEN}RUNNING${NC}"
        log_info "PID: $pid"
        
        # Get process info
        echo ""
        echo "Process Information:"
        ps -p "$pid" -o pid,ppid,user,%cpu,%mem,vsz,rss,etime,command | tail -n +2
        
        # Check health endpoint if available
        echo ""
        check_health
    else
        log_error "Status: ${RED}NOT RUNNING${NC}"
        return 1
    fi
}

################################################################################
# Health Check Functions
################################################################################

check_health() {
    log_info "Checking server health..."
    
    local health_url="http://localhost:$PORT/health"
    local response
    local status_code
    
    if command -v curl > /dev/null 2>&1; then
        response=$(curl -s -w "\n%{http_code}" "$health_url" 2>/dev/null) || {
            log_error "Health check failed: Cannot connect to $health_url"
            return 1
        }
        status_code=$(echo "$response" | tail -n1)
        response=$(echo "$response" | head -n-1)
    elif command -v wget > /dev/null 2>&1; then
        response=$(wget -qO- "$health_url" 2>&1) || {
            log_error "Health check failed: Cannot connect to $health_url"
            return 1
        }
        status_code=200
    else
        log_warn "Neither curl nor wget available, skipping health check"
        return 0
    fi
    
    if [[ "$status_code" == "200" ]]; then
        log_info "Health check: ${GREEN}PASSED${NC}"
        echo "Response: $response"
    else
        log_error "Health check: ${RED}FAILED${NC} (HTTP $status_code)"
        echo "Response: $response"
        return 1
    fi
}

check_ready() {
    log_info "Checking server readiness..."
    
    local ready_url="http://localhost:$PORT/ready"
    local response
    local status_code
    
    if command -v curl > /dev/null 2>&1; then
        response=$(curl -s -w "\n%{http_code}" "$ready_url" 2>/dev/null) || {
            log_error "Ready check failed: Cannot connect to $ready_url"
            return 1
        }
        status_code=$(echo "$response" | tail -n1)
        response=$(echo "$response" | head -n-1)
    else
        log_warn "curl not available, skipping ready check"
        return 0
    fi
    
    if [[ "$status_code" == "200" ]]; then
        log_info "Ready check: ${GREEN}OK${NC}"
        echo "Response: $response"
    elif [[ "$status_code" == "503" ]]; then
        log_warn "Ready check: ${YELLOW}BUSY${NC}"
        echo "Response: $response"
    else
        log_error "Ready check: ${RED}FAILED${NC} (HTTP $status_code)"
        echo "Response: $response"
        return 1
    fi
}

get_version() {
    log_info "Fetching server version..."
    
    local version_url="http://localhost:$PORT/version"
    
    if command -v curl > /dev/null 2>&1; then
        local version
        version=$(curl -s "$version_url" 2>/dev/null) || {
            log_error "Cannot fetch version from $version_url"
            return 1
        }
        echo "Server Version: $version"
    else
        log_warn "curl not available, cannot fetch version"
    fi
}

################################################################################
# Monitoring Functions
################################################################################

monitor_server() {
    print_header "Monitoring Midnight Proof Server"
    log_info "Press Ctrl+C to stop monitoring"
    log_info "Health check interval: ${HEALTH_CHECK_INTERVAL}s"
    
    local failures=0
    
    while true; do
        if ! is_running; then
            log_error "Server is not running!"
            ((failures++))
            
            if [[ $failures -ge $MAX_RESTART_ATTEMPTS ]]; then
                log_error "Maximum restart attempts reached ($MAX_RESTART_ATTEMPTS)"
                return 1
            fi
            
            log_warn "Attempting to restart server (attempt $failures/$MAX_RESTART_ATTEMPTS)"
            sleep "$RESTART_DELAY"
            start_server || continue
            failures=0
        else
            if check_health > /dev/null 2>&1; then
                log_info "$(date '+%Y-%m-%d %H:%M:%S') - Server is healthy (PID: $(get_pid))"
                failures=0
            else
                log_warn "$(date '+%Y-%m-%d %H:%M:%S') - Health check failed"
                ((failures++))
                
                if [[ $failures -ge 3 ]]; then
                    log_error "Multiple consecutive health check failures, restarting server"
                    restart_server
                    failures=0
                fi
            fi
        fi
        
        sleep "$HEALTH_CHECK_INTERVAL"
    done
}

watch_logs() {
    print_header "Watching Midnight Proof Server Logs"
    
    if [[ ! -f "$LOG_FILE" ]]; then
        log_error "Log file not found: $LOG_FILE"
        return 1
    fi
    
    tail -f "$LOG_FILE"
}

show_metrics() {
    print_header "Midnight Proof Server Metrics"
    
    if ! is_running; then
        log_error "Server is not running"
        return 1
    fi
    
    local pid
    pid=$(get_pid)
    
    echo "System Resources:"
    ps -p "$pid" -o pid,ppid,user,%cpu,%mem,vsz,rss,etime,command | head -2
    
    echo ""
    echo "Server Information:"
    get_version
    
    echo ""
    echo "Server Status:"
    check_ready
    
    echo ""
    echo "Open Files:"
    lsof -p "$pid" 2>/dev/null | wc -l || echo "N/A (requires lsof)"
    
    echo ""
    echo "Network Connections:"
    ss -tnp 2>/dev/null | grep "$pid" || netstat -tnp 2>/dev/null | grep "$pid" || echo "N/A"
}

################################################################################
# Configuration Functions
################################################################################

generate_config() {
    local config_file="${1:-$CONFIG_FILE}"
    
    print_header "Generating Configuration File"
    
    if [[ -f "$config_file" ]]; then
        log_warn "Configuration file already exists: $config_file"
        read -rp "Overwrite? (y/N): " confirm
        if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
            log_info "Cancelled"
            return 0
        fi
    fi
    
    cat > "$config_file" <<EOF
# Midnight Proof Server Configuration
# Generated: $(date)

# Server Configuration
PORT=$PORT
VERBOSE=$VERBOSE

# Worker Pool Configuration
JOB_CAPACITY=$JOB_CAPACITY
NUM_WORKERS=$NUM_WORKERS
JOB_TIMEOUT=$JOB_TIMEOUT
NO_FETCH_PARAMS=$NO_FETCH_PARAMS

# Build Configuration
CARGO_PROFILE=$CARGO_PROFILE
FEATURES=$FEATURES
RUST_BACKTRACE=$RUST_BACKTRACE

# File Paths
PID_FILE=$PID_FILE
LOG_FILE=$LOG_FILE

# Monitoring Configuration
HEALTH_CHECK_INTERVAL=$HEALTH_CHECK_INTERVAL
MAX_RESTART_ATTEMPTS=$MAX_RESTART_ATTEMPTS
RESTART_DELAY=$RESTART_DELAY
EOF
    
    log_info "Configuration file created: $config_file"
    log_info "Edit this file to customize your settings"
}

show_config() {
    print_header "Current Configuration"
    
    echo "Server Configuration:"
    echo "  PORT: $PORT"
    echo "  VERBOSE: $VERBOSE"
    echo ""
    echo "Worker Pool Configuration:"
    echo "  JOB_CAPACITY: $JOB_CAPACITY"
    echo "  NUM_WORKERS: $NUM_WORKERS"
    echo "  JOB_TIMEOUT: $JOB_TIMEOUT"
    echo "  NO_FETCH_PARAMS: $NO_FETCH_PARAMS"
    echo ""
    echo "Build Configuration:"
    echo "  CARGO_PROFILE: $CARGO_PROFILE"
    echo "  FEATURES: ${FEATURES:-none}"
    echo "  RUST_BACKTRACE: $RUST_BACKTRACE"
    echo ""
    echo "File Paths:"
    echo "  PID_FILE: $PID_FILE"
    echo "  LOG_FILE: $LOG_FILE"
    echo "  CONFIG_FILE: $CONFIG_FILE"
    echo ""
    echo "Monitoring Configuration:"
    echo "  HEALTH_CHECK_INTERVAL: ${HEALTH_CHECK_INTERVAL}s"
    echo "  MAX_RESTART_ATTEMPTS: $MAX_RESTART_ATTEMPTS"
    echo "  RESTART_DELAY: ${RESTART_DELAY}s"
}

################################################################################
# Feature Management
################################################################################

show_features() {
    print_header "Available Features"
    
    echo "GPU Features:"
    echo "  gpu          - Enable GPU acceleration (generic)"
    echo "  gpu-cuda     - Enable CUDA GPU acceleration"
    echo ""
    echo "Tracing Features:"
    echo "  trace-msm    - Enable MSM operation tracing"
    echo "  trace-fft    - Enable FFT operation tracing"
    echo "  trace-phases - Enable proof phase tracing"
    echo "  trace-kzg    - Enable KZG commitment tracing"
    echo "  trace-all    - Enable all tracing features"
    echo ""
    echo "Usage:"
    echo "  export FEATURES=\"gpu,trace-all\""
    echo "  ./manage-proof-server.sh build"
}

################################################################################
# API Testing
################################################################################

test_api() {
    print_header "Testing Proof Server API"
    
    if ! is_running; then
        log_error "Server is not running"
        return 1
    fi
    
    local base_url="http://localhost:$PORT"
    
    echo "Testing GET /version"
    curl -s "$base_url/version" || log_error "Failed"
    echo ""
    
    echo "Testing GET /health"
    curl -s "$base_url/health" | head -c 200 || log_error "Failed"
    echo ""
    
    echo "Testing GET /ready"
    curl -s "$base_url/ready" | head -c 200 || log_error "Failed"
    echo ""
    
    echo "Testing GET /proof-versions"
    curl -s "$base_url/proof-versions" || log_error "Failed"
    echo ""
    
    log_info "API tests completed"
}

check_gpu() {
    print_header "Checking GPU Support"
    
    if ! is_running; then
        log_error "Server is not running"
        return 1
    fi
    
    local pid
    pid=$(get_pid)
    
    echo "Checking loaded ICICLE libraries:"
    local icicle_libs
    icicle_libs=$(cat /proc/$pid/maps 2>/dev/null | grep -o "libicicle_[^/]*\.so" | sort -u)
    if [[ -n "$icicle_libs" ]]; then
        echo "$icicle_libs" | while read -r lib; do
            echo "  ✓ $lib"
        done
    else
        log_warn "No ICICLE libraries loaded"
    fi
    
    echo ""
    echo "Checking CUDA availability:"
    if command -v nvidia-smi > /dev/null 2>&1; then
        echo "  ✓ nvidia-smi found"
        nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader | while read -r line; do
            echo "  GPU: $line"
        done
        
        echo ""
        echo "GPU processes:"
        local gpu_processes
        gpu_processes=$(nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv,noheader 2>/dev/null)
        if echo "$gpu_processes" | grep -q "$pid"; then
            echo "  ✓ Proof server is using GPU (PID: $pid)"
            echo "$gpu_processes" | grep "$pid"
        else
            log_warn "Proof server not currently using GPU (idle - will activate on proof generation)"
        fi
    else
        log_warn "nvidia-smi not found - cannot verify GPU availability"
    fi
    
    echo ""
    echo "Binary features check:"
    local binary_path
    binary_path=$(get_binary_path) || return 1
    
    # Check if binary was built with GPU features
    if ldd "$binary_path" 2>/dev/null | grep -q "libicicle"; then
        log_info "✓ Binary linked with ICICLE GPU libraries"
    else
        log_error "✗ Binary NOT linked with ICICLE libraries"
        log_error "Rebuild with: FEATURES=\"gpu\" ./manage-proof-server.sh build"
        return 1
    fi
}

################################################################################
# Help and Usage
################################################################################

show_usage() {
    cat <<EOF
Midnight Proof Server Management Script

Usage: $0 [COMMAND] [OPTIONS]

COMMANDS:
  build              Build the proof server binary
  start              Start the proof server
  stop               Stop the proof server
  restart            Restart the proof server
  status             Show server status
  health             Check server health
  ready              Check server readiness
  version            Get server version
  monitor            Monitor server with auto-restart
  logs               Watch server logs
  metrics            Show server metrics
  test-api           Test API endpoints
  check-gpu          Validate GPU support and configuration
  config             Show current configuration
  generate-config    Generate configuration file
  features           Show available build features
  help               Show this help message

OPTIONS:
  --port PORT                   Set server port (default: 6300)
  --workers NUM                 Set number of workers (default: 2)
  --job-capacity NUM            Set job queue capacity (default: 0)
  --job-timeout SECONDS         Set job timeout (default: 600.0)
  --verbose                     Enable verbose logging
  --no-fetch-params             Disable parameter fetching
  --profile PROFILE             Set cargo build profile (default: release)
  --features FEATURES           Set cargo features (comma-separated)
  --config FILE                 Use custom configuration file

ENVIRONMENT VARIABLES:
  MIDNIGHT_PROOF_SERVER_PORT
  MIDNIGHT_PROOF_SERVER_VERBOSE
  MIDNIGHT_PROOF_SERVER_JOB_CAPACITY
  MIDNIGHT_PROOF_SERVER_NUM_WORKERS
  MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT
  MIDNIGHT_PROOF_SERVER_NO_FETCH_PARAMS

EXAMPLES:
  # Build with GPU support
  FEATURES="gpu" $0 build

  # Start server with custom configuration
  $0 start --port 8080 --workers 4 --verbose

  # Monitor server with auto-restart
  $0 monitor

  # Generate configuration file
  $0 generate-config

  # Check server status
  $0 status

EOF
}

################################################################################
# Main Function
################################################################################

main() {
    # Parse command line options
    while [[ $# -gt 0 ]]; do
        case $1 in
            --port)
                PORT="$2"
                shift 2
                ;;
            --workers)
                NUM_WORKERS="$2"
                shift 2
                ;;
            --job-capacity)
                JOB_CAPACITY="$2"
                shift 2
                ;;
            --job-timeout)
                JOB_TIMEOUT="$2"
                shift 2
                ;;
            --verbose)
                VERBOSE="true"
                shift
                ;;
            --no-fetch-params)
                NO_FETCH_PARAMS="true"
                shift
                ;;
            --profile)
                CARGO_PROFILE="$2"
                shift 2
                ;;
            --features)
                FEATURES="$2"
                shift 2
                ;;
            --config)
                CONFIG_FILE="$2"
                shift 2
                ;;
            build|start|stop|restart|status|health|ready|version|monitor|logs|metrics|config|generate-config|features|test-api|check-gpu|help)
                COMMAND="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
    done
    
    # Load configuration from file
    load_config
    
    # Execute command
    case "${COMMAND:-}" in
        build)
            build_binary
            ;;
        start)
            start_server
            ;;
        stop)
            stop_server
            ;;
        restart)
            restart_server
            ;;
        status)
            status_server
            ;;
        health)
            check_health
            ;;
        ready)
            check_ready
            ;;
        version)
            get_version
            ;;
        monitor)
            monitor_server
            ;;
        logs)
            watch_logs
            ;;
        metrics)
            show_metrics
            ;;
        config)
            show_config
            ;;
        generate-config)
            generate_config
            ;;
        features)
            show_features
            ;;
        test-api)
            test_api
            ;;
        check-gpu)
            check_gpu
            ;;
        help|"")
            show_usage
            ;;
        *)
            log_error "Unknown command: $COMMAND"
            show_usage
            exit 1
            ;;
    esac
}

# Run main function
main "$@"
