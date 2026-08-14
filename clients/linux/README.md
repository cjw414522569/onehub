# Linux client boundary

- Layer: L5 (native UI shell)
- Approved bridge: `abi-c`
- Status: `interface-only`; GTK4/libadwaita toolchains are intentionally not claimed as built in the Windows-first phase.
- The boundary is versioned in `contract.json` and must not link concrete SSH or persistence modules.

