#!/bin/bash

# OneDrive Sync Daemon DBus Test Script
# This script tests the DBus interface of the OneDrive sync daemon

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# DBus configuration
DBUS_BUS_NAME="org.freedesktop.OneDriveSync"
DBUS_OBJECT_PATH="/org/freedesktop/OneDriveSync"
DBUS_INTERFACE="org.freedesktop.OneDriveSync"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

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

# Function to run a test
run_test() {
    local test_name="$1"
    local command="$2"
    local expected_pattern="$3"
    
    print_status "TEST" "Running: $test_name"
    
    if output=$(eval "$command" 2>&1); then
        if [[ -z "$expected_pattern" ]] || echo "$output" | grep -q "$expected_pattern"; then
            print_status "SUCCESS" "$test_name passed"
            ((TESTS_PASSED++))
            echo -e "${CYAN}Output:${NC} $output"
        else
            print_status "ERROR" "$test_name failed - unexpected output"
            ((TESTS_FAILED++))
            echo -e "${CYAN}Output:${NC} $output"
            echo -e "${CYAN}Expected pattern:${NC} $expected_pattern"
        fi
    else
        print_status "ERROR" "$test_name failed - command error"
        ((TESTS_FAILED++))
        echo -e "${CYAN}Error output:${NC} $output"
    fi
    echo ""
}

# Function to check if daemon is running
check_daemon_running() {
    print_status "INFO" "Checking if OneDrive sync daemon is running..."
    
    # Check if the daemon process is running
    if pgrep -f "onedrive-sync-daemon" > /dev/null; then
        print_status "SUCCESS" "OneDrive sync daemon process is running"
        return 0
    else
        print_status "WARNING" "OneDrive sync daemon process not found"
        print_status "INFO" "You may need to start the daemon first:"
        echo "  cargo run --bin onedrive-sync-daemon"
        return 1
    fi
}

# Function to check DBus service availability
check_dbus_service() {
    print_status "INFO" "Checking DBus service availability..."
    
    # Check if the DBus service is available on session bus
    if dbus-send --session --print-reply --dest=org.freedesktop.DBus \
        /org/freedesktop/DBus org.freedesktop.DBus.ListNames | \
        grep -q "$DBUS_BUS_NAME"; then
        print_status "SUCCESS" "DBus service $DBUS_BUS_NAME is available on session bus"
        return 0
    else
        print_status "WARNING" "DBus service $DBUS_BUS_NAME is not available on session bus"
        print_status "INFO" "This is expected if the daemon is running without full DBus registration"
        return 1
    fi
}

# Function to test direct method calls (if daemon is running)
test_direct_methods() {
    print_status "INFO" "Testing direct method calls through daemon process..."
    
    # This would require the daemon to expose these methods via a different interface
    # For now, we'll just check if we can communicate with the daemon
    print_status "INFO" "Direct method calls require daemon to expose HTTP/Unix socket interface"
    print_status "INFO" "This feature is not yet implemented in the current version"
}

# Function to test DBus introspection
test_dbus_introspection() {
    print_status "INFO" "Testing DBus introspection..."
    
    run_test "DBus Introspection" \
        "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME $DBUS_OBJECT_PATH org.freedesktop.DBus.Introspectable.Introspect" \
        "interface"
}

# Function to test get daemon status
test_get_daemon_status() {
    print_status "INFO" "Testing get daemon status..."
    
    run_test "Get Daemon Status" \
        "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME $DBUS_OBJECT_PATH $DBUS_INTERFACE.GetDaemonStatus" \
        "method return"
}

# Function to test get download queue
test_get_download_queue() {
    print_status "INFO" "Testing get download queue..."
    
    run_test "Get Download Queue" \
        "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME $DBUS_OBJECT_PATH $DBUS_INTERFACE.GetDownloadQueue" \
        "method return"
}

# Function to test get upload queue
test_get_upload_queue() {
    print_status "INFO" "Testing get upload queue..."
    
    run_test "Get Upload Queue" \
        "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME $DBUS_OBJECT_PATH $DBUS_INTERFACE.GetUploadQueue" \
        "method return"
}

# Function to test full reset (with confirmation)
test_full_reset() {
    print_status "WARNING" "Testing full reset (this will clear all queues!)"
    echo -n "Do you want to proceed with full reset test? (y/N): "
    read -r response
    
    if [[ "$response" =~ ^[Yy]$ ]]; then
        run_test "Full Reset" \
            "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME $DBUS_OBJECT_PATH $DBUS_INTERFACE.FullReset" \
            "method return"
    else
        print_status "INFO" "Full reset test skipped"
    fi
}

# Function to test DBus signals (monitoring)
test_dbus_signals() {
    print_status "INFO" "Testing DBus signal monitoring..."
    print_status "INFO" "This will monitor for signals for 10 seconds..."
    
    print_status "INFO" "Starting signal monitor (press Ctrl+C to stop)..."
    timeout 10s dbus-monitor --session "type='signal',interface='$DBUS_INTERFACE'" || true
    
    print_status "INFO" "Signal monitoring completed"
}

# Function to test error handling
test_error_handling() {
    print_status "INFO" "Testing error handling..."
    
    # Test with invalid method name
    run_test "Invalid Method" \
        "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME $DBUS_OBJECT_PATH $DBUS_INTERFACE.invalid_method" \
        "error"
    
    # Test with invalid object path
    run_test "Invalid Object Path" \
        "dbus-send --session --print-reply --dest=$DBUS_BUS_NAME /invalid/path $DBUS_INTERFACE.get_daemon_status" \
        "error"
}

# Function to show test summary
show_summary() {
    echo ""
    print_status "INFO" "=== Test Summary ==="
    echo -e "${GREEN}Tests Passed: $TESTS_PASSED${NC}"
    echo -e "${RED}Tests Failed: $TESTS_FAILED${NC}"
    local total=$((TESTS_PASSED + TESTS_FAILED))
    echo -e "${BLUE}Total Tests: $total${NC}"
    
    if [ $TESTS_FAILED -eq 0 ]; then
        print_status "SUCCESS" "All tests passed!"
    else
        print_status "WARNING" "Some tests failed. Check the output above for details."
    fi
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -h, --help          Show this help message"
    echo "  -a, --all           Run all tests"
    echo "  -d, --daemon        Check daemon status only"
    echo "  -s, --status        Test get daemon status"
    echo "  -q, --queues        Test queue operations"
    echo "  -r, --reset         Test full reset (with confirmation)"
    echo "  -i, --introspect    Test DBus introspection"
    echo "  -m, --monitor       Monitor DBus signals"
    echo "  -e, --errors        Test error handling"
    echo ""
    echo "Examples:"
    echo "  $0 --all                    # Run all tests"
    echo "  $0 --status                 # Test only status methods"
    echo "  $0 --queues                 # Test queue operations"
    echo "  $0 --monitor                # Monitor signals for 10 seconds"
}

# Main function
main() {
    print_status "INFO" "OneDrive Sync Daemon DBus Test Script"
    print_status "INFO" "======================================"
    echo ""
    
    # Parse command line arguments
    if [ $# -eq 0 ]; then
        show_usage
        exit 1
    fi
    
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_usage
                exit 0
                ;;
            -a|--all)
                check_daemon_running
                check_dbus_service
                test_dbus_introspection
                test_get_daemon_status
                test_get_download_queue
                test_get_upload_queue
                test_full_reset
                test_dbus_signals
                test_error_handling
                ;;
            -d|--daemon)
                check_daemon_running
                check_dbus_service
                ;;
            -s|--status)
                test_get_daemon_status
                ;;
            -q|--queues)
                test_get_download_queue
                test_get_upload_queue
                ;;
            -r|--reset)
                test_full_reset
                ;;
            -i|--introspect)
                test_dbus_introspection
                ;;
            -m|--monitor)
                test_dbus_signals
                ;;
            -e|--errors)
                test_error_handling
                ;;
            *)
                print_status "ERROR" "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
        shift
    done
    
    show_summary
}

# Run main function with all arguments
main "$@" 