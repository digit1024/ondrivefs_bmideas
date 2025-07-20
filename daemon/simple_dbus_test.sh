#!/bin/bash

# Simple OneDrive Sync Daemon Test Script
# This script tests the daemon functionality in the current implementation

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    local status=$1
    local message=$2
    case $status in
        "INFO")
            echo -e "${BLUE}[INFO]${NC} $message"
            ;;
        "SUCCESS")
            echo -e "${GREEN}[SUCCESS]${NC} $message"
            ;;
        "WARNING")
            echo -e "${YELLOW}[WARNING]${NC} $message"
            ;;
        "ERROR")
            echo -e "${RED}[ERROR]${NC} $message"
            ;;
        "TEST")
            echo -e "${PURPLE}[TEST]${NC} $message"
            ;;
    esac
}

# Function to check if daemon is running
check_daemon_running() {
    print_status "INFO" "Checking if OneDrive sync daemon is running..."
    
    if pgrep -f "onedrive-sync-daemon" > /dev/null; then
        print_status "SUCCESS" "OneDrive sync daemon process is running"
        DAEMON_PID=$(pgrep -f "onedrive-sync-daemon")
        echo -e "${CYAN}Daemon PID:${NC} $DAEMON_PID"
        return 0
    else
        print_status "WARNING" "OneDrive sync daemon process not found"
        return 1
    fi
}

# Function to show daemon process info
show_daemon_info() {
    print_status "INFO" "Daemon Process Information:"
    
    if DAEMON_PID=$(pgrep -f "onedrive-sync-daemon"); then
        echo -e "${CYAN}PID:${NC} $DAEMON_PID"
        echo -e "${CYAN}Command:${NC} $(ps -p $DAEMON_PID -o cmd=)"
        echo -e "${CYAN}Memory Usage:${NC} $(ps -p $DAEMON_PID -o rss=) KB"
        echo -e "${CYAN}CPU Time:${NC} $(ps -p $DAEMON_PID -o time=)"
        
        # Check if daemon is listening on any ports
        if netstat -tlnp 2>/dev/null | grep -q "$DAEMON_PID"; then
            echo -e "${CYAN}Listening Ports:${NC}"
            netstat -tlnp 2>/dev/null | grep "$DAEMON_PID" || true
        else
            echo -e "${CYAN}Listening Ports:${NC} None detected"
        fi
    else
        print_status "ERROR" "Daemon not running"
    fi
}

# Function to check database files
check_database_files() {
    print_status "INFO" "Checking database files..."
    
    # Find the daemon's data directory
    local data_dir="$HOME/.local/share/onedrive-sync-daemon"
    if [ -d "$data_dir" ]; then
        echo -e "${CYAN}Data Directory:${NC} $data_dir"
        
        # Check for database file
        if [ -f "$data_dir/onedrive.db" ]; then
            echo -e "${CYAN}Database File:${NC} $data_dir/onedrive.db"
            echo -e "${CYAN}Database Size:${NC} $(du -h "$data_dir/onedrive.db" | cut -f1)"
        else
            print_status "WARNING" "Database file not found"
        fi
        
        # Check for other files
        echo -e "${CYAN}Directory Contents:${NC}"
        ls -la "$data_dir" 2>/dev/null || true
    else
        print_status "WARNING" "Data directory not found: $data_dir"
    fi
}

# Function to check log files
check_log_files() {
    print_status "INFO" "Checking log files..."
    
    # Check for log files in various locations
    local log_locations=(
        "$HOME/.local/share/onedrive-sync-daemon/logs"
        "$HOME/.cache/onedrive-sync-daemon"
        "/var/log/onedrive-sync-daemon"
        "./logs"
    )
    
    for log_dir in "${log_locations[@]}"; do
        if [ -d "$log_dir" ]; then
            echo -e "${CYAN}Log Directory:${NC} $log_dir"
            echo -e "${CYAN}Log Files:${NC}"
            ls -la "$log_dir" 2>/dev/null || true
            break
        fi
    done
    
    # Check journalctl for daemon logs
    if command -v journalctl >/dev/null 2>&1; then
        echo -e "${CYAN}Recent Journal Logs:${NC}"
        journalctl -u onedrive-sync-daemon --no-pager -n 10 2>/dev/null || \
        journalctl | grep -i onedrive | tail -10 2>/dev/null || \
        echo "No journal logs found"
    fi
}

# Function to test daemon startup
test_daemon_startup() {
    print_status "INFO" "Testing daemon startup..."
    
    if check_daemon_running; then
        print_status "SUCCESS" "Daemon is already running"
        return 0
    fi
    
    print_status "INFO" "Starting daemon in background..."
    
    # Start daemon in background
    cargo run --bin onedrive-sync-daemon > daemon.log 2>&1 &
    DAEMON_PID=$!
    
    # Wait a bit for daemon to start
    sleep 3
    
    if kill -0 $DAEMON_PID 2>/dev/null; then
        print_status "SUCCESS" "Daemon started successfully (PID: $DAEMON_PID)"
        echo -e "${CYAN}Log output:${NC}"
        tail -20 daemon.log 2>/dev/null || true
    else
        print_status "ERROR" "Daemon failed to start"
        echo -e "${CYAN}Error log:${NC}"
        cat daemon.log 2>/dev/null || true
    fi
}

# Function to test daemon shutdown
test_daemon_shutdown() {
    print_status "INFO" "Testing daemon shutdown..."
    
    if DAEMON_PID=$(pgrep -f "onedrive-sync-daemon"); then
        print_status "INFO" "Sending SIGTERM to daemon (PID: $DAEMON_PID)..."
        kill -TERM $DAEMON_PID
        
        # Wait for graceful shutdown
        sleep 2
        
        if kill -0 $DAEMON_PID 2>/dev/null; then
            print_status "WARNING" "Daemon didn't shut down gracefully, sending SIGKILL..."
            kill -KILL $DAEMON_PID
            sleep 1
        fi
        
        if ! kill -0 $DAEMON_PID 2>/dev/null; then
            print_status "SUCCESS" "Daemon shut down successfully"
        else
            print_status "ERROR" "Failed to shut down daemon"
        fi
    else
        print_status "WARNING" "No daemon running to shut down"
    fi
}

# Function to show system resources
show_system_resources() {
    print_status "INFO" "System Resource Usage:"
    
    echo -e "${CYAN}Memory Usage:${NC}"
    free -h
    
    echo -e "${CYAN}Disk Usage:${NC}"
    df -h
    
    echo -e "${CYAN}Process Count:${NC}"
    ps aux | grep -c onedrive || echo "0"
}

# Function to show help
show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -h, --help          Show this help message"
    echo "  -c, --check         Check daemon status"
    echo "  -i, --info          Show detailed daemon information"
    echo "  -d, --database      Check database files"
    echo "  -l, --logs          Check log files"
    echo "  -s, --start         Test daemon startup"
    echo "  -x, --stop          Test daemon shutdown"
    echo "  -r, --resources     Show system resources"
    echo "  -a, --all           Run all checks"
    echo ""
    echo "Examples:"
    echo "  $0 --check          # Check if daemon is running"
    echo "  $0 --info           # Show detailed daemon info"
    echo "  $0 --all            # Run all checks"
}

# Main function
main() {
    print_status "INFO" "OneDrive Sync Daemon Test Script"
    print_status "INFO" "================================="
    echo ""
    
    if [ $# -eq 0 ]; then
        show_help
        exit 1
    fi
    
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_help
                exit 0
                ;;
            -c|--check)
                check_daemon_running
                ;;
            -i|--info)
                show_daemon_info
                ;;
            -d|--database)
                check_database_files
                ;;
            -l|--logs)
                check_log_files
                ;;
            -s|--start)
                test_daemon_startup
                ;;
            -x|--stop)
                test_daemon_shutdown
                ;;
            -r|--resources)
                show_system_resources
                ;;
            -a|--all)
                check_daemon_running
                show_daemon_info
                check_database_files
                check_log_files
                show_system_resources
                ;;
            *)
                print_status "ERROR" "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
        shift
    done
}

# Run main function
main "$@" 