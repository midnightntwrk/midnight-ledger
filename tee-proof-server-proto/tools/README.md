# Midnight Proof Server Tools

Operational and diagnostic tools for managing and troubleshooting the Midnight Proof Server.

## Overview

This directory contains utility scripts for system diagnostics, monitoring, and operational tasks.

## Available Tools

### diagnose.sh

Comprehensive system diagnostics script for troubleshooting server issues.

**Usage:**
```bash
./diagnose.sh
```

**What it checks:**

1. **System Information**
   - OS and kernel version
   - CPU cores and architecture
   - Total memory
   - Disk space

2. **Runtime Dependencies**
   - Rust toolchain version
   - Cargo availability
   - Required system packages

3. **Server Binary**
   - Binary existence and location
   - File size and permissions
   - Compilation date

4. **Process Information**
   - Running server instances
   - Port binding (6300)
   - Memory and CPU usage

5. **Network Status**
   - Port 6300 availability
   - Active connections
   - Firewall rules (if applicable)

6. **TEE Environment**
   - Platform detection (AWS/GCP/Azure)
   - Enclave status (if in TEE)
   - Attestation capabilities

7. **Logs and Errors**
   - Recent log entries
   - Error patterns
   - Warning messages

**Output:**
The script provides a detailed report with:
- ✅ Green checkmarks for passing checks
- ❌ Red X marks for failures
- ⚠️ Yellow warnings for potential issues
- Recommended actions for problems

**Example:**
```bash
$ ./diagnose.sh

====================================
Midnight Proof Server Diagnostics
====================================

System Information:
  OS: Linux 5.15.0
  CPU: 16 cores (x86_64)
  Memory: 32GB
  Disk: 250GB free

✅ Rust toolchain: 1.75.0
✅ Server binary found
✅ Port 6300 available
⚠️ Running outside TEE environment

[Detailed output follows...]
```

## Adding New Tools

To add a new tool to this directory:

1. Create the script file:
```bash
touch tools/my-tool.sh
chmod +x tools/my-tool.sh
```

2. Add shebang and description:
```bash
#!/bin/bash
# my-tool.sh - Brief description of what this tool does
```

3. Follow bash best practices:
   - Use `set -euo pipefail` for strict error handling
   - Add usage/help information
   - Include descriptive output
   - Handle errors gracefully

4. Update this README with tool documentation

## Tool Development Guidelines

### Script Template

```bash
#!/bin/bash
# tool-name.sh - Description

set -euo pipefail  # Exit on error, undefined vars, pipe failures

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
log_info() {
    echo -e "${GREEN}✅${NC} $1"
}

log_error() {
    echo -e "${RED}❌${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}⚠️${NC} $1"
}

# Main logic
main() {
    echo "===================================="
    echo "Tool Name"
    echo "===================================="
    echo ""

    # Tool logic here

    echo ""
    echo "Done!"
}

main "$@"
```

### Best Practices

1. **Error Handling**
   ```bash
   if ! command -v cargo &> /dev/null; then
       log_error "Cargo not found"
       exit 1
   fi
   ```

2. **Clear Output**
   - Use visual indicators (✅ ❌ ⚠️)
   - Separate sections with headers
   - Provide actionable recommendations

3. **Documentation**
   - Add comments explaining complex logic
   - Include usage examples in script header
   - Update this README

4. **Portability**
   - Test on Linux and macOS
   - Avoid bash-specific features where possible
   - Handle missing commands gracefully

## Common Use Cases

### Pre-deployment Check

```bash
# Run diagnostics before deploying
./tools/diagnose.sh > deployment-readiness-report.txt

# Review the report
less deployment-readiness-report.txt
```

### Troubleshooting Server Issues

```bash
# Server won't start - run diagnostics
./tools/diagnose.sh

# Look for specific issues
./tools/diagnose.sh | grep "❌"
```

### Health Monitoring

```bash
# Add to cron for periodic checks
*/30 * * * * /path/to/tools/diagnose.sh > /var/log/midnight-diagnostics.log 2>&1
```

### Incident Response

```bash
# Capture state during incident
./tools/diagnose.sh > incident-$(date +%Y%m%d-%H%M%S).log

# Include in incident report
cat incident-*.log
```

## Integration with Monitoring

### CloudWatch

Send diagnostic output to CloudWatch Logs:

```bash
#!/bin/bash
# diagnose-and-log.sh

DIAGNOSIS=$(./tools/diagnose.sh)

aws logs put-log-events \
  --log-group-name "/midnight/proof-server/diagnostics" \
  --log-stream-name "$(hostname)" \
  --log-events "timestamp=$(date +%s000),message='$DIAGNOSIS'"
```

### Prometheus

Export diagnostic metrics:

```bash
#!/bin/bash
# diagnose-metrics.sh

# Run diagnostics
RESULT=$(./tools/diagnose.sh)

# Parse and expose metrics
echo "# HELP midnight_server_healthy Server health status"
echo "# TYPE midnight_server_healthy gauge"
if echo "$RESULT" | grep -q "✅ Server binary found"; then
    echo "midnight_server_healthy 1"
else
    echo "midnight_server_healthy 0"
fi
```

### Grafana

Create alerting rules based on diagnostic output:

```yaml
# grafana-alert.yml
groups:
  - name: midnight-diagnostics
    rules:
      - alert: MidnightServerUnhealthy
        expr: midnight_server_healthy == 0
        for: 5m
        annotations:
          summary: "Midnight server failed health diagnostics"
```

## Automated Workflows

### CI/CD Integration

```yaml
# .github/workflows/deploy.yml
- name: Run Diagnostics
  run: |
    ./tools/diagnose.sh
    if [ $? -ne 0 ]; then
      echo "Pre-deployment diagnostics failed"
      exit 1
    fi
```

### Deployment Scripts

```bash
#!/bin/bash
# deploy.sh

echo "Running pre-deployment diagnostics..."
if ! ./tools/diagnose.sh; then
    echo "❌ Diagnostics failed. Aborting deployment."
    exit 1
fi

echo "✅ Diagnostics passed. Proceeding with deployment..."
# Deployment logic here
```

## Future Tools

Planned additions to this directory:

- `monitor.sh` - Real-time monitoring dashboard
- `backup.sh` - Configuration and state backup
- `health-check.sh` - Lightweight health verification
- `performance-test.sh` - Load testing and benchmarking
- `security-audit.sh` - Security configuration audit
- `log-analyzer.sh` - Log parsing and analysis
- `alert-test.sh` - Test alerting integrations

## Contributing

To contribute new tools:

1. Create the tool script
2. Test thoroughly on target platforms
3. Add comprehensive documentation
4. Update this README
5. Submit pull request

## Support

For issues with tools:

- Check tool output for error messages
- Review script code for debugging
- Consult main [Troubleshooting Guide](../docs/troubleshooting.md)
- Report bugs to ops@midnight.network

## Related Documentation

- [Operations & Monitoring](../docs/operations-monitoring.md)
- [Troubleshooting Guide](../docs/troubleshooting.md)
- [Debugging Guide](../docs/debugging-guide.md)

## License

Apache License 2.0

Copyright (C) 2025 Midnight Foundation
