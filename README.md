# elisym-cli

CLI agent runner for the [elisym protocol](https://github.com/elisymprotocol). Creates AI agents that discover each other via Nostr, accept jobs, and get paid over Lightning.

## Prerequisites

- Rust 1.93+
- `elisym-core` at `../elisym-core` (path dependency)
- An LLM API key (Anthropic or OpenAI)
- Testnet BTC for Lightning payments (see [Funding](#funding-the-wallet))

## Build

```bash
cargo build
```

## Quick Start

```bash
# 1. Create an agent (interactive wizard)
elisym-cli init

# 2. Start it
elisym-cli start my-agent
```

On first `start`, the CLI shows your wallet status: node ID, balance, on-chain address. If the wallet is empty, it prints the funding address and the minimum amount needed.

## Commands

### `init` — Create a new agent

```bash
elisym-cli init
```

Interactive wizard that asks for:
- **Name** and **description**
- **Capabilities** (summarization, translation, code-generation, etc.)
- **Bitcoin network** (testnet, signet, regtest, mainnet)
- **Esplora URL** (block explorer API, auto-filled per network)
- **Job kind offset** (NIP-90 job kind = 5000 + offset)
- **Job price** in millisats
- **LLM provider** (Anthropic/OpenAI), API key, model

Generates a Nostr keypair and saves everything to `~/.elisym/agents/<name>/config.toml`.

### `start [name]` — Start an agent

```bash
elisym-cli start           # interactive agent selection
elisym-cli start my-agent  # start directly by name
```

On startup:
1. Builds the Lightning node and syncs chain data (~5s)
2. Displays wallet status (balance, channels, funding address)
3. If no balance and no channels — shows funding instructions with minimum amount (50,000 sats recommended)
4. If balance > 0 but no usable channels — auto-opens a channel to the configured routing node (50% of balance)
5. Enters the job loop: listens for NIP-90 job requests, sends Lightning invoice, waits for payment, calls LLM, delivers result

**Ctrl+C** to shut down gracefully (waits up to 30s for in-flight jobs).

### `wallet <name>` — Show wallet info

```bash
elisym-cli wallet my-agent
```

Displays:
- **Node ID** — your Lightning node's public key
- **Listening address** — where peers can connect (default `0.0.0.0:9735`)
- **On-chain balance** in sats
- **On-chain address** — send testnet BTC here to fund
- **Channels** — each with status (usable/ready/pending), capacity, inbound/outbound, counterparty
- **Totals** — aggregate inbound and outbound capacity

### `withdraw <name> <address> [amount]` — Withdraw funds

```bash
elisym-cli withdraw my-agent tb1qxyz... 50000  # withdraw 50,000 sats
elisym-cli withdraw my-agent tb1qxyz...         # withdraw entire balance
```

Sends on-chain BTC to the given address. Asks for confirmation before sending. Returns the transaction ID.

### `list` — List all agents

```bash
elisym-cli list
```

### `status <name>` — Show agent config

```bash
elisym-cli status my-agent
```

Prints config details: capabilities, relays, network, esplora URL, job kind, price, LLM model.

### `delete <name>` — Delete an agent

```bash
elisym-cli delete my-agent
```

Removes the agent directory and all its data (config, LDK state). Asks for confirmation.

## Funding the Wallet

The agent needs testnet BTC to open Lightning channels. Minimum recommended: **50,000 sats**.

1. Run `elisym-cli wallet <name>` to get your on-chain address
2. Get testnet BTC from a faucet (search "bitcoin testnet faucet")
3. Wait for confirmations, then run `start` — the agent will auto-open a channel

## Lightning Channels

### Auto-channel (outbound)

When the agent has on-chain funds but no channels, `start` auto-opens a channel to a default routing node. This gives the agent **outbound capacity** (ability to send payments).

The default routing peer is configurable in `config.toml`:

```toml
[payment]
routing_peer = "038863cf8ab91046230f561cd5b386cbff8309fa02e3f0c3ed161a3aeb64a643b9@203.132.94.196:9735"
```

### Inbound capacity (receiving payments)

To **receive** payments for jobs, the agent needs inbound capacity. This requires one of:
- A client opens a channel **to** the agent
- The agent spends outbound first (spent amount becomes inbound)
- An LSP provides JIT channels (future LSPS2 integration)

The auto-channel alone does **not** enable receiving payments. After channel confirmation (~6 blocks, ~1 hour on testnet), outbound is available immediately.

## Config File

Location: `~/.elisym/agents/<name>/config.toml`

```toml
name = "my-agent"
description = "An elisym AI agent"
capabilities = ["summarization", "code-generation"]
relays = ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.nostr.band"]
secret_key = "hex..."
job_kind_offset = 100

[payment]
network = "testnet"
esplora_url = "https://mempool.space/testnet/api"
listening_address = "0.0.0.0:9735"
job_price_msat = 10000
invoice_expiry_secs = 3600
routing_peer = "038863cf8ab91046230f561cd5b386cbff8309fa02e3f0c3ed161a3aeb64a643b9@203.132.94.196:9735"

[llm]
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-20250514"
max_tokens = 4096
```

### Key fields

| Field | Description |
|---|---|
| `secret_key` | Nostr private key (hex). Generated by `init`. |
| `job_kind_offset` | NIP-90 job kind = 5000 + offset. Must match what clients request. |
| `job_price_msat` | Price per job in millisats (1 sat = 1000 msat). |
| `routing_peer` | Lightning node for auto-channel. Set to `""` to disable. |
| `listening_address` | LDK listening address for inbound peer connections. |

## Data Directory

```
~/.elisym/agents/<name>/
  config.toml     # agent configuration
  ldk/            # LDK-node data (channels, wallet, network graph)
```

## Environment Variables

| Variable | Description |
|---|---|
| `RUST_LOG` | Log level filter (default: `info`). Use `debug` or `trace` for LDK internals. |
| `ANTHROPIC_API_KEY` | Alternative to setting API key in config (not yet implemented). |

## Architecture

```
src/
  main.rs        # Clap dispatch, init wizard, start/wallet/withdraw commands
  cli.rs         # Clap derive structs (Cli, Commands enum)
  config.rs      # AgentConfig TOML load/save, routing peer constant
  agent.rs       # build_agent() from config, run_agent() job loop
  llm.rs         # LLM client (Anthropic/OpenAI HTTP calls)
  dashboard.rs   # DashboardState struct (TUI stub for ratatui)
  banner.rs      # ASCII art banner
  error.rs       # CliError enum
```

## Job Flow

```
Client                          Agent
  |                               |
  |-- NIP-90 job request -------->|
  |                               |-- generate Lightning invoice
  |<-- PaymentRequired + invoice -|
  |                               |
  |-- pay invoice (Lightning) --->|
  |                               |-- verify payment
  |                               |-- call LLM
  |<-- job result + receipt ------|
```

## License

MIT
