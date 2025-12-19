# Operations and Monitoring Guide

## Guide for Operating Midnight TEE Proof Server

---

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## DANGER ZONE: All of the below is experimental, not yet tested ##

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## Document Control

| Version | Date       | Author               | Changes       |
| ------- | ---------- | -------------------- | ------------- |
| 1.0     | 2025-12-19 | Bob Blessing-Hartley | Initial draft |

---

## Table of Contents

1. [Overview](#overview)
2. [Day-to-Day Operations](#day-to-day-operations)
3. [Monitoring Strategy](#monitoring-strategy)
4. [Alerting](#alerting)
5. [Performance Tuning](#performance-tuning)
6. [Security Operations](#security-operations)
7. [Incident Response](#incident-response)
8. [Capacity Planning](#capacity-planning)
9. [Maintenance Windows](#maintenance-windows)
10. [Runbooks](#runbooks)

---

## Overview

### Operations Philosophy

The Midnight TEE Proof Server requires:
- **24/7 availability** - Users depend on proof generation
- **Security-first** - TEE attestation must always be valid
- **Performance monitoring** - Proof generation is compute-intensive
- **Rapid response** - Issues must be detected and resolved quickly

### Team Roles

**Primary On-Call (24/7):**
- Respond to critical alerts
- Execute runbooks
- Escalate to secondary if needed

**Secondary On-Call:**
- Security issues
- Complex troubleshooting
- Code changes

**Operations Manager:**
- Weekly reviews
- Capacity planning
- Vendor relations

---

## Day-to-Day Operations

### Daily Tasks (15 minutes)

#### Morning Health Check

```bash
#!/bin/bash
# daily-health-check.sh

echo "=== Midnight Proof Server Health Check ==="
echo "Date: $(date)"
echo ""

# 1. Check server is responding
echo "1. Health Check:"
if curl -sf https://proof.example.com/health > /dev/null; then
    echo "   ✅ Server responding"
else
    echo "   ❌ Server not responding!"
    exit 1
fi

# 2. Check version
echo "2. Version:"
curl -s https://proof.example.com/version
echo ""

# 3. Check ready status (queue depth)
echo "3. Ready Status:"
READY=$(curl -s https://proof.example.com/ready)
echo "$READY" | jq '.'

JOBS_PROCESSING=$(echo "$READY" | jq -r '.jobsProcessing')
JOBS_PENDING=$(echo "$READY" | jq -r '.jobsPending')

if [ "$JOBS_PENDING" -gt 100 ]; then
    echo "   ⚠️  High queue depth: $JOBS_PENDING"
fi

# 4. Check attestation
echo "4. Attestation:"
NONCE=$(openssl rand -hex 32)
if curl -sf https://proof.example.com/attestation -d "{\"nonce\":\"$NONCE\"}" > /dev/null; then
    echo "   ✅ Attestation working"
else
    echo "   ❌ Attestation failed!"
    exit 1
fi

# 5. Cloud-specific checks
case "$CLOUD_PROVIDER" in
    aws)
        echo "5. AWS Nitro Enclave:"
        ssh -i ~/.ssh/proof-server-key.pem ec2-user@$SERVER_IP \
            "nitro-cli describe-enclaves | jq -r '.[0].State'"
        ;;
    gcp)
        echo "5. GCP Confidential VM:"
        gcloud compute instances describe $INSTANCE_NAME \
            --zone=$ZONE \
            --format='value(status)'
        ;;
    azure)
        echo "5. Azure Confidential VM:"
        az vm get-instance-view \
            --resource-group $RESOURCE_GROUP \
            --name $VM_NAME \
            --query 'instanceView.statuses[?starts_with(code, `PowerState`)].displayStatus' \
            --output tsv
        ;;
esac

echo ""
echo "=== Health Check Complete ==="
```

Run daily:
```bash
./daily-health-check.sh
```

#### Check Metrics Dashboard

**Key Metrics to Review:**

| Metric | Normal Range | Alert Threshold |
|--------|--------------|-----------------|
| CPU Usage | 40-70% | >85% |
| Memory Usage | 50-80% | >90% |
| Queue Depth | 0-10 | >50 |
| Request Rate | Varies | Sudden drop |
| Error Rate | <0.1% | >1% |
| Attestation Success | >99.9% | <99% |
| P95 Latency (proof) | 15-45s | >60s |
| P95 Latency (attestation) | <500ms | >1s |

### Weekly Tasks (30 minutes)

#### Monday: Capacity Review

```bash
# weekly-capacity-report.sh

# Get last 7 days of metrics
START_DATE=$(date -d '7 days ago' +%Y-%m-%d)
END_DATE=$(date +%Y-%m-%d)

echo "=== Weekly Capacity Report ==="
echo "Period: $START_DATE to $END_DATE"
echo ""

# AWS CloudWatch
aws cloudwatch get-metric-statistics \
    --namespace AWS/EC2 \
    --metric-name CPUUtilization \
    --start-time $START_DATE \
    --end-time $END_DATE \
    --period 86400 \
    --statistics Average,Maximum \
    --dimensions Name=InstanceId,Value=$INSTANCE_ID

# Custom metrics
echo "Proof Generation Stats:"
aws cloudwatch get-metric-statistics \
    --namespace MidnightProofServer \
    --metric-name ProofsGenerated \
    --start-time $START_DATE \
    --end-time $END_DATE \
    --period 86400 \
    --statistics Sum

echo "Queue Depth Stats:"
aws cloudwatch get-metric-statistics \
    --namespace MidnightProofServer \
    --metric-name QueueDepth \
    --start-time $START_DATE \
    --end-time $END_DATE \
    --period 86400 \
    --statistics Average,Maximum
```

#### Wednesday: Security Review

- [ ] Review failed authentication attempts
- [ ] Check for unusual traffic patterns
- [ ] Verify attestation success rate >99.9%
- [ ] Review security group/firewall changes
- [ ] Check SSL certificate expiration (alert if <30 days)

```bash
# Check SSL expiration
echo | openssl s_client -servername proof.example.com \
    -connect proof.example.com:443 2>/dev/null | \
    openssl x509 -noout -dates
```

#### Friday: Cost Review

```bash
# AWS cost review
aws ce get-cost-and-usage \
    --time-period Start=$(date -d 'last monday' +%Y-%m-%d),End=$(date +%Y-%m-%d) \
    --granularity DAILY \
    --metrics BlendedCost \
    --filter file://cost-filter.json

# Compare to budget
# Alert if >10% over expected
```

### Monthly Tasks (2 hours)

#### First Monday: Maintenance

- [ ] Update parent instance/VM OS packages
- [ ] Review and update dependencies
- [ ] Rebuild enclave/container with patches
- [ ] Update PCR values if code changed
- [ ] Test failover procedures
- [ ] Review and update runbooks
- [ ] Audit access logs

#### Mid-Month: Performance Review

- [ ] Analyze proof generation times (trends)
- [ ] Review resource utilization
- [ ] Identify optimization opportunities
- [ ] Update capacity plan if needed
- [ ] Review error rates and root causes

#### End of Month: Business Review

- [ ] Generate uptime report
- [ ] Total proofs generated
- [ ] Cost per proof calculation
- [ ] User feedback summary
- [ ] Incident post-mortems
- [ ] Roadmap review

---

## Monitoring Strategy

### Metrics Hierarchy

```
Level 1: System Health (Black Box)
  └─ Is the service up? (HTTP 200)
  └─ Can users generate proofs? (end-to-end test)

Level 2: Service Metrics (Gray Box)
  └─ Request rate, error rate, latency
  └─ Queue depth, worker utilization
  └─ Attestation success rate

Level 3: Infrastructure Metrics (White Box)
  └─ CPU, memory, disk, network
  └─ TEE-specific metrics (enclave state, TPM status)
  └─ Application logs

Level 4: Business Metrics
  └─ Proofs per user/wallet
  └─ Revenue/cost per proof
  └─ User satisfaction
```

### Key Metrics to Track

#### Service Level Indicators (SLIs)

**Availability:**
```
Availability = (Successful Requests / Total Requests) × 100
Target: 99.9% (43 minutes downtime/month)
```

**Latency:**
```
P50 Proof Generation: <20 seconds
P95 Proof Generation: <45 seconds
P99 Proof Generation: <60 seconds
P95 Attestation: <500ms
```

**Error Rate:**
```
Error Rate = (Failed Requests / Total Requests) × 100
Target: <0.1%
```

#### Custom Metrics

**1. Proof Generation Metrics**

```typescript
// Custom metric collection (push to CloudWatch/Stackdriver/Azure Monitor)
interface ProofMetrics {
  timestamp: number;
  proof_duration_ms: number;
  proof_type: string;  // "check" | "prove" | "prove-tx"
  success: boolean;
  error_code?: string;
  queue_time_ms: number;
  worker_id: string;
}
```

**2. Attestation Metrics**

```typescript
interface AttestationMetrics {
  timestamp: number;
  attestation_duration_ms: number;
  success: boolean;
  pcr_match: boolean;
  cert_chain_valid: boolean;
  timestamp_fresh: boolean;
  debug_mode: boolean;
  tee_provider: "aws-nitro" | "gcp-confidential" | "azure-confidential";
}
```

**3. Queue Metrics**

```typescript
interface QueueMetrics {
  timestamp: number;
  jobs_pending: number;
  jobs_processing: number;
  jobs_completed_last_minute: number;
  average_queue_wait_ms: number;
  max_queue_wait_ms: number;
}
```

### Monitoring Tools by Cloud

#### AWS

```bash
# Install CloudWatch agent (on EC2 instance)
sudo yum install -y amazon-cloudwatch-agent

# Configure
sudo /opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-config-wizard

# Start
sudo /opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-ctl \
  -a fetch-config \
  -m ec2 \
  -s \
  -c file:/opt/aws/amazon-cloudwatch-agent/etc/config.json
```

**CloudWatch Dashboard JSON:**
```json
{
  "widgets": [
    {
      "type": "metric",
      "properties": {
        "metrics": [
          ["AWS/EC2", "CPUUtilization", {"stat": "Average"}],
          [".", ".", {"stat": "Maximum"}]
        ],
        "period": 300,
        "stat": "Average",
        "region": "us-east-1",
        "title": "CPU Utilization"
      }
    },
    {
      "type": "metric",
      "properties": {
        "metrics": [
          ["MidnightProofServer", "ProofDuration", {"stat": "p50"}],
          [".", ".", {"stat": "p95"}],
          [".", ".", {"stat": "p99"}]
        ],
        "period": 300,
        "stat": "Average",
        "region": "us-east-1",
        "title": "Proof Generation Latency"
      }
    }
  ]
}
```

#### GCP

```bash
# Install Ops Agent
curl -sSO https://dl.google.com/cloudagents/add-google-cloud-ops-agent-repo.sh
sudo bash add-google-cloud-ops-agent-repo.sh --also-install

# Configure logging
sudo tee /etc/google-cloud-ops-agent/config.yaml > /dev/null <<EOF
logging:
  receivers:
    syslog:
      type: files
      include_paths:
        - /var/log/messages
        - /var/log/proof-server.log
  service:
    pipelines:
      default_pipeline:
        receivers: [syslog]

metrics:
  receivers:
    hostmetrics:
      type: hostmetrics
      collection_interval: 60s
  service:
    pipelines:
      default_pipeline:
        receivers: [hostmetrics]
EOF

sudo systemctl restart google-cloud-ops-agent
```

#### Azure

```bash
# Install Azure Monitor agent
wget https://aka.ms/dependencyagentlinux -O InstallDependencyAgent-Linux64.bin
sudo sh InstallDependencyAgent-Linux64.bin

# Configure Log Analytics
az monitor log-analytics workspace create \
  --resource-group midnight-proof-server-rg \
  --workspace-name midnight-proof-server-workspace

# Connect VM
az vm extension set \
  --resource-group midnight-proof-server-rg \
  --vm-name midnight-proof-server \
  --name OmsAgentForLinux \
  --publisher Microsoft.EnterpriseCloud.Monitoring \
  --settings '{"workspaceId":"<workspace-id>"}'
```

---

## Alerting

### Alert Levels

**P0 - Critical (Page immediately)**
- Service completely down
- Attestation failure rate >1%
- No proofs generated in 5 minutes
- TEE enclave/VM crashed

**P1 - High (Page during business hours)**
- Error rate >1%
- P95 latency >60s
- CPU >90% for 10 minutes
- Memory >95%
- Queue depth >100 for 5 minutes

**P2 - Medium (Ticket)**
- Error rate >0.5%
- P95 latency >45s
- CPU >85% for 30 minutes
- SSL certificate expires in <14 days

**P3 - Low (Email)**
- CPU >80% for 1 hour
- Unusual traffic patterns
- Minor configuration drift

### Alert Configuration

#### AWS CloudWatch Alarms

```bash
# P0: Service Down
aws cloudwatch put-metric-alarm \
  --alarm-name midnight-proof-server-down \
  --alarm-description "Proof server is not responding" \
  --metric-name HealthCheckSuccess \
  --namespace MidnightProofServer \
  --statistic Average \
  --period 60 \
  --threshold 0 \
  --comparison-operator LessThanThreshold \
  --datapoints-to-alarm 3 \
  --evaluation-periods 3 \
  --alarm-actions arn:aws:sns:us-east-1:123456789:midnight-critical

# P0: Attestation Failure
aws cloudwatch put-metric-alarm \
  --alarm-name midnight-attestation-failure \
  --alarm-description "Attestation failure rate >1%" \
  --metric-name AttestationFailureRate \
  --namespace MidnightProofServer \
  --statistic Average \
  --period 300 \
  --threshold 1.0 \
  --comparison-operator GreaterThanThreshold \
  --datapoints-to-alarm 2 \
  --evaluation-periods 2 \
  --alarm-actions arn:aws:sns:us-east-1:123456789:midnight-critical

# P1: High Error Rate
aws cloudwatch put-metric-alarm \
  --alarm-name midnight-high-error-rate \
  --alarm-description "Error rate >1%" \
  --metric-name ErrorRate \
  --namespace MidnightProofServer \
  --statistic Average \
  --period 300 \
  --threshold 1.0 \
  --comparison-operator GreaterThanThreshold \
  --datapoints-to-alarm 3 \
  --evaluation-periods 3 \
  --alarm-actions arn:aws:sns:us-east-1:123456789:midnight-high

# P1: High Latency
aws cloudwatch put-metric-alarm \
  --alarm-name midnight-high-latency \
  --alarm-description "P95 proof latency >60s" \
  --metric-name ProofDuration \
  --namespace MidnightProofServer \
  --extended-statistic p95 \
  --period 300 \
  --threshold 60000 \
  --comparison-operator GreaterThanThreshold \
  --datapoints-to-alarm 3 \
  --evaluation-periods 3 \
  --alarm-actions arn:aws:sns:us-east-1:123456789:midnight-high
```

#### GCP Monitoring Policies

```bash
# Create alert policy
gcloud alpha monitoring policies create \
  --notification-channels=$CHANNEL_ID \
  --display-name="Proof Server Down" \
  --condition-display-name="Health check failing" \
  --condition-threshold-value=0 \
  --condition-threshold-duration=180s \
  --condition-filter='metric.type="custom.googleapis.com/proof_server/health_check"'
```

#### Azure Monitor Alerts

```bash
# Create action group
az monitor action-group create \
  --resource-group midnight-proof-server-rg \
  --name midnight-critical \
  --short-name midnight-cr \
  --email-receiver name=ops email=ops@example.com

# Create alert rule
az monitor metrics alert create \
  --name midnight-proof-server-down \
  --resource-group midnight-proof-server-rg \
  --scopes /subscriptions/$SUBSCRIPTION_ID/resourceGroups/midnight-proof-server-rg/providers/Microsoft.Compute/virtualMachines/midnight-proof-server \
  --condition "avg Percentage CPU > 90" \
  --window-size 5m \
  --evaluation-frequency 1m \
  --action midnight-critical
```

### On-Call Rotation

**Use PagerDuty/Opsgenie:**

```yaml
# PagerDuty schedule example
schedules:
  - name: Midnight Proof Server Primary
    timezone: America/New_York
    layers:
      - name: Week 1
        start: 2025-12-18T00:00:00Z
        rotation_virtual_start: 2025-12-18T00:00:00Z
        rotation_turn_length_seconds: 604800  # 1 week
        users:
          - user1@example.com
          - user2@example.com
          - user3@example.com

  - name: Midnight Proof Server Secondary
    timezone: America/New_York
    layers:
      - name: Week 1
        start: 2025-12-18T00:00:00Z
        rotation_virtual_start: 2025-12-18T00:00:00Z
        rotation_turn_length_seconds: 604800
        users:
          - senior1@example.com
          - senior2@example.com

escalation_policies:
  - name: Midnight Proof Server
    escalation_rules:
      - escalation_delay_in_minutes: 0
        targets:
          - type: schedule_reference
            id: primary_schedule
      - escalation_delay_in_minutes: 15
        targets:
          - type: schedule_reference
            id: secondary_schedule
      - escalation_delay_in_minutes: 30
        targets:
          - type: user_reference
            id: ops_manager
```

---

## Performance Tuning

### Bottleneck Identification

**1. CPU-Bound (Most Common)**

Symptoms:
- CPU consistently >80%
- Proof generation slow
- Queue backing up

Solution:
```bash
# Increase vCPUs
# AWS: Upgrade to larger instance type
aws ec2 modify-instance-attribute \
  --instance-id $INSTANCE_ID \
  --instance-type c5.4xlarge

# GCP: Resize VM
gcloud compute instances stop midnight-proof-server
gcloud compute instances set-machine-type midnight-proof-server \
  --machine-type n2d-standard-16
gcloud compute instances start midnight-proof-server

# Or: Increase worker pool size
# Edit proof server config:
--num-workers 16  # (was 8)
```

**2. Memory-Bound**

Symptoms:
- Memory >90%
- OOM errors in logs
- Enclave crashes

Solution:
```bash
# AWS Nitro: Increase enclave memory allocation
sudo sed -i 's/^memory_mib:.*/memory_mib: 32768/' /etc/nitro_enclaves/allocator.yaml
sudo systemctl restart nitro-enclaves-allocator.service

# Restart enclave with more memory
nitro-cli terminate-enclave --enclave-id $ENCLAVE_ID
nitro-cli run-enclave --memory 32768 --cpu-count 8 ...

# GCP/Azure: Resize to memory-optimized instance
```

**3. Network-Bound**

Symptoms:
- High latency but low CPU
- Network I/O wait time high
- Slow attestation responses

Solution:
```bash
# AWS: Use enhanced networking instance type
# Already enabled on c5.* instances

# Check network performance
iperf3 -c proof.example.com -t 30

# Consider using CloudFront/CDN for caching
# (attestation documents can be cached for 5 min)
```

### Proof Generation Optimization

**1. Worker Pool Tuning**

```bash
# Rule of thumb: num_workers = num_vCPUs
--num-workers 8  # for 8 vCPU instance

# For CPU-intensive proofs:
--num-workers 8  # match vCPUs exactly

# For mixed workload:
--num-workers 12  # slight oversubscription okay
```

**2. Job Timeout Tuning**

```bash
# Default: 600 seconds (10 minutes)
--job-timeout 600

# If proofs consistently fail:
--job-timeout 900  # 15 minutes

# If proofs are fast:
--job-timeout 300  # 5 minutes (fail fast)
```

**3. Queue Capacity**

```bash
# Default: 0 (unlimited)
--job-capacity 0

# Limit to prevent memory issues:
--job-capacity 100  # max 100 queued jobs

# This will return 503 when queue is full
# Clients should retry with backoff
```

### Caching Strategy

**Attestation Document Caching:**

```nginx
# nginx config
location /attestation {
    proxy_pass http://proof_server;

    # Cache attestation for 5 minutes
    proxy_cache_valid 200 5m;
    proxy_cache_key "$request_body";
    proxy_cache_methods POST;

    # Add cache status header
    add_header X-Cache-Status $upstream_cache_status;
}
```

**Benefits:**
- Reduces load on enclave
- Faster response to repeated requests
- Still maintains freshness (<5 min)

**Risks:**
- Stale attestations if enclave restarted
- Must not cache longer than timestamp freshness requirement

---

## Security Operations

### Daily Security Checks

```bash
#!/bin/bash
# daily-security-check.sh

echo "=== Security Check ==="

# 1. Verify attestation is valid
echo "1. Attestation Verification:"
NONCE=$(openssl rand -hex 32)
ATTESTATION=$(curl -s https://proof.example.com/attestation -d "{\"nonce\":\"$NONCE\"}")

# Extract PCR values
PCR0=$(echo "$ATTESTATION" | jq -r '.pcr_measurements["0"]')
PCR1=$(echo "$ATTESTATION" | jq -r '.pcr_measurements["1"]')
PCR2=$(echo "$ATTESTATION" | jq -r '.pcr_measurements["2"]')

# Compare with published values
EXPECTED_PCR0="abc123..."  # From GitHub release
if [ "$PCR0" == "$EXPECTED_PCR0" ]; then
    echo "   ✅ PCR0 matches"
else
    echo "   ❌ PCR0 MISMATCH! Possible compromise!"
    exit 1
fi

# 2. Check debug mode is OFF
DEBUG_MODE=$(echo "$ATTESTATION" | jq -r '.debug_mode')
if [ "$DEBUG_MODE" == "false" ]; then
    echo "   ✅ Debug mode OFF"
else
    echo "   ❌ DEBUG MODE ON! CRITICAL SECURITY ISSUE!"
    exit 1
fi

# 3. Check failed auth attempts
echo "2. Failed Authentication Attempts:"
# AWS CloudWatch Logs
aws logs filter-log-events \
    --log-group-name /aws/ec2/midnight-proof-server \
    --filter-pattern "Authentication failed" \
    --start-time $(date -d '24 hours ago' +%s)000 \
    | jq -r '.events | length'

# 4. Check for unusual traffic
echo "3. Traffic Analysis:"
# Get request count by IP
aws logs filter-log-events \
    --log-group-name /aws/ec2/midnight-proof-server/nginx-access \
    --start-time $(date -d '1 hour ago' +%s)000 \
    | jq -r '.events[].message' \
    | awk '{print $1}' \
    | sort | uniq -c | sort -rn | head -10

# Alert if any IP has >1000 requests/hour
```

### Incident Response Runbook

#### Security Incident: PCR Mismatch Detected

**Severity:** P0 - Critical
**Response Time:** Immediate

**Steps:**

1. **Verify the Alert**
   ```bash
   # Get current attestation
   curl -s https://proof.example.com/attestation -d '{"nonce":"test"}' | jq '.pcr_measurements'
   
   # Compare with published values
   curl -s https://github.com/midnight/proof-server/releases/download/v6.2.0/pcr-values.json | jq '.pcrs'
   ```

2. **Immediate Actions**
   ```bash
   # STOP accepting new requests
   # AWS: Update security group to block port 443
   aws ec2 revoke-security-group-ingress \
       --group-id $SG_ID \
       --protocol tcp \
       --port 443 \
       --cidr 0.0.0.0/0
   
   # GCP: Update firewall rule
   gcloud compute firewall-rules delete allow-proof-server
   
   # Azure: Update NSG
   az network nsg rule delete \
       --resource-group $RG \
       --nsg-name $NSG \
       --name allow-https
   ```

3. **Investigation**
   ```bash
   # Check if enclave/VM was restarted
   # AWS:
   nitro-cli describe-enclaves
   
   # Check system logs
   sudo journalctl -u nitro-enclaves-allocator -n 1000
   
   # Check for unauthorized access
   sudo ausearch -m USER_LOGIN -sv no
   
   # Preserve evidence
   sudo tar -czf /tmp/incident-$(date +%Y%m%d-%H%M%S).tar.gz \
       /var/log \
       /home/ec2-user/.bash_history \
       /etc/nginx
   ```

4. **Recovery**
   ```bash
   # If legitimate (e.g., you updated the code):
   # 1. Publish new PCR values
   # 2. Update wallet providers
   # 3. Re-enable traffic
   
   # If compromise suspected:
   # 1. DO NOT restart - preserve for forensics
   # 2. Deploy NEW instance from known-good AMI/image
   # 3. Rotate ALL credentials
   # 4. Notify users
   # 5. File security incident report
   ```

5. **Post-Incident**
   - Root cause analysis
   - Update runbooks
   - Implement additional controls
   - Notify stakeholders
   - Public disclosure if user data at risk

---

## Capacity Planning

### Growth Projections

**Track These Metrics:**

| Metric | Week 1 | Week 2 | Week 3 | Week 4 | Growth Rate |
|--------|---------|---------|---------|---------|-------------|
| Proofs/day | 1,000 | 1,500 | 2,200 | 3,300 | +50%/week |
| Peak RPS | 2 | 3 | 4.5 | 6.8 | +50%/week |
| Avg CPU | 45% | 65% | 82% | 98% | ⚠️ At capacity! |

**Scaling Triggers:**

- **Add capacity when:**
  - CPU >70% sustained for 1 week
  - Queue depth >10 sustained
  - P95 latency increasing trend
  - Growth rate >30%/week

- **Scale up (vertical):**
  - Double instance size
  - More memory for enclave
  - Better for consistent load

- **Scale out (horizontal):**
  - Add more instances behind load balancer
  - Better for variable load
  - Requires load balancer setup

### Load Balancer Setup

**AWS Application Load Balancer:**

```bash
# Create target group
aws elbv2 create-target-group \
    --name midnight-proof-servers \
    --protocol HTTPS \
    --port 443 \
    --vpc-id $VPC_ID \
    --health-check-path /health \
    --health-check-interval-seconds 30

# Create ALB
aws elbv2 create-load-balancer \
    --name midnight-proof-lb \
    --subnets $SUBNET1 $SUBNET2 \
    --security-groups $SG_ID \
    --scheme internet-facing

# Register instances
aws elbv2 register-targets \
    --target-group-arn $TG_ARN \
    --targets Id=$INSTANCE_ID_1 Id=$INSTANCE_ID_2
```

**GCP Load Balancer:**

```bash
# Create instance group
gcloud compute instance-groups unmanaged create midnight-proof-servers \
    --zone=us-central1-a

gcloud compute instance-groups unmanaged add-instances midnight-proof-servers \
    --instances=midnight-proof-server-1,midnight-proof-server-2 \
    --zone=us-central1-a

# Create health check
gcloud compute health-checks create https midnight-proof-health \
    --port=443 \
    --request-path=/health \
    --check-interval=30s

# Create backend service
gcloud compute backend-services create midnight-proof-backend \
    --protocol=HTTPS \
    --health-checks=midnight-proof-health \
    --global

# Add instance group to backend
gcloud compute backend-services add-backend midnight-proof-backend \
    --instance-group=midnight-proof-servers \
    --instance-group-zone=us-central1-a \
    --global
```

---

## Runbooks

### Runbook: Restart Enclave/Service

**When to Use:**
- Service not responding
- Enclave crashed
- After configuration change
- Scheduled maintenance

**Steps:**

1. **Pre-Check**
   ```bash
   # Check current status
   # AWS:
   nitro-cli describe-enclaves
   
   # Verify users are notified if this is scheduled
   # Check time - avoid peak hours
   ```

2. **Drain Connections**
   ```bash
   # If behind load balancer, remove from rotation
   aws elbv2 deregister-targets \
       --target-group-arn $TG_ARN \
       --targets Id=$INSTANCE_ID
   
   # Wait for existing proofs to complete
   # Check queue depth until 0
   watch -n 5 'curl -s https://localhost:6300/ready | jq .jobsProcessing'
   ```

3. **Stop Service**
   ```bash
   # AWS Nitro:
   ENCLAVE_ID=$(nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')
   nitro-cli terminate-enclave --enclave-id $ENCLAVE_ID
   
   # GCP/Azure:
   docker stop midnight-proof-server
   ```

4. **Start Service**
   ```bash
   # AWS Nitro:
   nitro-cli run-enclave \
       --eif-path midnight-proof-server.eif \
       --memory 24576 \
       --cpu-count 8 \
       --enclave-cid 16 \
       --debug-mode false
   
   # Restart vsock proxy
   pkill socat
   socat TCP-LISTEN:6300,reuseaddr,fork VSOCK-CONNECT:16:6300 &
   
   # GCP/Azure:
   docker start midnight-proof-server
   ```

5. **Verify**
   ```bash
   # Health check
   curl http://localhost:6300/health
   
   # Attestation
   curl http://localhost:6300/attestation -d '{"nonce":"test"}'
   
   # Version
   curl http://localhost:6300/version
   ```

6. **Re-enable Traffic**
   ```bash
   # Add back to load balancer
   aws elbv2 register-targets \
       --target-group-arn $TG_ARN \
       --targets Id=$INSTANCE_ID
   
   # Monitor for 5 minutes
   watch -n 10 'curl -s https://proof.example.com/ready | jq .'
   ```

---

## Summary Checklist

### Daily
- [ ] Run health check script
- [ ] Review metrics dashboard
- [ ] Check alert status

### Weekly
- [ ] Capacity review
- [ ] Security review
- [ ] Cost review

### Monthly
- [ ] Full maintenance
- [ ] Performance review
- [ ] Business review

### Quarterly
- [ ] Update dependencies
- [ ] Rebuild enclaves/containers
- [ ] Security audit
- [ ] Disaster recovery test

---

**END OF OPERATIONS AND MONITORING GUIDE**

**Next Steps:**
- Set up monitoring on your first deployment
- Configure alerting for P0/P1 events
- Schedule weekly capacity reviews
- Create incident response team

**Support:**
- ops@midnight.network
- https://discord.gg/midnight
