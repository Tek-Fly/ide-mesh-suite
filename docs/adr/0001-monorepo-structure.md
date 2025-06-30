# ADR-0001: Monorepo Structure with pnpm and Turborepo

## Status
Accepted

## Context
The IDE-Mesh Suite consists of multiple interconnected services and applications:
- Master-LLM Console (React/TypeScript)
- Chat Service (Rust)
- Code Service (TypeScript/Node.js)
- Shared libraries and utilities
- Common configuration and tooling

We need a repository structure that:
- Enables code sharing between services
- Maintains consistent tooling and dependencies
- Supports independent deployment of services
- Provides fast build times
- Handles TypeScript, Rust, and potentially other languages

Options considered:
1. **Polyrepo**: Separate repository for each service
2. **Monorepo with Yarn Workspaces**: Traditional Node.js monorepo
3. **Monorepo with pnpm**: Efficient disk space, strict dependencies
4. **Monorepo with Nx**: Full-featured but complex
5. **Monorepo with Turborepo**: Build orchestration focus

## Decision
Use a monorepo structure with pnpm for package management and Turborepo for build orchestration.

### Repository Structure
```
ide-mesh-suite/
├── apps/
│   ├── master-llm-console/    # React frontend
│   ├── chat-service/          # Rust WebSocket service
│   ├── code-service/          # TypeScript analysis service
│   └── docs/                  # Documentation site
├── packages/
│   ├── shared-types/          # TypeScript type definitions
│   ├── ui-components/         # Shared React components
│   ├── llm-client/           # LLM integration library
│   ├── auth/                 # Authentication utilities
│   └── config/               # Shared configuration
├── tools/
│   ├── eslint-config/        # Shared ESLint rules
│   ├── tsconfig/             # Shared TypeScript config
│   └── build-scripts/        # Custom build utilities
├── .github/                  # GitHub Actions workflows
├── pnpm-workspace.yaml       # pnpm workspace config
├── turbo.json               # Turborepo config
└── package.json             # Root package.json
```

### pnpm Configuration
```yaml
# pnpm-workspace.yaml
packages:
  - 'apps/*'
  - 'packages/*'
  - 'tools/*'
```

### Turborepo Configuration
```json
{
  "$schema": "https://turbo.build/schema.json",
  "pipeline": {
    "build": {
      "dependsOn": ["^build"],
      "outputs": ["dist/**", ".next/**", "target/**"]
    },
    "test": {
      "dependsOn": ["build"],
      "outputs": [],
      "cache": false
    },
    "lint": {
      "outputs": []
    },
    "dev": {
      "persistent": true,
      "cache": false
    },
    "deploy": {
      "dependsOn": ["build", "test"],
      "outputs": []
    }
  }
}
```

### Dependency Management
```json
// packages/shared-types/package.json
{
  "name": "@ide-mesh/shared-types",
  "version": "1.0.0",
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.mjs",
      "require": "./dist/index.js"
    }
  }
}

// apps/master-llm-console/package.json
{
  "name": "@ide-mesh/master-llm-console",
  "dependencies": {
    "@ide-mesh/shared-types": "workspace:*",
    "@ide-mesh/ui-components": "workspace:*",
    "@ide-mesh/llm-client": "workspace:*"
  }
}
```

### Build Orchestration
```bash
# Root package.json scripts
{
  "scripts": {
    "build": "turbo run build",
    "dev": "turbo run dev --parallel",
    "test": "turbo run test",
    "lint": "turbo run lint",
    "deploy:console": "turbo run deploy --filter=@ide-mesh/master-llm-console",
    "deploy:all": "turbo run deploy"
  }
}
```

### Rust Integration
```toml
# apps/chat-service/Cargo.toml
[workspace]
members = [".", "crates/*"]

[workspace.dependencies]
tokio = { version = "1.35", features = ["full"] }
tonic = "0.11"
serde = { version = "1.0", features = ["derive"] }

[package]
name = "chat-service"
version = "1.0.0"

[dependencies]
tokio = { workspace = true }
tonic = { workspace = true }
```

## Consequences

### Positive
- **Atomic Commits**: Changes across services in single commit
- **Code Sharing**: Easy to share types, utilities, and components
- **Consistent Tooling**: Single version of tools across all services
- **Efficient Storage**: pnpm's hard links save disk space
- **Fast Builds**: Turborepo caches and parallelizes builds
- **Type Safety**: Shared TypeScript types ensure consistency
- **Simplified CI/CD**: Single pipeline for all services

### Negative
- **Repository Size**: Can become large over time
- **Build Complexity**: More complex than single service builds
- **Access Control**: Harder to restrict access to specific services
- **Git Performance**: May slow down with many files
- **Learning Curve**: Team needs to understand monorepo tools
- **Rust Integration**: Less native than pure JS/TS monorepos

### Mitigation Strategies
1. **Git LFS**: Use for large binary files
2. **Sparse Checkouts**: Clone only needed directories
3. **Build Caching**: Aggressive Turborepo caching
4. **Documentation**: Comprehensive monorepo guides
5. **Gradual Migration**: Move services incrementally

## Implementation Guidelines

### Adding a New Service
```bash
# Create new app
mkdir -p apps/new-service
cd apps/new-service
pnpm init

# Add to workspace
echo "apps/new-service" >> ../../.pnpmfile.yaml

# Configure Turborepo
# Add to turbo.json pipeline

# Install dependencies
pnpm install
```

### Sharing Code
```typescript
// packages/shared-types/src/api.ts
export interface ChatMessage {
  id: string;
  userId: string;
  content: string;
  timestamp: Date;
  llmModel?: 'gpt-4' | 'claude-3';
}

// apps/master-llm-console/src/hooks/useChat.ts
import { ChatMessage } from '@ide-mesh/shared-types';

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  // ...
}
```

### CI/CD Integration
```yaml
# .github/workflows/ci.yml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - uses: pnpm/action-setup@v2
        with:
          version: 8
          
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: 'pnpm'
          
      - name: Install dependencies
        run: pnpm install --frozen-lockfile
        
      - name: Build and test
        run: pnpm turbo run build test lint
```

## Performance Optimizations

### Remote Caching
```json
{
  "remoteCache": {
    "signature": true
  }
}
```

### Selective Builds
```bash
# Only build affected services
pnpm turbo run build --filter='...[origin/main]'

# Build specific service and dependencies
pnpm turbo run build --filter=@ide-mesh/master-llm-console...
```

### Parallel Execution
```bash
# Run dev servers in parallel
pnpm turbo run dev --parallel --filter=@ide-mesh/*-service
```

## References
- [pnpm Workspaces](https://pnpm.io/workspaces)
- [Turborepo Documentation](https://turbo.build/repo/docs)
- [Monorepo Tools Comparison](https://monorepo.tools/)
- [Rust Workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html)

## Review History
- 2025-06-29: Initial decision by Claude (AI Assistant)
- 2025-06-29: Structure defined and implemented
- 2025-06-30: Performance optimizations added

---

*"Two are better than one, because they have a good return for their labor."* - Ecclesiastes 4:9