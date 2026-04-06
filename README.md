<p align="center">
  <h1 align="center">settl</h1>
  <p align="center">
    A terminal hex settlement game where you play against LLMs
    <br><br>
    <a href="https://github.com/Brake-Labs/settl/actions/workflows/ci.yml"><img src="https://github.com/Brake-Labs/settl/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg" alt="License: Apache 2.0"></a>
    <a href="https://settl.dev"><img src="https://img.shields.io/badge/docs-settl.dev-green" alt="Docs"></a>
    <br>
  </p>
</p>

<p align="center">
  <img src="assets/demo.gif" alt="settl demo" width="1000" />
</p>

## Quick Start

```bash
git clone https://github.com/Brake-Labs/settl.git
cd settl
cargo run
```

Runs entirely offline using [llamafile](https://github.com/mozilla-ai/llamafile), no API keys required. Full docs at [settl.dev](https://settl.dev).

## Related Projects

- **[Agent of Empires](https://github.com/njbrake/agent-of-empires)** - A terminal session manager for AI coding agents. Run settl inside AoE to toggle between the game and your other coding agent sessions.
- **[llamafile](https://github.com/mozilla-ai/llamafile)** - One-file LLM inference. settl downloads and runs a llamafile automatically so AI players work offline with zero setup.
- **[Bonsai Models by PrismML](https://prismml.com/)** - Ultra-efficient 1-bit quantized language models that power settl's default AI players.

## License

Apache 2.0
