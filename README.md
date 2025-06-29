# Tekfly TAAS V4 · IDE-Mesh & Master-LLM Suite

> **Mission**: Deliver a secure, mobile-first cockpit that merges research orchestration with coding capabilities, powered by ChatGPT Pro (OpenAI o3) and Claude Max (Claude Code CLI).

[![Security](https://img.shields.io/badge/security-MILITARY_GRADE-green)](./security/policies)
[![Architecture](https://img.shields.io/badge/arch-MICROSERVICES-blue)](./docs/adr)
[![License](https://img.shields.io/badge/license-PROPRIETARY-red)](./LICENSE)

## 🎯 Overview

The IDE-Mesh Suite provides a unified development environment integrating:

| Workspace | Core Function | Fixed-Cost LLM Source |
|-----------|---------------|----------------------|
| **Master-LLM Console** | Research, orchestration, agent chat | **ChatGPT Pro** (OpenAI o3) |
| **Secure IDE** | Coding, terminal, agent-assisted refactor | **Claude Max** (Claude Code CLI) |

Both workspaces share a raw-BSON **Virtual DOM** mediated by FlyByWireMemory V4.4 and comply with Tekfly's A2A / MCP / AG-UI standards.

## 🏗️ Architecture

```mermaid
flowchart TD
  CF[Cloudflare WAF\nAccess JWT] --> TR(Traefik Mesh)
  TR --> CHAT(chat-srv | OpenAI o3)
  TR --> TOKEN(token-meter)
  TR --> IDE(ide-srv | openvscode + Claude CLI)
  CHAT -. gRPC/QUIC .-> GW(memory-gateway)
  IDE -->|gRPC/QUIC| GW
  GW --> MEM[FlyByWireMemory V4.4]
  MEM --> MDB[(MongoDB 7\nraw-BSON)]
  MDB --> GB[github-bridge]
```

## 🚀 Quick Start

### Prerequisites
- Docker 24+ with BuildKit
- Node.js 20+
- Rust 1.78+
- Go 1.22+
- Flutter 3.4+

### Local Development

```bash
# Clone the repository
git clone git@github.com:Tek-Fly/ide-mesh-suite.git
cd ide-mesh-suite

# Bootstrap development environment
make bootstrap   # installs pre-commit hooks & toolchains

# Start services
docker-compose up -d

# Access services
# IDE: https://local.ide.test
# Chat: https://local.chat.test
```

## 📁 Repository Structure

```
ide-mesh-suite/               # Monorepo root
├─ docs/                      # MkDocs, ADRs, diagrams
│   ├─ adr/                   # Architecture Decision Records
│   └─ threat-models/         # Security threat models
├─ infra/                     # Infrastructure as Code
│   ├─ terraform/             # Cloudflare, Keycloak, Swarm, DNS
│   └─ ansible/               # OS hardening, PQC cert rollout
├─ services/                  # One folder → one OCI image
│   ├─ chat-srv/              # Rust (OpenAI proxy)
│   ├─ ide-srv/               # Node (openvscode) + Rust wrapper
│   ├─ memory-gateway/        # Rust gRPC façade
│   ├─ token-meter/           # Go (Redis quota)
│   ├─ github-bridge/         # Go (Git push bot)
│   └─ taas-broker/           # Rust + iceoryx bus
├─ datastore/
│   └─ mongo-ice/             # Mongo FIPS build, schema, $redact rules
├─ ui/
│   ├─ flutter_pwa/           # Material 3 split-view, BLoC
│   └─ wasm_components/       # WIT + Rust→WASM build
├─ extensions/
│   ├─ vscode-tekfly-chat/    # VS Code chat extension
│   └─ vscode-tekfly-claude/  # VS Code Claude integration
├─ deployments/
│   ├─ docker-compose.yml     # Local development
│   ├─ swarm-stack.yml        # Production Swarm
│   └─ k8s/                   # Kubernetes manifests (future)
└─ security/
    ├─ sbom/                  # Software Bill of Materials
    ├─ scans/                 # Security scan results
    └─ policies/              # Security policies
```

## 🔒 Security

### Multi-Layer Security Architecture

1. **Edge Protection**
   - Cloudflare WAF with custom rules
   - Bot detection and mitigation
   - Rate limiting (60 req/min)
   - DDoS protection

2. **Transport Security**
   - Kyber768 + X25519 hybrid TLS 1.3
   - HSTS with 2-year max-age
   - Certificate pinning

3. **Runtime Security**
   - Rootless Docker containers
   - Seccomp default-deny profiles
   - Read-only root filesystems
   - Non-root user execution

4. **Secrets Management**
   - HashiCorp Vault integration
   - 1-hour lease durations
   - Automatic rotation
   - Zero-knowledge architecture

5. **Audit & Compliance**
   - MongoDB Oplog → EU audit log
   - Immutable audit trail
   - GDPR compliant logging
   - EU AI Act compliance

## 🛠️ Services

### chat-srv (Rust)
- OpenAI o3 proxy with streaming
- Token usage tracking
- Context window management
- Rate limiting per user

### ide-srv (Node + Rust)
- OpenVSCode server integration
- Claude Code CLI wrapper
- File system synchronization
- Terminal multiplexing

### memory-gateway (Rust)
- gRPC/QUIC interface
- Virtual DOM synchronization
- Conflict resolution
- Zero-copy BSON operations

### token-meter (Go)
- Redis-backed quota management
- Real-time usage analytics
- Billing integration hooks
- Alert thresholds

### github-bridge (Go)
- Automated Git operations
- Signed commits with GPG
- Branch protection enforcement
- PR automation

### taas-broker (Rust)
- iceoryx shared memory bus
- Zero-copy message passing
- Service discovery
- Health monitoring

## 🎨 UI Components

### Flutter PWA
- Material 3 design system
- Tekfly brand colors
- Split-view layout
- Offline-first architecture
- WebSocket real-time sync
- Haptic feedback support

### WASM Components
- WIT interface definitions
- Rust → WASM compilation
- Browser-native performance
- Secure sandbox execution

## 📊 Monitoring & Observability

- **Metrics**: Prometheus + Grafana
- **Logs**: Loki with 90-day retention
- **Traces**: Tempo with OpenTelemetry
- **Alerts**: PagerDuty integration

## 🚦 CI/CD Pipeline

```yaml
Pipeline:
  - Build → Unit Tests → SBOM Generation
  - Security Scan (Trivy, Semgrep)
  - Container Sign (Cosign)
  - Push to ghcr.io/tekfly
  - Deploy to Staging
  - Integration Tests
  - Production Deploy (manual approval)
```

## 🤝 Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for guidelines.

### Code Standards
- **Rust**: `cargo fmt` + `clippy --deny warnings`
- **Go**: `golangci-lint` with strict config
- **TypeScript**: ESLint + Prettier
- **Dart**: `dart format` + analyzer

### Commit Convention
We use [Conventional Commits](https://www.conventionalcommits.org/):
- `feat:` New features
- `fix:` Bug fixes
- `docs:` Documentation changes
- `perf:` Performance improvements
- `security:` Security fixes

## 📜 License

© 2025 Tekfly Ltd. All rights reserved.

This is proprietary software. Unauthorized copying, modification, or distribution is strictly prohibited.

## 🆘 Support

- **Issues**: Create a GitHub issue
- **Security**: security@tekfly.io
- **Enterprise**: support@tekfly.io

---

Built with ❤️ by Tekfly Engineering