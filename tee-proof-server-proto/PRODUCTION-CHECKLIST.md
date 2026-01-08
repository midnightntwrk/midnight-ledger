# Production Deployment Checklist

## Pre-Deployment

### Security Configuration
- [ ] **Enable API Key Authentication**
  - Generate strong API keys
  - Set `MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=false`
  - Distribute keys securely to authorized clients only

- [ ] **Remove Debug Mode**
  - Deploy enclave without `--debug-mode` flag
  - Console logging disabled in production

- [ ] **Configure Rate Limiting**
  - Review and adjust `MIDNIGHT_PROOF_SERVER_RATE_LIMIT` (default: 10 req/s)
  - Consider per-API-key rate limiting

### Infrastructure
- [ ] **Set up TLS/HTTPS**
  - Configure nginx/HAProxy reverse proxy with valid TLS certificates
  - Use Let's Encrypt or AWS Certificate Manager
  - Redirect HTTP to HTTPS

- [ ] **Configure EC2 Instance**
  - Use appropriate instance type (m5.xlarge or larger recommended)
  - Enable Nitro Enclaves in instance configuration
  - Configure security groups (allow only 443/HTTPS inbound)
  - Set up CloudWatch monitoring

- [ ] **Persistent Storage**
  - Ensure vsock-proxy systemd service is enabled
  - Verify enclave auto-starts on reboot (if required)

## Deployment

### Build and Deploy
- [ ] **Build from clean source**
  ```bash
  git clone https://github.com/midnight/midnight-ledger
  cd midnight-ledger
  git checkout <release-tag>
  ```

- [ ] **Build Docker image**
  ```bash
  docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:<version> .
  ```

- [ ] **Build EIF and save PCRs**
  ```bash
  nitro-cli build-enclave \
    --docker-uri midnight/proof-server:<version> \
    --output-file proof-server-<version>.eif
  ```
  - **CRITICAL**: Save PCR measurements from build output

- [ ] **Run automated deployment**
  ```bash
  cd tee-proof-server-proto
  DEBUG_MODE=false ./deploy-nitro-enclave.sh
  ```

### PCR Publication
- [ ] **Publish PCR measurements**
  - Copy `pcr-measurements-<version>.json` to web server
  - Make accessible via HTTPS at known URL
  - Example: `https://proof.midnight.network/.well-known/pcr-measurements.json`

- [ ] **Sign PCR file** (recommended)
  - Sign PCR JSON with GPG or similar
  - Publish signature alongside PCR file
  - Document verification process

- [ ] **Update client configurations**
  - Configure Lace to fetch PCRs from published URL
  - Update SDK documentation with PCR URL

## Post-Deployment Verification

### Functional Testing
- [ ] **Health check**
  ```bash
  curl https://proof.midnight.network/health
  ```

- [ ] **Attestation verification**
  ```bash
  curl 'https://proof.midnight.network/attestation?nonce=test123' | jq '.'
  ```
  - Verify attestation document is generated
  - Verify platform is "AWS Nitro Enclaves"

- [ ] **Proof generation test**
  - Submit test proof request with valid API key
  - Verify proof is generated successfully
  - Check response time is acceptable

### Security Verification
- [ ] **Test authentication**
  - Verify requests without API key are rejected (401)
  - Verify requests with invalid API key are rejected (401)
  - Verify requests with valid API key succeed (200)

- [ ] **Test rate limiting**
  - Send burst of requests
  - Verify rate limit kicks in (429)

- [ ] **PCR verification in client**
  - Test Lace attestation verification
  - Confirm "PCR verification skipped" warning is GONE
  - Verify PCRs match expected values

### Monitoring Setup
- [ ] **Set up CloudWatch dashboards**
  - Enclave CPU/memory usage
  - Request rate and latency
  - Error rates (4xx, 5xx)

- [ ] **Configure alerts**
  - Enclave crashes or restarts
  - High error rates
  - High latency
  - Rate limit exceeded frequently

- [ ] **Log aggregation**
  - Forward logs to CloudWatch Logs or similar
  - Set up log retention policy

## Production Operations

### Maintenance
- [ ] **Document rollback procedure**
  - Keep previous working EIF file
  - Document steps to revert to previous version

- [ ] **Certificate renewal**
  - Set up auto-renewal for TLS certificates
  - Test renewal process

- [ ] **Backup and disaster recovery**
  - Document recovery process for instance failure
  - Test recovery on staging environment

### Monitoring and Alerting
- [ ] **Daily checks**
  - Review error logs
  - Check enclave status
  - Verify attestation working

- [ ] **Weekly checks**
  - Review capacity and scaling needs
  - Check for security updates
  - Review API key usage patterns

### Security Updates
- [ ] **Regular updates**
  - Monitor for Rust/dependency security updates
  - Plan regular maintenance windows
  - Test updates on staging first

- [ ] **PCR rotation**
  - After any code update, PCR0 will change
  - Publish new PCR measurements
  - Notify clients of PCR updates
  - Maintain backward compatibility period

## Compliance and Documentation

- [ ] **Document deployment**
  - Record PCR measurements with git commit SHA
  - Document any configuration changes
  - Update runbook

- [ ] **Audit logging**
  - Enable request logging (without sensitive data)
  - Set up log retention per compliance requirements

- [ ] **Incident response plan**
  - Document on-call procedures
  - Define escalation path
  - Prepare communication templates

## Launch Checklist

Final checks before going live:

- [ ] All functional tests pass
- [ ] All security tests pass
- [ ] PCR measurements published and verified
- [ ] Monitoring and alerts configured
- [ ] Documentation updated
- [ ] Stakeholders notified
- [ ] Rollback plan tested
- [ ] API keys distributed securely
- [ ] Rate limits configured appropriately
- [ ] TLS certificate valid and auto-renewing

## Post-Launch

- [ ] Monitor for 24 hours continuously
- [ ] Review metrics and logs
- [ ] Gather feedback from early users
- [ ] Plan next iteration based on metrics

---

## Emergency Contacts

- **On-call engineer**: [Contact info]
- **Infrastructure team**: [Contact info]
- **Security team**: [Contact info]

## Useful Links

- Deployment guide: `DEPLOYMENT.md`
- Troubleshooting: `TROUBLESHOOTING.md` (to be created)
- Monitoring dashboard: [URL]
- Log aggregation: [URL]
